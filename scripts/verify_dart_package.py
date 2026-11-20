"""Install a Dart source artifact and run its example from a clean consumer."""

from __future__ import annotations

import os
import argparse
import hashlib
import json
import sys
import tarfile
import tempfile
from pathlib import Path, PurePosixPath

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scripts.build_runtime import ROOT, _tool
from scripts.package_bindings import _pack_source_snapshot, _package_path, _source_sdk_files

DART_INSTALLED_SCENARIOS = r'''
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';
import 'package:grafikr/grafikr.dart';

Future<void> counterForm(String host) async {
  final app = await App.start(host);
  try {
    final window = await app.createWindow('Installed counter', 480, 320);
    final count = await app.createState({'type': 'i64', 'value': '0'});
    final button = await app.createNode(window, 'Button', {'label': 'Increment'});
    final label = await app.createNode(window, 'Text', {'text': 'Count: 0'});
    final field = await app.createNode(window, 'TextField', {'text': 'Grafikr 雪', 'width': 240});
    await button.attach(window.root); await label.attach(window.root); await field.attach(window.root);
    final handled = Completer<void>();
    final subscription = await app.on(button.handle, 'click', (_) async {
      final current = (await count.get() as Map)['value'] as String;
      final next = int.parse(current) + 1;
      await count.set({'type': 'i64', 'value': '$next'});
      await label.update({'text': 'Count: $next'});
      if (!handled.isCompleted) handled.complete();
    });
    await handled.future.timeout(const Duration(seconds: 5));
    if ((await count.get() as Map)['value'] != '1') throw StateError('counter state did not update');
    await field.update({'text': 'Input 雪'});
    await subscription.close(); await count.close(); await window.close();
  } finally { await app.close(); }
}

Future<void> dashboard(String host) async {
  final app = await App.start(host);
  try {
    final window = await app.createWindow('Installed dashboard', 640, 480);
    final rows = await app.createNode(window, 'VirtualList', {'row_count': 50000, 'row_height': 24, 'width': 600, 'height': 360});
    await rows.attach(window.root);
    final provider = await app.createCollectionProvider(rows, const CollectionMetadata(50000, 24, ['label']));
    final request = await provider.nextRequest(timeout: const Duration(seconds: 5));
    if (request == null || request.start != 0 || request.count != 32) throw StateError('collection range missing');
    await provider.provide(request, List.generate(request.count, (index) => ['Row $index']));
    final canvas = await app.createNode(window, 'Canvas', {'commands': [], 'label': 'Dashboard canvas', 'width': 320, 'height': 120});
    await canvas.attach(window.root);
    await app.request('canvas.update', {'node': canvas.handle, 'commands': [{'kind': 'rect', 'rect': {'x': 0.0, 'y': 0.0, 'width': 32.0, 'height': 24.0}, 'color': '#336699', 'radius': 0.0}]});
    final upload = (await app.request('image.begin', {'width': 1, 'height': 1, 'byte_length': 4}) as Map)['upload'];
    await app.request('image.chunk', {'upload': upload, 'offset': 0, 'data': base64Encode(Uint8List.fromList([0, 0, 0, 255]))});
    final image = (await app.request('image.finalize', {'upload': upload}) as Map)['image'];
    final imageNode = await app.createNode(window, 'Image', {'image': image, 'width': 1, 'height': 1, 'label': 'Pixel'});
    await imageNode.attach(window.root); await app.request('image.destroy', {'image': image});
    await imageNode.destroy(); await canvas.destroy(); await provider.close(); await rows.destroy(); await window.close();
  } finally { await app.close(); }
}

Future<void> multiWindow(String host) async {
  final app = await App.start(host);
  try {
    final first = await app.createWindow('First', 320, 240);
    final second = await app.createWindow('Second', 320, 240);
    final modal = await app.createNode(second, 'Modal', {'open': true, 'width': 240});
    await modal.attach(second.root);
    final message = await app.createNode(second, 'Text', {'text': 'Confirm'});
    await message.attach(modal.handle);
    final state = await app.createState({'type': 'i64', 'value': '1'});
    await first.close(); await state.set({'type': 'i64', 'value': '2'});
    if ((await state.get() as Map)['value'] != '2') throw StateError('remaining window lost session state');
    await message.destroy(); await modal.destroy(); await state.close(); await second.close();
  } finally { await app.close(); }
}

Future<void> main() async {
  final host = Platform.environment['GRAFIKR_FIXTURE_HOST'];
  if (host == null || host.isEmpty) throw StateError('GRAFIKR_FIXTURE_HOST is required');
  await counterForm(host); await dashboard(host); await multiWindow(host);
}
'''


def _indexed_dart_artifact(package_root: Path) -> Path:
    index = json.loads((package_root / "package-index.json").read_text(encoding="utf-8"))
    if index.get("format") != "grafikr-bindings-package-index-v1":
        raise ValueError("unsupported local package index format")
    matches = [row for row in index.get("packages", []) if row.get("sdk") == "dart" and row.get("target") == "source"]
    if len(matches) != 1:
        raise ValueError("package index must contain exactly one Dart source archive")
    artifact = package_root.joinpath(*_package_path(matches[0].get("path")).parts)
    if not artifact.is_file() or artifact.is_symlink():
        raise ValueError(f"Dart source archive is missing or unsafe: {artifact}")
    if hashlib.sha256(artifact.read_bytes()).hexdigest() != matches[0].get("sha256"):
        raise ValueError("Dart source archive checksum does not match its package index")
    return artifact


def verify(bundle: Path, fixture_scenarios: bool = False, package_root: Path | None = None) -> None:
    target = ROOT / "target"
    target.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="grafikr-dart-consumer-", dir=target) as temporary:
        work = Path(temporary)
        artifact = (
            _indexed_dart_artifact(package_root.resolve())
            if package_root is not None
            else _pack_source_snapshot(_source_sdk_files("dart", "1.1.0"), work, "grafikr-dart-1.1.0.tar.gz")
        )
        package = work / "package"
        package.mkdir()
        with tarfile.open(artifact, "r:gz") as archive:
            for entry in archive:
                path = PurePosixPath(entry.name)
                if path.is_absolute() or any(part in ("", ".", "..") for part in path.parts) or not entry.isfile():
                    raise ValueError(f"unsafe Dart package archive entry: {entry.name}")
                destination = package.joinpath(*path.parts)
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(archive.extractfile(entry).read())
        consumer = work / "consumer"
        consumer.mkdir()
        (consumer / "pubspec.yaml").write_text(
            "name: grafikr_consumer\n"
            "environment:\n  sdk: '>=3.10.0 <4.0.0'\n"
            "dependencies:\n  grafikr:\n    path: ../package\n",
            encoding="utf-8",
        )
        _tool("dart", "pub", "get", "--offline", cwd=consumer)
        environment_keys = (
            "GRAFIKR_BUNDLE", "GRAFIKR_CACHE", "GRAFIKR_FIXTURE_EARLY_SUBSCRIBE",
            "GRAFIKR_FIXTURE_COLLECTION_REQUEST",
        )
        original = {key: os.environ.get(key) for key in environment_keys}
        os.environ["GRAFIKR_BUNDLE"] = str(bundle.resolve())
        os.environ["GRAFIKR_CACHE"] = str(work / "runtime-cache")
        try:
            _tool("dart", "run", str((package / "example/state_smoke.dart").resolve()), cwd=consumer)
            if fixture_scenarios:
                fixture = os.environ.get("GRAFIKR_FIXTURE_HOST")
                if not fixture or not Path(fixture).is_absolute() or not Path(fixture).is_file():
                    raise ValueError("installed Dart scenarios require an absolute GRAFIKR_FIXTURE_HOST")
                scenario = consumer / "installed_scenarios.dart"
                scenario.write_text(DART_INSTALLED_SCENARIOS, encoding="utf-8")
                os.environ["GRAFIKR_FIXTURE_EARLY_SUBSCRIBE"] = "1"
                os.environ["GRAFIKR_FIXTURE_COLLECTION_REQUEST"] = "1"
                _tool("dart", "run", str(scenario.resolve()), cwd=consumer)
        finally:
            for key, value in original.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value
    print("Dart source package installed offline; its example passed an i64 round trip")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-root", type=Path)
    args = parser.parse_args(argv)
    bundle = os.environ.get("GRAFIKR_RUNTIME_DIR")
    if not bundle:
        parser.error("GRAFIKR_RUNTIME_DIR must identify the release bundle")
    try:
        verify(
            Path(bundle),
            fixture_scenarios=bool(os.environ.get("GRAFIKR_FIXTURE_HOST")),
            package_root=args.package_root,
        )
    except (OSError, ValueError, RuntimeError) as error:
        parser.exit(1, f"Dart package verification failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
