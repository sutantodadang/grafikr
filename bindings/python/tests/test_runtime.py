import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from grafikr.runtime import PACKAGE_RUNTIME_DIR, runtime_host_path, target_triple


class RuntimeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="grafikr runtime ")
        self.root = Path(self.temp.name) / "Gráfïkr bundle"
        self.root.mkdir()

    def tearDown(self):
        self.temp.cleanup()

    def bundle(self, *, target=None, development_fixture=False):
        binary = self.root / "bin" / ("grafikr-host.exe" if target_triple().endswith("-msvc") else "grafikr-host")
        binary.parent.mkdir(parents=True, exist_ok=True)
        binary.write_bytes(b"verified runtime host")
        manifest = {
            "target": target or target_triple(),
            "development_fixture": development_fixture,
            "min_os": _minimum_os(target or target_triple()),
            "min_glibc": "2.35" if (target or target_triple()).endswith("-unknown-linux-gnu") else None,
            "files": [
                {
                    "path": binary.relative_to(self.root).as_posix(),
                    "role": "host",
                    "size": binary.stat().st_size,
                    "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                }
            ],
        }
        (self.root / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        return binary, manifest

    def test_explicit_unicode_bundle_resolves_host_independent_of_cwd(self):
        binary, _ = self.bundle()
        self.assertEqual(runtime_host_path(self.root), binary.resolve())

    def test_default_runtime_comes_from_installed_package_resources(self):
        binary, _ = self.bundle()
        with patch("grafikr.runtime.PACKAGE_RUNTIME_DIR", self.root.parent):
            installed_root = self.root.parent / target_triple()
            self.root.rename(installed_root)
            self.assertEqual(runtime_host_path(), (installed_root / "bin" / binary.name).resolve())

    def test_rejects_wrong_target_fixture_host_and_tampered_binary(self):
        current = target_triple()
        wrong = current.replace("x86_64", "aarch64") if "x86_64" in current else current.replace("aarch64", "x86_64")
        binary, manifest = self.bundle(target=wrong)
        with self.assertRaises(ValueError):
            runtime_host_path(self.root)

        manifest["target"] = target_triple()
        manifest["development_fixture"] = True
        (self.root / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaises(ValueError):
            runtime_host_path(self.root)

        manifest["development_fixture"] = False
        (self.root / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        binary.write_bytes(b"modified host")
        with self.assertRaises(ValueError):
            runtime_host_path(self.root)

    def test_rejects_minimum_os_metadata_that_does_not_match_target(self):
        _, manifest = self.bundle()
        manifest["min_os"] = "Windows 7"
        (self.root / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaises(ValueError):
            runtime_host_path(self.root)

    def test_missing_bundle_has_an_actionable_error(self):
        with self.assertRaisesRegex(ValueError, "runtime bundle"):
            runtime_host_path(self.root / "missing")


def _minimum_os(target: str) -> str:
    if target.endswith("-msvc"):
        return "Windows 11 24H2 build 26100"
    if target.endswith("-apple-darwin"):
        return "macOS 14"
    return "Ubuntu 22.04 / glibc 2.35"


if __name__ == "__main__":
    unittest.main()
