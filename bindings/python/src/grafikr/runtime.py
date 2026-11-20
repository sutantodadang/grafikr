"""Resolve and verify the native host shipped with a Python wheel."""

from __future__ import annotations

import hashlib
import json
import platform
import stat
import sys
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any

PACKAGE_RUNTIME_DIR = Path(__file__).resolve().parent / "_runtime"
MAX_MANIFEST_BYTES = 1024 * 1024
_ARCHES = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}
_MIN_OS = {
    "windows": "Windows 11 24H2 build 26100",
    "linux": "Ubuntu 22.04 / glibc 2.35",
    "macos": "macOS 14",
}


def target_triple() -> str:
    architecture = _ARCHES.get(platform.machine().lower())
    if architecture is None:
        raise ValueError(f"unsupported runtime architecture: {platform.machine()}")
    system = platform.system()
    if system == "Windows":
        return f"{architecture}-pc-windows-msvc"
    if system == "Darwin":
        return f"{architecture}-apple-darwin"
    if system == "Linux":
        libc, _ = platform.libc_ver()
        if libc.lower() != "glibc":
            raise ValueError("Grafikr runtime wheels require Linux with glibc")
        return f"{architecture}-unknown-linux-gnu"
    raise ValueError(f"unsupported runtime operating system: {system}")


def runtime_host_path(bundle: Path | None = None) -> Path:
    target = target_triple()
    root = PACKAGE_RUNTIME_DIR / target if bundle is None else Path(bundle)
    try:
        if root.is_symlink() or root.is_junction() or not root.is_dir():
            raise ValueError("runtime bundle directory is missing or linked")
        root = root.resolve(strict=True)
        manifest_path = root / "manifest.json"
        # Wheel installers may hard-link package files into their cache; hashes below still detect changes.
        metadata = manifest_path.stat(follow_symlinks=False)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > MAX_MANIFEST_BYTES:
            raise ValueError("runtime manifest is not a regular bounded file")
        if manifest_path.is_symlink() or manifest_path.is_junction():
            raise ValueError("runtime manifest must not be linked")
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"runtime bundle manifest is missing or invalid: {error}") from error
    if not isinstance(manifest, dict) or manifest.get("target") != target:
        raise ValueError(f"runtime bundle target does not match this machine ({target})")
    if manifest.get("development_fixture") is not False:
        raise ValueError("development fixture bundles cannot be used by the Python SDK")
    if not isinstance(manifest.get("min_os"), str) or not manifest["min_os"]:
        raise ValueError("runtime manifest has no minimum OS requirement")
    _check_minimum_os(manifest)

    files = manifest.get("files")
    if not isinstance(files, list):
        raise ValueError("runtime manifest file inventory is invalid")
    host_name = "grafikr-host.exe" if target.endswith("-msvc") else "grafikr-host"
    hosts = [entry for entry in files if isinstance(entry, dict) and entry.get("role") == "host"]
    if len(hosts) != 1:
        raise ValueError("runtime bundle must contain exactly one host executable")
    entry = hosts[0]
    relative = _safe_path(entry.get("path"))
    if relative.as_posix() != f"bin/{host_name}":
        raise ValueError("runtime manifest names an unexpected host executable")
    for file_entry in files:
        if not isinstance(file_entry, dict):
            raise ValueError("runtime manifest file entry is invalid")
        name = PurePosixPath(_safe_path(file_entry.get("path")).as_posix()).name
        if file_entry.get("role") == "fixture_host" or name in {"fixture-host", "fixture-host.exe"}:
            raise ValueError("runtime bundle contains a development fixture host")

    host = root.joinpath(*relative.parts)
    current = root
    for part in relative.parts:
        current /= part
        if current.is_symlink() or current.is_junction():
            raise ValueError("runtime host path contains a link")
    try:
        host.resolve(strict=True).relative_to(root)
        host_metadata = host.stat(follow_symlinks=False)
    except (OSError, ValueError) as error:
        raise ValueError(f"runtime host is missing or escapes its bundle: {error}") from error
    if not stat.S_ISREG(host_metadata.st_mode):
        raise ValueError("runtime host must be a regular file")
    if host_metadata.st_size != entry.get("size") or _sha256(host) != entry.get("sha256"):
        raise ValueError("runtime host size or SHA-256 does not match the manifest")
    return host.resolve(strict=True)


def _safe_path(value: Any) -> PurePosixPath:
    if not isinstance(value, str) or not value or "\\" in value or "\0" in value:
        raise ValueError("runtime manifest contains an unsafe file path")
    posix, windows = PurePosixPath(value), PureWindowsPath(value)
    if posix.is_absolute() or windows.is_absolute() or windows.drive or posix.as_posix() != value:
        raise ValueError("runtime manifest contains an unsafe file path")
    if any(part in {"", ".", ".."} for part in posix.parts):
        raise ValueError("runtime manifest contains an unsafe file path")
    return posix


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _check_minimum_os(manifest: dict[str, Any]) -> None:
    target = manifest["target"]
    if target.endswith("-msvc"):
        if manifest["min_os"] != _MIN_OS["windows"] or manifest.get("min_glibc") is not None:
            raise ValueError("runtime Windows minimum does not match the target")
        try:
            build = sys.getwindowsversion().build
        except AttributeError as error:
            raise ValueError("cannot determine Windows build number") from error
        minimum = 26100
        if build < minimum:
            raise ValueError(f"runtime requires Windows build {minimum} or newer")
    elif target.endswith("-apple-darwin"):
        if manifest["min_os"] != _MIN_OS["macos"] or manifest.get("min_glibc") is not None:
            raise ValueError("runtime macOS minimum does not match the target")
        version = tuple(int(part) for part in platform.mac_ver()[0].split(".")[:2] if part.isdigit())
        if not version or version < (14, 0):
            raise ValueError("runtime requires macOS 14 or newer")
    else:
        if manifest["min_os"] != _MIN_OS["linux"]:
            raise ValueError("runtime Linux minimum does not match the target")
        minimum = manifest.get("min_glibc")
        if not isinstance(minimum, str):
            raise ValueError("Linux runtime manifest has no glibc minimum")
        libc, current = platform.libc_ver()
        if libc.lower() != "glibc" or not current:
            raise ValueError("cannot determine the current glibc version")
        if _version_tuple(current) < _version_tuple(minimum):
            raise ValueError(f"runtime requires glibc {minimum} or newer")


def _version_tuple(version: str) -> tuple[int, ...]:
    try:
        return tuple(int(part) for part in version.split("."))
    except ValueError as error:
        raise ValueError(f"invalid version string: {version}") from error
