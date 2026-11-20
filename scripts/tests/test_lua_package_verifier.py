import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from scripts.verify_lua_package import _indexed_artifact


class LuaPackageVerifierTests(unittest.TestCase):
    def test_indexed_source_rock_must_match_checksum(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rock = root / "lua" / "grafikr-1.1.0-1.src.rock"
            rock.parent.mkdir()
            rock.write_bytes(b"rock")
            index = {
                "format": "grafikr-bindings-package-index-v1",
                "packages": [{
                    "sdk": "lua",
                    "target": "source",
                    "path": "lua/grafikr-1.1.0-1.src.rock",
                    "sha256": hashlib.sha256(rock.read_bytes()).hexdigest(),
                }],
            }
            (root / "package-index.json").write_text(json.dumps(index), encoding="utf-8")
            self.assertEqual(_indexed_artifact(root), rock)
            rock.write_bytes(b"tampered")
            with self.assertRaisesRegex(ValueError, "checksum"):
                _indexed_artifact(root)

    def test_indexed_source_rock_rejects_traversal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "package-index.json").write_text(
                json.dumps({
                    "format": "grafikr-bindings-package-index-v1",
                    "packages": [{"sdk": "lua", "target": "source", "path": "../outside", "sha256": "0" * 64}],
                }),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "unsafe"):
                _indexed_artifact(root)


if __name__ == "__main__":
    unittest.main()
