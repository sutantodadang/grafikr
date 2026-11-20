"""Keep the cross-language public documentation complete and internally linked."""

import json
import re
import unittest
from pathlib import Path
from urllib.parse import unquote, urlsplit


ROOT = Path(__file__).resolve().parents[2]
SDK_DIRS = {
    "c": "c",
    "cpp": "cpp",
    "python": "python",
    "javascript": "node",
    "typescript": "node",
    "dotnet": "dotnet",
    "go": "go",
    "java": "jvm",
    "kotlin": "jvm",
    "swift": "swift",
    "dart": "dart",
    "ruby": "ruby",
    "php": "php",
    "lua": "lua",
}
PACKAGE_FILES = {
    "c": ("CMakeLists.txt",),
    "cpp": ("CMakeLists.txt",),
    "python": ("pyproject.toml",),
    "javascript": ("package.json",),
    "typescript": ("package.json",),
    "dotnet": ("Grafikr.Client.csproj",),
    "go": ("go.mod",),
    "java": ("build.gradle.kts", "build.gradle"),
    "kotlin": ("build.gradle.kts", "build.gradle"),
    "swift": ("Package.swift",),
    "dart": ("pubspec.yaml",),
    "ruby": ("grafikr.gemspec",),
    "php": ("composer.json",),
    "lua": ("grafikr-1.1.0-1.rockspec",),
}
LINK_RE = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
PUBLISHED_PACKAGE_CLAIM = re.compile(
    r"\b(?:packages?\s+(?:are|is|have been|has been)\s+published|"
    r"published packages?\s+(?:are|is)\s+available|"
    r"packages?\s+(?:are|is)\s+available\s+(?:on|via)\s+(?:npm|pypi|crates\.io))\b",
    re.IGNORECASE,
)


def _example_exists(directory: Path) -> bool:
    return any(
        (directory / name).is_dir()
        for name in ("example", "examples", "Example", "Examples")
    )


def _has_package_manifest(directory: Path, candidates: tuple[str, ...]) -> bool:
    ignored = {".build", "bin", "node_modules", "obj", "target", "vendor"}
    return any(
        path.is_file() and not ignored.intersection(path.parts)
        for candidate in candidates
        for path in directory.rglob(candidate)
    )


class BindingDocsTests(unittest.TestCase):
    def test_every_matrix_sdk_has_guide_package_and_example(self):
        matrix = json.loads((ROOT / "bindings/sdk-matrix.json").read_text(encoding="utf-8"))
        names = {sdk["name"] for sdk in matrix["sdks"]}
        self.assertEqual(names, set(SDK_DIRS))
        for name, folder in SDK_DIRS.items():
            with self.subTest(sdk=name):
                directory = ROOT / "bindings" / folder
                self.assertTrue((directory / "README.md").is_file(), f"{name}: README.md")
                self.assertTrue(
                    _has_package_manifest(directory, PACKAGE_FILES[name]),
                    f"{name}: package manifest {PACKAGE_FILES[name]}",
                )
                self.assertTrue(_example_exists(directory), f"{name}: missing example directory")

    def test_release_docs_keep_license_versions_and_publication_claims_explicit(self):
        license_text = (ROOT / "LICENSE").read_text(encoding="utf-8").lower()
        self.assertIn("gnu affero general public license", license_text)
        self.assertTrue((ROOT / "bindings/c/vendor/cjson/LICENSE").is_file())

        readme = (ROOT / "README.md").read_text(encoding="utf-8").lower()
        status = (ROOT / "docs/bindings/support-matrix.md").read_text(encoding="utf-8").lower()
        self.assertIn("not published", readme)
        self.assertIn("no package registry has been published", status)
        self.assertIn("not a release", status)
        self.assertIn("support promise", status)
        self.assertIsNone(PUBLISHED_PACKAGE_CLAIM.search(readme))

        matrix = json.loads((ROOT / "bindings/sdk-matrix.json").read_text(encoding="utf-8"))
        self.assertEqual({sdk["sdk_version"] for sdk in matrix["sdks"]}, {"1.1.0"})
        protocol = (ROOT / "bindings/protocol/v1/README.md").read_text(encoding="utf-8")
        header = (ROOT / "bindings/c/include/grafikr.h").read_text(encoding="utf-8")
        self.assertRegex(protocol, r"(?i)protocol\s+1\.0")
        self.assertIn("ABI 1.0", header)
        contract = (ROOT / "docs/bindings/sdk-contract.md").read_text(encoding="utf-8")
        self.assertRegex(contract, r"(?i)protocol\s+1\.0")
        self.assertRegex(contract, r"(?i)abi version is 1\.0")

    def test_repository_markdown_relative_links_resolve(self):
        docs = [ROOT / "README.md", *ROOT.joinpath("docs").rglob("*.md")]
        docs += [
            path
            for folder in set(SDK_DIRS.values())
            if (path := ROOT / "bindings" / folder / "README.md").is_file()
        ]
        broken = []
        for source in docs:
            text = source.read_text(encoding="utf-8")
            for raw in LINK_RE.findall(text):
                target = raw.strip().split(maxsplit=1)[0].strip("<>")
                parsed = urlsplit(target)
                if parsed.scheme or parsed.netloc or not parsed.path:
                    continue
                path = Path(unquote(parsed.path))
                resolved = (source.parent / path).resolve()
                if not resolved.exists():
                    broken.append(f"{source.relative_to(ROOT)} -> {target}")
        self.assertEqual(broken, [])


if __name__ == "__main__":
    unittest.main()
