"""Validation helpers for cross-language release package indexes."""

from __future__ import annotations

import hashlib
import argparse
import json
import os
import re
import shutil
import stat
import tempfile
import tarfile
import tomllib
import zipfile
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any

from scripts.build_runtime import (
    ROOT,
    _source_dirty,
    _sha256 as file_sha256,
    _host_triple,
    _targets,
    _tool,
    install_bundle,
    validate_manifest,
)
from scripts.package_node_runtime import package as package_node_runtime
from scripts.package_python import package as package_python
from scripts.verify_jvm_package import (
    RUNTIME_CLASSIFIERS as JVM_RUNTIME_CLASSIFIERS,
    package_runtime as package_jvm_runtime,
    verify as package_jvm,
)
from scripts.verify_dotnet_package import RIDS as DOTNET_RIDS, verify as package_dotnet
from scripts.verify_go_package import MODULE as GO_MODULE, verify as package_go

NODE_PROJECT = ROOT / "bindings/node"
TARGETS_PATH = ROOT / "bindings/runtime/targets.json"
MATRIX_PATH = ROOT / "bindings/sdk-matrix.json"
WORKSPACE_PATH = ROOT / "Cargo.toml"
WHEEL_TAGS = {
    "x86_64-pc-windows-msvc": "win_amd64",
    "aarch64-pc-windows-msvc": "win_arm64",
    "x86_64-unknown-linux-gnu": "manylinux_2_35_x86_64",
    "aarch64-unknown-linux-gnu": "manylinux_2_35_aarch64",
    "x86_64-apple-darwin": "macosx_14_0_x86_64",
    "aarch64-apple-darwin": "macosx_14_0_arm64",
}
NODE_TARGETS = {
    "x86_64-pc-windows-msvc": "win32-x64",
    "aarch64-pc-windows-msvc": "win32-arm64",
    "x86_64-unknown-linux-gnu": "linux-x64",
    "aarch64-unknown-linux-gnu": "linux-arm64",
    "x86_64-apple-darwin": "darwin-x64",
    "aarch64-apple-darwin": "darwin-arm64",
}
RUBY_PLATFORMS = {
    "x86_64-pc-windows-msvc": "x64-mingw-ucrt",
    "aarch64-pc-windows-msvc": "arm64-mingw-ucrt",
    "x86_64-unknown-linux-gnu": "x86_64-linux-gnu",
    "aarch64-unknown-linux-gnu": "aarch64-linux-gnu",
    "x86_64-apple-darwin": "x86_64-darwin-23",
    "aarch64-apple-darwin": "arm64-darwin-23",
}


def _ruby_platform_for_target(target: str) -> str:
    try:
        return RUBY_PLATFORMS[target]
    except KeyError as error:
        raise ValueError(f"unsupported Ruby gem target: {target}") from error


def _php_runtime_slug(target: str) -> str:
    try:
        return NODE_TARGETS[target].replace("win32-", "windows-").replace("darwin-", "macos-")
    except KeyError as error:
        raise ValueError(f"unsupported PHP runtime target: {target}") from error

def validate_metadata(
    version: str,
    engine_version: str,
    sdk_versions: list[str],
    manifests: list[dict[str, Any]],
) -> None:
    if not sdk_versions or not manifests:
        raise ValueError("release SDK and runtime inventory must not be empty")
    if engine_version != version or any(item != version for item in sdk_versions):
        raise ValueError("engine and SDK package versions must match the requested release version")
    for manifest in manifests:
        if manifest.get("engine_version") != version:
            raise ValueError("runtime manifest engine version does not match the requested release version")
        if manifest.get("protocol") != {"major": 1, "minor": 0}:
            raise ValueError("protocol must remain version 1.0 for this release")
        if manifest.get("abi") != {"major": 1, "minor": 0}:
            raise ValueError("ABI must remain version 1.0 for this release")
        if manifest.get("development_fixture") is not False:
            raise ValueError("development fixture bundles cannot enter release packages")


def _package_path(value: Any) -> PurePosixPath:
    if not isinstance(value, str) or not value or "\\" in value or "\0" in value:
        raise ValueError(f"unsafe package artifact path: {value!r}")
    posix, windows = PurePosixPath(value), PureWindowsPath(value)
    if (
        posix.is_absolute()
        or windows.is_absolute()
        or windows.drive
        or posix.as_posix() != value
        or any(part in {"", ".", ".."} for part in posix.parts)
    ):
        raise ValueError(f"unsafe package artifact path: {value!r}")
    return posix


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _check_archive_members(
    path: Path,
    expected_version: str | None = None,
    expected_target: str | None = None,
    expected_sdk: str | None = None,
) -> None:
    archive_suffixes = (".zip", ".whl", ".nupkg", ".tgz", ".gem", ".tar.gz", ".jar", ".rock")
    if not path.name.lower().endswith(archive_suffixes):
        return
    names: set[str] = set()
    member_names: set[str] = set()
    manifests: list[bytes] = []
    composer_metadata: bytes | None = None
    runtime_manifest: bytes | None = None
    runtime_files: dict[str, tuple[int, str]] = {}

    def add_runtime_file(name: str, stream: Any) -> None:
        nonlocal runtime_manifest
        folded = name.casefold()
        if folded == "runtime/manifest.json":
            runtime_manifest = stream.read()
            manifests.append(runtime_manifest)
            return
        if not folded.startswith("runtime/"):
            return
        digest = hashlib.sha256()
        size = 0
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
            size += len(chunk)
        runtime_files[folded] = (size, digest.hexdigest())

    def check_name(name: str, is_link: bool = False) -> None:
        if is_link:
            raise ValueError(f"unsafe archive link: {path.name}:{name}")
        safe = name.rstrip("/")
        try:
            relative = _package_path(safe)
        except ValueError as error:
            raise ValueError(f"unsafe archive member: {path.name}:{name}") from error
        folded = relative.as_posix().casefold()
        if folded in names:
            raise ValueError(f"duplicate package archive member: {path.name}:{name}")
        names.add(folded)
        lower = folded
        if "fixture-host" in lower or "development-fixture" in lower:
            raise ValueError(f"development fixture leaked into package archive: {path.name}:{name}")

    if zipfile.is_zipfile(path):
        with zipfile.ZipFile(path) as archive:
            for member in archive.infolist():
                mode = member.external_attr >> 16
                check_name(member.filename, stat.S_ISLNK(mode))
                folded = member.filename.rstrip("/").casefold()
                member_names.add(folded)
                if folded == "composer.json":
                    composer_metadata = archive.read(member)
                if folded.endswith("manifest.json"):
                    manifests.append(archive.read(member))
                if folded.startswith("runtime/") and not member.is_dir():
                    with archive.open(member) as stream:
                        add_runtime_file(member.filename, stream)
    elif tarfile.is_tarfile(path):
        with tarfile.open(path, "r:*") as archive:
            for member in archive.getmembers():
                check_name(member.name, member.issym() or member.islnk() or not (member.isfile() or member.isdir()))
                member_names.add(member.name.rstrip("/").casefold())
                if member.isfile() and member.name.rstrip("/").endswith("manifest.json"):
                    stream = archive.extractfile(member)
                    if stream is not None:
                        with stream:
                            manifests.append(stream.read())
                if path.suffix.lower() == ".gem" and member.name == "data.tar.gz":
                    payload = archive.extractfile(member)
                    if payload is None:
                        raise ValueError(f"Ruby gem is missing its data archive: {path.name}")
                    with tarfile.open(fileobj=payload, mode="r|gz") as data_archive:
                        for data_member in data_archive:
                            check_name(
                                data_member.name,
                                data_member.issym()
                                or data_member.islnk()
                                or not (data_member.isfile() or data_member.isdir()),
                            )
                            folded = data_member.name.rstrip("/").casefold()
                            member_names.add(folded)
                            if data_member.isfile():
                                stream = data_archive.extractfile(data_member)
                                if stream is not None:
                                    with stream:
                                        if folded.endswith("manifest.json"):
                                            contents = stream.read()
                                            manifests.append(contents)
                                            if folded == "runtime/manifest.json":
                                                runtime_manifest = contents
                                        else:
                                            add_runtime_file(data_member.name, stream)
    else:
        raise ValueError(f"package archive is corrupt or unsupported: {path.name}")
    for contents in manifests:
        try:
            manifest = json.loads(contents)
        except (UnicodeDecodeError, json.JSONDecodeError):
            continue
        if isinstance(manifest, dict) and manifest.get("development_fixture") is True:
            raise ValueError(f"development fixture manifest leaked into package archive: {path.name}")
        if (
            isinstance(manifest, dict)
            and expected_version is not None
            and manifest.get("engine_version") is not None
            and manifest["engine_version"] != expected_version
        ):
            raise ValueError(f"stale runtime manifest inside package archive: {path.name}")
        if (
            isinstance(manifest, dict)
            and expected_target not in (None, "source")
            and manifest.get("target") is not None
            and manifest["target"] != expected_target
        ):
            raise ValueError(f"runtime manifest target mismatch inside package archive: {path.name}")
    if expected_sdk == "dotnet":
        rid = DOTNET_RIDS.get(expected_target or "")
        expected_manifest = f"runtimes/{rid}/native/manifest.json".casefold() if rid else ""
        if expected_manifest not in member_names:
            raise ValueError(f".NET package is missing the runtime for {expected_target}: {path.name}")
    if expected_sdk in {"java", "kotlin"} and expected_target not in (None, "source"):
        if "runtime/manifest.json" not in member_names:
            raise ValueError(f"JVM runtime package is missing its manifest: {path.name}")
    if expected_sdk == "php" and expected_target not in (None, "source"):
        if "runtime/manifest.json" not in member_names:
            raise ValueError(f"PHP runtime package is missing its manifest: {path.name}")
    if expected_sdk == "php":
        try:
            package_data = json.loads(composer_metadata) if composer_metadata is not None else None
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise ValueError(f"PHP Composer package metadata is invalid: {path.name}") from error
        expected_name = (
            "grafikr/ui"
            if expected_target == "source"
            else f"grafikr/runtime-{_php_runtime_slug(expected_target or '')}"
        )
        if (
            not isinstance(package_data, dict)
            or package_data.get("name") != expected_name
            or package_data.get("version") != expected_version
        ):
            raise ValueError(f"PHP Composer package metadata mismatch: {path.name}")
        if expected_target != "source":
            extra = package_data.get("extra")
            grafikr_extra = extra.get("grafikr") if isinstance(extra, dict) else None
            if not isinstance(grafikr_extra, dict) or grafikr_extra.get("runtime_target") != expected_target:
                raise ValueError(f"PHP Composer runtime target metadata mismatch: {path.name}")
    if expected_sdk == "ruby" or (expected_sdk == "php" and expected_target not in (None, "source")):
        if runtime_manifest is None:
            raise ValueError(f"runtime package is missing its embedded manifest: {path.name}")
        try:
            runtime_data = json.loads(runtime_manifest)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise ValueError(f"runtime package manifest is invalid: {path.name}") from error
        if (
            not isinstance(runtime_data, dict)
            or runtime_data.get("engine_version") != expected_version
            or runtime_data.get("target") != expected_target
            or runtime_data.get("development_fixture") is not False
            or not isinstance(runtime_data.get("files"), list)
        ):
            raise ValueError(f"runtime package manifest metadata mismatch: {path.name}")
        expected_files: set[str] = set()
        for entry in runtime_data["files"]:
            if not isinstance(entry, dict):
                raise ValueError(f"runtime package manifest entry is invalid: {path.name}")
            relative = _package_path(entry.get("path")).as_posix()
            member = f"runtime/{relative}".casefold()
            if member in expected_files:
                raise ValueError(f"duplicate runtime manifest path: {path.name}:{relative}")
            expected_files.add(member)
            if runtime_files.get(member) != (entry.get("size"), entry.get("sha256")):
                raise ValueError(f"runtime checksum or size mismatch: {path.name}:{relative}")


def _check_package_filename(package: dict[str, Any], version: str) -> None:
    sdk, target, name = package["sdk"], package["target"], PurePosixPath(package["path"]).name
    expected = None
    if sdk == "python" and target in WHEEL_TAGS:
        expected = f"grafikr_ui-{version}-py3-none-{WHEEL_TAGS[target]}.whl"
        message = "Python wheel target"
    elif sdk in {"c", "cpp"} and target in NODE_TARGETS:
        expected = f"grafikr-native-{target}.zip"
        message = "native archive target"
    elif sdk == "dotnet" and target in DOTNET_RIDS:
        expected = f"Grafikr.Client.{version}.nupkg"
        message = ".NET package target"
    elif sdk in {"java", "kotlin"}:
        if target == "source":
            prefix = f"grafikr-jvm-{version}"
            allowed = {f"{prefix}.jar", f"{prefix}.pom", f"{prefix}.module"}
        elif target in JVM_RUNTIME_CLASSIFIERS:
            prefix = f"grafikr-runtime-{version}"
            allowed = {
                f"{prefix}-{JVM_RUNTIME_CLASSIFIERS[target]}.jar",
                f"{prefix}.pom",
                f"{prefix}.module",
            }
        else:
            prefix = ""
            allowed = set()
        if prefix and name not in allowed:
            expected = sorted(allowed)[0]
        message = "JVM package target"
    elif sdk in {"javascript", "typescript"}:
        if target == "source":
            expected = f"grafikr-ui-{version}.tgz"
        elif target in NODE_TARGETS:
            expected = f"grafikr-runtime-{NODE_TARGETS[target]}-{version}.tgz"
        message = "Node package target"
    elif sdk == "ruby" and target in RUBY_PLATFORMS:
        expected = f"grafikr-ui-{version}-{RUBY_PLATFORMS[target]}.gem"
        message = "Ruby gem target"
    elif sdk == "php":
        if target == "source":
            expected = f"grafikr-php-{version}.zip"
        elif target in NODE_TARGETS:
            expected = f"grafikr-php-runtime-{version}-{_php_runtime_slug(target)}.zip"
        message = "PHP Composer package target"
    elif target == "source" and sdk in {"swift", "dart", "lua"}:
        expected = {
            "swift": f"grafikr-swift-{version}.zip",
            "dart": f"grafikr-dart-{version}.tar.gz",
            "lua": f"grafikr-{version}-1.src.rock",
        }[sdk]
        message = f"{sdk} source package"
    else:
        return
    if expected is not None and name != expected:
        raise ValueError(f"{message} mismatch: expected {expected}, got {name}")


def _validate_jvm_publications(packages: list[dict[str, Any]], version: str) -> None:
    grouped: dict[tuple[str, str], set[str]] = {}
    for package in packages:
        if package.get("sdk") in {"java", "kotlin"}:
            grouped.setdefault((package["sdk"], package["target"]), set()).add(
                PurePosixPath(package["path"]).name
            )
    for identity, names in grouped.items():
        sdk, target = identity
        if target != "source" and target not in JVM_RUNTIME_CLASSIFIERS:
            raise ValueError(f"JVM publication has an unsupported target: {sdk}/{target}")
        base = f"grafikr-jvm-{version}" if target == "source" else f"grafikr-runtime-{version}"
        jar = f"{base}.jar" if target == "source" else f"{base}-{JVM_RUNTIME_CLASSIFIERS[target]}.jar"
        if jar not in names or f"{base}.pom" not in names:
            raise ValueError(f"JVM publication {sdk}/{target} requires its JAR and Maven POM")


def validate_package_index(
    index: dict[str, Any],
    package_root: Path,
    version: str,
    *,
    allow_incomplete: bool = False,
) -> None:
    if (
        not isinstance(index, dict)
        or index.get("format") != "grafikr-bindings-package-index-v1"
        or index.get("version") != version
        or index.get("protocol") != {"major": 1, "minor": 0}
        or index.get("abi") != {"major": 1, "minor": 0}
        or type(index.get("complete")) is not bool
        or type(index.get("source_dirty")) is not bool
        or not isinstance(index.get("pending"), list)
        or index["complete"] != (not index["pending"] and not index["source_dirty"])
        or not isinstance(index.get("packages"), list)
        or not index["packages"]
    ):
        raise ValueError("package index has invalid release metadata or completion status")
    root = Path(package_root)
    if root.is_symlink() or not root.is_dir():
        raise ValueError("package output root must be a regular directory")
    root = root.resolve()
    seen: set[tuple[str, str | None, str]] = set()
    for package in index["packages"]:
        if not isinstance(package, dict):
            raise ValueError("package index entry must be an object")
        identity = (package.get("sdk"), package.get("target"))
        if not isinstance(identity[0], str) or not identity[0]:
            raise ValueError(f"invalid package identity: {identity}")
        if package.get("engine_version") != version or package.get("development_fixture") is not False:
            raise ValueError(f"package metadata is incompatible with release {version}: {identity}")
        relative = _package_path(package.get("path"))
        artifact_identity = (identity[0], identity[1], relative.as_posix())
        if artifact_identity in seen:
            raise ValueError(f"duplicate package artifact identity: {artifact_identity}")
        seen.add(artifact_identity)
        _check_package_filename(package, version)
        artifact = root.joinpath(*relative.parts)
        current = root
        for part in relative.parts:
            current = current / part
            if current.is_symlink():
                raise ValueError(f"package artifact path contains a symlink: {relative}")
        try:
            resolved = artifact.resolve()
            resolved.relative_to(root)
            metadata = artifact.stat(follow_symlinks=False)
        except (OSError, ValueError) as error:
            raise ValueError(f"package artifact is missing or escapes output root: {relative}") from error
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise ValueError(f"package artifact is not a regular unlinked file: {relative}")
        expected_hash = package.get("sha256")
        if not isinstance(expected_hash, str) or _sha256(artifact) != expected_hash:
            raise ValueError(f"package artifact hash mismatch: {relative}")
        _check_archive_members(artifact, version, package.get("target"), package.get("sdk"))
    _validate_jvm_publications(index["packages"], version)
    if not allow_incomplete and (index["pending"] or index["source_dirty"]):
        raise ValueError(
            f"package index is not release-clean: {len(index['pending'])} pending packages, "
            f"source_dirty={index['source_dirty']}"
        )


def build_package_index(
    version: str,
    packages: list[dict[str, Any]],
    pending: list[dict[str, str]],
    runtimes: list[dict[str, str]],
    *,
    source_dirty: bool = False,
) -> dict[str, Any]:
    return {
        "format": "grafikr-bindings-package-index-v1",
        "version": version,
        "protocol": {"major": 1, "minor": 0},
        "abi": {"major": 1, "minor": 0},
        "complete": not pending and not source_dirty,
        "source_dirty": source_dirty,
        "packages": packages,
        "pending": pending,
        "runtimes": runtimes,
    }


def _release_metadata(version: str) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    workspace = tomllib.loads(WORKSPACE_PATH.read_text(encoding="utf-8"))
    matrix = json.loads(MATRIX_PATH.read_text(encoding="utf-8"))
    sdk_rows = matrix.get("sdks")
    if not isinstance(sdk_rows, list) or not sdk_rows:
        raise ValueError("SDK matrix has no package inventory")
    sdk_versions = [row.get("sdk_version") for row in sdk_rows if isinstance(row, dict)]
    if len(sdk_versions) != len(sdk_rows):
        raise ValueError("SDK matrix contains an invalid package row")
    engine_version = workspace["workspace"]["package"]["version"]
    _source_sdk_versions(expected_version=engine_version)
    return {"engine_version": engine_version, "sdk_versions": sdk_versions}, sdk_rows


def _source_sdk_versions(root: Path = ROOT, expected_version: str | None = None) -> dict[str, str]:
    gemspec = (Path(root) / "bindings/ruby/grafikr.gemspec").read_text(encoding="utf-8")
    ruby_version = re.search(r'(?m)^\s*s\.version\s*=\s*["\']([^"\']+)["\']', gemspec)
    if ruby_version is None:
        raise ValueError("Ruby gemspec does not declare a package version")
    composer = json.loads((Path(root) / "bindings/php/composer.json").read_text(encoding="utf-8"))
    php_version = composer.get("version") if isinstance(composer, dict) else None
    if not isinstance(php_version, str) or not php_version:
        raise ValueError("PHP Composer metadata does not declare a package version")
    versions = {"ruby": ruby_version.group(1), "php": php_version}
    if expected_version is not None:
        for sdk, package_version in versions.items():
            if package_version != expected_version:
                raise ValueError(
                    f"{sdk.title()} package version {package_version} "
                    f"does not match engine version {expected_version}"
                )
    return versions


def _load_runtimes(runtime_root: Path) -> tuple[dict[str, Path], dict[str, dict[str, Any]]]:
    root = Path(runtime_root)
    if root.is_symlink() or not root.is_dir():
        raise ValueError("runtime root must be a regular directory")
    bundles: dict[str, Path] = {}
    manifests: dict[str, dict[str, Any]] = {}
    for target in _targets():
        bundle = root / target
        if not bundle.exists():
            continue
        manifest = validate_manifest(bundle, target)
        bundles[target] = bundle
        manifests[target] = manifest
    if not manifests:
        raise ValueError("runtime root contains no target-named verified runtime bundles")
    return bundles, manifests


def _record(sdk: str, target: str, artifact: Path, output: Path, version: str) -> dict[str, Any]:
    _check_archive_members(artifact, version, target, sdk)
    record = {
        "sdk": sdk,
        "target": target,
        "path": artifact.relative_to(output).as_posix(),
        "sha256": _sha256(artifact),
        "engine_version": version,
        "development_fixture": False,
    }
    _check_package_filename(record, version)
    return record


def _pack_source_snapshot(files: list[tuple[Path, str]], output: Path, archive_name: str) -> Path:
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    artifact = output / archive_name
    if artifact.exists():
        raise FileExistsError(f"refusing to overwrite source package: {artifact}")
    entries: list[tuple[Path, str]] = []
    seen: set[str] = set()
    for source, member_name in files:
        relative = _package_path(member_name).as_posix()
        if relative.casefold() in seen:
            raise ValueError(f"duplicate source package path: {relative}")
        seen.add(relative.casefold())
        metadata = Path(source).stat(follow_symlinks=False)
        if Path(source).is_symlink() or not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise ValueError(f"source package input must be a regular unlinked file: {source}")
        entries.append((Path(source), relative))
    if not entries:
        raise ValueError("source package file list cannot be empty")
    if archive_name.lower().endswith((".tar.gz", ".tgz")):
        with tarfile.open(artifact, "x:gz") as archive:
            for source, member_name in entries:
                archive.add(source, arcname=member_name, recursive=False)
    else:
        with zipfile.ZipFile(artifact, "x", compression=zipfile.ZIP_DEFLATED) as archive:
            for source, member_name in entries:
                archive.write(source, member_name)
    _check_archive_members(artifact)
    return artifact


def _source_sdk_files(sdk: str, version: str) -> list[tuple[Path, str]]:
    project = ROOT / "bindings" / sdk
    patterns = {
        "swift": [
            ("Package.swift", True),
            ("Package.resolved", False),
            ("README.md", True),
            ("LICENSE", True),
            ("ThirdPartyNotices/*.txt", True),
            ("Sources/**/*.swift", True),
        ],
        "dart": [("pubspec.yaml", True), ("README.md", True), ("lib/**/*.dart", True), ("bin/**/*.dart", True), ("example/**/*.dart", True)],
        "php": [("composer.json", True), ("README.md", True), ("src/**/*.php", True), ("bin/**/*.php", True), ("examples/**/*.php", True)],
        "lua": [(f"grafikr-{version}-1.rockspec", True), ("README.md", True), ("src/**/*.lua", True)],
    }
    if sdk not in patterns:
        raise ValueError(f"source package adapter is not implemented for {sdk}")
    files: dict[str, tuple[Path, str]] = {}
    for pattern, required in patterns[sdk]:
        matches = sorted(project.glob(pattern)) if "*" in pattern else [project / pattern]
        matches = [path for path in matches if path.is_file()]
        if required and not matches:
            raise ValueError(f"{sdk} source package input is missing: {pattern}")
        for path in matches:
            member = path.relative_to(project).as_posix()
            files[member] = (path, member)
    license_path = ROOT / "LICENSE"
    if license_path.is_file():
        files["LICENSE"] = (license_path, "LICENSE")
    return [files[key] for key in sorted(files)]


def _ruby_sdk_files() -> list[tuple[Path, str]]:
    project = ROOT / "bindings/ruby"
    required = [project / "grafikr.gemspec", project / "README.md"]
    files = [(path, path.relative_to(project).as_posix()) for path in required]
    files.extend(
        (path, path.relative_to(project).as_posix())
        for path in sorted((project / "lib").rglob("*.rb"))
        if path.is_file()
    )
    files.extend(
        (path, path.relative_to(project).as_posix())
        for path in sorted((project / "examples").rglob("*.rb"))
        if path.is_file()
    )
    files.append((ROOT / "LICENSE", "LICENSE"))
    for path, _ in files:
        if not path.is_file() or path.is_symlink():
            raise ValueError(f"Ruby package input is missing or unsafe: {path}")
    return files


def _pack_ruby_gem(bundle: Path, output: Path, version: str) -> Path:
    manifest = validate_manifest(bundle)
    target = manifest["target"]
    platform = _ruby_platform_for_target(target)
    artifact = Path(output).resolve() / f"grafikr-ui-{version}-{platform}.gem"
    artifact.parent.mkdir(parents=True, exist_ok=True)
    if artifact.exists():
        raise FileExistsError(f"refusing to overwrite Ruby gem: {artifact}")
    with tempfile.TemporaryDirectory(prefix="grafikr-ruby-gem-", dir=artifact.parent) as temporary:
        stage = Path(temporary)
        for source, member in _ruby_sdk_files():
            destination = stage / member
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
        install_bundle(bundle, stage / "runtime")
        _tool(
            "gem", "build", "grafikr.gemspec", "--platform", platform,
            "--output", str(artifact), cwd=stage,
        )
    if not artifact.is_file():
        raise RuntimeError(f"RubyGems did not create the package: {artifact.name}")
    _check_archive_members(artifact, version, target, "ruby")
    return artifact


def _pack_php_runtime(bundle: Path, output: Path, version: str) -> Path:
    manifest = validate_manifest(bundle)
    target = manifest["target"]
    slug = _php_runtime_slug(target)
    artifact = Path(output) / f"grafikr-php-runtime-{version}-{slug}.zip"
    artifact.parent.mkdir(parents=True, exist_ok=True)
    if artifact.exists():
        raise FileExistsError(f"refusing to overwrite PHP runtime package: {artifact}")
    package = {
        "name": f"grafikr/runtime-{slug}",
        "version": version,
        "type": "library",
        "license": "AGPL-3.0-only",
        "require": {"php": ">=8.4"},
        "extra": {"grafikr": {"runtime_target": target}},
    }
    with zipfile.ZipFile(artifact, "x", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("composer.json", json.dumps(package, indent=2) + "\n")
        archive.write(bundle / "manifest.json", "runtime/manifest.json")
        for entry in manifest["files"]:
            archive.write(bundle / entry["path"], f"runtime/{entry['path']}")
        archive.write(ROOT / "LICENSE", "LICENSE")
    _check_archive_members(artifact, version, target, "php")
    return artifact


def _archive_native_bundle(bundle: Path, manifest: dict[str, Any], output: Path) -> Path:
    output.mkdir(parents=True, exist_ok=True)
    artifact = output / f"grafikr-native-{manifest['target']}.zip"
    with zipfile.ZipFile(artifact, "x", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.write(bundle / "manifest.json", "manifest.json")
        for entry in manifest["files"]:
            archive.write(bundle / entry["path"], entry["path"])
    return artifact


def _pack_node_api(output: Path) -> Path:
    output.mkdir(parents=True, exist_ok=True)
    _tool("npm", "run", "build", cwd=NODE_PROJECT)
    packed = _tool("npm", "pack", "--json", "--pack-destination", str(output), cwd=NODE_PROJECT)
    details = json.loads(packed.stdout)
    if len(details) != 1:
        raise RuntimeError("npm pack did not create exactly one Node SDK package")
    artifact = output / details[0]["filename"]
    if not artifact.is_file():
        raise RuntimeError("npm pack reported a missing Node SDK artifact")
    return artifact


def _consumer_runtime_bundle(bundles: dict[str, Path]) -> Path:
    target = _host_triple()
    if target not in bundles:
        raise RuntimeError(f"package consumers require the native runner target; bundle missing: {target}")
    return bundles[target]


def _is_native_target(target: str, host_target: str) -> bool:
    return target == host_target


def _pending_sdk_targets(
    sdk_rows: list[dict[str, Any]],
    bundles: dict[str, Path],
    packages: list[dict[str, Any]],
    failures: dict[tuple[str, str], str] | None = None,
) -> list[dict[str, str]]:
    covered = {(item["sdk"], item["target"]) for item in packages}
    runtime_sdks = {"c", "cpp", "python", "javascript", "typescript", "dotnet", "java", "kotlin", "ruby", "php"}
    source_additional = {"javascript", "typescript", "java", "kotlin", "php"}
    targets = list(_targets())
    pending: list[dict[str, str]] = []
    failures = failures or {}
    for row in sdk_rows:
        sdk = row["name"]
        if sdk in source_additional:
            required_targets = targets + ["source"]
        elif sdk in runtime_sdks:
            required_targets = targets
        else:
            required_targets = ["source"]
        for target in required_targets:
            if (sdk, target) in covered:
                continue
            reason = failures.get((sdk, target)) or (
                "verified runtime bundle is unavailable"
                if target != "source" and target not in bundles
                else "ecosystem package adapter is not implemented"
            )
            pending.append({"sdk": sdk, "target": target, "reason": reason})
    return pending


def package_bindings(version: str, runtime_root: Path, output: Path) -> dict[str, Any]:
    metadata, sdk_rows = _release_metadata(version)
    bundles, manifests = _load_runtimes(runtime_root)
    validate_metadata(version, metadata["engine_version"], metadata["sdk_versions"], list(manifests.values()))
    output = Path(output).absolute()
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        raise FileExistsError(f"refusing to overwrite package output: {output}")

    with tempfile.TemporaryDirectory(prefix="grafikr-packages-", dir=output.parent) as temporary:
        stage = Path(temporary) / "packages"
        stage.mkdir()
        packages: list[dict[str, Any]] = []
        failures: dict[tuple[str, str], str] = {}
        consumer_target = _host_triple()
        runtimes = [
            {
                "target": target,
                "build_sha": manifest["build_sha"],
                "manifest_sha256": file_sha256(bundle / "manifest.json"),
            }
            for target, bundle in sorted(bundles.items())
            for manifest in [manifests[target]]
        ]
        for target, bundle in sorted(bundles.items()):
            manifest = manifests[target]
            try:
                native_archive = _archive_native_bundle(bundle, manifest, stage / "native")
                for sdk in ("c", "cpp"):
                    packages.append(_record(sdk, target, native_archive, stage, version))
            except (OSError, RuntimeError, ValueError) as error:
                for sdk in ("c", "cpp"):
                    failures[(sdk, target)] = f"native archive build failed: {error}"
            try:
                wheel = package_python(bundle, stage / "python")
                packages.append(_record("python", target, wheel, stage, version))
            except (OSError, RuntimeError, ValueError) as error:
                failures[("python", target)] = f"Python wheel build failed: {error}"
            try:
                dotnet_output = stage / "dotnet" / target
                package_dotnet(
                    bundle,
                    dotnet_output,
                    test_consumer=_is_native_target(target, consumer_target),
                )
                packages.append(_record("dotnet", target, next(dotnet_output.glob("*.nupkg")), stage, version))
            except (OSError, RuntimeError, ValueError) as error:
                failures[("dotnet", target)] = f"NuGet package build failed: {error}"
            try:
                runtime_package = package_node_runtime(bundle, stage / "node-runtime")
                for sdk in ("javascript", "typescript"):
                    packages.append(_record(sdk, target, runtime_package, stage, version))
            except (OSError, RuntimeError, ValueError) as error:
                for sdk in ("javascript", "typescript"):
                    failures[(sdk, target)] = f"Node runtime package build failed: {error}"
            try:
                ruby_gem = _pack_ruby_gem(bundle, stage / "ruby", version)
                packages.append(_record("ruby", target, ruby_gem, stage, version))
            except (OSError, RuntimeError, ValueError) as error:
                failures[("ruby", target)] = f"Ruby gem build failed: {error}"
            try:
                php_runtime = _pack_php_runtime(bundle, stage / "php", version)
                packages.append(_record("php", target, php_runtime, stage, version))
            except (OSError, RuntimeError, ValueError) as error:
                failures[("php", target)] = f"PHP runtime package build failed: {error}"
        try:
            node_api = _pack_node_api(stage / "node-sdk")
            for sdk in ("javascript", "typescript"):
                packages.append(_record(sdk, "source", node_api, stage, version))
        except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
            for sdk in ("javascript", "typescript"):
                failures[(sdk, "source")] = f"Node SDK package build failed: {error}"
        try:
            go_output = stage / "go"
            package_go(_consumer_runtime_bundle(bundles), go_output, f"v{version}")
            module_version_dir = go_output / GO_MODULE / "@v"
            for artifact in sorted(module_version_dir.iterdir()):
                if artifact.is_file():
                    packages.append(_record("go", "source", artifact, stage, version))
        except (OSError, RuntimeError, ValueError) as error:
            failures[("go", "source")] = f"Go module package build failed: {error}"
        for sdk, archive_name in (
            ("swift", f"grafikr-swift-{version}.zip"),
            ("dart", f"grafikr-dart-{version}.tar.gz"),
            ("php", f"grafikr-php-{version}.zip"),
            ("lua", f"grafikr-{version}-1.src.rock"),
        ):
            try:
                artifact = _pack_source_snapshot(_source_sdk_files(sdk, version), stage / sdk, archive_name)
                packages.append(_record(sdk, "source", artifact, stage, version))
            except (OSError, RuntimeError, ValueError) as error:
                failures[(sdk, "source")] = f"{sdk} source package build failed: {error}"
        try:
            jvm_repository = stage / "jvm-maven"
            package_jvm(_consumer_runtime_bundle(bundles), jvm_repository)
            versioned_jvm = jvm_repository / "dev/grafikr/grafikr-jvm" / version
            jvm_api = versioned_jvm / f"grafikr-jvm-{version}.jar"
            if not jvm_api.is_file():
                raise RuntimeError("JVM Maven repository is missing the versioned API JAR")
            jvm_api_files = [
                jvm_api,
                *versioned_jvm.glob(f"grafikr-jvm-{version}.pom"),
                *versioned_jvm.glob(f"grafikr-jvm-{version}.module"),
            ]
            for artifact in jvm_api_files:
                for sdk in ("java", "kotlin"):
                    packages.append(_record(sdk, "source", artifact, stage, version))
            for target, bundle in sorted(bundles.items()):
                runtime_artifact = package_jvm_runtime(bundle, jvm_repository)
                runtime_version_dir = runtime_artifact.parent
                publication_files = [
                    runtime_artifact,
                    *runtime_version_dir.glob(f"grafikr-runtime-{version}.pom"),
                    *runtime_version_dir.glob(f"grafikr-runtime-{version}.module"),
                ]
                for sdk in ("java", "kotlin"):
                    packages.extend(_record(sdk, target, artifact, stage, version) for artifact in publication_files)
        except (OSError, RuntimeError, ValueError) as error:
            failures[("java", "source")] = f"JVM API package build failed: {error}"
            failures[("kotlin", "source")] = f"JVM API package build failed: {error}"
            for target in bundles:
                failures[("java", target)] = f"JVM runtime package build failed: {error}"
                failures[("kotlin", target)] = f"JVM runtime package build failed: {error}"
        pending = _pending_sdk_targets(sdk_rows, bundles, packages, failures)
        source_dirty = _source_dirty() or any(manifest.get("source_dirty") for manifest in manifests.values())
        if source_dirty:
            pending.append({
                "sdk": "release",
                "target": "source",
                "reason": "release artifacts must be rebuilt from a clean committed source tree",
            })
        index = build_package_index(version, packages, pending, runtimes, source_dirty=source_dirty)
        (stage / "package-index.json").write_text(json.dumps(index, indent=2) + "\n", encoding="utf-8")
        os.replace(stage, output)
    return index


def check_packages(version: str, runtime_root: Path, output: Path) -> dict[str, Any]:
    metadata, sdk_rows = _release_metadata(version)
    bundles, manifests = _load_runtimes(runtime_root)
    validate_metadata(version, metadata["engine_version"], metadata["sdk_versions"], list(manifests.values()))
    index_path = Path(output) / "package-index.json"
    index = json.loads(index_path.read_text(encoding="utf-8"))
    validate_package_index(index, output, version, allow_incomplete=True)
    current_runtime_hashes = {
        target: file_sha256(bundle / "manifest.json") for target, bundle in bundles.items()
    }
    recorded_runtime_hashes = {entry["target"]: entry["manifest_sha256"] for entry in index.get("runtimes", [])}
    if any(recorded_runtime_hashes.get(target) != digest for target, digest in current_runtime_hashes.items()):
        raise ValueError("package index runtime manifests are stale")
    expected_targets = set(_targets())
    if set(bundles) != expected_targets:
        missing = sorted(expected_targets - set(bundles))
        raise ValueError(f"package check requires all six runtime bundles; missing: {', '.join(missing)}")
    if current_runtime_hashes != recorded_runtime_hashes:
        raise ValueError("package index runtime manifests are stale or incomplete")
    if index["pending"] or index["source_dirty"]:
        raise ValueError(
            f"package index is not release-clean: {len(index['pending'])} pending packages, "
            f"source_dirty={index['source_dirty']}"
        )
    required = set()
    runtime_sdks = {"c", "cpp", "python", "javascript", "typescript", "dotnet", "java", "kotlin", "ruby", "php"}
    source_additional = {"javascript", "typescript", "java", "kotlin", "php"}
    for row in sdk_rows:
        sdk = row["name"]
        required_targets = list(_targets()) if sdk in runtime_sdks else []
        if sdk in source_additional or sdk not in runtime_sdks:
            required_targets.append("source")
        required.update((sdk, target) for target in required_targets)
    actual = {(item["sdk"], item["target"]) for item in index["packages"]}
    if actual != required:
        missing = sorted(required - actual)
        extra = sorted(actual - required)
        raise ValueError(f"package index coverage mismatch (missing={missing}, extra={extra})")
    go_artifacts = {
        Path(item["path"]).suffix.lower()
        for item in index["packages"]
        if item["sdk"] == "go" and item["target"] == "source"
    }
    if go_artifacts and not {".zip", ".mod", ".info"}.issubset(go_artifacts):
        raise ValueError("Go package index is missing its .zip, .mod, or .info module artifact")
    return index


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--runtime-root", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--check", action="store_true", help="verify an existing package index and all package hashes")
    args = parser.parse_args(argv)
    try:
        if args.check:
            index = check_packages(args.version, args.runtime_root, args.out)
            print(f"package index verified: {len(index['packages'])} artifacts, complete")
            return 0
        index = package_bindings(args.version, args.runtime_root, args.out)
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as error:
        parser.exit(1, f"binding packaging failed: {error}\n")
    print(
        f"package index written: {len(index['packages'])} artifacts, "
        f"{len(index['pending'])} pending; complete={index['complete']}"
    )
    return 0 if index["complete"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
