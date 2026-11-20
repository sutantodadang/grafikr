import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from scripts.check_bindings_evidence import audit


ROOT = Path(__file__).resolve().parents[2]


def write_evidence(root: Path, name: str, record: dict) -> None:
    (root / name).write_text(json.dumps(record), encoding="utf-8")


def attachment(root: Path, name: str, kind: str) -> dict:
    path = root / name
    path.write_bytes(name.encode("utf-8"))
    return {"path": name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "kind": kind}


def gui_record(root: Path, stage: str, *, input_method: str = "manual_supervised") -> dict:
    attachments = [attachment(root, "screen.png", "screenshot"), attachment(root, "client-state.txt", "client_state")]
    record = {
        "stage": stage,
        "sdk": "python",
        "consumer_language": "Python",
        "engine_version": "1.0.0",
        "sdk_version": "1.0.0",
        "protocol_version": {"major": 1, "minor": 0},
        "runtime_version": "1.0.0",
        "target": "x86_64-pc-windows-msvc",
        "backend": "D3D12",
        "gpu_kind": "physical" if stage == "hardware" else "hardware",
        "gpu_vendor": "test-vendor",
        "source_sha": "a" * 40,
        "runner_sha": "b" * 64,
        "artifact_sha256": "c" * 64,
        "command": [{"name": "gui", "argv": ["consumer"]}],
        "exit_code": 0,
        "status": "PASS",
        "started_at": "2026-11-15T00:00:00Z",
        "finished_at": "2026-11-15T00:01:00Z",
        "attachments": attachments,
        "reason": None,
        "display_cell": "win-x64-d3d12",
        "input_method": input_method,
    }
    if stage == "gui":
        record["scenario"] = "counter_form"
    else:
        record["attachments"] = [attachment(root, "gpu.txt", "gpu_report"), attachment(root, "a11y.txt", "accessibility_report")]
    return record


class BindingsEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.matrix = self.root / "matrix.json"
        self.cases = self.root / "gui-cases.json"
        self.matrix.write_text(json.dumps({"sdks": [{"name": "python"}]}), encoding="utf-8")
        self.cases.write_text(
            json.dumps({
                "display_cells": [{"id": "win-x64-d3d12", "target": "x86_64-pc-windows-msvc", "backend": "D3D12"}],
                "scenarios": ["counter_form"],
                "benchmark_workloads": ["request_latency", "label_update", "scroll_50000", "resource_cycles_100", "rss_counters", "idle_redraw_4s"],
                "os_input_methods": ["windows_sendinput", "xdotool", "osascript", "manual_supervised"],
                "hardware_gpu_kinds": ["integrated", "discrete", "physical"],
            }),
            encoding="utf-8",
        )

    def tearDown(self):
        self.temp.cleanup()

    def test_local_mode_reports_subset_without_claiming_full_support(self):
        write_evidence(self.root, "unit.json", {
            "stage": "unit", "sdk": "python", "consumer_language": "Python",
            "engine_version": "1.0.0", "sdk_version": "1.0.0",
            "protocol_version": {"major": 1, "minor": 0}, "runtime_version": None,
            "target": "Windows-AMD64", "backend": "headless", "gpu_kind": None, "gpu_vendor": None,
            "source_sha": "a" * 40, "runner_sha": "b" * 64, "artifact_sha256": "c" * 64,
            "command": [], "exit_code": 0, "status": "PASS",
            "started_at": "2026-11-15T00:00:00Z", "finished_at": "2026-11-15T00:01:00Z",
            "attachments": [], "reason": None,
        })
        result = audit(self.root, "local", self.matrix, self.cases)
        self.assertEqual(result["status"], "LOCAL_ONLY")
        self.assertEqual(result["evidence_count"], 1)

    def test_full_mode_requires_every_cell_and_rejects_synthetic_gui_input(self):
        gui = gui_record(self.root, "gui", input_method="synthetic_ui_event")
        gui.update(scenario="counter_form")
        write_evidence(self.root, "gui.json", gui)
        result = audit(self.root, "full", self.matrix, self.cases, expected_source_sha="a" * 40)
        self.assertEqual(result["status"], "FAIL")
        self.assertTrue(any("synthetic" in error for error in result["errors"]))
        self.assertTrue(any("hardware" in error for error in result["errors"]))

    def test_full_mode_passes_only_with_gui_and_hardware_evidence(self):
        write_evidence(self.root, "gui.json", gui_record(self.root, "gui"))
        write_evidence(self.root, "hardware.json", gui_record(self.root, "hardware"))
        write_evidence(self.root, "benchmark.json", self.benchmark_record())
        result = audit(self.root, "full", self.matrix, self.cases, expected_source_sha="a" * 40)
        self.assertEqual(result["status"], "PASS", result)
        self.assertEqual(result["missing_gui_cells"], [])
        self.assertEqual(result["missing_hardware_cells"], [])
        self.assertEqual(result["missing_benchmark_cells"], [])

    def test_full_mode_requires_native_ipc_benchmarks_per_display_cell(self):
        write_evidence(self.root, "gui.json", gui_record(self.root, "gui"))
        write_evidence(self.root, "hardware.json", gui_record(self.root, "hardware"))
        result = audit(self.root, "full", self.matrix, self.cases, expected_source_sha="a" * 40)
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(result["missing_benchmark_cells"], ["win-x64-d3d12"])
        self.assertTrue(any("benchmark" in error for error in result["errors"]))

    def test_full_mode_rejects_benchmark_claims_missing_raw_workload_data(self):
        write_evidence(self.root, "gui.json", gui_record(self.root, "gui"))
        write_evidence(self.root, "hardware.json", gui_record(self.root, "hardware"))
        benchmark = self.benchmark_record()
        raw_path = self.root / benchmark["attachments"][0]["path"]
        raw = json.loads(raw_path.read_text(encoding="utf-8"))
        raw["workloads"].pop("scroll_50000")
        raw_path.write_text(json.dumps(raw), encoding="utf-8")
        benchmark["attachments"][0]["sha256"] = hashlib.sha256(raw_path.read_bytes()).hexdigest()
        write_evidence(self.root, "benchmark.json", benchmark)
        result = audit(self.root, "full", self.matrix, self.cases, expected_source_sha="a" * 40)
        self.assertEqual(result["status"], "FAIL")
        self.assertTrue(any("scroll_50000" in error for error in result["errors"]))

    def benchmark_record(self, *, ipc_endpoint: str = "request_to_response") -> dict:
        samples = [1000] * 1000
        raw_data = {
            "format": "grafikr-bindings-benchmark-evidence-v1",
            "workloads": {
                "request_latency": {
                    endpoint: {"endpoint": "request_to_response", "samples_ns": samples}
                    for endpoint in ("native", "ipc")
                },
                "label_update": {
                    endpoint: {"endpoint": "request_to_response", "samples_ns": samples}
                    for endpoint in ("native", "ipc")
                },
                "scroll_50000": {
                    endpoint: {"rows": 50000, "elapsed_ns": 1000, "visible_range_rows": 32}
                    for endpoint in ("native", "ipc")
                },
                "resource_cycles_100": {
                    endpoint: {
                        "cycles": 100, "anchor_window_open": True,
                        "objects_before": 10, "objects_after": 10,
                        "resources_before": 2, "resources_after": 2,
                    }
                    for endpoint in ("native", "ipc")
                },
                "rss_counters": {
                    endpoint: {"before_bytes": 1000, "after_bytes": 1200}
                    for endpoint in ("native", "ipc")
                },
                "idle_redraw_4s": {
                    endpoint: {"duration_ms": 4000, "redraw_delta": 0}
                    for endpoint in ("native", "ipc")
                },
            },
        }
        raw_path = self.root / "benchmark-raw.json"
        raw_path.write_text(json.dumps(raw_data), encoding="utf-8")
        raw = {"path": raw_path.name, "sha256": hashlib.sha256(raw_path.read_bytes()).hexdigest(), "kind": "benchmark_samples"}
        return {
            **gui_record(self.root, "hardware"),
            "stage": "benchmark",
            "sdk": "runtime",
            "consumer_language": "C ABI and IPC",
            "gpu_kind": "physical",
            "display_cell": "win-x64-d3d12",
            "native_endpoint": "request_to_response",
            "ipc_endpoint": ipc_endpoint,
            "sample_count": 1000,
            "benchmark_workloads": ["request_latency", "label_update", "scroll_50000", "resource_cycles_100", "rss_counters", "idle_redraw_4s"],
            "attachments": [raw],
        }

    def test_full_mode_rejects_stale_source_sha(self):
        record = gui_record(self.root, "gui")
        record["source_sha"] = "d" * 40
        write_evidence(self.root, "gui.json", record)
        write_evidence(self.root, "hardware.json", gui_record(self.root, "hardware"))
        result = audit(self.root, "full", self.matrix, self.cases, expected_source_sha="a" * 40)
        self.assertEqual(result["status"], "FAIL")
        self.assertTrue(any("stale source SHA" in error for error in result["errors"]))

    def test_repository_full_gate_requires_336_gui_scenarios_and_eight_hardware_cells(self):
        result = audit(self.root, "full", expected_source_sha="a" * 40)
        self.assertEqual(result["expected_gui_cells"], 336)
        self.assertEqual(result["expected_hardware_cells"], 8)
        self.assertEqual(result["expected_benchmark_cells"], 8)
        self.assertEqual(len(result["missing_benchmark_cells"]), 8)
        self.assertEqual(result["status"], "FAIL")


if __name__ == "__main__":
    unittest.main()
