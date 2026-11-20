"""Install the indexed Swift source archive into a clean consumer and run scenarios."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import tempfile
import zipfile
from pathlib import Path, PurePosixPath

from scripts.build_runtime import ROOT, _tool
from scripts.package_bindings import _package_path

SWIFT_INSTALLED_SCENARIOS = r'''
import Foundation
import Grafikr

actor Completion {
    private var finished = false
    func complete() { finished = true }
    func wait(seconds: TimeInterval = 5) async throws {
        let deadline = Date().addingTimeInterval(seconds)
        while !finished {
            if Date() >= deadline { throw NSError(domain: "GrafikrConsumer", code: 1, userInfo: [NSLocalizedDescriptionKey: "scenario timed out"]) }
            try await Task.sleep(nanoseconds: 10_000_000)
        }
    }
}

func stateValue(_ data: Data) throws -> String {
    let value = try JSONSerialization.jsonObject(with: data) as! [String: Any]
    return value["value"] as! String
}

func start(_ host: URL) async throws -> App {
    let app = try App(host: host)
    try await app.start()
    return app
}

func counterForm(_ host: URL) async throws {
    let app = try await start(host)
    do {
        let window = try await app.createWindow(title: "Installed counter", width: 480, height: 320)
        let count = try await app.createState(type: "i64", value: "0")
        let button = try await app.createNode(window: window, widget: "Button", properties: ["label": "Increment"])
        let label = try await app.createNode(window: window, widget: "Text", properties: ["text": "Count: 0"])
        let field = try await app.createNode(window: window, widget: "TextField", properties: ["text": "Grafikr 雪", "width": 240])
        try await button.attach(to: window.root); try await label.attach(to: window.root); try await field.attach(to: window.root)
        let done = Completion()
        let subscription = try await app.on(node: button.handle, event: "click") { _ in
            Task {
                do {
                    let next = Int(try await stateValue(count.get()))! + 1
                    try await count.set(type: "i64", value: String(next))
                    try await label.update(["text": "Count: \\(next)"])
                    await done.complete()
                } catch { await done.complete() }
            }
        }
        try await done.wait()
        let current = try await count.get()
        guard try stateValue(current) == "1" else { throw NSError(domain: "GrafikrConsumer", code: 2) }
        try await field.update(["text": "Input 雪"])
        await subscription.close(); try await count.destroy(); try await window.close()
        await app.close()
    } catch { await app.close(); throw error }
}

func dashboard(_ host: URL) async throws {
    let app = try await start(host)
    do {
        let window = try await app.createWindow(title: "Installed dashboard", width: 640, height: 480)
        let rows = try await app.createNode(window: window, widget: "VirtualList", properties: ["row_count": 50000, "row_height": 24, "width": 600, "height": 360])
        try await rows.attach(to: window.root)
        let provider = try await app.createCollectionProvider(node: rows, metadata: CollectionMetadata(rowCount: 50000, rowHeight: 24, columns: ["label"]))
        guard let request = try await provider.nextRequest(timeout: 5), request.start == 0, request.count == 32 else { throw NSError(domain: "GrafikrConsumer", code: 3) }
        try await provider.provide(request, rows: (0..<request.count).map { ["Row \($0)"] })
        let canvas = try await app.createNode(window: window, widget: "Canvas", properties: ["commands": [], "label": "Dashboard canvas", "width": 320, "height": 120])
        try await canvas.attach(to: window.root)
        _ = try await app.request("canvas.update", ["node": canvas.handle, "commands": [["kind": "rect", "rect": ["x": 0.0, "y": 0.0, "width": 32.0, "height": 24.0], "color": "#336699", "radius": 0.0]]])
        let upload = try JSONSerialization.jsonObject(with: try await app.request("image.begin", ["width": 1, "height": 1, "byte_length": 4])) as! [String: String]
        let pixel = Data([0, 0, 0, 255]).base64EncodedString()
        _ = try await app.request("image.chunk", ["upload": upload["upload"]!, "offset": 0, "data": pixel])
        let image = try JSONSerialization.jsonObject(with: try await app.request("image.finalize", ["upload": upload["upload"]!])) as! [String: String]
        let imageNode = try await app.createNode(window: window, widget: "Image", properties: ["image": image["image"]!, "width": 1, "height": 1, "label": "Pixel"])
        try await imageNode.attach(to: window.root)
        _ = try await app.request("image.destroy", ["image": image["image"]!])
        _ = try await app.request("node.destroy", ["node": imageNode.handle]); _ = try await app.request("node.destroy", ["node": canvas.handle])
        await provider.close(); _ = try await app.request("node.destroy", ["node": rows.handle]); try await window.close()
        await app.close()
    } catch { await app.close(); throw error }
}

func multiWindow(_ host: URL) async throws {
    let app = try await start(host)
    do {
        let first = try await app.createWindow(title: "First", width: 320, height: 240)
        let second = try await app.createWindow(title: "Second", width: 320, height: 240)
        let modal = try await app.createNode(window: second, widget: "Modal", properties: ["open": true, "width": 240])
        try await modal.attach(to: second.root)
        let message = try await app.createNode(window: second, widget: "Text", properties: ["text": "Confirm"])
        try await message.attach(to: modal.handle)
        let shared = try await app.createState(type: "i64", value: "1")
        try await first.close(); try await shared.set(type: "i64", value: "2")
        let state = try await shared.get()
        guard try stateValue(state) == "2" else { throw NSError(domain: "GrafikrConsumer", code: 4) }
        _ = try await app.request("node.destroy", ["node": message.handle]); _ = try await app.request("node.destroy", ["node": modal.handle])
        try await shared.destroy(); try await second.close()
        await app.close()
    } catch { await app.close(); throw error }
}

@main struct Main {
    static func main() async throws {
        guard let host = ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"], !host.isEmpty else { throw NSError(domain: "GrafikrConsumer", code: 5) }
        let url = URL(fileURLWithPath: host)
        try await counterForm(url); try await dashboard(url); try await multiWindow(url)
        print("installed Swift counter/form, dashboard, and multi-window passed")
    }
}
'''


def _artifact(package_root: Path) -> Path:
    index = json.loads((package_root / "package-index.json").read_text(encoding="utf-8"))
    if index.get("format") != "grafikr-bindings-package-index-v1":
        raise ValueError("unsupported local package index format")
    matches = [row for row in index.get("packages", []) if row.get("sdk") == "swift" and row.get("target") == "source"]
    if len(matches) != 1:
        raise ValueError("package index must contain exactly one Swift source archive")
    artifact = package_root.joinpath(*_package_path(matches[0].get("path")).parts)
    if not artifact.is_file() or artifact.is_symlink():
        raise ValueError(f"Swift source archive is missing or unsafe: {artifact}")
    if hashlib.sha256(artifact.read_bytes()).hexdigest() != matches[0].get("sha256"):
        raise ValueError("Swift source archive checksum does not match its package index")
    return artifact


def verify(package_root: Path, fixture_host: Path | None = None) -> None:
    package_root = package_root.resolve()
    fixture_host = fixture_host or Path(os.environ.get("GRAFIKR_FIXTURE_HOST", ""))
    fixture_host = fixture_host.resolve()
    if not fixture_host.is_file() or fixture_host.is_symlink():
        raise ValueError("fixture host must be an existing safe file")
    archive_path = _artifact(package_root)
    (ROOT / "target").mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="grafikr-swift-consumer-", dir=ROOT / "target", ignore_cleanup_errors=True) as temporary:
        root = Path(temporary)
        package = root / "grafikr"
        consumer = root / "consumer"
        package.mkdir(); consumer.mkdir()
        with zipfile.ZipFile(archive_path) as archive:
            for info in archive.infolist():
                member = PurePosixPath(info.filename)
                mode = info.external_attr >> 16
                if member.is_absolute() or any(part in ("", ".", "..") for part in member.parts) or (mode & 0o170000) == 0o120000:
                    raise ValueError(f"unsafe Swift archive entry: {info.filename}")
                destination = package.joinpath(*member.parts)
                if info.is_dir():
                    destination.mkdir(parents=True, exist_ok=True)
                else:
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    destination.write_bytes(archive.read(info))
        (consumer / "Package.swift").write_text(
            "// swift-tools-version: 6.0\nimport PackageDescription\n"
            "let package = Package(name: \"Consumer\", dependencies: [.package(path: \"../grafikr\")], "
            "targets: [.executableTarget(name: \"Consumer\", dependencies: [.product(name: \"Grafikr\", package: \"grafikr\")])])\n",
            encoding="utf-8",
        )
        source = consumer / "Sources" / "Consumer"
        source.mkdir(parents=True)
        (source / "main.swift").write_text(SWIFT_INSTALLED_SCENARIOS, encoding="utf-8")
        environment = os.environ.copy() | {"GRAFIKR_FIXTURE_HOST": str(fixture_host)}
        previous = os.environ.get("GRAFIKR_FIXTURE_HOST")
        os.environ["GRAFIKR_FIXTURE_HOST"] = str(fixture_host)
        try:
            _tool("swift", "run", "--package-path", str(consumer), cwd=root)
        finally:
            if previous is None:
                os.environ.pop("GRAFIKR_FIXTURE_HOST", None)
            else:
                os.environ["GRAFIKR_FIXTURE_HOST"] = previous
    print("Swift source archive installed as a clean package consumer and passed fixture-backed scenarios")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-root", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        verify(args.package_root)
    except (OSError, RuntimeError, ValueError, zipfile.BadZipFile, json.JSONDecodeError) as error:
        parser.exit(1, f"Swift package verification failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
