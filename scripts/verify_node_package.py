"""Install the packed Node SDK offline and exercise its packaged API."""

from __future__ import annotations

import argparse
import json
import os
import tarfile
import tempfile
from pathlib import Path

from scripts.build_runtime import ROOT, _tool, validate_manifest
from scripts.package_node_runtime import package as package_runtime

NODE_PROJECT = ROOT / "bindings/node"
RUNTIME_TARGETS = ("win32-x64", "win32-arm64", "linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64")
NODE_INSTALLED_SCENARIOS = r'''
import { App, CollectionProvider, Node, State, Window } from "@grafikr/ui";

const app = new App(process.env.GRAFIKR_FIXTURE_HOST, "installed");
await app.ready;
try {
  const window = await Window.create(app, "Installed counter", 480, 320);
  const count = await State.create(app, { type: "i64", value: 0n });
  const button = await Node.create(app, window, "Button", { label: "Increment" });
  const label = await Node.create(app, window, "Text", { text: "Count: 0" });
  const field = await Node.create(app, window, "TextField", { text: "Grafikr 雪", width: 240 });
  await button.attach(window); await label.attach(window); await field.attach(window);
  let finish;
  const handled = new Promise(resolve => { finish = resolve; });
  const subscription = await app.on(button.handle, "click", async () => {
    const next = (await count.get()) + 1n;
    await count.set({ type: "i64", value: next });
    await label.update({ text: `Count: ${next}` });
    finish();
  });
  await Promise.race([handled, new Promise((_, reject) => setTimeout(() => reject(new Error("counter click timed out")), 5000))]);
  if (await count.get() !== 1n) throw new Error("counter state did not update");
  await field.update({ text: "Input 雪" });
  await subscription.close(); await count.close(); await window.close();

  const dashboard = await Window.create(app, "Installed dashboard", 640, 480);
  const rows = await Node.create(app, dashboard, "VirtualList", { row_count: 50000, row_height: 24, width: 600, height: 360 });
  await rows.attach(dashboard);
  const provider = await CollectionProvider.create(rows, { row_count: 50000, row_height: 24, columns: ["label"] });
  const request = await Promise.race([
    provider.nextRequest(),
    new Promise((_, reject) => setTimeout(() => reject(new Error("collection request timed out")), 5000)),
  ]);
  if (!request || request.start !== 0 || request.count !== 32) throw new Error("visible range was not requested");
  await provider.provide(request, Array.from({ length: request.count }, (_, index) => [`Row ${index}`]));
  const canvas = await Node.create(app, dashboard, "Canvas", { commands: [], label: "Dashboard canvas", width: 320, height: 120 });
  await canvas.attach(dashboard);
  await app.request("canvas.update", { node: canvas.handle, commands: [{
    kind: "rect", rect: { x: 0, y: 0, width: 32, height: 24 }, color: "#336699", radius: 0,
  }] });
  const upload = (await app.request("image.begin", { width: 1, height: 1, byte_length: 4 })).upload;
  await app.request("image.chunk", { upload, offset: 0, data: Buffer.from([0, 0, 0, 255]).toString("base64") });
  const image = (await app.request("image.finalize", { upload })).image;
  const imageNode = await Node.create(app, dashboard, "Image", { image, width: 1, height: 1, label: "Pixel" });
  await imageNode.attach(dashboard);
  await app.request("image.destroy", { image });
  await imageNode.destroy(); await canvas.destroy(); await provider.close(); await rows.destroy(); await dashboard.close();

  const first = await Window.create(app, "First window", 320, 240);
  const second = await Window.create(app, "Second window", 320, 240);
  const modal = await Node.create(app, second, "Modal", { open: true, width: 240 });
  await modal.attach(second);
  const message = await Node.create(app, second, "Text", { text: "Confirm" });
  await message.attach(modal);
  const shared = await State.create(app, { type: "i64", value: 1n });
  await first.close();
  await shared.set({ type: "i64", value: 2n });
  if (await shared.get() !== 2n) throw new Error("remaining window lost session state");
  await message.destroy(); await modal.destroy(); await shared.close(); await second.close();
} finally {
  await app.close();
}
console.log("installed JS/TS counter_form, dashboard, and multi_window passed");
'''


def validate_optional_dependencies(metadata: dict) -> None:
    version = metadata.get("version")
    dependencies = metadata.get("optionalDependencies")
    for target in RUNTIME_TARGETS:
        name = f"@grafikr/runtime-{target}"
        if not isinstance(version, str) or not isinstance(dependencies, dict) or dependencies.get(name) != version:
            raise ValueError(f"missing or incorrect optional runtime dependency {name}")


def verify(
    bundle: Path,
    runtime_artifact: Path | None = None,
    output: Path | None = None,
    fixture_scenarios: bool = False,
) -> None:
    bundle = bundle.resolve()
    manifest = validate_manifest(bundle)
    if manifest["development_fixture"]:
        raise ValueError("development fixture bundles cannot be installed as Node SDK runtimes")
    if manifest["engine_version"] != json.loads((NODE_PROJECT / "package.json").read_text(encoding="utf-8"))["version"]:
        raise ValueError("Node SDK version does not match the runtime engine version")
    with tempfile.TemporaryDirectory(prefix="grafikr-node-consumer-", dir=ROOT / "target") as temporary:
        work = Path(temporary)
        tarballs = output.resolve() if output else work / "tarballs"
        tarballs.mkdir(parents=True, exist_ok=True)
        if runtime_artifact is None:
            runtime_package = package_runtime(bundle, work / "runtime-package")
        elif runtime_artifact.is_dir():
            packages = [path.resolve() for path in runtime_artifact.glob("*.tgz")]
            if len(packages) != 1:
                raise ValueError("expected exactly one Node runtime package in the selected directory")
            runtime_package = packages[0]
        else:
            runtime_package = runtime_artifact.resolve()
        packed = _tool("npm", "pack", "--json", "--pack-destination", str(tarballs), cwd=NODE_PROJECT)
        package = json.loads(packed.stdout)
        if len(package) != 1:
            raise RuntimeError("npm pack did not create exactly one SDK tarball")
        tarball = tarballs / package[0]["filename"]
        with tarfile.open(tarball, "r:gz") as archive:
            metadata_file = archive.extractfile("package/package.json")
            if metadata_file is None:
                raise RuntimeError("packed SDK is missing package.json")
            validate_optional_dependencies(json.load(metadata_file))
        installed = work / "consumer"
        installed.mkdir()
        _tool("npm", "install", "--offline", "--omit=optional", "--ignore-scripts", "--no-audit", "--no-fund", "--prefix", str(installed), str(tarball), str(runtime_package), cwd=installed)
        declarations = installed / "consumer.mts"
        declarations.write_text((NODE_PROJECT / "test/consumer-types.mts").read_text(encoding="utf-8"), encoding="utf-8")
        _tool(
            "npm",
            "exec",
            "--offline",
            "--prefix",
            str(NODE_PROJECT),
            "--",
            "tsc",
            "--noEmit",
            "--strict",
            "--target",
            "ES2022",
            "--module",
            "NodeNext",
            "--moduleResolution",
            "NodeNext",
            str(declarations),
            cwd=installed,
        )
        ts_consumer = installed / "consumer-runtime.mts"
        ts_consumer.write_text(
            '''import { Client, State } from "@grafikr/ui";
const client = await Client.start();
try {
  const state = await State.create(client, { type: "i64", value: -(1n << 63n) });
  if (await state.get() !== -(1n << 63n)) throw new Error("TypeScript i64 minimum mismatch");
  await state.set({ type: "i64", value: (1n << 63n) - 1n });
  if (await state.get() !== (1n << 63n) - 1n) throw new Error("TypeScript i64 maximum mismatch");
  await state.close();
} finally { await client.close(); }
''',
            encoding="utf-8",
        )
        ts_output = installed / "typescript-output"
        _tool(
            "npm",
            "exec",
            "--offline",
            "--prefix",
            str(NODE_PROJECT),
            "--",
            "tsc",
            "--strict",
            "--skipLibCheck",
            "--target",
            "ES2022",
            "--module",
            "NodeNext",
            "--moduleResolution",
            "NodeNext",
            "--outDir",
            str(ts_output),
            str(ts_consumer),
            cwd=installed,
        )
        _tool("node", str(ts_output / "consumer-runtime.mjs"), cwd=installed)
        code = """
import { Client, State } from '@grafikr/ui';
const client = await Client.start();
try {
  const state = await State.create(client, { type: 'i64', value: -(1n << 63n) });
  if (await state.get() !== -(1n << 63n)) throw new Error('i64 minimum round trip failed');
  await state.set({ type: 'i64', value: (1n << 63n) - 1n });
  if (await state.get() !== (1n << 63n) - 1n) throw new Error('i64 maximum round trip failed');
  await state.close();
} finally { await client.close(); }
"""
        entry = installed / "consumer.mjs"
        entry.write_text(code, encoding="utf-8")
        _tool("node", str(entry), cwd=installed)
        if fixture_scenarios:
            fixture = os.environ.get("GRAFIKR_FIXTURE_HOST")
            if not fixture or not Path(fixture).is_absolute() or not Path(fixture).is_file():
                raise ValueError("installed fixture scenarios require an absolute GRAFIKR_FIXTURE_HOST")
            scenario_entry = installed / "installed-scenarios.mjs"
            scenario_entry.write_text(NODE_INSTALLED_SCENARIOS, encoding="utf-8")
            _tool("node", str(scenario_entry), cwd=installed)
    suffix = "; installed counter, dashboard, and multi-window scenarios passed" if fixture_scenarios else ""
    print(f"Node SDK/runtime tarballs installed offline; JS and TS consumers, declarations, and signed i64 IPC passed{suffix}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--runtime-package", type=Path)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--fixture-scenarios", action="store_true")
    args = parser.parse_args(argv)
    try:
        verify(args.bundle, args.runtime_package, args.out, args.fixture_scenarios)
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as error:
        parser.exit(1, f"Node package verification failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
