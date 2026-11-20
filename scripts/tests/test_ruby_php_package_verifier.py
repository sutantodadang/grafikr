import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts.verify_ruby_php_packages import _artifact, _php_command


class RubyPhpPackageVerifierTests(unittest.TestCase):
    def test_php_command_enables_only_explicit_local_openssl_and_zip_extensions(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            executable = root / "php.exe"
            openssl = root / "ext" / "php_openssl.dll"
            zip_extension = root / "ext" / "php_zip.dll"
            for path in (executable, openssl, zip_extension):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"test")
            with patch.dict(
                "os.environ",
                {
                    "GRAFIKR_PHP_BINARY": str(executable),
                    "GRAFIKR_PHP_OPENSSL_EXTENSION": str(openssl),
                    "GRAFIKR_PHP_ZIP_EXTENSION": str(zip_extension),
                },
                clear=False,
            ):
                command = _php_command()
            self.assertEqual(
                command,
                [
                    str(executable), "-d", f"extension_dir={openssl.parent}",
                    "-d", "extension=openssl", "-d", "extension=zip",
                ],
            )

    def test_artifact_lookup_requires_one_safe_existing_target_record(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package = root / "runtime.gem"
            package.write_bytes(b"gem")
            index = {"packages": [{"sdk": "ruby", "target": "x86_64-unknown-linux-gnu", "path": package.name}]}
            self.assertEqual(
                _artifact(root, index, "ruby", "x86_64-unknown-linux-gnu"), package
            )
            index["packages"].append(dict(index["packages"][0]))
            with self.assertRaisesRegex(ValueError, "exactly one"):
                _artifact(root, index, "ruby", "x86_64-unknown-linux-gnu")

    def test_artifact_lookup_rejects_unsafe_or_missing_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for path, message in (("../outside.gem", "unsafe"), ("missing.gem", "missing or unsafe")):
                index = {"packages": [{"sdk": "ruby", "target": "linux", "path": path}]}
                with self.subTest(path=path), self.assertRaisesRegex(ValueError, message):
                    _artifact(root, index, "ruby", "linux")


if __name__ == "__main__":
    unittest.main()
