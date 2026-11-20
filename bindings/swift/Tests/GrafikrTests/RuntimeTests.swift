import Foundation
import XCTest
@testable import Grafikr

final class RuntimeTests: XCTestCase {
    func testInstallVerifiesAndReusesBundleCache() throws {
        let root = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("grafikr-runtime-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let bundle = root.appendingPathComponent("bundle", isDirectory: true)
        try makeBundle(bundle)
        let cache = root.appendingPathComponent("cache", isDirectory: true)

        let installed = try Runtime.install(bundle: bundle, cache: cache)
        XCTAssertEqual(try Data(contentsOf: installed), Data("host executable".utf8))
        XCTAssertEqual(try Runtime.install(bundle: bundle, cache: cache), installed)
    }

    func testRejectsTraversalAndChecksumMismatchBeforeCopying() throws {
        let root = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("grafikr-runtime-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let bundle = root.appendingPathComponent("bundle", isDirectory: true)
        try makeBundle(bundle, path: "../escape")
        XCTAssertThrowsError(try Runtime.install(bundle: bundle, cache: root.appendingPathComponent("traversal-cache"))) {
            XCTAssertEqual(($0 as? BridgeError)?.code, "runtime_path")
        }

        try makeBundle(bundle, hashOverride: String(repeating: "0", count: 64))
        let cache = root.appendingPathComponent("bad-cache", isDirectory: true)
        XCTAssertThrowsError(try Runtime.install(bundle: bundle, cache: cache)) {
            XCTAssertEqual(($0 as? BridgeError)?.code, "runtime_hash")
        }
        XCTAssertFalse(FileManager.default.fileExists(atPath: cache.path))
    }

    func testRejectsCachedManifestSubstitution() throws {
        let root = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("grafikr-runtime-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let bundle = root.appendingPathComponent("bundle", isDirectory: true)
        try makeBundle(bundle)
        let cache = root.appendingPathComponent("cache", isDirectory: true)
        let host = try Runtime.install(bundle: bundle, cache: cache)
        let manifestURL = host.deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("manifest.json")
        var manifest = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: manifestURL)) as? [String: Any])
        manifest["source_dirty"] = true
        try JSONSerialization.data(withJSONObject: manifest).write(to: manifestURL)
        XCTAssertThrowsError(try Runtime.install(bundle: bundle, cache: cache)) {
            XCTAssertEqual(($0 as? BridgeError)?.code, "runtime_cache")
        }
    }

    func testRejectsSymbolicLinkRuntimeFile() throws {
        let root = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("grafikr-runtime-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let bundle = root.appendingPathComponent("bundle", isDirectory: true)
        let hostPath = "bin/grafikr-host" + (RuntimeTests.isWindows ? ".exe" : "")
        let external = root.appendingPathComponent("external")
        try FileManager.default.createDirectory(at: external, withIntermediateDirectories: true)
        try Data("host executable".utf8).write(to: external.appendingPathComponent("host"))
        try FileManager.default.createDirectory(at: bundle.appendingPathComponent("bin"), withIntermediateDirectories: true)
        do {
            try FileManager.default.createSymbolicLink(at: bundle.appendingPathComponent(hostPath), withDestinationURL: external.appendingPathComponent("host"))
        } catch { throw XCTSkip("Symbolic link creation is unavailable") }
        try writeManifest(bundle, path: hostPath, contents: Data("host executable".utf8))
        XCTAssertThrowsError(try Runtime.install(bundle: bundle, cache: root.appendingPathComponent("cache"))) {
            XCTAssertEqual(($0 as? BridgeError)?.code, "runtime_file")
        }
    }

    private static var isWindows: Bool {
        #if os(Windows)
        true
        #else
        false
        #endif
    }

    private func makeBundle(_ root: URL, path: String = "bin/grafikr-host", hashOverride: String? = nil) throws {
        let relative = path == "bin/grafikr-host" && RuntimeTests.isWindows ? "bin/grafikr-host.exe" : path
        let data = Data("host executable".utf8)
        let file = root.appendingPathComponent(relative)
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: file)
        try writeManifest(root, path: relative, contents: data, hashOverride: hashOverride)
    }

    private func writeManifest(_ root: URL, path: String, contents: Data, hashOverride: String? = nil) throws {
        let target = Self.target
        let manifest: [String: Any] = [
            "format_version": 1, "engine_version": "9.9.9", "target": target,
            "protocol": ["major": 1, "minor": 0], "abi": ["major": 1, "minor": 0],
            "build_sha": String(repeating: "a", count: 40), "build_config_sha256": String(repeating: "b", count: 64),
            "development_fixture": false, "source_dirty": false, "min_os": "test",
            "min_glibc": NSNull(), "system_requirements": ["test runtime"], "limits": Self.limits,
            "files": [["path": path, "size": contents.count, "sha256": hashOverride ?? "4c579eb6d37f1ca7c3b1708b6b354f48d003864d880eea1385e7a478167e39ea", "role": "host"]],
        ]
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try JSONSerialization.data(withJSONObject: manifest).write(to: root.appendingPathComponent("manifest.json"))
    }

    private static var target: String {
        #if arch(x86_64)
        let arch = "x86_64"
        #else
        let arch = "aarch64"
        #endif
        #if os(Windows)
        return arch + "-pc-windows-msvc"
        #elseif os(Linux)
        return arch + "-unknown-linux-gnu"
        #else
        return arch + "-apple-darwin"
        #endif
    }

    private static let limits: [String: Int] = [
        "frame_bytes": 8_388_608, "json_depth": 64, "command_queue_messages": 1024,
        "command_queue_bytes": 16_777_216, "output_queue_messages": 1024,
        "output_queue_bytes": 16_777_216, "windows": 16, "nodes": 100_000,
        "states": 100_000, "subscriptions": 100_000, "image_bytes": 67_108_864,
        "image_session_bytes": 268_435_456, "image_chunk_bytes": 1_048_576,
        "canvas_commands": 10_000, "canvas_bytes": 8_388_608, "model_bytes": 134_217_728,
        "collection_rows_per_node": 2048, "collection_bytes_per_node": 8_388_608,
        "collection_bytes_session": 67_108_864, "collection_request_rows": 512,
        "tree_items": 50_000, "tree_depth": 64, "host_shutdown_seconds": 5,
    ]
}
