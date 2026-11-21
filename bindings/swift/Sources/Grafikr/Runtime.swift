import Foundation
#if os(Windows)
import WinSDK
#else
import Crypto
#endif

public enum Runtime {
    private struct Manifest: Decodable {
        struct File: Decodable {
            let path: String
            let size: Int64
            let sha256: String
            let role: String
        }

        let format_version: Int
        let engine_version: String
        let protocolVersion: Version
        let abi: Version
        let target: String
        let min_os: String
        let min_glibc: String?
        let build_sha: String
        let build_config_sha256: String
        let source_dirty: Bool
        let development_fixture: Bool
        let system_requirements: [String]
        let limits: [String: Int64]
        let files: [File]

        enum CodingKeys: String, CodingKey {
            case format_version, engine_version, target, min_os, min_glibc, build_sha, build_config_sha256
            case source_dirty, development_fixture, system_requirements, limits, files
            case protocolVersion = "protocol"
            case abi
        }
    }

    private struct Version: Decodable { let major: Int; let minor: Int }
    private struct Validated { let manifest: Manifest; let data: Data; let host: String }

    public static func install(bundle: URL, cache: URL) throws -> URL {
        guard bundle.isFileURL, cache.isFileURL else { throw failure("runtime_path", "Bundle and cache must be file URLs") }
        let target = try currentTarget()
        let source = try validate(bundle.standardizedFileURL, target: target)
        let cacheRoot = try createDirectoryChain(cache.standardizedFileURL)
        let versionRoot = cacheRoot.appendingPathComponent(target, isDirectory: true)
            .appendingPathComponent(source.manifest.engine_version, isDirectory: true)
        _ = try createDirectoryChain(versionRoot)
        let destination = versionRoot.appendingPathComponent(source.manifest.build_sha, isDirectory: true)

        if FileManager.default.fileExists(atPath: destination.path) {
            let existing = try validate(destination, target: target)
            guard existing.data == source.data else { throw failure("runtime_cache", "Cached runtime manifest does not match bundle") }
            return destination.appendingPathComponent(existing.host)
        }

        let stage = versionRoot.appendingPathComponent(".grafikr-runtime-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: stage, withIntermediateDirectories: false)
        do {
            for file in source.manifest.files {
                let relative = try safePath(file.path)
                let sourceFile = bundle.appendingPathComponent(relative)
                let stagedFile = stage.appendingPathComponent(relative)
                _ = try createDirectoryChain(stagedFile.deletingLastPathComponent())
                try FileManager.default.copyItem(at: sourceFile, to: stagedFile)
            }
            try source.data.write(to: stage.appendingPathComponent("manifest.json"), options: .atomic)
            let copied = try validate(stage, target: target)
            guard copied.data == source.data else { throw failure("runtime_manifest", "Staged runtime manifest changed") }
            do {
                try FileManager.default.moveItem(at: stage, to: destination)
            } catch {
                let winner = try validate(destination, target: target)
                guard winner.data == source.data else { throw failure("runtime_cache", "Concurrent cache entry does not match bundle") }
            }
            return destination.appendingPathComponent(copied.host)
        } catch {
            try? FileManager.default.removeItem(at: stage)
            throw error
        }
    }

    private static func validate(_ root: URL, target: String) throws -> Validated {
        try requireType(root, .typeDirectory)
        let manifestURL = root.appendingPathComponent("manifest.json")
        try requireType(manifestURL, .typeRegular)
        let attributes = try FileManager.default.attributesOfItem(atPath: manifestURL.path)
        let manifestSize = (attributes[.size] as? NSNumber)?.int64Value ?? Int64.max
        guard manifestSize <= 1_048_576 else {
            throw failure("runtime_manifest", "Manifest exceeds 1 MiB")
        }
        let data = try Data(contentsOf: manifestURL, options: .mappedIfSafe)
        let manifest: Manifest
        do { manifest = try JSONDecoder().decode(Manifest.self, from: data) }
        catch { throw failure("runtime_manifest", "Runtime manifest is invalid") }
        guard manifest.format_version == 1,
              manifest.engine_version.range(of: #"^\d+\.\d+\.\d+$"#, options: .regularExpression) != nil,
              manifest.target == target, !manifest.development_fixture,
              manifest.protocolVersion.major == 1, manifest.abi.major == 1,
              !manifest.min_os.isEmpty, Self.requiredLimits.isSubset(of: Set(manifest.limits.keys)),
              manifest.limits.values.allSatisfy({ $0 > 0 }), !manifest.system_requirements.isEmpty,
              manifest.build_sha.range(of: #"^[0-9a-f]{40}$"#, options: .regularExpression) != nil,
              manifest.build_config_sha256.range(of: #"^[0-9a-f]{64}$"#, options: .regularExpression) != nil,
              !manifest.files.isEmpty, manifest.files.count <= 256 else {
            throw failure("runtime_manifest", "Runtime target or release metadata is invalid")
        }

        var seen = Set<String>()
        var hosts: [String] = []
        for file in manifest.files {
            let relative = try safePath(file.path)
            guard file.size >= 0,
                  file.sha256.range(of: #"^[0-9a-f]{64}$"#, options: .regularExpression) != nil,
                  seen.insert(relative.lowercased()).inserted, relative != "manifest.json" else {
                throw failure("runtime_manifest", "Runtime file record is invalid or duplicated")
            }
            if file.role == "host" { hosts.append(relative) }
            let components = relative.split(separator: "/")
            var current = root
            for (index, component) in components.enumerated() {
                current.appendPathComponent(String(component))
                let last = index == components.count - 1
                try requireType(current, last ? .typeRegular : .typeDirectory)
                if last {
                    let fileAttributes = try FileManager.default.attributesOfItem(atPath: current.path)
                    guard (fileAttributes[.size] as? NSNumber)?.int64Value == file.size,
                          try sha256(current) == file.sha256 else {
                        throw failure("runtime_hash", "Runtime checksum or size does not match: \(file.path)")
                    }
                }
            }
        }
        guard hosts.count == 1, seen.count == manifest.files.count else {
            throw failure("runtime_manifest", "Runtime manifest must contain exactly one host")
        }
        return Validated(manifest: manifest, data: data, host: hosts[0])
    }

    private static func safePath(_ value: String) throws -> String {
        let components = value.split(separator: "/", omittingEmptySubsequences: false)
        guard !value.isEmpty, value.utf8.count <= 240, !value.hasPrefix("/"),
              !value.contains("\\"), !value.contains(":"), !value.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains),
              components.allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".." }) else {
            throw failure("runtime_path", "Runtime manifest contains an unsafe relative path")
        }
        return value
    }

    private static func requireType(_ url: URL, _ expected: FileAttributeType) throws {
        let isSymbolicLink = (try? url.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink) == true
        guard !isSymbolicLink else { throw failure("runtime_file", "Runtime path is a symbolic link: \(url.lastPathComponent)") }
        let attributes: [FileAttributeKey: Any]
        do { attributes = try FileManager.default.attributesOfItem(atPath: url.path) }
        catch { throw failure("runtime_file", "Runtime path is missing or unreadable: \(url.lastPathComponent)") }
        guard let type = attributes[.type] as? FileAttributeType, type == expected else {
            throw failure("runtime_file", "Runtime path has an unsafe type: \(url.lastPathComponent)")
        }
    }

    private static func createDirectoryChain(_ url: URL) throws -> URL {
        let windowsAbsolute = url.path.range(of: #"^[A-Za-z]:[\\/]"#, options: .regularExpression) != nil
        guard url.isFileURL, url.path.hasPrefix("/") || windowsAbsolute else {
            throw failure("runtime_path", "Cache path must be absolute")
        }
        let components = url.standardizedFileURL.pathComponents
        var current = URL(fileURLWithPath: components[0], isDirectory: true)
        for component in components.dropFirst() {
            current.appendPathComponent(component, isDirectory: true)
            if !FileManager.default.fileExists(atPath: current.path) {
                try FileManager.default.createDirectory(at: current, withIntermediateDirectories: false)
            }
            try requireType(current, .typeDirectory)
        }
        return current
    }

    private static func sha256(_ url: URL) throws -> String {
        let input = try FileHandle(forReadingFrom: url)
        defer { try? input.close() }
        #if os(Windows)
        var algorithm: BCRYPT_ALG_HANDLE?
        let algorithmName = Array("SHA256".utf16) + [0]
        let openStatus = algorithmName.withUnsafeBufferPointer {
            BCryptOpenAlgorithmProvider(&algorithm, $0.baseAddress, nil, 0)
        }
        guard openStatus >= 0, let algorithm else { throw failure("runtime_hash", "Windows SHA-256 provider could not be opened") }
        defer { _ = BCryptCloseAlgorithmProvider(algorithm, 0) }

        let propertyName = Array("ObjectLength".utf16) + [0]
        var objectLength: ULONG = 0
        var propertyLength: ULONG = 0
        let propertyStatus = propertyName.withUnsafeBufferPointer {
            BCryptGetProperty(algorithm, $0.baseAddress, &objectLength, ULONG(MemoryLayout<ULONG>.size), &propertyLength, 0)
        }
        guard propertyStatus >= 0 else { throw failure("runtime_hash", "Windows SHA-256 provider metadata is unavailable") }
        var object = [UInt8](repeating: 0, count: Int(objectLength))
        var hash: BCRYPT_HASH_HANDLE?
        let createStatus = object.withUnsafeMutableBufferPointer {
            BCryptCreateHash(algorithm, &hash, $0.baseAddress, objectLength, nil, 0, 0)
        }
        guard createStatus >= 0, let hash else { throw failure("runtime_hash", "Windows SHA-256 hash could not be created") }
        defer { _ = BCryptDestroyHash(hash) }
        while var chunk = try input.read(upToCount: 1_048_576), !chunk.isEmpty {
            let status = chunk.withUnsafeMutableBytes {
                BCryptHashData(hash, $0.baseAddress?.assumingMemoryBound(to: UCHAR.self), ULONG($0.count), 0)
            }
            guard status >= 0 else { throw failure("runtime_hash", "Windows SHA-256 update failed") }
        }
        var digest = [UInt8](repeating: 0, count: 32)
        let finishStatus = digest.withUnsafeMutableBufferPointer {
            BCryptFinishHash(hash, $0.baseAddress, ULONG($0.count), 0)
        }
        guard finishStatus >= 0 else { throw failure("runtime_hash", "Windows SHA-256 finalization failed") }
        return digest.map { String(format: "%02x", $0) }.joined()
        #else
        var digest = SHA256()
        while let chunk = try input.read(upToCount: 1_048_576), !chunk.isEmpty { digest.update(data: chunk) }
        return digest.finalize().map { String(format: "%02x", $0) }.joined()
        #endif
    }

    private static func currentTarget() throws -> String {
        #if os(Windows)
        let os = "pc-windows-msvc"
        #elseif os(Linux)
        let os = "unknown-linux-gnu"
        #elseif os(macOS)
        let os = "apple-darwin"
        #else
        throw failure("runtime_target", "Grafikr has no runtime for this operating system")
        #endif
#if arch(x86_64)
        let arch = "x86_64"
        #elseif arch(arm64)
        let arch = "aarch64"
        #else
        throw failure("runtime_target", "Grafikr has no runtime for this architecture")
        #endif
        return arch + "-" + os
    }

    private static let requiredLimits: Set<String> = [
        "frame_bytes", "json_depth", "command_queue_messages", "command_queue_bytes",
        "output_queue_messages", "output_queue_bytes", "windows", "nodes", "states",
        "subscriptions", "image_bytes", "image_session_bytes", "image_chunk_bytes",
        "canvas_commands", "canvas_bytes", "model_bytes", "collection_rows_per_node",
        "collection_bytes_per_node", "collection_bytes_session", "collection_request_rows",
        "tree_items", "tree_depth", "host_shutdown_seconds",
    ]

    private static func failure(_ code: String, _ message: String) -> BridgeError {
        BridgeError(code: code, message: message)
    }
}
