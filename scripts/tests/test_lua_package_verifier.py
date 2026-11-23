import hashlib
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts.verify_lua_package import _indexed_artifact, verify


class LuaPackageVerifierTests(unittest.TestCase):
    def test_lua_command_name_resolves_through_path(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            executable = root / ("lua.exe" if os.name == "nt" else "lua")
            executable.write_text("", encoding="utf-8")
            luarocks = root / ("luarocks.exe" if os.name == "nt" else "luarocks")
            luarocks.write_text("", encoding="utf-8")
            modules = root / "modules"
            modules.mkdir()
            fixture_host = root / ("fixture-host.exe" if os.name == "nt" else "fixture-host")
            fixture_host.write_text("", encoding="utf-8")
            rock = root / "grafikr-1.1.0-1.src.rock"
            rock.write_bytes(b"rock")
            package_index = {
                "format": "grafikr-bindings-package-index-v1",
                "packages": [{
                    "sdk": "lua",
                    "target": "source",
                    "path": rock.name,
                    "sha256": hashlib.sha256(rock.read_bytes()).hexdigest(),
                }],
            }
            (root / "package-index.json").write_text(json.dumps(package_index), encoding="utf-8")
            commands = []

            def fake_tool(*argv, cwd=None):
                commands.append((argv, cwd))
                if argv[0] == str(luarocks):
                    tree = Path(argv[argv.index("--tree") + 1])
                    sdk_directory = tree / "share" / "lua" / "5.4"
                    sdk_directory.mkdir(parents=True)
                    (sdk_directory / "grafikr.lua").write_text("return {}", encoding="utf-8")

            with patch.dict(os.environ, {
                "PATH": str(root),
                "GRAFIKR_LUA54": "lua",
                "GRAFIKR_LUA54_MODULES": str(modules),
                "GRAFIKR_FIXTURE_HOST": str(fixture_host),
            }), patch("scripts.verify_lua_package._tool", side_effect=fake_tool):
                try:
                    verify(root)
                except ValueError as error:
                    self.fail(f"Lua command name should resolve through PATH: {error}")

            self.assertEqual(len(commands), 2)
            self.assertEqual(commands[1][0][0], str(executable.resolve()))

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
