import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from scripts.verify_dart_package import _indexed_dart_artifact


class DartPackageVerifierTests(unittest.TestCase):
    def test_indexed_source_archive_must_match_its_checksum(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "dart" / "grafikr-dart-1.1.0.tar.gz"
            archive.parent.mkdir()
            archive.write_bytes(b"source package")
            index = {
                "format": "grafikr-bindings-package-index-v1",
                "packages": [{
                    "sdk": "dart",
                    "target": "source",
                    "path": "dart/grafikr-dart-1.1.0.tar.gz",
                    "sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
                }],
            }
            (root / "package-index.json").write_text(json.dumps(index), encoding="utf-8")
            self.assertEqual(_indexed_dart_artifact(root), archive)
            archive.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "checksum"):
                _indexed_dart_artifact(root)

    def test_indexed_source_archive_rejects_traversal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "package-index.json").write_text(
                json.dumps({
                    "format": "grafikr-bindings-package-index-v1",
                    "packages": [{"sdk": "dart", "target": "source", "path": "../outside", "sha256": "0" * 64}],
                }),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "unsafe"):
                _indexed_dart_artifact(root)


if __name__ == "__main__":
    unittest.main()
