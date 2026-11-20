import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts import run_cmake_binding_tests


class CMakeBindingTests(unittest.TestCase):
    def test_installed_consumer_mode_skips_source_build_and_runs_bundle_consumer(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "target").mkdir()
            runtime = root / "runtime"
            runtime.mkdir()
            with (
                patch.object(run_cmake_binding_tests, "ROOT", root),
                patch.object(run_cmake_binding_tests, "validate_manifest", return_value={"target": "x86_64-pc-windows-msvc"}),
                patch.object(run_cmake_binding_tests, "_verify_bundle_cmake_package") as consumer,
                patch.object(run_cmake_binding_tests.subprocess, "run") as build,
            ):
                run_cmake_binding_tests.run("c", runtime, installed_consumer_only=True)

            consumer.assert_called_once_with(runtime, root / "target")
            build.assert_not_called()


if __name__ == "__main__":
    unittest.main()
