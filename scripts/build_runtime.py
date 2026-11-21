"""Build and verify a self-contained Grafikr runtime bundle."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import stat
import struct
import subprocess
import tempfile
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib

from jsonschema import Draft202012Validator


ROOT = Path(__file__).resolve().parents[1]
TARGETS_PATH = ROOT / "bindings/runtime/targets.json"
MANIFEST_SCHEMA_PATH = ROOT / "bindings/runtime/manifest.schema.json"
MANIFEST_PATH = "manifest.json"


def _json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def _targets() -> dict[str, dict[str, Any]]:
    data = _json(TARGETS_PATH)
    return {item["triple"]: item for item in data["targets"]}


def _safe_member_path(value: str, allow_manifest: bool = False) -> PurePosixPath:
    if not isinstance(value, str) or not value or "\\" in value or "\0" in value:
        raise ValueError(f"unsafe manifest path: {value!r}")
    posix = PurePosixPath(value)
    windows = PureWindowsPath(value)
    if posix.is_absolute() or windows.is_absolute() or windows.drive:
        raise ValueError(f"absolute manifest path is forbidden: {value!r}")
    if (
        posix.as_posix() != value
        or any(part in ("", ".", "..") for part in posix.parts)
        or (value == MANIFEST_PATH and not allow_manifest)
    ):
        raise ValueError(f"unsafe manifest path: {value!r}")
    return posix


def _is_link_or_junction(path: Path) -> bool:
    is_junction = getattr(path, "is_junction", None)
    return path.is_symlink() or (is_junction is not None and is_junction())


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _binary_platform_arch(path: Path) -> tuple[str, str]:
    data = path.read_bytes()
    if data.startswith(b"MZ") and len(data) >= 0x40:
        pe_offset = struct.unpack_from("<I", data, 0x3C)[0]
        if len(data) >= pe_offset + 6 and data[pe_offset : pe_offset + 4] == b"PE\0\0":
            machine = struct.unpack_from("<H", data, pe_offset + 4)[0]
            architectures = {0x8664: "x86_64", 0xAA64: "aarch64"}
            if machine in architectures:
                return "windows", architectures[machine]
    if data.startswith(b"\x7fELF") and len(data) >= 20:
        endian = "<" if data[5] == 1 else ">" if data[5] == 2 else None
        if endian:
            machine = struct.unpack_from(f"{endian}H", data, 18)[0]
            architectures = {62: "x86_64", 183: "aarch64"}
            if machine in architectures:
                return "linux", architectures[machine]
    mach_o = {
        b"\xfe\xed\xfa\xce": ">",
        b"\xce\xfa\xed\xfe": "<",
        b"\xfe\xed\xfa\xcf": ">",
        b"\xcf\xfa\xed\xfe": "<",
    }
    endian = mach_o.get(data[:4])
    if endian and len(data) >= 8:
        cpu_type = struct.unpack_from(f"{endian}I", data, 4)[0] & 0xFFFFFFFF
        architectures = {0x01000007: "x86_64", 0x0100000C: "aarch64"}
        if cpu_type in architectures:
            return "macos", architectures[cpu_type]
    raise ValueError(f"cannot identify runtime binary OS/architecture: {path}")


def validate_manifest(bundle: Path, expected_target: str | None = None) -> dict[str, Any]:
    bundle = Path(bundle)
    if _is_link_or_junction(bundle) or not bundle.is_dir():
        raise ValueError("bundle root must be a regular directory")
    root = bundle.resolve()
    manifest_path = root / MANIFEST_PATH
    if _is_link_or_junction(manifest_path) or not manifest_path.is_file():
        raise ValueError("bundle manifest is missing or is not a regular file")
    if manifest_path.stat().st_nlink != 1:
        raise ValueError("bundle manifest must not be hard-linked")
    manifest = _json(manifest_path)
    errors = list(Draft202012Validator(_json(MANIFEST_SCHEMA_PATH)).iter_errors(manifest))
    if errors:
        error = min(errors, key=lambda item: len(item.absolute_path))
        raise ValueError(f"invalid bundle manifest at {list(error.absolute_path)}: {error.message}")

    targets = _targets()
    target = manifest["target"]
    target_info = targets.get(target)
    if target_info is None:
        raise ValueError(f"unsupported runtime target: {target}")
    if expected_target is not None and target != expected_target:
        raise ValueError(f"bundle target {target} does not match expected target {expected_target}")
    if manifest["min_os"] != target_info["min_os"] or manifest["min_glibc"] != target_info["min_glibc"]:
        raise ValueError("manifest minimum OS does not match the target matrix")

    seen: set[str] = set()
    host_paths: list[Path] = []
    for entry in manifest["files"]:
        relative = _safe_member_path(entry["path"])
        collision_key = relative.as_posix().casefold()
        if collision_key in seen:
            raise ValueError(f"duplicate manifest path: {entry['path']}")
        seen.add(collision_key)
        member = root.joinpath(*relative.parts)
        current = root
        for part in relative.parts:
            current = current / part
            if _is_link_or_junction(current):
                raise ValueError(f"link or junction in runtime bundle: {entry['path']}")
        resolved = member.resolve()
        try:
            resolved.relative_to(root)
        except ValueError as error:
            raise ValueError(f"manifest path escapes bundle root: {entry['path']}") from error
        try:
            metadata = member.stat(follow_symlinks=False)
        except OSError as error:
            raise ValueError(f"manifest file is missing: {entry['path']}") from error
        if not stat.S_ISREG(metadata.st_mode):
            raise ValueError(f"manifest entry is not a regular file: {entry['path']}")
        if metadata.st_nlink != 1:
            raise ValueError(f"hard-linked runtime file is forbidden: {entry['path']}")
        if metadata.st_size != entry["size"] or _sha256(member) != entry["sha256"]:
            raise ValueError(f"runtime file size or hash mismatch: {entry['path']}")
        if entry["role"] == "host":
            host_paths.append(member)

    if len(host_paths) != 1:
        raise ValueError("bundle must contain exactly one host executable")
    fixture_paths = [
        entry
        for entry in manifest["files"]
        if entry["role"] == "fixture_host"
        or PurePosixPath(entry["path"]).name in {"fixture-host", "fixture-host.exe"}
    ]
    if bool(fixture_paths) != manifest["development_fixture"] or len(fixture_paths) > 1:
        raise ValueError("development fixture marker and fixture host file do not match")
    if fixture_paths and fixture_paths[0]["role"] != "fixture_host":
        raise ValueError("fixture-host executable has an invalid manifest role")
    for entry in manifest["files"]:
        if entry["role"] not in {"host", "fixture_host", "native_example"}:
            continue
        binary = root.joinpath(*_safe_member_path(entry["path"]).parts)
        actual_platform, actual_arch = _binary_platform_arch(binary)
        if actual_platform != target_info["os"] or actual_arch != target_info["architecture"]:
            raise ValueError(
                f"binary {entry['path']} is {actual_platform}/{actual_arch}, manifest target is {target}"
            )
    return manifest


def install_bundle(source: Path, destination: Path) -> Path:
    source = Path(source)
    destination = Path(destination).absolute()
    manifest = validate_manifest(source)
    destination_parent = destination.parent.resolve()
    destination_parent.mkdir(parents=True, exist_ok=True)
    destination = destination_parent / destination.name
    if _is_link_or_junction(destination):
        raise ValueError("runtime cache destination must not be a link or junction")
    if destination.exists():
        existing = validate_manifest(destination, manifest["target"])
        if existing == manifest:
            return destination
        raise ValueError("runtime cache already contains a different verified bundle")

    stage = Path(tempfile.mkdtemp(prefix=f".{destination.name}-", dir=destination.parent))
    try:
        for entry in manifest["files"]:
            relative = _safe_member_path(entry["path"])
            source_file = source.joinpath(*relative.parts)
            staged_file = stage.joinpath(*relative.parts)
            staged_file.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source_file, staged_file, follow_symlinks=False)
        shutil.copy2(source / MANIFEST_PATH, stage / MANIFEST_PATH)
        validate_manifest(stage, manifest["target"])
        listed = {entry["path"].casefold() for entry in manifest["files"]}
        actual = {
            path.relative_to(stage).as_posix().casefold()
            for path in stage.rglob("*")
            if path.is_file() and path.name != MANIFEST_PATH
        }
        if actual != listed:
            raise ValueError("staged runtime contains files absent from its manifest")
        os.replace(stage, destination)
    except BaseException:
        if stage.exists():
            shutil.rmtree(stage)
        raise
    return destination


def _tool(executable: str, *args: str, cwd: Path = ROOT) -> subprocess.CompletedProcess[str]:
    """Use RTK when available in an interactive workspace; CI can use raw tools."""
    rtk = shutil.which("rtk")
    command = ([rtk, "proxy", executable] if rtk else [executable]) + list(args)
    try:
        result = subprocess.run(
            command,
            cwd=cwd,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=1800,
        )
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"command timed out after {error.timeout} seconds: {command!r}") from error
    if result.returncode:
        detail = (result.stdout + "\n" + result.stderr)[-8000:]
        raise RuntimeError(f"command failed ({result.returncode}): {command!r}\n{detail}")
    return result


def _host_triple() -> str:
    output = _tool("rustc", "-vV").stdout
    for line in output.splitlines():
        if line.startswith("host: "):
            return line.split(": ", 1)[1]
    raise RuntimeError("rustc -vV did not report a host target")


def _git_sha() -> str:
    sha = _tool("git", "rev-parse", "HEAD").stdout.strip()
    if len(sha) != 40 or any(char not in "0123456789abcdef" for char in sha):
        raise RuntimeError("git did not return a valid source SHA")
    return sha


def _source_dirty() -> bool:
    result = _tool(
        "git",
        "status",
        "--porcelain",
        "--",
        "Cargo.toml",
        "Cargo.lock",
        "crates",
        "tools/grafikr-conformance",
        "scripts/build_runtime.py",
        "bindings/runtime/targets.json",
        "bindings/runtime/manifest.schema.json",
        "LICENSE",
        "bindings/c",
        "bindings/cpp",
    )
    return bool(result.stdout.strip())


def _build_config_digest(target: str) -> str:
    digest = hashlib.sha256(target.encode("utf-8"))
    inputs = [
        ROOT / "Cargo.toml",
        ROOT / "Cargo.lock",
        ROOT / "LICENSE",
        ROOT / "scripts/build_runtime.py",
        ROOT / "bindings/runtime/targets.json",
        ROOT / "bindings/runtime/manifest.schema.json",
    ]
    for directory in ("crates", "tools/grafikr-conformance", "bindings/c", "bindings/cpp"):
        inputs.extend(path for path in (ROOT / directory).rglob("*") if path.is_file())
    for path in sorted(set(inputs)):
        relative = path.relative_to(ROOT).as_posix().encode("utf-8")
        digest.update(len(relative).to_bytes(4, "big"))
        digest.update(relative)
        digest.update(bytes.fromhex(_sha256(path)))
    return digest.hexdigest()


def _copy(source: Path, root: Path, relative: str) -> Path:
    destination = root / relative
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    return destination


def _linux_system_requirements(binaries: list[Path], bundle_root: Path) -> list[str]:
    requirements: set[str] = set()
    root = bundle_root.resolve()
    for binary in binaries:
        output = _tool("ldd", str(binary)).stdout
        for line in output.splitlines():
            if "=>" not in line:
                continue
            name, resolved = (part.strip() for part in line.split("=>", 1))
            if resolved == "not found":
                raise RuntimeError(f"unresolved Linux runtime dependency {name} in {binary}")
            path = resolved.split(" (", 1)[0]
            if PurePosixPath(path).is_absolute():
                try:
                    Path(path).resolve().relative_to(root)
                except ValueError:
                    requirements.add(name)
    # winit loads X11 keymap support with dlopen, so ldd cannot see this dependency.
    requirements.add("libxkbcommon-x11.so.0")
    return sorted(requirements)


def _cmake_install_clients(target: str, static_ffi: Path, stage: Path) -> None:
    target_root = ROOT / "target"
    target_root.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=f"grafikr-cmake-{target}-", dir=target_root) as temporary:
        build_root = Path(temporary)
        for language, source in (("c", ROOT / "bindings/c"), ("cpp", ROOT / "bindings/cpp")):
            build_dir = build_root / language
            configure = [
                "cmake", "-S", str(source), "-B", str(build_dir),
                f"-DGRAFIKR_FFI_LIBRARY={static_ffi}",
                "-DGRAFIKR_FFI_STATIC=ON", "-DBUILD_TESTING=ON",
                "-DCMAKE_INSTALL_LIBDIR=lib", "-DCMAKE_INSTALL_INCLUDEDIR=include",
            ]
            _tool(configure[0], *configure[1:])
            _tool("cmake", "--build", str(build_dir), "--config", "Release")
            _tool("ctest", "--test-dir", str(build_dir), "-C", "Release", "--output-on-failure")
            _tool("cmake", "--install", str(build_dir), "--prefix", str(stage), "--config", "Release")


def _cmake_config(stage: Path, target_os: str) -> None:
    if target_os == "windows":
        ffi_static_name = "grafikr_ffi.lib"
        client_name = "grafikr_client.lib"
    elif target_os in {"linux", "macos"}:
        ffi_static_name = "libgrafikr_ffi.a"
        client_name = "libgrafikr_client.a"
    else:
        raise ValueError(f"unsupported CMake package target OS: {target_os}")
    config = f'''get_filename_component(GRAFIKR_ROOT "${{CMAKE_CURRENT_LIST_DIR}}/.." ABSOLUTE)
set(GRAFIKR_INCLUDE_DIR "${{GRAFIKR_ROOT}}/include")
set(GRAFIKR_FFI_LIBRARY "${{GRAFIKR_ROOT}}/lib/{ffi_static_name}")
set(GRAFIKR_CLIENT_LIBRARY "${{GRAFIKR_ROOT}}/lib/{client_name}")
if(NOT TARGET Grafikr::ffi)
  add_library(Grafikr::ffi STATIC IMPORTED)
  set_target_properties(Grafikr::ffi PROPERTIES IMPORTED_LOCATION "${{GRAFIKR_FFI_LIBRARY}}" INTERFACE_COMPILE_DEFINITIONS GRAFIKR_STATIC)
  if(WIN32)
    set_property(TARGET Grafikr::ffi PROPERTY INTERFACE_LINK_LIBRARIES "ws2_32;ntdll;dwmapi;imm32;uxtheme")
  elseif(UNIX AND NOT APPLE)
    find_package(Threads REQUIRED)
    find_library(GRAFIKR_FONTCONFIG_LIBRARY NAMES fontconfig REQUIRED)
    set_property(TARGET Grafikr::ffi PROPERTY INTERFACE_LINK_LIBRARIES "${{GRAFIKR_FONTCONFIG_LIBRARY}};m;dl;Threads::Threads")
  elseif(APPLE)
    set(GRAFIKR_APPLE_FRAMEWORKS "")
    foreach(GRAFIKR_FRAMEWORK AppKit Foundation CoreFoundation CoreGraphics CoreVideo CoreText Metal Carbon)
      find_library(GRAFIKR_${{GRAFIKR_FRAMEWORK}}_FRAMEWORK NAMES ${{GRAFIKR_FRAMEWORK}} REQUIRED)
      list(APPEND GRAFIKR_APPLE_FRAMEWORKS "${{GRAFIKR_${{GRAFIKR_FRAMEWORK}}_FRAMEWORK}}")
    endforeach()
    find_library(GRAFIKR_OBJC_LIBRARY NAMES objc REQUIRED)
    list(APPEND GRAFIKR_APPLE_FRAMEWORKS "${{GRAFIKR_OBJC_LIBRARY}}")
    set_property(TARGET Grafikr::ffi PROPERTY INTERFACE_LINK_LIBRARIES "${{GRAFIKR_APPLE_FRAMEWORKS}}")
  endif()
endif()
if(NOT TARGET Grafikr::client)
  add_library(Grafikr::client STATIC IMPORTED)
  set_target_properties(Grafikr::client PROPERTIES IMPORTED_LOCATION "${{GRAFIKR_CLIENT_LIBRARY}}" INTERFACE_INCLUDE_DIRECTORIES "${{GRAFIKR_INCLUDE_DIR}}" INTERFACE_LINK_LIBRARIES Grafikr::ffi INTERFACE_COMPILE_FEATURES c_std_11)
endif()
if(NOT TARGET Grafikr::cpp)
  add_library(Grafikr::cpp INTERFACE IMPORTED)
  set_target_properties(Grafikr::cpp PROPERTIES INTERFACE_INCLUDE_DIRECTORIES "${{GRAFIKR_INCLUDE_DIR}}" INTERFACE_LINK_LIBRARIES Grafikr::client INTERFACE_COMPILE_FEATURES cxx_std_17)
endif()
'''
    config_dir = stage / "cmake"
    config_dir.mkdir(parents=True, exist_ok=True)
    (config_dir / "GrafikrConfig.cmake").write_text(config, encoding="utf-8")


def build_runtime(target_argument: str, output: Path, development_fixture: bool = False) -> Path:
    targets = _targets()
    host = _host_triple()
    target = host if target_argument == "host" else target_argument
    if target not in targets:
        raise ValueError(f"unsupported target: {target}")
    if target != host:
        raise RuntimeError(
            f"{target} must be built on a native {target} runner so the C/C++ clients match the bundle"
        )
    target_info = targets[target]
    if target_info["os"] == "linux":
        libc = _tool("getconf", "GNU_LIBC_VERSION").stdout.strip().split()
        if len(libc) != 2 or libc[0] != "glibc":
            raise RuntimeError("Linux runtime bundles require glibc")
        if tuple(map(int, libc[1].split(".")[:2])) > tuple(map(int, target_info["min_glibc"].split(".")[:2])):
            raise RuntimeError(f"build on glibc {target_info['min_glibc']} or older, found {libc[1]}")
    if target_info["os"] == "macos":
        os.environ["MACOSX_DEPLOYMENT_TARGET"] = "14.0"
    packages = ["cargo", "build", "--locked", "--release", "--target", target, "-p", "grafikr-host", "-p", "grafikr-ffi"]
    if development_fixture:
        packages.extend(("-p", "grafikr-conformance"))
    _tool(packages[0], *packages[1:])

    suffix = ".exe" if target_info["os"] == "windows" else ""
    built = ROOT / "target" / target / "release"
    host_binary = built / f"grafikr-host{suffix}"
    fixture_binary = built / f"fixture-host{suffix}"
    if target_info["os"] == "windows":
        ffi_static = built / "grafikr_ffi.lib"
        ffi_shared = built / "grafikr_ffi.dll"
        ffi_import = built / "grafikr_ffi.dll.lib"
        ffi_shared_name = ffi_shared.name
    elif target_info["os"] == "linux":
        ffi_static = built / "libgrafikr_ffi.a"
        ffi_shared = built / "libgrafikr_ffi.so"
        ffi_import = None
        ffi_shared_name = ffi_shared.name
    else:
        ffi_static = built / "libgrafikr_ffi.a"
        ffi_shared = built / "libgrafikr_ffi.dylib"
        ffi_import = None
        ffi_shared_name = ffi_shared.name
    for required in (host_binary, ffi_static, ffi_shared):
        if not required.is_file():
            raise RuntimeError(f"expected build artifact is missing: {required}")
    if development_fixture and not fixture_binary.is_file():
        raise RuntimeError(f"expected fixture host is missing: {fixture_binary}")

    output = Path(output).absolute()
    output_parent = output.parent.resolve()
    output_parent.mkdir(parents=True, exist_ok=True)
    output = output_parent / output.name
    if _is_link_or_junction(output):
        raise ValueError("runtime output destination must not be a link or junction")
    if output.exists():
        existing = validate_manifest(output, target)
        raise ValueError(f"verified runtime already exists at {output} (build {existing['build_sha']})")

    with tempfile.TemporaryDirectory(prefix="grafikr-runtime-stage-", dir=output.parent) as temporary:
        stage = Path(temporary) / "bundle"
        stage.mkdir()
        _copy(host_binary, stage, f"bin/{host_binary.name}")
        if development_fixture:
            _copy(fixture_binary, stage, f"bin/{fixture_binary.name}")
        _copy(ffi_static, stage, f"lib/{ffi_static.name}")
        _copy(ffi_shared, stage, f"lib/{ffi_shared.name}")
        if ffi_import is not None:
            if not ffi_import.is_file():
                raise RuntimeError(f"expected import library is missing: {ffi_import}")
            _copy(ffi_import, stage, f"lib/{ffi_import.name}")
        _cmake_install_clients(target, ffi_static, stage)
        _cmake_config(stage, target_info["os"])
        _copy(ROOT / "LICENSE", stage, "LICENSE")

        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        engine_version = workspace["workspace"]["package"]["version"]
        files = []
        for path in sorted(stage.rglob("*")):
            if path.is_file():
                relative = path.relative_to(stage).as_posix()
                if relative == MANIFEST_PATH:
                    continue
                if relative.startswith("share/grafikr/examples/"):
                    role = "native_example"
                elif relative.startswith("bin/"):
                    role = "fixture_host" if path.name.startswith("fixture-host") else "host"
                elif relative.startswith("include/"):
                    role = "header"
                elif relative.startswith("cmake/"):
                    role = "cmake_config"
                elif relative == "LICENSE" or relative.endswith("/LICENSE"):
                    role = "license"
                elif path.name.endswith(".dll") or path.name.endswith(".so") or path.name.endswith(".dylib"):
                    role = "shared_library"
                elif "dll.lib" in path.name:
                    role = "import_library"
                elif path.name.endswith(".a") or path.name.endswith(".lib"):
                    role = "static_library"
                else:
                    role = "native_client"
                files.append({"path": relative, "size": path.stat().st_size, "sha256": _sha256(path), "role": role})
        manifest = {
            "format_version": 1,
            "engine_version": engine_version,
            "protocol": {"major": 1, "minor": 0},
            "abi": {"major": 1, "minor": 0},
            "target": target,
            "min_os": target_info["min_os"],
            "min_glibc": target_info["min_glibc"],
            "build_sha": _git_sha(),
            "build_config_sha256": _build_config_digest(target),
            "source_dirty": _source_dirty(),
            "development_fixture": development_fixture,
            "system_requirements": (
                [
                    "Microsoft Visual C++ 2015-2022 Redistributable (x64)"
                    if target_info["architecture"] == "x86_64"
                    else "Microsoft Visual C++ 2015-2022 Redistributable (ARM64)"
                ]
                if target_info["os"] == "windows"
                else _linux_system_requirements(
                    [host_binary, ffi_shared], stage
                ) if target_info["os"] == "linux" else []
            ),
            "limits": {
                "frame_bytes": 8388608,
                "json_depth": 64,
                "command_queue_messages": 1024,
                "command_queue_bytes": 16777216,
                "output_queue_messages": 1024,
                "output_queue_bytes": 16777216,
                "windows": 16,
                "nodes": 100000,
                "states": 100000,
                "subscriptions": 100000,
                "image_bytes": 67108864,
                "image_session_bytes": 268435456,
                "image_chunk_bytes": 1048576,
                "canvas_commands": 10000,
                "canvas_bytes": 8388608,
                "model_bytes": 134217728,
                "collection_rows_per_node": 2048,
                "collection_bytes_per_node": 8388608,
                "collection_bytes_session": 67108864,
                "collection_request_rows": 512,
                "tree_items": 50000,
                "tree_depth": 64,
                "host_shutdown_seconds": 5,
            },
            "files": files,
        }
        (stage / MANIFEST_PATH).write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        validate_manifest(stage, target)
        return install_bundle(stage, output)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, help="host or one of the six supported Rust target triples")
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--development-fixture", action="store_true")
    args = parser.parse_args(argv)
    try:
        output = build_runtime(args.target, args.out, args.development_fixture)
    except (OSError, ValueError, RuntimeError) as error:
        parser.exit(1, f"runtime bundle failed: {error}\n")
    print(f"runtime bundle verified: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
