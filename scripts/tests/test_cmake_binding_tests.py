import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from scripts import run_cmake_binding_tests


class CMakeBindingTests(unittest.TestCase):
    def test_installed_c_scenarios_include_private_thread_header(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / "target"
            target.mkdir()
            runtime = root / "runtime"
            examples = runtime / "share/grafikr/examples"
            examples.mkdir(parents=True)
            suffix = ".exe" if run_cmake_binding_tests.os.name == "nt" else ""
            for name in (
                "grafikr_counter_form",
                "grafikr_dashboard",
                "grafikr_multi_window",
                "grafikr_cpp_counter_form",
                "grafikr_cpp_dashboard",
                "grafikr_cpp_multi_window",
            ):
                (examples / f"{name}{suffix}").touch()

            generated = []

            def capture_configure(command, **_kwargs):
                if command[1] == "-S":
                    generated.append((Path(command[2]) / "CMakeLists.txt").read_text())
                return SimpleNamespace(returncode=0, stdout="", stderr="")

            with (
                patch.object(run_cmake_binding_tests, "ROOT", root),
                patch.object(run_cmake_binding_tests.subprocess, "run", side_effect=capture_configure),
            ):
                run_cmake_binding_tests._verify_bundle_cmake_package(runtime, target)

            self.assertIn(
                f'target_include_directories(c-installed-scenarios PRIVATE "{(root / "bindings/c/include").as_posix()}")',
                generated[0],
            )

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
