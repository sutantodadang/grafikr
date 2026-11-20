import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts.package_python import package, wheel_platform_tag


class PythonWheelTests(unittest.TestCase):
    def test_each_runtime_target_maps_to_a_platform_wheel_tag(self):
        self.assertEqual(wheel_platform_tag("x86_64-pc-windows-msvc"), "win_amd64")
        self.assertEqual(wheel_platform_tag("aarch64-pc-windows-msvc"), "win_arm64")
        self.assertEqual(wheel_platform_tag("x86_64-unknown-linux-gnu"), "manylinux_2_35_x86_64")
        self.assertEqual(wheel_platform_tag("aarch64-unknown-linux-gnu"), "manylinux_2_35_aarch64")
        self.assertEqual(wheel_platform_tag("x86_64-apple-darwin"), "macosx_14_0_x86_64")
        self.assertEqual(wheel_platform_tag("aarch64-apple-darwin"), "macosx_14_0_arm64")

    def test_unknown_target_cannot_be_published_as_a_wheel(self):
        with self.assertRaises(ValueError):
            wheel_platform_tag("mystery-os-target")

    def test_python_packager_rejects_fixtures_and_sdk_engine_drift(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "wheels"
            with patch(
                "scripts.package_python.validate_manifest",
                return_value={"development_fixture": True, "engine_version": "1.0.0", "target": "x86_64-pc-windows-msvc"},
            ):
                with self.assertRaisesRegex(ValueError, "development fixture"):
                    package(Path(temporary) / "bundle", output)
            with patch(
                "scripts.package_python.validate_manifest",
                return_value={"development_fixture": False, "engine_version": "2.0.0", "target": "x86_64-pc-windows-msvc"},
            ):
                with self.assertRaisesRegex(ValueError, "version"):
                    package(Path(temporary) / "bundle", output)


if __name__ == "__main__":
    unittest.main()
