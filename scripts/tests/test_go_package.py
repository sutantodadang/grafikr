import tempfile
import unittest
import zipfile
from pathlib import Path

from scripts.verify_go_package import MODULE, VERSION, create_proxy, export_proxy


class GoPackageTests(unittest.TestCase):
    def test_local_proxy_contains_a_versioned_module_and_license(self):
        root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as temporary:
            proxy = Path(temporary)
            archive = create_proxy(root / "bindings/go", proxy, root / "LICENSE")
            prefix = f"{MODULE}@{VERSION}/"
            with zipfile.ZipFile(archive) as package:
                names = set(package.namelist())
                self.assertIn(prefix + "go.mod", names)
                self.assertIn(prefix + "client.go", names)
                self.assertIn(prefix + "LICENSE", names)
                self.assertTrue(all(name.startswith(prefix) for name in names))
            metadata = archive.parent
            self.assertEqual((metadata / "list").read_text(), VERSION + "\n")
            self.assertIn(VERSION, (metadata / f"{VERSION}.info").read_text())
            output = proxy / "package"
            export_proxy(proxy, output)
            exported = output / MODULE / "@v"
            self.assertEqual({path.name for path in exported.iterdir()}, {"list", f"{VERSION}.info", f"{VERSION}.mod", f"{VERSION}.zip"})


if __name__ == "__main__":
    unittest.main()
