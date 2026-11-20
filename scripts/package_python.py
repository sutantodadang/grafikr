"""Build one platform-specific Python wheel with a verified Grafikr runtime."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import tempfile
import sys
import tomllib
import zipfile
from pathlib import Path

from scripts.build_runtime import ROOT, _tool, install_bundle, validate_manifest

PYTHON_PROJECT = ROOT / "bindings/python"
PLATFORM_TAGS = {
    "x86_64-pc-windows-msvc": "win_amd64",
    "aarch64-pc-windows-msvc": "win_arm64",
    "x86_64-unknown-linux-gnu": "manylinux_2_35_x86_64",
    "aarch64-unknown-linux-gnu": "manylinux_2_35_aarch64",
    "x86_64-apple-darwin": "macosx_14_0_x86_64",
    "aarch64-apple-darwin": "macosx_14_0_arm64",
}


def wheel_platform_tag(target: str) -> str:
    try:
        return PLATFORM_TAGS[target]
    except KeyError as error:
        raise ValueError(f"unsupported Python wheel target: {target}") from error


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _verify_wheel(path: Path, target: str, manifest: dict) -> None:
    platform_tag = wheel_platform_tag(target)
    expected_prefix = f"grafikr_ui-{manifest['engine_version']}-py3-none-{platform_tag}.whl"
    if path.name != expected_prefix:
        raise ValueError(f"wheel platform tag mismatch: expected {expected_prefix}, got {path.name}")
    package_root = f"grafikr/_runtime/{target}/"
    with zipfile.ZipFile(path) as wheel:
        names = wheel.namelist()
        wheel_metadata = next((name for name in names if name.endswith(".dist-info/WHEEL")), None)
        if wheel_metadata is None:
            raise ValueError("built wheel has no WHEEL metadata")
        metadata = wheel.read(wheel_metadata).decode("utf-8")
        if f"Tag: py3-none-{platform_tag}" not in metadata.splitlines():
            raise ValueError("wheel WHEEL metadata has the wrong target tag")
        bundled_manifest_path = package_root + "manifest.json"
        bundled_manifest = json.loads(wheel.read(bundled_manifest_path))
        if bundled_manifest != manifest:
            raise ValueError("wheel runtime manifest differs from the verified bundle")
        for entry in manifest["files"]:
            member = package_root + entry["path"]
            contents = wheel.read(member)
            if len(contents) != entry["size"] or _sha256(contents) != entry["sha256"]:
                raise ValueError(f"wheel runtime file failed verification: {entry['path']}")


def package(bundle: Path, output: Path) -> Path:
    bundle = Path(bundle).resolve()
    manifest = validate_manifest(bundle)
    if manifest["development_fixture"]:
        raise ValueError("development fixture bundles cannot be packaged for Python")
    project_metadata = tomllib.loads((PYTHON_PROJECT / "pyproject.toml").read_text(encoding="utf-8"))
    project_version = project_metadata["project"]["version"]
    if project_version != manifest["engine_version"]:
        raise ValueError("Python SDK version does not match the runtime engine version")
    platform_tag = wheel_platform_tag(manifest["target"])
    workspace_target = ROOT / "target"
    workspace_target.mkdir(exist_ok=True)
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="grafikr-python-wheel-", dir=workspace_target) as temporary:
        staging = Path(temporary)
        project = staging / "project"
        ignore = shutil.ignore_patterns(".venv", "__pycache__", "*.pyc", "build", "dist", "*.egg-info", "tests")
        shutil.copytree(PYTHON_PROJECT, project, ignore=ignore)
        runtime_dir = project / "src/grafikr/_runtime" / manifest["target"]
        install_bundle(bundle, runtime_dir)
        (project / "setup.cfg").write_text(
            f"[bdist_wheel]\nplat_name = {platform_tag}\n", encoding="utf-8"
        )
        wheel_dir = staging / "wheels"
        wheel_dir.mkdir()
        _tool(
            sys.executable,
            "-m",
            "build",
            "--wheel",
            "--outdir",
            str(wheel_dir),
            str(project),
            cwd=project,
        )
        wheels = list(wheel_dir.glob("*.whl"))
        if len(wheels) != 1:
            raise RuntimeError(f"expected one Python wheel, found {len(wheels)}")
        built = wheels[0]
        expected_name = f"grafikr_ui-{project_version}-py3-none-{platform_tag}.whl"
        if built.name != expected_name:
            raise RuntimeError(f"unexpected Python wheel name: {built.name}")
        _verify_wheel(built, manifest["target"], manifest)
        destination = output / built.name
        if destination.exists():
            if _sha256(destination.read_bytes()) == _sha256(built.read_bytes()):
                return destination
            raise FileExistsError(f"refusing to overwrite a different wheel: {destination}")
        staged_output = output / f".{built.name}.tmp"
        shutil.copyfile(built, staged_output)
        os.replace(staged_output, destination)
        return destination


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        wheel = package(args.bundle, args.out)
    except (OSError, ValueError, RuntimeError) as error:
        parser.exit(1, f"Python wheel packaging failed: {error}\n")
    print(f"Python wheel verified: {wheel}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
