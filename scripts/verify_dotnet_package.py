"""Pack Grafikr.Client and exercise it from an independent NuGet consumer."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import tempfile
import zipfile
import xml.etree.ElementTree as ET
from pathlib import Path

from scripts.build_runtime import ROOT, _tool, validate_manifest

PROJECT = ROOT / "bindings/dotnet/Grafikr.Client/Grafikr.Client.csproj"
RIDS = {
    "x86_64-pc-windows-msvc": "win-x64",
    "aarch64-pc-windows-msvc": "win-arm64",
    "x86_64-unknown-linux-gnu": "linux-x64",
    "aarch64-unknown-linux-gnu": "linux-arm64",
    "x86_64-apple-darwin": "osx-x64",
    "aarch64-apple-darwin": "osx-arm64",
}


def project_version(project: Path = PROJECT) -> str:
    version = ET.parse(project).getroot().findtext("./PropertyGroup/Version")
    if not version:
        raise ValueError("Grafikr.Client project does not declare a package version")
    return version


def verify(
    bundle: Path,
    output: Path | None = None,
    keep: bool = False,
    *,
    test_consumer: bool = True,
    fixture_scenarios: bool = False,
) -> None:
    os.environ.setdefault("MSBUILDDISABLENODEREUSE", "1")
    bundle = bundle.resolve()
    manifest = validate_manifest(bundle)
    if manifest["development_fixture"]:
        raise ValueError("development fixture bundles cannot be used by the NuGet consumer")
    try:
        rid = RIDS[manifest["target"]]
    except KeyError as error:
        raise ValueError(f"unsupported NuGet runtime target: {manifest['target']}") from error
    if not (ROOT / "bindings/dotnet/Grafikr.Client/Client.cs").is_file():
        raise ValueError("Grafikr.Client source is missing")
    version = project_version()
    (ROOT / "target").mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="grafikr-dotnet-consumer-", dir=ROOT / "target", ignore_cleanup_errors=True, delete=not keep) as temporary:
        work = Path(temporary)
        feed = work / "feed"
        feed.mkdir()
        _tool("dotnet", "pack", str(PROJECT), "-c", "Release", "--disable-build-servers", f"-p:PackageOutputPath={feed}", f"-p:RuntimeIdentifier={rid}", f"-p:GrafikrRuntimeBundle={bundle}", cwd=ROOT)
        package = feed / f"Grafikr.Client.{version}.nupkg"
        if not package.is_file():
            raise RuntimeError(f"dotnet pack did not create Grafikr.Client.{version}.nupkg")
        with zipfile.ZipFile(package) as archive:
            for entry in manifest["files"]:
                member = f"runtimes/{rid}/native/{entry['path']}"
                contents = archive.read(member)
                if len(contents) != entry["size"] or hashlib.sha256(contents).hexdigest() != entry["sha256"]:
                    raise ValueError(f"NuGet runtime file failed verification: {entry['path']}")
        if output:
            destination = output.resolve()
            destination.mkdir(parents=True, exist_ok=True)
            (destination / package.name).write_bytes(package.read_bytes())
        if not test_consumer:
            print(f"NuGet package and {rid} runtime file hashes verified; foreign consumer not launched")
            return
        consumer = work / "consumer"
        consumer.mkdir()
        os.environ["NUGET_PACKAGES"] = str(work / "nuget-cache")
        (consumer / "Consumer.csproj").write_text(
            f"""<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><RuntimeIdentifier>{rid}</RuntimeIdentifier><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
  <ItemGroup><PackageReference Include="Grafikr.Client" Version="{version}" /></ItemGroup>
</Project>
""",
            encoding="utf-8",
        )
        code = """using Grafikr;
await using var app = await App.StartAsync(new RuntimeOptions());
var state = await app.CreateStateAsync(new { type = "i64", value = long.MinValue.ToString() });
if ((await state.GetAsync()).GetProperty("value").GetString() != long.MinValue.ToString()) throw new Exception("i64 minimum mismatch");
await state.SetAsync(new { type = "i64", value = long.MaxValue.ToString() });
if ((await state.GetAsync()).GetProperty("value").GetString() != long.MaxValue.ToString()) throw new Exception("i64 maximum mismatch");
await state.DestroyAsync();
"""
        if fixture_scenarios:
            code += """
var fixtureHost = Environment.GetEnvironmentVariable("GRAFIKR_FIXTURE_HOST");
if (string.IsNullOrWhiteSpace(fixtureHost)) throw new Exception("GRAFIKR_FIXTURE_HOST is required");
await using (var scenarioApp = await App.StartAsync(new RuntimeOptions(HostPath: fixtureHost)))
{
    var counter = await scenarioApp.CreateWindowAsync("Installed counter", 480, 320);
    var count = await scenarioApp.CreateStateAsync(new { type = "i64", value = "0" });
    var button = await scenarioApp.CreateNodeAsync(counter, "Button", new { label = "Increment" });
    var label = await scenarioApp.CreateNodeAsync(counter, "Text", new { text = "Count: 0" });
    var field = await scenarioApp.CreateNodeAsync(counter, "TextField", new { text = "Grafikr 雪", width = 240 });
    await button.AttachAsync(counter.Root); await label.AttachAsync(counter.Root); await field.AttachAsync(counter.Root);
    var clicked = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
    await using var subscription = await scenarioApp.SubscribeAsync(button.Handle, "click", async _ =>
    {
        var next = long.Parse((await count.GetAsync()).GetProperty("value").GetString()!) + 1;
        await count.SetAsync(new { type = "i64", value = next.ToString() });
        await label.UpdateAsync(new { text = $"Count: {next}" });
        clicked.TrySetResult();
    });
    await clicked.Task.WaitAsync(TimeSpan.FromSeconds(5));
    if ((await count.GetAsync()).GetProperty("value").GetString() != "1") throw new Exception("counter state did not update");
    await field.UpdateAsync(new { text = "Input 雪" });
    await count.DestroyAsync(); await counter.CloseAsync();

    var dashboard = await scenarioApp.CreateWindowAsync("Installed dashboard", 640, 480);
    var rows = await scenarioApp.CreateNodeAsync(dashboard, "VirtualList", new { row_count = 50000, row_height = 24, width = 600, height = 360 });
    await rows.AttachAsync(dashboard.Root);
    await using var provider = await scenarioApp.CreateCollectionProviderAsync(rows, new CollectionMetadata(50000, 24, new[] { "label" }));
    var request = await provider.NextRequestAsync(TimeSpan.FromSeconds(5));
    if (request is null || request.Start != 0 || request.Count != 32) throw new Exception("visible row range was not requested");
    await provider.ProvideAsync(request, Enumerable.Range(0, request.Count).Select(index => new[] { $"Row {index}" }).ToArray());
    var canvas = await scenarioApp.CreateNodeAsync(dashboard, "Canvas", new { commands = Array.Empty<object>(), label = "Dashboard canvas", width = 320, height = 120 });
    await canvas.AttachAsync(dashboard.Root);
    await scenarioApp.RequestAsync("canvas.update", new { node = canvas.Handle, commands = new[] { new { kind = "rect", rect = new { x = 0.0, y = 0.0, width = 32.0, height = 24.0 }, color = "#336699", radius = 0.0 } } });
    var upload = (await scenarioApp.RequestAsync("image.begin", new { width = 1, height = 1, byte_length = 4 })).GetProperty("upload").GetString()!;
    await scenarioApp.RequestAsync("image.chunk", new { upload, offset = 0, data = Convert.ToBase64String(new byte[] { 0, 0, 0, 255 }) });
    var image = (await scenarioApp.RequestAsync("image.finalize", new { upload })).GetProperty("image").GetString()!;
    var imageNode = await scenarioApp.CreateNodeAsync(dashboard, "Image", new { image, width = 1, height = 1, label = "Pixel" });
    await imageNode.AttachAsync(dashboard.Root);
    await scenarioApp.RequestAsync("image.destroy", new { image });
    await imageNode.DestroyAsync(); await canvas.DestroyAsync(); await provider.DisposeAsync(); await rows.DestroyAsync(); await dashboard.CloseAsync();

    var first = await scenarioApp.CreateWindowAsync("First window", 320, 240);
    var second = await scenarioApp.CreateWindowAsync("Second window", 320, 240);
    var modal = await scenarioApp.CreateNodeAsync(second, "Modal", new { open = true, width = 240 });
    await modal.AttachAsync(second.Root);
    var message = await scenarioApp.CreateNodeAsync(second, "Text", new { text = "Confirm" });
    await message.AttachAsync(modal.Handle);
    var shared = await scenarioApp.CreateStateAsync(new { type = "i64", value = "1" });
    await first.CloseAsync();
    await shared.SetAsync(new { type = "i64", value = "2" });
    if ((await shared.GetAsync()).GetProperty("value").GetString() != "2") throw new Exception("remaining window lost state");
    await message.DestroyAsync(); await modal.DestroyAsync(); await shared.DestroyAsync(); await second.CloseAsync();
}
"""
        (consumer / "Program.cs").write_text(code, encoding="utf-8")
        _tool("dotnet", "restore", "Consumer.csproj", "--disable-build-servers", "--source", str(feed), "--ignore-failed-sources", cwd=consumer)
        _tool("dotnet", "run", "--no-restore", "--disable-build-servers", "--project", "Consumer.csproj", cwd=consumer)
        if keep:
            print(f"NuGet consumer retained at {consumer}")
    print("Grafikr.Client NuGet package installed in a clean consumer and passed signed i64 IPC round trips")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--keep", action="store_true", help="keep the temporary consumer for debugging")
    parser.add_argument("--fixture-scenarios", action="store_true")
    args = parser.parse_args(argv)
    try:
        verify(args.bundle, args.out, args.keep, fixture_scenarios=args.fixture_scenarios)
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as error:
        parser.exit(1, f"NuGet package verification failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
