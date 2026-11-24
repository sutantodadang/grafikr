"""Build and run one native C or C++ consumer against the selected bundle."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import platform
import shutil
import socket
import subprocess
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

from scripts.build_runtime import ROOT, _git_sha, validate_manifest
from scripts.runtime_evidence import validate_evidence
from scripts.windows_gui import (
    _capture_windows_accessibility, _capture_windows_window, _wait_for_windows_window,
    _windows_click, _windows_key, _windows_paste_text,
)


def run(
    project: str,
    runtime: Path,
    installed_consumer_only: bool = False,
    native_contract: Path | None = None,
    native_widget_inventory: bool = False,
    installed_scenarios: bool = False,
    evidence_dir: Path | None = None,
) -> None:
    if project not in {"c", "cpp"}:
        raise ValueError("project must be c or cpp")
    if installed_scenarios and not installed_consumer_only:
        raise ValueError("installed GUI scenarios require --installed-consumer-only")
    manifest = validate_manifest(runtime)
    target = ROOT / "target"
    target.mkdir(exist_ok=True)
    if installed_consumer_only:
        if installed_scenarios:
            _verify_bundle_cmake_package(
                runtime, target, project, run_scenarios=True,
                evidence_dir=evidence_dir, manifest=manifest,
            )
        else:
            _verify_bundle_cmake_package(runtime, target)
        return

    ffi_library_name = "grafikr_ffi.lib" if manifest["target"].endswith("-msvc") else "libgrafikr_ffi.a"
    ffi_library = runtime / "lib" / ffi_library_name
    if not ffi_library.is_file():
        raise FileNotFoundError(f"runtime bundle is missing {ffi_library_name}")
    with tempfile.TemporaryDirectory(prefix=f"grafikr-{project}-", dir=target) as temporary:
        build = Path(temporary) / "build"
        source = ROOT / "bindings" / project
        commands = (
            [
                "cmake", "-S", str(source), "-B", str(build),
                f"-DGRAFIKR_FFI_LIBRARY={ffi_library}",
                "-DGRAFIKR_FFI_STATIC=ON", "-DBUILD_TESTING=ON",
            ],
            ["cmake", "--build", str(build), "--config", "Release"],
            ["ctest", "--test-dir", str(build), "-C", "Release", "--output-on-failure"],
        )
        for command in commands:
            result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, check=False, timeout=1800)
            if result.returncode:
                raise RuntimeError(
                    f"command failed ({result.returncode}): {command!r}\n{(result.stdout + result.stderr)[-5000:]}"
                )
        if native_contract is not None:
            scenario = native_contract.resolve()
            try:
                scenario.relative_to(ROOT)
            except ValueError as error:
                raise ValueError("native contract fixture must stay inside the repository") from error
            if not scenario.is_file():
                raise FileNotFoundError(f"native contract fixture is missing: {scenario}")
            executable_name = "grafikr_native_scenario.exe" if os.name == "nt" else "grafikr_native_scenario"
            candidates = [
                build / "Release" / executable_name,
                build / "grafikr-c" / "Release" / executable_name,
                build / executable_name,
                build / "grafikr-c" / executable_name,
            ]
            executable = next((candidate for candidate in candidates if candidate.is_file()), None)
            if executable is None:
                raise FileNotFoundError(f"native scenario executable not found under {build}")
            if native_widget_inventory:
                _run_native_collection_provider_scenario(build, project)
            result = subprocess.run(
                [str(executable), "--scenario", str(scenario)],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
                timeout=120,
            )
            if result.returncode:
                raise RuntimeError(
                    f"native scenario failed ({result.returncode}):\n{(result.stdout + result.stderr)[-5000:]}"
                )
            _validate_native_parity(result.stdout, scenario)
            if native_widget_inventory:
                _run_native_widget_inventory(executable)
        elif native_widget_inventory:
            raise ValueError("native widget inventory requires --native-contract build/run setup")
    _verify_bundle_cmake_package(runtime, target)


def _validate_native_parity(output: str, fixture: Path | None = None) -> None:
    try:
        responses = [json.loads(line) for line in output.splitlines() if line]
    except json.JSONDecodeError as error:
        raise ValueError("native scenario emitted invalid JSONL") from error
    by_id = {response.get("id"): response for response in responses if response.get("kind") == "response"}
    requests = (
        [json.loads(line) for line in fixture.read_text(encoding="utf-8").splitlines() if line]
        if fixture is not None
        else []
    )
    required = {request["id"] for request in requests} if requests else {str(number) for number in range(1, 17)}
    if set(by_id) != required:
        raise ValueError("native scenario did not return every parity fixture response")
    if requests:
        states: dict[str, object] = {}
        for request in requests:
            operation = request.get("op")
            args = request.get("args", {})
            request_id = request["id"]
            response = by_id[request_id]
            if operation == "state.create" and response.get("ok"):
                states[response["result"]["state"]] = args["value"]
            elif operation == "state.set" and response.get("ok"):
                states[args["state"]] = args["value"]
            elif operation == "transition.start" and args.get("duration_ms") == 0 and response.get("ok"):
                states[args["state"]] = args["to"]
            elif operation == "state.get" and args.get("state") in states and response.get("ok"):
                if response.get("result") != states[args["state"]]:
                    raise ValueError(f"native scenario returned the wrong state value for request {request_id}")
            elif operation == "state.destroy" and response.get("ok"):
                states.pop(args["state"], None)
        shutdown_ids = [request["id"] for request in requests if request.get("op") == "shutdown"]
        shutdown_id = shutdown_ids[-1] if shutdown_ids else ""
    else:
        for request_id, expected in (("3", "-9223372036854775808"), ("5", "9223372036854775807")):
            if by_id[request_id].get("result", {}).get("value") != expected:
                raise ValueError(f"native scenario lost signed i64 value for request {request_id}")
        for request_id in ("7", "10"):
            if by_id[request_id].get("error", {}).get("code") != "invalid_handle":
                raise ValueError(f"native scenario did not preserve invalid handle error for request {request_id}")
        shutdown_id = "16"
    if by_id[shutdown_id].get("result", {}).get("stopped") is not True:
        raise ValueError("native scenario did not stop cleanly")


def _run_native_widget_inventory(executable: Path) -> None:
    inventory = json.loads((ROOT / "bindings/protocol/v1/widgets.json").read_text(encoding="utf-8"))["widgets"]
    process = subprocess.Popen([str(executable), "--stream"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    next_id = 0

    def request(op: str, args: dict) -> dict:
        nonlocal next_id
        if process.stdin is None or process.stdout is None:
            raise RuntimeError("native scenario pipes are unavailable")
        next_id += 1
        request_id = str(next_id)
        frame = json.dumps({"id": request_id, "op": op, "args": args}, separators=(",", ":")) + "\n"
        process.stdin.write(frame.encode("utf-8"))
        process.stdin.flush()
        while True:
            line = process.stdout.readline()
            if not line:
                raise RuntimeError(f"native scenario ended before {op} response")
            message = json.loads(line)
            if message.get("kind") == "event":
                continue
            if message.get("id") != request_id:
                raise RuntimeError(f"native response ID mismatch for {op}: {message}")
            if not message.get("ok"):
                raise RuntimeError(f"native {op} failed: {message.get('error')}")
            return message["result"]

    def sample(kind: str, name: str, type_name: str, state: str, image: str):
        if kind == "Radio" and name == "group": return state
        if kind == "TreeView" and name == "selected": return "root"
        if kind == "Spinner" and name == "size": return 16
        if kind == "Slider":
            return {"min": 0, "max": 10, "step": 1, "width": 120}.get(name, 1)
        if kind in {"VirtualList", "DataGrid"} and name == "row_count": return 1
        if kind in {"VirtualList", "DataGrid"} and name == "row_height": return 24
        if type_name == "string": return "sample"
        if type_name == "boolean": return False
        if type_name == "u16": return 1
        if type_name == "u64": return 0
        if type_name in {"number", "f32"}: return 1.0
        if type_name == "color": return "#336699"
        if type_name == "transform": return {"x": 0.0, "y": 0.0, "scale": 1.0}
        if type_name == "primary|secondary|icon": return "primary"
        if type_name == "state": return state
        if type_name == "ImageHandle": return image
        if type_name == "string[]": return ["One", "Two"]
        if type_name == "GridColumn[]": return [{"title": "Column", "width": 120.0}]
        if type_name == "TreeNode[]": return [{"key": "root", "label": "Root", "children": []}]
        if type_name == "CanvasCommand[]":
            return [{"kind": "rect", "rect": {"x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0}, "color": "#336699", "radius": 0.0}]
        raise ValueError(f"no native widget sample for {kind}.{name}: {type_name}")

    try:
        request("hello", {"protocol_major": 1, "protocol_minor": 0, "sdk_version": "native-contract"})
        window = request("window.create", {"title": "Native widget contract", "width": 640, "height": 480})
        state = request("state.create", {"value": {"type": "i64", "value": "1"}})["state"]
        begin = request("image.begin", {"width": 1, "height": 1, "byte_length": 4})["upload"]
        request("image.chunk", {"upload": begin, "offset": 0, "data": base64.b64encode(bytes((0, 0, 0, 255))).decode("ascii")})
        image = request("image.finalize", {"upload": begin})["image"]

        large = "line one\nline two 雪 😀" + "x" * (100 * 1024)
        text_state = request("state.create", {"value": {"type": "string", "value": large}})["state"]
        if request("state.get", {"state": text_state}).get("value") != large:
            raise RuntimeError("native transport did not preserve the 100 KiB UTF-8 state")
        request("state.destroy", {"state": text_state})

        for kind, spec in inventory.items():
            properties = {
                name: sample(kind, name, type_name, state, image)
                for name, type_name in spec.get("properties", {}).items()
            }
            node = request("node.create", {"window": window["window"], "widget": kind, "properties": properties})["node"]
            request("node.attach", {"node": node, "parent": window["root"]})
            for event in spec.get("events", []):
                request("event.subscribe", {"node": node, "event": event})
                if event == "data.request":
                    request("collection.create", {"node": node, "row_count": 1, "row_height": 24, "columns": ["label"]})
            request("node.destroy", {"node": node})

        request("image.destroy", {"image": image})
        request("state.destroy", {"state": state})
        request("shutdown", {})
        if process.stdin is not None:
            process.stdin.close()
        try:
            code = process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            raise RuntimeError("native widget inventory did not stop after shutdown")
        stderr = process.stderr.read() if process.stderr is not None else b""
        if code != 0:
            raise RuntimeError(f"native widget inventory exited {code}: {stderr.decode('utf-8', errors='replace')[-2000:]}")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()


def _run_native_collection_provider_scenario(build: Path, project: str) -> None:
    executable_name = (
        "grafikr_cpp_collection_provider_scenario"
        if project == "cpp"
        else "grafikr_collection_provider_scenario"
    )
    if os.name == "nt":
        executable_name += ".exe"
    candidates = [build / "Release" / executable_name, build / executable_name]
    if project == "cpp":
        c_executable = "grafikr_collection_provider_scenario" + (".exe" if os.name == "nt" else "")
        candidates.extend((
            build / "grafikr-c" / "Release" / c_executable,
            build / "grafikr-c" / c_executable,
        ))
    executable = next((candidate for candidate in candidates if candidate.is_file()), None)
    if executable is None:
        raise FileNotFoundError(f"native collection provider scenario not found under {build}")
    try:
        result = subprocess.run(
            [str(executable)], cwd=ROOT, capture_output=True, text=True, check=False, timeout=60
        )
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"native collection provider scenario timed out: {executable_name}") from error
    if result.returncode:
        raise RuntimeError(
            f"native collection provider scenario failed ({result.returncode}):\n"
            f"{(result.stdout + result.stderr)[-5000:]}"
        )


def _verify_bundle_cmake_package(
    runtime: Path,
    target: Path,
    project: str = "c",
    run_scenarios: bool = False,
    evidence_dir: Path | None = None,
    manifest: dict | None = None,
) -> None:
    """Compile and run clean C/C++ consumers through the installed package config."""
    suffix = ".exe" if os.name == "nt" else ""
    examples = (
        "grafikr_counter_form",
        "grafikr_dashboard",
        "grafikr_multi_window",
        "grafikr_cpp_counter_form",
        "grafikr_cpp_dashboard",
        "grafikr_cpp_multi_window",
    )
    for name in examples:
        path = runtime / "share/grafikr/examples" / f"{name}{suffix}"
        if not path.is_file():
            raise FileNotFoundError(f"runtime bundle is missing native example {path}")
    with tempfile.TemporaryDirectory(prefix="grafikr-cmake-consumer-", dir=target) as temporary:
        root = Path(temporary)
        config_dir = (runtime / "cmake").as_posix()
        cmake = (
            "cmake_minimum_required(VERSION 3.28)\n"
            "project(GrafikrConsumer LANGUAGES C CXX)\n"
            "enable_testing()\n"
            f'find_package(Grafikr CONFIG REQUIRED PATHS "{config_dir}" NO_DEFAULT_PATH)\n'
        )
        if not run_scenarios:
            cmake += (
                "add_executable(c-consumer c.c)\n"
                "target_link_libraries(c-consumer PRIVATE Grafikr::client)\n"
                "add_test(NAME c-consumer COMMAND c-consumer)\n"
            )
        cmake += (
            f'add_executable(c-installed-scenarios "{(ROOT / "bindings/c/tests/installed_scenarios.c").as_posix()}")\n'
            f'target_include_directories(c-installed-scenarios PRIVATE "{(ROOT / "bindings/c/include").as_posix()}")\n'
            "target_link_libraries(c-installed-scenarios PRIVATE Grafikr::client)\n"
        )
        if not run_scenarios:
            cmake += (
                "add_executable(cpp-consumer cpp.cpp)\n"
                "target_link_libraries(cpp-consumer PRIVATE Grafikr::cpp)\n"
                "add_test(NAME cpp-consumer COMMAND cpp-consumer)\n"
            )
        cmake += (
            f'add_executable(cpp-installed-scenarios "{(ROOT / "bindings/cpp/tests/installed_scenarios.cpp").as_posix()}")\n'
            "target_link_libraries(cpp-installed-scenarios PRIVATE Grafikr::cpp)\n"
        )
        (root / "CMakeLists.txt").write_text(cmake, encoding="utf-8")
        (root / "c.c").write_text(
            '#include "grafikr_client.h"\n'
            "int main(void) { gf_value *value = gf_value_i64(42); "
            "if (!value) return 1; gf_value_free(value); "
            "return grafikr_abi_version() == 0x00010000u ? 0 : 2; }\n",
            encoding="utf-8",
        )
        (root / "cpp.cpp").write_text(
            '#include "grafikr.hpp"\n'
            "int main() { auto value = grafikr::Value::i64(42); "
            "return value.as_i64() == 42 ? 0 : 1; }\n",
            encoding="utf-8",
        )
        build = root / "build"
        commands = (
            ["cmake", "-S", str(root), "-B", str(build)],
            ["cmake", "--build", str(build), "--config", "Release"],
            ["ctest", "--test-dir", str(build), "-C", "Release", "--output-on-failure"],
        )
        for command in commands:
            result = subprocess.run(command, cwd=root, capture_output=True, text=True, check=False, timeout=1800)
            if result.returncode:
                raise RuntimeError(
                    f"installed CMake consumer failed ({result.returncode}): {command!r}\n"
                    f"{(result.stdout + result.stderr)[-5000:]}"
                )
        if run_scenarios:
            if evidence_dir is not None and manifest is None:
                raise ValueError("GUI evidence requires the validated runtime manifest")
            _run_installed_cmake_scenarios(build, project, evidence_dir, manifest)


def _run_installed_cmake_scenarios(
    build: Path,
    project: str,
    evidence_dir: Path | None = None,
    manifest: dict | None = None,
) -> None:
    windows_native = os.name == "nt"
    if not windows_native and not os.environ.get("DISPLAY"):
        raise RuntimeError("installed C/C++ GUI scenarios require an X11 display")
    if not windows_native:
        for tool in ("xdotool", "xclip"):
            if shutil.which(tool) is None:
                raise RuntimeError(f"installed C/C++ GUI scenarios require {tool}")
    executable_name = f"{project}-installed-scenarios" + (".exe" if windows_native else "")
    executable = next(
        (candidate for candidate in (build / "Release" / executable_name, build / executable_name) if candidate.is_file()),
        None,
    )
    if executable is None:
        raise FileNotFoundError(f"installed scenario executable is missing under {build}")

    for mode, title, expected in (
        ("counter", "Grafikr installed counter", {"counter.click-state-update", "counter.unicode-binding"}),
        ("dashboard", "Grafikr installed dashboard", {"dashboard.initial-range", "dashboard.scroll-range", "dashboard.selection", "dashboard.canvas", "dashboard.image-resource-cleanup", "dashboard.multi-resource-cleanup"}),
        ("multi", "Grafikr second window", {"multi-window-state-survives-first-close", "multi-window.keyboard-binding", "multi-window.modal-escape", "multi-window-modal-close"}),
    ):
        with tempfile.TemporaryDirectory(prefix="grafikr-installed-scenario-") as temporary:
            started_at = datetime.now(timezone.utc).isoformat()
            report_path = Path(temporary) / "events.txt"
            environment = os.environ.copy()
            environment["GRAFIKR_SCENARIO_REPORT"] = str(report_path)
            environment["WINIT_UNIX_BACKEND"] = "x11"
            environment.pop("WAYLAND_DISPLAY", None)
            if windows_native:
                environment["WGPU_BACKEND"] = "dx12"
                if evidence_dir is not None:
                    environment["GRAFIKR_RENDERER_INFO"] = "1"
            screenshot_path: Path | None = None
            accessibility_report: dict | None = None
            accessibility_error: str | None = None
            process = subprocess.Popen(
                [str(executable), mode], cwd=ROOT, env=environment,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            )
            failure: Exception | None = None
            try:
                ready_marker = "multi-window.ready" if mode == "multi" else f"{mode}.ready"
                _wait_for_scenario_marker(process, report_path, ready_marker, 20)
                if windows_native:
                    window = _wait_for_windows_window(process, title, 10)
                    screenshot_dir = os.environ.get("GRAFIKR_SCENARIO_SCREENSHOT_DIR")
                    if screenshot_dir:
                        destination = Path(screenshot_dir).resolve()
                        destination.mkdir(parents=True, exist_ok=True)
                        _capture_windows_window(window, destination / f"{project}-{mode}.bmp")
                    if evidence_dir is not None:
                        if manifest is None:
                            raise ValueError("GUI evidence requires a validated runtime manifest")
                        destination = evidence_dir.absolute() / "attachments"
                        destination.mkdir(parents=True, exist_ok=True)
                        screenshot_path = destination / f"{project}-{mode}.bmp"
                        if screenshot_path.exists():
                            raise FileExistsError(f"refusing to overwrite GUI screenshot {screenshot_path}")
                        _capture_windows_window(window, screenshot_path)
                        if mode == "counter":
                            time.sleep(1.0)
                            try:
                                accessibility_report = _capture_windows_accessibility(window, title)
                            except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
                                accessibility_error = str(error)
                if mode == "dashboard":
                    _wait_for_scenario_marker(process, report_path, "dashboard.initial-range", 10)
                    if windows_native:
                        _windows_click(window, 100, 150)
                        _windows_key(0x23)  # VK_END
                    else:
                        window = _wait_for_x11_window(process, title, 10)
                        subprocess.run(["xdotool", "windowfocus", "--sync", window], check=True, timeout=5)
                        subprocess.run(["xdotool", "mousemove", "100", "150", "click", "1", "key", "End"], check=True, timeout=5)
                    _wait_for_scenario_marker(process, report_path, "dashboard.selection", 5)
                    _wait_for_scenario_marker(process, report_path, "dashboard.scroll-range", 15)
                else:
                    if not windows_native:
                        window = _wait_for_x11_window(process, title, 10)
                        subprocess.run(["xdotool", "windowfocus", "--sync", window], check=True, timeout=5)
                    if mode == "counter":
                        if windows_native:
                            _windows_click(window, 50, 42)
                        else:
                            subprocess.run(["xdotool", "mousemove", "50", "42", "click", "1"], check=True, timeout=5)
                        _wait_for_scenario_marker(process, report_path, "counter.click-state-update", 5)
                        if windows_native:
                            _windows_click(window, 120, 65)
                            time.sleep(0.2)
                            _windows_paste_text("Input 雪")
                        else:
                            _paste_text("Input 雪")
                            subprocess.run(["xdotool", "mousemove", "120", "82", "click", "1", "key", "ctrl+a", "key", "ctrl+v"], check=True, timeout=5)
                    else:
                        if windows_native:
                            _windows_click(window, 150, 160)
                            _windows_key(0x09)  # VK_TAB
                            _windows_paste_text("Confirm 雪")
                        else:
                            _paste_text("Confirm 雪")
                            subprocess.run(["xdotool", "mousemove", "150", "160", "click", "1", "key", "Tab", "key", "ctrl+a", "key", "ctrl+v"], check=True, timeout=5)
                        _wait_for_scenario_marker(process, report_path, "multi-window.keyboard-binding", 5)
                        if windows_native:
                            _windows_key(0x1B)  # VK_ESCAPE
                        else:
                            subprocess.run(["xdotool", "key", "Escape"], check=True, timeout=5)
                        _wait_for_scenario_marker(process, report_path, "multi-window.modal-escape", 5)
                _wait_for_process(process, 30)
                markers = set(report_path.read_text(encoding="utf-8").splitlines()) if report_path.exists() else set()
                missing = expected - markers
                if missing:
                    raise RuntimeError(f"{project} installed {mode} scenario missed markers: {', '.join(sorted(missing))}")
            except Exception as error:
                failure = error
            finally:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                stdout, stderr = process.communicate()
            markers = set(report_path.read_text(encoding="utf-8").splitlines()) if report_path.exists() else set()
            if process.returncode not in (0, None):
                exit_error = RuntimeError(
                    f"process exited {process.returncode}: {(stdout + stderr)[-3000:]}"
                )
                failure = exit_error if failure is None else RuntimeError(f"{failure}; {exit_error}")
            if failure is not None:
                raise RuntimeError(f"{project} installed {mode}: {failure}; markers={sorted(markers)}") from failure
            if evidence_dir is not None:
                if manifest is None or screenshot_path is None or process.returncode != 0:
                    raise RuntimeError("cannot emit GUI evidence without a validated bundle, screenshot, and clean scenario exit")
                _write_gui_evidence(
                    evidence_dir.absolute(), project, mode, executable, manifest,
                    sorted(markers), screenshot_path, started_at, stderr,
                    accessibility_report, accessibility_error,
                )


def _write_gui_evidence(
    evidence_root: Path,
    project: str,
    mode: str,
    executable: Path,
    manifest: dict,
    markers: list[str],
    screenshot_path: Path,
    started_at: str,
    stderr: str = "",
    accessibility_report: dict | None = None,
    accessibility_error: str | None = None,
) -> None:
    scenario = {"counter": "counter_form", "dashboard": "dashboard", "multi": "multi_window"}[mode]
    display_cell = {"x86_64-pc-windows-msvc": "win-x64-d3d12"}.get(manifest["target"])
    if display_cell is None:
        raise ValueError(f"no native GUI evidence cell is defined for {manifest['target']}")
    if screenshot_path.is_symlink() or not screenshot_path.is_file():
        raise ValueError("GUI evidence screenshot must be a regular file")

    adapter = _parse_adapter_report(stderr)
    actual_backend = {
        "Dx12": "D3D12",
        "Vulkan": "Vulkan",
        "Metal": "Metal",
        "Gl": "OpenGL",
    }.get(adapter.get("backend"), "unknown")
    notes = {
        "counter_form": "The current scenario verifies OS click, Unicode input and state binding, but does not verify IME composition or focus preservation after unrelated label updates.",
        "dashboard": None,
        "multi_window": "The current scenario verifies the remaining window, keyboard input and modal Escape, but does not verify that focus remains trapped inside the modal.",
    }
    reasons = [notes[scenario]] if notes[scenario] else []
    if actual_backend != "D3D12":
        reasons.append(f"runtime adapter backend was {actual_backend}, expected D3D12")
    reason = "; ".join(reasons) or None
    state_path = evidence_root / "attachments" / f"{project}-{mode}-state.json"
    if state_path.exists():
        raise FileExistsError(f"refusing to overwrite GUI state report {state_path}")
    state_path.write_text(
        json.dumps(
            {
                "sdk": project,
                "scenario": scenario,
                "target": manifest["target"],
                "display_cell": display_cell,
                "input_method": "windows_sendinput",
                "markers": markers,
                "runtime_adapter": adapter,
                "exit_code": 0,
                "scenario_limit": reason,
            },
            ensure_ascii=False,
            indent=2,
        ) + "\n",
        encoding="utf-8",
    )
    relative_screenshot = screenshot_path.relative_to(evidence_root).as_posix()
    relative_state = state_path.relative_to(evidence_root).as_posix()
    evidence = {
        "stage": "gui",
        "scenario": scenario,
        "display_cell": display_cell,
        "input_method": "windows_sendinput",
        "sdk": project,
        "consumer_language": project,
        "engine_version": manifest["engine_version"],
        "sdk_version": manifest["engine_version"],
        "protocol_version": manifest["protocol"],
        "runtime_version": manifest["engine_version"],
        "target": manifest["target"],
        "backend": actual_backend,
        "gpu_kind": None,
        "gpu_vendor": None,
        "source_sha": _git_sha(),
        "runtime_build_sha": manifest["build_sha"],
        "runner_sha": hashlib.sha256(
            f"{platform.platform()}|{platform.machine()}|{socket.gethostname()}".encode("utf-8")
        ).hexdigest(),
        "artifact_sha256": _sha256(executable),
        "command": [{"name": executable.name, "argv": [str(executable), mode]}],
        "exit_code": 0,
        "status": "UNVERIFIED" if reason else "PASS",
        "started_at": started_at,
        "finished_at": datetime.now(timezone.utc).isoformat(),
        "attachments": [
            {"path": relative_screenshot, "sha256": _sha256(screenshot_path), "kind": "screenshot"},
            {"path": relative_state, "sha256": _sha256(state_path), "kind": "client_state"},
        ],
        "reason": reason,
    }
    validate_evidence(evidence, evidence_root)
    evidence_path = evidence_root / f"gui-{project}-{scenario}.json"
    if evidence_path.exists():
        raise FileExistsError(f"refusing to overwrite GUI evidence {evidence_path}")
    evidence_path.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    if mode == "counter" and project == "c" and (
        accessibility_report is not None or accessibility_error is not None
    ):
        _write_windows_hardware_evidence(
            evidence_root, project, executable, manifest, started_at, stderr,
            accessibility_report, accessibility_error, markers,
        )


def _parse_adapter_report(stderr: str) -> dict[str, str]:
    line = next(
        (value for value in stderr.splitlines() if value.startswith("grafikr-adapter-v1\t")),
        None,
    )
    if line is None:
        return {}
    report = {}
    for field in line.split("\t")[1:]:
        if "=" in field:
            key, value = field.split("=", 1)
            report[key] = value
    return report


def _write_windows_hardware_evidence(
    evidence_root: Path,
    project: str,
    executable: Path,
    manifest: dict,
    started_at: str,
    stderr: str,
    accessibility_report: dict | None,
    accessibility_error: str | None,
    markers: list[str],
) -> None:
    display_cell = "win-x64-d3d12"
    adapter = _parse_adapter_report(stderr)
    try:
        gpu_query = subprocess.run(
            ["nvidia-smi", "--query-gpu=name,driver_version", "--format=csv,noheader"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        gpu_query = None
        gpu_error = str(error)
    else:
        gpu_error = None if gpu_query.returncode == 0 else gpu_query.stderr.strip()
    gpu_text = gpu_query.stdout.strip() if gpu_query and gpu_query.returncode == 0 else ""
    gpu_name = gpu_text.split(",", 1)[0].strip() if gpu_text else ""
    hardware_verified = (
        adapter.get("backend") == "Dx12"
        and adapter.get("device_type") == "DiscreteGpu"
        and adapter.get("vendor", "").lower() == "10de"
        and "NVIDIA" in adapter.get("name", "")
        and gpu_name
        and adapter.get("name", "").casefold() == gpu_name.casefold()
    )
    controls = accessibility_report.get("controls", []) if accessibility_report else []
    accessible_button = any(
        item.get("control_type") == "ControlType.Button"
        and item.get("name") == "Increment"
        and item.get("keyboard_focusable") is True
        for item in controls
    )
    accessible_field = any(
        item.get("control_type") == "ControlType.Edit"
        and (item.get("value") == "Grafikr 雪" or item.get("name") == "Grafikr 雪")
        and item.get("keyboard_focusable") is True
        for item in controls
    )
    keyboard_verified = "counter.unicode-binding" in markers
    reasons = []
    if not hardware_verified:
        reasons.append("the application adapter could not be matched to a physical NVIDIA D3D12 adapter")
    if not accessible_button:
        reasons.append("UI Automation did not expose a keyboard-focusable Increment button by name")
    if not accessible_field:
        reasons.append("UI Automation did not expose the initial text field value and keyboard focus")
    if not keyboard_verified:
        reasons.append("the native keyboard scenario did not produce its Unicode binding marker")
    if accessibility_error:
        reasons.append(f"UI Automation query failed: {accessibility_error}")

    gpu_path = evidence_root / "attachments" / "windows-x64-gpu-report.json"
    accessibility_path = evidence_root / "attachments" / "windows-x64-accessibility-report.json"
    if gpu_path.exists() or accessibility_path.exists():
        raise FileExistsError("refusing to overwrite Windows hardware evidence attachments")
    gpu_path.write_text(
        json.dumps(
            {
                "display_cell": display_cell,
                "expected_backend": "D3D12",
                "runtime_adapter": adapter,
                "nvidia_smi": gpu_text,
                "nvidia_smi_error": gpu_error,
                "host_os": platform.platform(),
                "host_gpu_inventory": gpu_name,
            },
            indent=2,
        ) + "\n",
        encoding="utf-8",
    )
    accessibility_path.write_text(
        json.dumps(
            accessibility_report or {"error": accessibility_error or "UI Automation report unavailable"},
            ensure_ascii=False,
            indent=2,
        ) + "\n",
        encoding="utf-8",
    )
    source_sha = _git_sha()
    runner_sha = hashlib.sha256(
        f"{platform.platform()}|{platform.machine()}|{socket.gethostname()}".encode("utf-8")
    ).hexdigest()
    evidence = {
        "stage": "hardware",
        "display_cell": display_cell,
        "sdk": project,
        "consumer_language": project,
        "engine_version": manifest["engine_version"],
        "sdk_version": manifest["engine_version"],
        "protocol_version": manifest["protocol"],
        "runtime_version": manifest["engine_version"],
        "target": manifest["target"],
        "backend": "D3D12",
        "gpu_kind": "discrete" if hardware_verified else None,
        "gpu_vendor": "NVIDIA" if hardware_verified else None,
        "source_sha": source_sha,
        "runtime_build_sha": manifest["build_sha"],
        "runner_sha": runner_sha,
        "artifact_sha256": _sha256(executable),
        "command": [{"name": executable.name, "argv": [str(executable), "counter"]}],
        "exit_code": 0,
        "status": "PASS" if not reasons else "UNVERIFIED",
        "started_at": started_at,
        "finished_at": datetime.now(timezone.utc).isoformat(),
        "attachments": [
            {"path": gpu_path.relative_to(evidence_root).as_posix(), "sha256": _sha256(gpu_path), "kind": "gpu_report"},
            {"path": accessibility_path.relative_to(evidence_root).as_posix(), "sha256": _sha256(accessibility_path), "kind": "accessibility_report"},
        ],
        "reason": "; ".join(reasons) or None,
    }
    validate_evidence(evidence, evidence_root)
    evidence_path = evidence_root / "hardware-win-x64-d3d12.json"
    if evidence_path.exists():
        raise FileExistsError(f"refusing to overwrite hardware evidence {evidence_path}")
    evidence_path.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _wait_for_scenario_marker(process: subprocess.Popen, path: Path, marker: str, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.exists() and marker in path.read_text(encoding="utf-8").splitlines():
            return
        if process.poll() is not None:
            stdout, stderr = process.communicate()
            raise RuntimeError(f"scenario exited before {marker}: {(stdout + stderr)[-3000:]}")
        time.sleep(0.05)
    raise RuntimeError(f"timed out waiting for installed scenario marker {marker}")


def _wait_for_x11_window(process: subprocess.Popen, title: str, timeout: float) -> str:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", title], capture_output=True, text=True, check=False)
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout.splitlines()[-1]
        if process.poll() is not None:
            break
        time.sleep(0.1)
    raise RuntimeError(f"could not find X11 window titled {title!r}")


def _wait_for_process(process: subprocess.Popen, timeout: float) -> None:
    try:
        process.wait(timeout=timeout)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError("installed C/C++ scenario did not close its windows") from error


def _paste_text(value: str) -> None:
    subprocess.run(["xclip", "-selection", "clipboard"], input=value, text=True, check=True, timeout=5)



def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", required=True, choices=("c", "cpp"))
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--installed-consumer-only", action="store_true")
    parser.add_argument("--installed-scenarios", action="store_true")
    parser.add_argument("--evidence-dir", type=Path, help="write screenshot, client-state, and schema-validated GUI records")
    parser.add_argument("--native-contract", type=Path)
    parser.add_argument("--native-widget-inventory", action="store_true")
    args = parser.parse_args(argv)
    try:
        run(
            args.project,
            args.runtime.resolve(),
            args.installed_consumer_only,
            args.native_contract,
            args.native_widget_inventory,
            args.installed_scenarios,
            args.evidence_dir.absolute() if args.evidence_dir else None,
        )
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        parser.exit(1, f"native binding tests failed: {error}\n")
    print(f"{args.project} CMake/CTest passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
