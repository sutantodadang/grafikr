"""Pack one verified Grafikr host bundle as a Node platform package."""

from __future__ import annotations

import argparse
import hashlib
import json
import tarfile
import tempfile
from pathlib import Path

from scripts.build_runtime import ROOT, _tool, install_bundle, validate_manifest

NODE_TARGETS = {
    "x86_64-pc-windows-msvc": ("win32-x64", "win32", "x64"),
    "aarch64-pc-windows-msvc": ("win32-arm64", "win32", "arm64"),
    "x86_64-unknown-linux-gnu": ("linux-x64", "linux", "x64"),
    "aarch64-unknown-linux-gnu": ("linux-arm64", "linux", "arm64"),
    "x86_64-apple-darwin": ("darwin-x64", "darwin", "x64"),
    "aarch64-apple-darwin": ("darwin-arm64", "darwin", "arm64"),
}


def package(bundle: Path, output: Path) -> Path:
    bundle = bundle.resolve()
    manifest = validate_manifest(bundle)
    if manifest["development_fixture"]:
        raise ValueError("development fixture bundles cannot be published as Node runtimes")
    try:
        npm_target, os_name, cpu = NODE_TARGETS[manifest["target"]]
    except KeyError as error:
        raise ValueError(f"unsupported Node runtime target: {manifest['target']}") from error
    project_version = json.loads((ROOT / "bindings/node/package.json").read_text(encoding="utf-8"))["version"]
    if project_version != manifest["engine_version"]:
        raise ValueError("Node SDK version does not match the runtime engine version")
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="grafikr-node-runtime-", dir=ROOT / "target") as temporary:
        package_dir = Path(temporary) / "package"
        runtime_dir = package_dir / "runtime"
        install_bundle(bundle, runtime_dir)
        metadata = {
            "name": f"@grafikr/runtime-{npm_target}",
            "version": project_version,
            "description": f"Grafikr native runtime for {npm_target}",
            "files": ["runtime"],
            "os": [os_name],
            "cpu": [cpu],
            "engines": {"node": ">=24"},
            "license": "AGPL-3.0-only",
        }
        (package_dir / "package.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
        (package_dir / "README.md").write_text(
            f"# Grafikr runtime for {npm_target}\n\nPlatform runtime bundle for the matching `@grafikr/ui` package.\n",
            encoding="utf-8",
        )
        packed = _tool("npm", "pack", "--json", "--pack-destination", str(output), cwd=package_dir)
        details = json.loads(packed.stdout)
        if len(details) != 1:
            raise RuntimeError("npm pack did not create exactly one runtime package")
        artifact = output / details[0]["filename"]
        expected = {"package/runtime/manifest.json"}
        expected.update(f"package/runtime/{entry['path']}" for entry in manifest["files"])
        with tarfile.open(artifact, "r:gz") as archive:
            members = {item.name: item for item in archive.getmembers() if item.isfile()}
            if not expected.issubset(members):
                raise RuntimeError("runtime package is missing a manifest-listed file")
            packed_manifest = json.loads(archive.extractfile(members["package/runtime/manifest.json"]).read())
            if packed_manifest != manifest:
                raise RuntimeError("packed Node runtime manifest differs from the verified bundle")
            for entry in manifest["files"]:
                path = f"package/runtime/{entry['path']}"
                data = archive.extractfile(members[path]).read()
                if len(data) != entry["size"]:
                    raise RuntimeError(f"packed runtime file size differs: {entry['path']}")
                if hashlib.sha256(data).hexdigest() != entry["sha256"]:
                    raise RuntimeError(f"packed runtime file checksum differs: {entry['path']}")
        return artifact


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        artifact = package(args.bundle, args.out)
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as error:
        parser.exit(1, f"Node runtime packaging failed: {error}\n")
    print(f"Node runtime package verified: {artifact}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
