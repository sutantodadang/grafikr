import unittest

from scripts.benchmark_bindings import build_report


class BenchmarkReportTests(unittest.TestCase):
    def setUp(self):
        self.metadata = {
            "endpoint": "request_to_response",
            "workload": "state.get",
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
        self.assertEqual(report["scope"], "headless state.get request-to-response")
        self.assertEqual(report["hardware"], self.hardware)
        self.assertEqual(report["native"]["sample_count"], 3)
        self.assertEqual(report["native"]["p50_ns"], 20)
        self.assertEqual(report["ipc"]["p50_ns"], 50)
        self.assertEqual(report["endpoint"], "request_to_response")


if __name__ == "__main__":
    unittest.main()
