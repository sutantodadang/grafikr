using System.Reflection;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text.Json;

namespace Grafikr;

public sealed record RuntimeOptions(string? Bundle = null, string? HostPath = null);

public static class Runtime
{
    private static readonly IReadOnlyDictionary<string, string> Targets = new Dictionary<string, string>
    {
        ["win-x64"] = "x86_64-pc-windows-msvc",
        ["win-arm64"] = "aarch64-pc-windows-msvc",
        ["linux-x64"] = "x86_64-unknown-linux-gnu",
        ["linux-arm64"] = "aarch64-unknown-linux-gnu",
        ["osx-x64"] = "x86_64-apple-darwin",
        ["osx-arm64"] = "aarch64-apple-darwin",
    };

    public static string ResolveHost(string? bundle = null)
    {
        var architecture = RuntimeInformation.ProcessArchitecture switch
        {
            Architecture.X64 => "x64",
            Architecture.Arm64 => "arm64",
            _ => throw new BridgeException("runtime_target", $"Grafikr runtime is unavailable for {RuntimeInformation.ProcessArchitecture}"),
        };
        var operatingSystem = OperatingSystem.IsWindows() ? "win" : OperatingSystem.IsLinux() ? "linux" : OperatingSystem.IsMacOS() ? "osx" : null;
        var rid = operatingSystem is null ? "unknown" : $"{operatingSystem}-{architecture}";
        if (!Targets.TryGetValue(rid, out var target)) throw new BridgeException("runtime_target", $"Grafikr runtime is unavailable for {rid}");
        var root = bundle is null
            ? Path.Combine(AppContext.BaseDirectory, "runtimes", rid, "native")
            : Path.GetFullPath(bundle);
        FileAttributes attributes;
        try { attributes = File.GetAttributes(root); }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            throw new BridgeException("runtime_missing", "Grafikr runtime bundle is missing or inaccessible");
        }
        if ((attributes & FileAttributes.Directory) == 0 || (attributes & FileAttributes.ReparsePoint) != 0)
            throw new BridgeException("runtime_path", "Grafikr runtime bundle directory is missing or unsafe");

        JsonDocument manifest;
        var manifestPath = Path.Combine(root, "manifest.json");
        try
        {
            if (IsReparsePoint(manifestPath) || new FileInfo(manifestPath).Length > 1024 * 1024)
                throw new InvalidDataException("unsafe manifest");
            manifest = JsonDocument.Parse(File.ReadAllBytes(manifestPath));
        }
        catch (Exception error) when (error is IOException or JsonException or InvalidDataException)
        {
            throw new BridgeException("runtime_manifest", "Grafikr runtime manifest is missing or invalid");
        }
        using (manifest)
        {
            var document = manifest.RootElement;
            if (document.GetProperty("target").GetString() != target)
                throw new BridgeException("runtime_target", $"Grafikr runtime target does not match {rid}");
            if (document.GetProperty("development_fixture").GetBoolean())
                throw new BridgeException("runtime_fixture", "Development fixture runtimes cannot be used by Grafikr.Client");
            string? host = null;
            foreach (var entry in document.GetProperty("files").EnumerateArray())
            {
                var relative = entry.GetProperty("path").GetString()!;
                if (relative.Contains('\\') || Path.IsPathRooted(relative) || relative.Split('/').Any(part => part is "" or "." or ".."))
                    throw new BridgeException("runtime_manifest", "Runtime manifest contains an unsafe path");
                var path = Path.GetFullPath(Path.Combine(root, relative.Replace('/', Path.DirectorySeparatorChar)));
                var fromRoot = Path.GetRelativePath(root, path);
                if (Path.IsPathRooted(fromRoot) || fromRoot == ".." || fromRoot.StartsWith(".." + Path.DirectorySeparatorChar, StringComparison.Ordinal))
                    throw new BridgeException("runtime_manifest", "Runtime manifest path escapes its bundle");
                var directory = root;
                foreach (var part in relative.Split('/').SkipLast(1))
                {
                    directory = Path.Combine(directory, part);
                    if (IsReparsePoint(directory)) throw new BridgeException("runtime_integrity", $"Runtime path is unsafe: {relative}");
                }
                if (IsReparsePoint(path)) throw new BridgeException("runtime_integrity", $"Runtime file is unsafe: {relative}");
                var info = new FileInfo(path);
                if (!info.Exists || info.Length != entry.GetProperty("size").GetInt64())
                    throw new BridgeException("runtime_integrity", $"Runtime file is missing or changed: {relative}");
                using var stream = info.OpenRead();
                var hash = Convert.ToHexString(SHA256.HashData(stream)).ToLowerInvariant();
                if (hash != entry.GetProperty("sha256").GetString())
                    throw new BridgeException("runtime_integrity", $"Runtime file checksum does not match: {relative}");
                if (entry.GetProperty("role").GetString() == "host") host = path;
            }
            return host ?? throw new BridgeException("runtime_manifest", "Runtime bundle has no host executable");
        }
    }

    private static bool IsReparsePoint(string path)
    {
        try { return (File.GetAttributes(path) & FileAttributes.ReparsePoint) != 0; }
        catch (FileNotFoundException) { return false; }
        catch (DirectoryNotFoundException) { return false; }
    }
}
