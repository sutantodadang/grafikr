"""Guard the SDK runner against incomplete declared coverage and install helpers."""

import json
import sys
import tempfile
import unittest
from pathlib import Path

from scripts.run_binding_checks import run_binding_checks


class SDKCoverageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.bundle = self.root / "bundle"
        self.bundle.mkdir()
        self.fixture = self.root / "fixture-host.exe"
        self.fixture.write_bytes(b"fixture")
        self.helper = self.root / "contract.py"
        self.helper.write_text("# contract helper\n", encoding="utf-8")
        self.cases = self.root / "cases.json"
        self.cases.write_text(json.dumps({"cases": [
            {"id": "transport.responses", "area": "transport"},
            {"id": "installed.counter_form", "area": "installed"},
            {"id": "installed.dashboard", "area": "installed"},
        ]}), encoding="utf-8")
        self.widgets = self.root / "widgets.json"
        self.widgets.write_text(json.dumps({"widgets": {
            "Button": {"properties": {"label": "string"}, "events": ["click"]},
            "Text": {"properties": {"text": "string"}, "events": []},
        }}), encoding="utf-8")
        self.matrix = self.root / "matrix.json"
        self.out = self.root / "evidence.json"

    def tearDown(self):
        self.temp.cleanup()

    def run_sdk(self, suite_name, coverage, helper_paths=None):
        suite = {
            "cwd": ".",
            "checks": [{"name": "contract-smoke", "argv": [sys.executable, "-c", "from pathlib import Path; Path('executed.txt').write_text('yes')"]}],
            "helper_paths": helper_paths or ["contract.py"],
            "coverage": coverage,
        }
        if suite_name == "contract":
            suite["requires_fixture_host"] = True
        matrix = {
            "coverage_requirements": {"cases_file": "cases.json", "widgets_file": "widgets.json"},
            "sdks": [{"name": "demo", "ecosystem": "Demo SDK", "consumer_language": "Demo", "sdk_version": "1.0.0", "suites": {suite_name: suite}}],
        }
        self.matrix.write_text(json.dumps(matrix), encoding="utf-8")
        result = run_binding_checks(
            "demo", suite_name, self.bundle, self.out, self.matrix, self.root,
            self.fixture.resolve() if suite_name == "contract" else None,
        )
        return result, json.loads(self.out.read_text(encoding="utf-8"))

    def test_contract_missing_coverage_fails_after_declared_checks_run(self):
        passed, evidence = self.run_sdk("contract", {
            "cases": [],
            "widgets": ["Button", "Button.label", "Text", "Text.text"],
        })

        self.assertFalse(passed)
        self.assertEqual(evidence["sdk"], "demo")
        self.assertEqual((self.root / "executed.txt").read_text(), "yes")
        self.assertEqual(evidence["checks"][0]["status"], "PASS")
        message = " ".join(evidence["errors"])
        self.assertIn("Button@click", message)
        self.assertIn("transport.responses", message)

    def test_installed_missing_scenario_and_consumer_helper_fail(self):
        passed, evidence = self.run_sdk(
            "installed",
            {"cases": ["installed.counter_form"], "widgets": []},
            ["missing/consumer-worker.js"],
        )

        self.assertFalse(passed)
        self.assertEqual(evidence["sdk"], "demo")
        self.assertEqual(evidence["checks"], [])
        message = " ".join(evidence["errors"])
        self.assertIn("missing/consumer-worker.js", message)
        self.assertIn("installed.dashboard", message)

    def test_case_exclusions_are_explicit_and_sdk_specific(self):
        cases = json.loads(self.cases.read_text(encoding="utf-8"))
        cases["cases"][0]["exclude_sdks"] = ["demo"]
        cases["cases"][0]["exclusion_reason"] = "The demo transport does not own a child process."
        self.cases.write_text(json.dumps(cases), encoding="utf-8")
        passed, evidence = self.run_sdk("contract", {
            "cases": ["installed.counter_form", "installed.dashboard"],
            "widgets": ["Button", "Button.label", "Button@click", "Text", "Text.text"],
        })
        self.assertTrue(passed, evidence["errors"])

        cases["cases"][0]["exclude_sdks"] = ["unknown"]
        self.cases.write_text(json.dumps(cases), encoding="utf-8")
        passed, evidence = self.run_sdk("contract", {
            "cases": ["installed.counter_form", "installed.dashboard"],
            "widgets": ["Button", "Button.label", "Button@click", "Text", "Text.text"],
        })
        self.assertFalse(passed)
        self.assertIn("coverage requirements are invalid", " ".join(evidence["errors"]))


if __name__ == "__main__":
    unittest.main()
