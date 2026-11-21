import json
import os
import sys
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

from scripts.run_binding_checks import run_all_binding_checks, run_binding_checks
from scripts.runtime_evidence import validate_evidence


class BindingRunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.bundle = self.root / "bundle"
        self.bundle.mkdir()
        self.matrix_path = self.root / "matrix.json"
        self.out = self.root / "evidence.json"
        self.helper = self.root / "helper.py"
        self.helper.write_text("# test helper\n", encoding="utf-8")

    def tearDown(self):
        self.temp.cleanup()

    def matrix(self, suite):
        self.matrix_path.write_text(
            json.dumps(
                {
                    "coverage_requirements": {"cases": ["case.a"], "widgets": ["Button.click"]},
                    "sdks": [
                        {
                            "name": "demo",
                            "ecosystem": "Demo",
                            "runtime_floor": "1.0",
                            "suites": {"unit": suite},
                        }
                    ],
                }
            ),
            encoding="utf-8",
        )

    def run_suite(self, fixture_host=None, package_root=None):
        return run_binding_checks(
            "demo", "unit", self.bundle, self.out, self.matrix_path, self.root, fixture_host, package_root
        )

    def test_missing_suite_writes_failure_evidence(self):
        self.matrix(None)

        self.assertFalse(self.run_suite())
        evidence = json.loads(self.out.read_text(encoding="utf-8"))
        self.assertEqual(evidence["status"], "FAIL")
        self.assertIn("unit suite", " ".join(evidence["errors"]))

    def test_missing_helper_or_coverage_cannot_report_success(self):
        self.matrix(
            {
                "cwd": ".",
                "checks": [{"name": "unit", "argv": [sys.executable, "-c", "print('ok')"]}],
                "coverage": {"cases": ["case.a"], "widgets": ["Button.click"]},
                "helper_paths": ["missing.py"],
            }
        )

        self.assertFalse(self.run_suite())
        evidence = json.loads(self.out.read_text(encoding="utf-8"))
        self.assertEqual(evidence["checks"], [])
        self.assertEqual(evidence["status"], "FAIL")

    def test_all_pass_text_does_not_override_nonzero_exit(self):
        self.matrix(
            {
                "cwd": ".",
                "checks": [
                    {
                        "name": "fake",
                        "argv": [sys.executable, "-c", "print('all checks passed'); raise SystemExit(9)"],
                    }
                ],
                "coverage": {"cases": ["case.a"], "widgets": ["Button.click"]},
                "helper_paths": ["helper.py"],
            }
        )

        self.assertFalse(self.run_suite())
        evidence = json.loads(self.out.read_text(encoding="utf-8"))
        self.assertEqual(evidence["status"], "FAIL")
        self.assertEqual(evidence["checks"][0]["exit_code"], 9)

    def test_success_requires_helpers_coverage_and_zero_exit(self):
        fixture_host = self.root / "fixture-host.exe"
        fixture_host.write_text("headless fixture executable marker\n", encoding="utf-8")
        self.matrix(
            {
                "cwd": ".",
                "checks": [
                    {
                        "name": "unit",
                        "argv": [
                            sys.executable,
                            "-c",
                            "import os,sys; assert os.environ['GRAFIKR_RUNTIME_DIR']==sys.argv[1]; assert os.environ['GRAFIKR_FIXTURE_HOST']==sys.argv[2]",
                            "{runtime_dir}",
                            str(fixture_host),
                        ],
                    }
                ],
                "coverage": {"cases": ["case.a"], "widgets": ["Button.click"]},
                "helper_paths": ["helper.py"],
            }
        )

        self.assertTrue(self.run_suite(fixture_host.resolve()))
        evidence = json.loads(self.out.read_text(encoding="utf-8"))
        self.assertEqual(evidence["status"], "PASS")
        self.assertEqual(evidence["checks"][0]["exit_code"], 0)

    def test_python_command_uses_the_runners_current_interpreter(self):
        self.matrix(
            {
                "cwd": ".",
                "checks": [{"name": "python", "argv": ["python", "-c", "import sys; print(sys.executable)"]}],
                "helper_paths": ["helper.py"],
            }
        )

        self.assertTrue(self.run_suite())
        evidence = json.loads(self.out.read_text(encoding="utf-8"))
        self.assertEqual(evidence["checks"][0]["argv"][0], sys.executable)
        self.assertEqual(Path(evidence["checks"][0]["stdout"].strip()), Path(sys.executable))

    def test_package_root_is_required_and_substituted_for_installed_consumers(self):
        self.matrix(
            {
                "cwd": ".",
                "requires_package_root": True,
                "checks": [{
                    "name": "installed",
                    "argv": [sys.executable, "-c", "import os,sys; assert sys.argv[1] == os.environ['GRAFIKR_PACKAGE_ROOT']", "{package_root}"],
                }],
                "coverage": {"cases": ["case.a"], "widgets": ["Button.click"]},
                "helper_paths": ["helper.py"],
            }
        )
        self.assertFalse(self.run_suite())
        self.assertIn("requires an explicit package-root", " ".join(json.loads(self.out.read_text())["errors"]))

        package_root = self.root / "packages"
        package_root.mkdir()
        self.assertTrue(self.run_suite(package_root=package_root.resolve()))
        self.assertEqual(json.loads(self.out.read_text())["status"], "PASS")

    def test_lua_command_uses_explicit_lua54_override(self):
        loader_path = "PATH" if os.name == "nt" else "DYLD_LIBRARY_PATH" if sys.platform == "darwin" else "LD_LIBRARY_PATH"
        probe = (
            "import os; print(os.path.basename(os.getenv('LUA','')) == os.getenv('LUA_EXPECTED_EXECUTABLE')); "
            "print(os.getenv('LUA_PATH') == os.getenv('LUA_EXPECTED_PATH')); "
            "print(os.getenv('LUA_CPATH') == os.getenv('LUA_EXPECTED_CPATH')); "
            f"print(os.getenv('LUA_MODULES_EXPECTED','') in os.getenv('{loader_path}',''))"
        )
        self.matrix(
            {
                "cwd": ".",
                "checks": [{"name": "lua", "argv": ["lua", "-c", probe]}],
                "helper_paths": ["helper.py"],
            }
        )
        modules = self.root / "lua54"
        lua_share = modules / "share" / "lua" / "5.4"
        lua_lib = modules / "lib" / "lua" / "5.4"
        with patch.dict(
            os.environ,
            {
                "GRAFIKR_LUA54": sys.executable,
                "GRAFIKR_LUA54_MODULES": str(modules),
                "LUA_MODULES_EXPECTED": str(lua_lib),
                "LUA_EXPECTED_EXECUTABLE": Path(sys.executable).name,
                "LUA_EXPECTED_PATH": f"{lua_share / '?.lua'};{lua_share / '?/init.lua'};;",
                "LUA_EXPECTED_CPATH": ";".join(
                    [*(str(lua_lib / f"?.{extension}") for extension in ("dll", "so", "dylib")), "", ""]
                ),
            },
        ):
            self.assertTrue(self.run_suite())
        evidence = json.loads(self.out.read_text(encoding="utf-8"))
        self.assertEqual(evidence["checks"][0]["argv"][0], sys.executable)
        self.assertEqual(evidence["checks"][0]["stdout"].splitlines(), ["True"] * 4)

    def test_runtime_classifier_placeholder_uses_the_selected_bundle_target(self):
        (self.bundle / "manifest.json").write_text(
            json.dumps({"target": "x86_64-pc-windows-msvc", "engine_version": "1.1.0"}),
            encoding="utf-8",
        )
        self.matrix(
            {
                "cwd": ".",
                "checks": [
                    {
                        "name": "classifier",
                        "argv": [
                            sys.executable,
                            "-c",
                            "import sys; assert sys.argv[1] == 'windows-x64'",
                            "{runtime_classifier}",
                        ],
                    }
                ],
                "helper_paths": ["helper.py"],
            }
        )

        self.assertTrue(self.run_suite())
        evidence = json.loads(self.out.read_text(encoding="utf-8"))
        self.assertEqual(evidence["checks"][0]["argv"][-1], "windows-x64")

    def test_partial_check_results_never_become_success(self):
        self.matrix(
            {
                "cwd": ".",
                "checks": [
                    {"name": "first", "argv": [sys.executable, "-c", "print('ok')"]},
                    {"name": "second", "argv": [sys.executable, "-c", "print('all passed'); raise SystemExit(4)"]},
                ],
                "helper_paths": ["helper.py"],
            }
        )

        self.assertFalse(self.run_suite())
        evidence = json.loads(self.out.read_text(encoding="utf-8"))
        self.assertEqual(evidence["status"], "FAIL")
        self.assertEqual([check["status"] for check in evidence["checks"]], ["PASS", "FAIL"])
        self.assertEqual(evidence["checks"][1]["exit_code"], 4)

    def test_command_failure_keeps_diagnostic_before_large_output_tail(self):
        self.matrix(
            {
                "cwd": ".",
                "checks": [
                    {
                        "name": "swift-build",
                        "argv": [
                            sys.executable,
                            "-c",
                            "import sys; print('swift compiler diagnostic'); print('x' * 8000); raise SystemExit(1)",
                        ],
                    }
                ],
                "helper_paths": ["helper.py"],
            }
        )

        self.assertFalse(self.run_suite())
        evidence = json.loads(self.out.read_text(encoding="utf-8"))
        self.assertIn("swift compiler diagnostic", evidence["checks"][0]["stdout"])

    def test_missing_test_tool_is_evidence_not_an_exception(self):
        self.matrix(
            {
                "cwd": ".",
                "checks": [{"name": "missing", "argv": ["grafikr-tool-that-does-not-exist"]}],
                "helper_paths": ["helper.py"],
            }
        )

        self.assertFalse(self.run_suite())
        evidence = json.loads(self.out.read_text(encoding="utf-8"))
        self.assertEqual(evidence["checks"][0]["status"], "FAIL")
        self.assertIn("missing", evidence["checks"][0]["stderr"])
        self.assertIsNone(evidence["exit_code"])

    def test_timeout_is_a_failed_check(self):
        self.matrix(
            {
                "cwd": ".",
                "checks": [{"name": "slow", "argv": [sys.executable, "-c", "import time; time.sleep(1)"], "timeout_seconds": 0.02}],
                "helper_paths": ["helper.py"],
            }
        )

        self.assertFalse(self.run_suite())
        evidence = json.loads(self.out.read_text(encoding="utf-8"))
        self.assertIn("timed out", evidence["checks"][0]["stderr"])

    def test_contract_omission_of_widget_event_fails_after_checks_run(self):
        cases_path = self.root / "cases.json"
        cases_path.write_text(json.dumps({"cases": [{"id": "case.a"}]}), encoding="utf-8")
        widgets_path = self.root / "widgets.json"
        widgets_path.write_text(
            json.dumps({"widgets": {"Button": {"properties": {"label": "string"}, "events": ["click"]}}}),
            encoding="utf-8",
        )
        self.matrix_path.write_text(
            json.dumps(
                {
                    "coverage_requirements": {
                        "cases_file": "cases.json",
                        "widgets_file": "widgets.json",
                    },
                    "sdks": [
                        {
                            "name": "demo",
                            "consumer_language": "Demo",
                            "runtime_floor": "1.0",
                            "suites": {
                                "contract": {
                                    "cwd": ".",
                                    "checks": [{"name": "coverage-smoke", "argv": [sys.executable, "-c", "print('ran')"]}],
                                    "requires_fixture_host": True,
                                    "helper_paths": ["helper.py"],
                                    "coverage": {
                                        "cases": ["case.a"],
                                        "widgets": ["Button", "Button.label"],
                                    },
                                }
                            },
                        }
                    ],
                }
            ),
            encoding="utf-8",
        )
        fixture = self.root / "fixture.exe"
        fixture.write_text("fixture", encoding="utf-8")

        passed = run_binding_checks(
            "demo", "contract", self.bundle, self.out, self.matrix_path, self.root, fixture.resolve()
        )
        evidence = json.loads(self.out.read_text(encoding="utf-8"))
        self.assertFalse(passed)
        self.assertEqual(evidence["checks"][0]["status"], "PASS")
        self.assertEqual(evidence["checks"][0]["stdout"], "ran\n")
        self.assertIn("Button@click", " ".join(evidence["errors"]))

    def test_shared_cases_are_unique_and_machine_readable(self):
        root = Path(__file__).resolve().parents[2]
        corpus = json.loads(
            (root / "bindings/protocol/v1/fixtures/sdk-cases.json").read_text(encoding="utf-8")
        )
        rows = corpus["cases"]
        ids = [row["id"] for row in rows]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertGreaterEqual(len(rows), 13)
        for row in rows:
            with self.subTest(case=row["id"]):
                self.assertIsInstance(row.get("scenario"), dict)
                self.assertIn("action", row["scenario"])
                self.assertIsInstance(row["scenario"].get("input"), dict)
                self.assertIsInstance(row["scenario"].get("expected"), dict)

    def test_sdk_matrix_has_all_consumer_rows_and_existing_helpers(self):
        root = Path(__file__).resolve().parents[2]
        matrix = json.loads((root / "bindings/sdk-matrix.json").read_text(encoding="utf-8"))
        rows = matrix["sdks"]
        names = [row["name"] for row in rows]
        self.assertEqual(len(names), len(set(names)))
        self.assertEqual(
            set(names),
            {"c", "cpp", "python", "javascript", "typescript", "dotnet", "go", "java", "kotlin", "swift", "dart", "ruby", "php", "lua"},
        )
        self.assertEqual(matrix["evidence"]["directory"], ".artifacts/bindings")
        for sdk in rows:
            with self.subTest(sdk=sdk["name"]):
                self.assertEqual(set(sdk["suites"]), {"unit", "contract", "installed"})
                for suite in sdk["suites"].values():
                    for helper in suite["helper_paths"]:
                        self.assertTrue((root / helper).is_file(), helper)

    def test_all_sdk_run_writes_per_sdk_and_aggregate_evidence(self):
        rows = []
        for name in ("alpha", "beta"):
            rows.append(
                {
                    "name": name,
                    "consumer_language": name,
                    "sdk_version": "1.0.0",
                    "suites": {
                        "unit": {
                            "cwd": ".",
                            "checks": [{"name": name, "argv": [sys.executable, "-c", "print('ok')"]}],
                            "helper_paths": ["helper.py"],
                        }
                    },
                }
            )
        self.matrix_path.write_text(
            json.dumps({"protocol_version": {"major": 1, "minor": 0}, "sdks": rows}),
            encoding="utf-8",
        )
        aggregate = self.root / "all-unit.json"

        self.assertTrue(run_all_binding_checks("unit", self.bundle, aggregate, self.matrix_path, self.root))

        report = json.loads(aggregate.read_text(encoding="utf-8"))
        self.assertEqual(report["status"], "PASS")
        self.assertEqual({row["sdk"] for row in report["results"]}, {"alpha", "beta"})
        for row in report["results"]:
            evidence = json.loads((self.root / row["evidence"]).read_text(encoding="utf-8"))
            self.assertEqual(evidence["status"], "PASS")
            validate_evidence(evidence)


if __name__ == "__main__":
    unittest.main()
