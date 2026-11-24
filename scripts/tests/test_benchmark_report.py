import unittest

from scripts.benchmark_bindings import _require_release_runtime, build_report


class BenchmarkReportTests(unittest.TestCase):
    def setUp(self):
        self.metadata = {
            "endpoint": "request_to_response",
            "workload": "request_latency",
            "warmup_count": 100,
            "measured_count": 3,
        }
        self.hardware = {
            "os": "Windows 11",
            "architecture": "AMD64",
            "cpu": "test CPU",
            "logical_cpu_count": 8,
        }

    def test_report_requires_raw_samples_for_both_endpoints(self):
        with self.assertRaisesRegex(ValueError, "samples"):
            build_report(
                {**self.metadata, "samples_ns": [1, 2, 3]},
                {**self.metadata, "samples_ns": []},
                self.hardware,
            )

    def test_report_requires_hardware_metadata(self):
        with self.assertRaisesRegex(ValueError, "hardware"):
            build_report(
                {**self.metadata, "samples_ns": [1, 2, 3]},
                {**self.metadata, "samples_ns": [2, 3, 4]},
                {"os": "Windows 11"},
            )

    def test_report_rejects_mixed_latency_endpoints(self):
        with self.assertRaisesRegex(ValueError, "endpoint"):
            build_report(
                {**self.metadata, "samples_ns": [1, 2, 3]},
                {**self.metadata, "endpoint": "submit_accept", "samples_ns": [2, 3, 4]},
                self.hardware,
            )

    def test_report_rejects_incomplete_sample_plan(self):
        native = {**self.metadata, "samples_ns": [1, 2, 3], "warmup_count": -1}
        ipc = {**self.metadata, "samples_ns": [2, 3, 4], "warmup_count": -1}
        with self.assertRaisesRegex(ValueError, "sample plan"):
            build_report(native, ipc, self.hardware)

    def test_report_summarizes_identical_endpoints_and_preserves_sample_counts(self):
        report = build_report(
            {**self.metadata, "samples_ns": [30, 10, 20]},
            {**self.metadata, "samples_ns": [60, 40, 50]},
            self.hardware,
        )
        self.assertEqual(report["scope"], "headless native/IPC request and label update latency")
        self.assertEqual(report["hardware"], self.hardware)
        self.assertEqual(report["native"]["sample_count"], 3)
        self.assertEqual(report["native"]["p50_ns"], 20)
        self.assertEqual(report["ipc"]["p50_ns"], 50)
        self.assertEqual(report["endpoint"], "request_to_response")
        self.assertEqual(report["workloads"]["request_latency"]["native"]["sample_count"], 3)

    def test_report_rejects_workload_mismatch(self):
        native = {**self.metadata, "workload": "label_update", "samples_ns": [1, 2, 3]}
        ipc = {**self.metadata, "samples_ns": [2, 3, 4]}
        with self.assertRaisesRegex(ValueError, "workload"):
            build_report(native, ipc, self.hardware)

    def test_report_keeps_request_and_label_update_as_distinct_workloads(self):
        request_native = {**self.metadata, "samples_ns": [10, 20, 30]}
        request_ipc = {**self.metadata, "samples_ns": [20, 30, 40]}
        label_native = {
            **self.metadata,
            "workload": "label_update",
            "operation": "property.update",
            "samples_ns": [100, 200, 300],
        }
        label_ipc = {**label_native, "samples_ns": [200, 300, 400]}

        report = build_report(
            {"request_latency": request_native, "label_update": label_native},
            {"request_latency": request_ipc, "label_update": label_ipc},
            self.hardware,
        )

        self.assertEqual(set(report["workloads"]), {"request_latency", "label_update"})
        self.assertEqual(report["workloads"]["label_update"]["ipc"]["p50_ns"], 300)

    def test_benchmark_requires_clean_non_fixture_runtime(self):
        clean = {"source_dirty": False, "development_fixture": False}
        _require_release_runtime(clean)
        with self.assertRaisesRegex(ValueError, "clean source tree"):
            _require_release_runtime({**clean, "source_dirty": True})
        with self.assertRaisesRegex(ValueError, "development fixture"):
            _require_release_runtime({**clean, "development_fixture": True})


if __name__ == "__main__":
    unittest.main()
