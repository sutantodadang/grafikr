"""Audit local evidence or require the complete installed GUI and hardware gate."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from scripts.build_runtime import ROOT, _git_sha
from scripts.runtime_evidence import validate_evidence

MATRIX_PATH = ROOT / "bindings/sdk-matrix.json"
GUI_CASES_PATH = ROOT / "bindings/runtime/gui-cases.json"
REQUIRED_BENCHMARK_WORKLOADS = {
    "request_latency",
    "label_update",
    "scroll_50000",
    "resource_cycles_100",
    "rss_counters",
    "idle_redraw_4s",
}


def _benchmark_data_errors(raw: Any, required_workloads: list[str]) -> list[str]:
    errors: list[str] = []
    if not isinstance(raw, dict) or raw.get("format") != "grafikr-bindings-benchmark-evidence-v1" or not isinstance(raw.get("workloads"), dict):
        return ["benchmark raw attachment has an invalid format"]
    workloads = raw["workloads"]
    if set(required_workloads) - set(workloads):
        errors.append("benchmark raw data omits required workload measurements")
    for workload in required_workloads:
        result = workloads.get(workload)
        if not isinstance(result, dict) or not all(isinstance(result.get(endpoint), dict) for endpoint in ("native", "ipc")):
            errors.append(f"workload {workload} is missing native or IPC measurements")
            continue
        measurements = (("native", result["native"]), ("ipc", result["ipc"]))
        if workload in {"request_latency", "label_update"}:
            for endpoint, measurement in measurements:
                samples = measurement.get("samples_ns")
                if (
                    measurement.get("endpoint") != "request_to_response"
                    or not isinstance(samples, list)
                    or len(samples) < 1000
                    or any(type(sample) is not int or sample < 0 for sample in samples)
                ):
                    errors.append(f"workload {workload} {endpoint} lacks 1,000 complete-response samples")
        elif workload == "scroll_50000":
            for endpoint, measurement in measurements:
                if (
                    measurement.get("rows") != 50000
                    or type(measurement.get("elapsed_ns")) is not int
                    or measurement["elapsed_ns"] <= 0
                    or type(measurement.get("visible_range_rows")) is not int
                    or measurement["visible_range_rows"] < 1
                ):
                    errors.append(f"workload scroll_50000 {endpoint} has invalid row/scroll measurements")
        elif workload == "resource_cycles_100":
            for endpoint, measurement in measurements:
                if (
                    measurement.get("cycles") != 100
                    or measurement.get("anchor_window_open") is not True
                    or type(measurement.get("objects_before")) is not int
                    or measurement.get("objects_before") != measurement.get("objects_after")
                    or type(measurement.get("resources_before")) is not int
                    or measurement.get("resources_before") != measurement.get("resources_after")
                ):
                    errors.append(f"workload resource_cycles_100 {endpoint} did not return counters to baseline")
        elif workload == "rss_counters":
            for endpoint, measurement in measurements:
                if any(type(measurement.get(key)) is not int or measurement[key] < 0 for key in ("before_bytes", "after_bytes")):
                    errors.append(f"workload rss_counters {endpoint} has invalid RSS counters")
        elif workload == "idle_redraw_4s":
            for endpoint, measurement in measurements:
                if (
                    type(measurement.get("duration_ms")) is not int
                    or measurement["duration_ms"] < 4000
                    or measurement.get("redraw_delta") != 0
                ):
                    errors.append(f"workload idle_redraw_4s {endpoint} has redraws or ran for less than 4 seconds")
    return errors


def audit(
    root: Path,
    mode: str,
    matrix_path: Path = MATRIX_PATH,
    gui_cases_path: Path = GUI_CASES_PATH,
    expected_source_sha: str | None = None,
) -> dict[str, Any]:
    root = Path(root)
    if mode not in {"local", "full"}:
        raise ValueError("mode must be local or full")
    if root.is_symlink() or not root.is_dir():
        raise ValueError("evidence root must be a regular directory")
    root = root.resolve()
    matrix = json.loads(Path(matrix_path).read_text(encoding="utf-8"))
    gui_cases = json.loads(Path(gui_cases_path).read_text(encoding="utf-8"))
    rows = matrix.get("sdks")
    cells = gui_cases.get("display_cells")
    scenarios = gui_cases.get("scenarios")
    benchmark_workloads = gui_cases.get("benchmark_workloads")
    input_methods = set(gui_cases.get("os_input_methods", []))
    if (
        not isinstance(rows, list)
        or not isinstance(cells, list)
        or not isinstance(scenarios, list)
        or not isinstance(benchmark_workloads, list)
        or not all(isinstance(name, str) for name in benchmark_workloads)
        or set(benchmark_workloads) != REQUIRED_BENCHMARK_WORKLOADS
        or len(benchmark_workloads) != len(REQUIRED_BENCHMARK_WORKLOADS)
    ):
        raise ValueError("SDK matrix or GUI case inventory is malformed")

    evidence: list[tuple[str, dict[str, Any]]] = []
    invalid_evidence: set[str] = set()
    errors: list[str] = []
    for path in sorted(root.rglob("*.json")):
        if path.name == "release-gate.json":
            continue
        if path.is_symlink():
            errors.append(f"evidence file is a symlink: {path.name}")
            continue
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            errors.append(f"cannot read {path.relative_to(root)}: {error}")
            continue
        if not isinstance(value, dict) or "stage" not in value or "sdk" not in value or value["sdk"] == "all":
            continue
        relative = path.relative_to(root).as_posix()
        evidence.append((relative, value))
        try:
            validate_evidence(value, root)
        except (OSError, ValueError) as error:
            errors.append(f"invalid {relative}: {error}")
            invalid_evidence.add(relative)

    pass_count = sum(item.get("status") == "PASS" for _, item in evidence)
    fail_count = sum(item.get("status") == "FAIL" for _, item in evidence)
    unverified_count = sum(item.get("status") == "UNVERIFIED" for _, item in evidence)
    report: dict[str, Any] = {
        "format": "grafikr-bindings-release-gate-v1",
        "mode": mode,
        "status": "FAIL",
        "evidence_count": len(evidence),
        "pass_count": pass_count,
        "fail_count": fail_count,
        "unverified_count": unverified_count,
        "errors": errors,
        "records": [
            {
                "path": path,
                "sdk": item.get("sdk"),
                "stage": item.get("stage"),
                "status": item.get("status"),
                "target": item.get("target"),
                "source_sha": item.get("source_sha"),
                "artifact_sha256": item.get("artifact_sha256"),
                "reason": item.get("reason"),
            }
            for path, item in evidence
        ],
    }
    if mode == "local":
        report["status"] = "LOCAL_ONLY" if not errors and evidence else "FAIL"
        report["notice"] = "Local evidence does not establish full platform or SDK support."
        return report

    if expected_source_sha is None:
        errors.append("full mode requires the expected tested source SHA")

    sdk_names = {row.get("name") for row in rows if isinstance(row, dict)}
    display_by_id = {cell.get("id"): cell for cell in cells if isinstance(cell, dict)}
    scenario_names = set(scenarios)
    gui_expected = {
        (sdk, display_id, scenario)
        for sdk in sdk_names
        for display_id in display_by_id
        for scenario in scenario_names
    }
    gui_seen: set[tuple[str, str, str]] = set()
    hardware_seen: set[str] = set()
    benchmark_seen: set[str] = set()
    hardware_kinds = set(gui_cases.get("hardware_gpu_kinds", []))

    for relative, item in evidence:
        stage = item.get("stage")
        if stage not in {"gui", "hardware", "benchmark"}:
            continue
        valid = relative not in invalid_evidence
        if item.get("status") != "PASS" or item.get("exit_code") != 0:
            errors.append(f"{relative}: {stage} evidence is not a passing run")
            valid = False
        if expected_source_sha and item.get("source_sha") != expected_source_sha:
            errors.append(f"{relative}: stale source SHA")
            valid = False
        display_id = item.get("display_cell")
        if not isinstance(display_id, str):
            errors.append(f"{relative}: display cell is missing or invalid")
            continue
        cell = display_by_id.get(display_id)
        if cell is None:
            errors.append(f"{relative}: unknown display cell {display_id!r}")
            continue
        if item.get("target") != cell.get("target") or item.get("backend") != cell.get("backend"):
            errors.append(f"{relative}: target or backend does not match {display_id}")
            valid = False
        if stage == "gui":
            if item.get("input_method") not in input_methods:
                errors.append(f"{relative}: synthetic or unsupported GUI input method")
                valid = False
            required_kinds = {"screenshot", "client_state"}
            found_kinds = {attachment.get("kind") for attachment in item.get("attachments", [])}
            if required_kinds - found_kinds:
                errors.append(f"{relative}: GUI evidence is missing screenshot or client-state attachment")
                valid = False
            scenario = item.get("scenario")
            sdk_name = item.get("sdk")
            if not isinstance(scenario, str) or not isinstance(sdk_name, str):
                errors.append(f"{relative}: SDK or scenario identity is invalid")
                continue
            key = (sdk_name, display_id, scenario)
            if key not in gui_expected:
                errors.append(f"{relative}: unknown SDK/display/scenario cell {key}")
            elif key in gui_seen:
                errors.append(f"{relative}: duplicate GUI evidence cell {key}")
                valid = False
            if key in gui_expected and valid:
                gui_seen.add(key)
        elif stage == "hardware":
            if item.get("gpu_kind") not in hardware_kinds or not item.get("gpu_vendor"):
                errors.append(f"{relative}: hardware evidence does not identify a physical GPU")
                valid = False
            required_kinds = {"gpu_report", "accessibility_report"}
            found_kinds = {attachment.get("kind") for attachment in item.get("attachments", [])}
            if required_kinds - found_kinds:
                errors.append(f"{relative}: hardware evidence is missing GPU or accessibility report")
                valid = False
            if display_id in hardware_seen:
                errors.append(f"{relative}: duplicate hardware evidence for {display_id}")
                valid = False
            if valid:
                hardware_seen.add(display_id)
        else:
            if item.get("gpu_kind") not in hardware_kinds or not item.get("gpu_vendor"):
                errors.append(f"{relative}: benchmark evidence does not identify physical hardware")
                valid = False
            if item.get("native_endpoint") != "request_to_response" or item.get("ipc_endpoint") != "request_to_response":
                errors.append(f"{relative}: native and IPC benchmarks must use the same complete-response endpoint")
                valid = False
            if type(item.get("sample_count")) is not int or item["sample_count"] < 1000:
                errors.append(f"{relative}: benchmark requires at least 1,000 measured samples")
                valid = False
            workloads = item.get("benchmark_workloads")
            if not isinstance(workloads, list) or set(benchmark_workloads) - set(workloads):
                errors.append(f"{relative}: benchmark is missing required workloads")
                valid = False
            found_kinds = {attachment.get("kind") for attachment in item.get("attachments", [])}
            if "benchmark_samples" not in found_kinds:
                errors.append(f"{relative}: benchmark evidence is missing raw sample attachments")
                valid = False
            sample_attachment = next(
                (attachment for attachment in item.get("attachments", []) if attachment.get("kind") == "benchmark_samples"),
                None,
            )
            raw = None
            if sample_attachment is not None:
                try:
                    raw = json.loads((root / sample_attachment["path"]).read_text(encoding="utf-8"))
                except (OSError, json.JSONDecodeError, KeyError) as error:
                    errors.append(f"{relative}: cannot read benchmark raw samples: {error}")
                    valid = False
            for error in _benchmark_data_errors(raw, benchmark_workloads):
                errors.append(f"{relative}: {error}")
                valid = False
            if display_id in benchmark_seen:
                errors.append(f"{relative}: duplicate benchmark evidence for {display_id}")
                valid = False
            if valid:
                benchmark_seen.add(display_id)

    missing_gui = sorted(gui_expected - gui_seen)
    missing_hardware = sorted(set(display_by_id) - hardware_seen)
    missing_benchmarks = sorted(set(display_by_id) - benchmark_seen)
    if missing_gui:
        errors.append(f"missing {len(missing_gui)} installed GUI scenario cells")
    if missing_hardware:
        errors.append(f"missing {len(missing_hardware)} hardware/a11y display cells")
    if missing_benchmarks:
        errors.append(f"missing {len(missing_benchmarks)} native/IPC benchmark display cells")
    report.update(
        status="PASS" if not errors else "FAIL",
        expected_gui_cells=len(gui_expected),
        passed_gui_cells=len(gui_seen),
        missing_gui_cells=[{"sdk": sdk, "display_cell": display, "scenario": scenario} for sdk, display, scenario in missing_gui],
        expected_hardware_cells=len(display_by_id),
        passed_hardware_cells=len(hardware_seen),
        missing_hardware_cells=missing_hardware,
        expected_benchmark_cells=len(display_by_id),
        passed_benchmark_cells=len(benchmark_seen),
        missing_benchmark_cells=missing_benchmarks,
        errors=errors,
    )
    return report


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--mode", choices=("local", "full"), required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--source-sha", help="expected source SHA; defaults to repository HEAD in full mode")
    args = parser.parse_args(argv)
    try:
        source_sha = args.source_sha or (_git_sha() if args.mode == "full" else None)
        report = audit(args.root, args.mode, expected_source_sha=source_sha)
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    except (OSError, ValueError, json.JSONDecodeError) as error:
        parser.exit(1, f"evidence audit failed: {error}\n")
    print(
        f"{report['mode']} evidence audit: {report['status']} "
        f"({report['evidence_count']} records, {report['pass_count']} pass, {report['fail_count']} fail)"
    )
    return 0 if report["status"] in {"LOCAL_ONLY", "PASS"} else 1


if __name__ == "__main__":
    raise SystemExit(main())
