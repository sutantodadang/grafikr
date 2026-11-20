import hashlib
import io
import json
import tarfile
import tempfile
import unittest
import zipfile
import xml.etree.ElementTree as ET
from pathlib import Path
from unittest.mock import patch

from scripts.package_bindings import (
    _pending_sdk_targets,
    _check_package_filename,
    _check_archive_members,
    _consumer_runtime_bundle,
    _is_native_target,
    _pack_source_snapshot,
    _pack_ruby_gem,
    _php_runtime_slug,
    _ruby_sdk_files,
    _ruby_platform_for_target,
    _source_sdk_versions,
    _source_sdk_files,
    _validate_jvm_publications,
    _release_metadata,
    build_package_index,
    validate_metadata,
    validate_package_index,
)
from scripts.verify_dotnet_package import PROJECT as DOTNET_PROJECT, project_version
from scripts.verify_jvm_package import project_version as jvm_project_version


class PackageBindingTests(unittest.TestCase):
    def setUp(self):
        self.manifest = {
            "engine_version": "1.1.0",
            "target": "x86_64-pc-windows-msvc",
            "protocol": {"major": 1, "minor": 0},
            "abi": {"major": 1, "minor": 0},
            "development_fixture": False,
        }

    def test_release_metadata_rejects_engine_and_sdk_version_drift(self):
        with self.assertRaisesRegex(ValueError, "version"):
            validate_metadata("1.1.0", "1.0.0", ["1.1.0", "1.0.0"], [self.manifest])

    def test_repository_release_metadata_is_frozen_at_1_1_0(self):
        metadata, sdk_rows = _release_metadata("1.1.0")
        self.assertEqual(metadata["engine_version"], "1.1.0")
        self.assertEqual({row["sdk_version"] for row in sdk_rows}, {"1.1.0"})
        self.assertEqual(_source_sdk_versions(), {"ruby": "1.1.0", "php": "1.1.0"})

    def test_source_sdk_package_versions_reject_drift(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "bindings/ruby").mkdir(parents=True)
            (root / "bindings/php").mkdir(parents=True)
            (root / "bindings/ruby/grafikr.gemspec").write_text('s.version = "1.0.0"\n', encoding="utf-8")
            (root / "bindings/php/composer.json").write_text('{"version":"1.1.0"}', encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "Ruby package version"):
                _source_sdk_versions(root, "1.1.0")

    def test_release_metadata_rejects_protocol_or_abi_bumps(self):
        manifest = {**self.manifest, "protocol": {"major": 2, "minor": 0}}
        with self.assertRaisesRegex(ValueError, "protocol"):
            validate_metadata("1.1.0", "1.1.0", ["1.1.0"], [manifest])
        manifest = {**self.manifest, "abi": {"major": 1, "minor": 1}}
        with self.assertRaisesRegex(ValueError, "ABI"):
            validate_metadata("1.1.0", "1.1.0", ["1.1.0"], [manifest])

    def test_release_metadata_rejects_development_fixture(self):
        manifest = {**self.manifest, "development_fixture": True}
        with self.assertRaisesRegex(ValueError, "fixture"):
            validate_metadata("1.1.0", "1.1.0", ["1.1.0"], [manifest])

    def test_release_metadata_rejects_empty_sdk_or_runtime_inventory(self):
        with self.assertRaisesRegex(ValueError, "inventory"):
            validate_metadata("1.1.0", "1.1.0", [], [])

    def test_package_index_rejects_traversal_and_hash_mismatch(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / "grafikr_ui-1.1.0-py3-none-win_amd64.whl"
            with zipfile.ZipFile(artifact, "w") as archive:
                archive.writestr("grafikr/__init__.py", "")
            good = {
                "format": "grafikr-bindings-package-index-v1",
                "version": "1.1.0",
                "protocol": {"major": 1, "minor": 0},
                "abi": {"major": 1, "minor": 0},
                "complete": True,
                "source_dirty": False,
                "pending": [],
                "packages": [{
                    "sdk": "python",
                    "target": "x86_64-pc-windows-msvc",
                    "path": artifact.name,
                    "sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
                    "engine_version": "1.1.0",
                    "development_fixture": False,
                }],
            }
            validate_package_index(good, root, "1.1.0")
            unsafe = json.loads(json.dumps(good))
            unsafe["packages"][0]["path"] = "../client.whl"
            with self.assertRaisesRegex(ValueError, "unsafe"):
                validate_package_index(unsafe, root, "1.1.0")
            corrupt = json.loads(json.dumps(good))
            corrupt["packages"][0]["sha256"] = "0" * 64
            with self.assertRaisesRegex(ValueError, "hash"):
                validate_package_index(corrupt, root, "1.1.0")

    def test_package_index_rejects_pending_or_incomplete_packages(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / "grafikr_ui-1.1.0-py3-none-win_amd64.whl"
            with zipfile.ZipFile(artifact, "w") as archive:
                archive.writestr("grafikr/__init__.py", "")
            index = {
                "format": "grafikr-bindings-package-index-v1",
                "version": "1.1.0",
                "protocol": {"major": 1, "minor": 0},
                "abi": {"major": 1, "minor": 0},
                "complete": False,
                "source_dirty": False,
                "pending": [{"sdk": "swift", "reason": "toolchain unavailable"}],
                "packages": [{
                    "sdk": "python", "target": "x86_64-pc-windows-msvc", "path": artifact.name,
                    "sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
                    "engine_version": "1.1.0", "development_fixture": False,
                }],
            }
            with self.assertRaisesRegex(ValueError, "pending"):
                validate_package_index(index, root, "1.1.0")

    def test_package_index_can_verify_artifacts_while_release_is_still_pending(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / "grafikr_ui-1.1.0-py3-none-win_amd64.whl"
            with zipfile.ZipFile(artifact, "w") as archive:
                archive.writestr("grafikr/__init__.py", "")
            index = {
                "format": "grafikr-bindings-package-index-v1",
                "version": "1.1.0",
                "protocol": {"major": 1, "minor": 0},
                "abi": {"major": 1, "minor": 0},
                "complete": False,
                "source_dirty": False,
                "pending": [{"sdk": "swift", "target": "source", "reason": "toolchain missing"}],
                "packages": [{
                    "sdk": "python", "target": "x86_64-pc-windows-msvc", "path": artifact.name,
                    "sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
                    "engine_version": "1.1.0", "development_fixture": False,
                }],
            }
            validate_package_index(index, root, "1.1.0", allow_incomplete=True)

    def test_package_index_rejects_unsafe_archive_members(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / "client.zip"
            with zipfile.ZipFile(artifact, "w") as archive:
                archive.writestr("../outside.txt", "escape")
            index = {
                "format": "grafikr-bindings-package-index-v1",
                "version": "1.1.0",
                "protocol": {"major": 1, "minor": 0},
                "abi": {"major": 1, "minor": 0},
                "complete": True,
                "source_dirty": False,
                "pending": [],
                "packages": [{
                    "sdk": "native", "target": "x86_64-pc-windows-msvc", "path": "client.zip",
                    "sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
                    "engine_version": "1.1.0", "development_fixture": False,
                }],
            }
            with self.assertRaisesRegex(ValueError, "unsafe archive"):
                validate_package_index(index, root, "1.1.0")

    def test_package_index_rejects_development_fixture_manifest_inside_archive(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / "runtime.zip"
            with zipfile.ZipFile(artifact, "w") as archive:
                archive.writestr("runtime/manifest.json", json.dumps({"development_fixture": True}))
            index = {
                "format": "grafikr-bindings-package-index-v1",
                "version": "1.1.0",
                "protocol": {"major": 1, "minor": 0},
                "abi": {"major": 1, "minor": 0},
                "complete": True,
                "source_dirty": False,
                "pending": [],
                "packages": [{
                    "sdk": "native", "target": "x86_64-pc-windows-msvc", "path": "runtime.zip",
                    "sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
                    "engine_version": "1.1.0", "development_fixture": False,
                }],
            }
            with self.assertRaisesRegex(ValueError, "development fixture manifest"):
                validate_package_index(index, root, "1.1.0")

    def test_package_index_rejects_stale_runtime_manifest_inside_archive(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / "runtime.zip"
            with zipfile.ZipFile(artifact, "w") as archive:
                archive.writestr("runtime/manifest.json", json.dumps({
                    "engine_version": "1.0.0",
                    "target": "x86_64-pc-windows-msvc",
                    "development_fixture": False,
                }))
            index = {
                "format": "grafikr-bindings-package-index-v1",
                "version": "1.1.0",
                "protocol": {"major": 1, "minor": 0},
                "abi": {"major": 1, "minor": 0},
                "complete": True,
                "source_dirty": False,
                "pending": [],
                "packages": [{
                    "sdk": "native", "target": "x86_64-pc-windows-msvc", "path": "runtime.zip",
                    "sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
                    "engine_version": "1.1.0", "development_fixture": False,
                }],
            }
            with self.assertRaisesRegex(ValueError, "stale runtime manifest"):
                validate_package_index(index, root, "1.1.0")

    def test_package_index_rejects_python_wheel_for_wrong_target(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / "grafikr_ui-1.1.0-py3-none-win_amd64.whl"
            with zipfile.ZipFile(artifact, "w") as archive:
                archive.writestr("grafikr/__init__.py", "")
            index = {
                "format": "grafikr-bindings-package-index-v1",
                "version": "1.1.0",
                "protocol": {"major": 1, "minor": 0},
                "abi": {"major": 1, "minor": 0},
                "complete": True,
                "source_dirty": False,
                "pending": [],
                "packages": [{
                    "sdk": "python", "target": "aarch64-pc-windows-msvc",
                    "path": artifact.name, "sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
                    "engine_version": "1.1.0", "development_fixture": False,
                }],
            }
            with self.assertRaisesRegex(ValueError, "wheel target"):
                validate_package_index(index, root, "1.1.0")

    def test_package_index_rejects_nuget_package_with_wrong_runtime_identifier(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / "Grafikr.Client.1.1.0.nupkg"
            with zipfile.ZipFile(artifact, "w") as archive:
                archive.writestr("runtimes/win-x64/native/manifest.json", json.dumps({
                    "engine_version": "1.1.0",
                    "target": "x86_64-pc-windows-msvc",
                    "development_fixture": False,
                }))
            index = {
                "format": "grafikr-bindings-package-index-v1",
                "version": "1.1.0",
                "protocol": {"major": 1, "minor": 0},
                "abi": {"major": 1, "minor": 0},
                "complete": True,
                "source_dirty": False,
                "pending": [],
                "packages": [{
                    "sdk": "dotnet", "target": "aarch64-pc-windows-msvc",
                    "path": artifact.name, "sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
                    "engine_version": "1.1.0", "development_fixture": False,
                }],
            }
            with self.assertRaisesRegex(ValueError, "runtime manifest target mismatch"):
                validate_package_index(index, root, "1.1.0")

    def test_package_index_supports_multiple_files_for_one_ecosystem_package(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            module = root / "v1.1.0.mod"
            module.write_text("module github.com/grafikr/grafikr-go\n", encoding="utf-8")
            archive = root / "v1.1.0.zip"
            with zipfile.ZipFile(archive, "w") as package:
                package.writestr("github.com/grafikr/grafikr-go@v1.1.0/go.mod", "module github.com/grafikr/grafikr-go\n")
            packages = []
            for path in (module, archive):
                packages.append({
                    "sdk": "go", "target": "source", "path": path.name,
                    "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                    "engine_version": "1.1.0", "development_fixture": False,
                })
            index = {
                "format": "grafikr-bindings-package-index-v1",
                "version": "1.1.0",
                "protocol": {"major": 1, "minor": 0},
                "abi": {"major": 1, "minor": 0},
                "complete": True, "source_dirty": False, "pending": [], "packages": packages,
            }
            validate_package_index(index, root, "1.1.0")

    def test_package_index_records_pending_adapters_without_claiming_complete(self):
        index = build_package_index(
            "1.1.0",
            [],
            [{"sdk": "swift", "target": "source", "reason": "package adapter unavailable"}],
            [],
        )
        self.assertFalse(index["complete"])
        self.assertEqual(len(index["pending"]), 1)
        self.assertEqual(index["protocol"], {"major": 1, "minor": 0})
        self.assertEqual(index["abi"], {"major": 1, "minor": 0})

    def test_package_index_cannot_be_complete_for_dirty_release_sources(self):
        index = build_package_index("1.1.0", [], [], [], source_dirty=True)
        self.assertFalse(index["complete"])
        self.assertTrue(index["source_dirty"])

    def test_source_snapshot_contains_only_manifested_allowlisted_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            (source / "src").mkdir(parents=True)
            (source / "src/client.swift").write_text("public struct Client {}", encoding="utf-8")
            (source / "tests").mkdir()
            (source / "tests/internal.swift").write_text("test fixture", encoding="utf-8")
            output = root / "packages"
            artifact = _pack_source_snapshot(
                [(source / "src/client.swift", "Sources/Client.swift")],
                output,
                "grafikr-swift-1.1.0.zip",
            )
            with zipfile.ZipFile(artifact) as archive:
                self.assertEqual(archive.namelist(), ["Sources/Client.swift"])
                self.assertEqual(archive.read("Sources/Client.swift"), b"public struct Client {}")

    def test_source_sdk_file_lists_include_package_inputs_but_exclude_tests(self):
        for sdk, expected in {
            "swift": "Package.swift",
            "dart": "pubspec.yaml",
            "php": "composer.json",
            "lua": "grafikr-1.1.0-1.rockspec",
        }.items():
            with self.subTest(sdk=sdk):
                files = _source_sdk_files(sdk, "1.1.0")
                paths = {member for _, member in files}
                self.assertIn(expected, paths)
                self.assertFalse(any("test" in member.lower() for member in paths))
        dart_paths = {member for _, member in _source_sdk_files("dart", "1.1.0")}
        self.assertIn("bin/grafikr_runtime.dart", dart_paths)
        self.assertIn("example/state_smoke.dart", dart_paths)
        lua_paths = {member for _, member in _source_sdk_files("lua", "1.1.0")}
        self.assertIn("src/grafikr_reader.lua", lua_paths)
        self.assertIn("vendor/sha2/LICENSE.txt", lua_paths)
        self.assertIn("vendor/sha2/sha2lib.c", lua_paths)
        swift_paths = {member for _, member in _source_sdk_files("swift", "1.1.0")}
        self.assertIn("LICENSE", swift_paths)
        self.assertIn("ThirdPartyNotices/swift-crypto-LICENSE.txt", swift_paths)
        self.assertIn("ThirdPartyNotices/swift-asn1-LICENSE.txt", swift_paths)

    def test_ruby_platform_gem_contains_sdk_example_and_license_sources(self):
        paths = {member for _, member in _ruby_sdk_files()}
        self.assertIn("lib/grafikr/runtime.rb", paths)
        self.assertIn("examples/counter_form.rb", paths)
        self.assertIn("LICENSE", paths)
        self.assertFalse(any(member.startswith("test/") for member in paths))

    def test_ruby_gem_build_uses_an_absolute_output_from_its_staging_directory(self):
        with tempfile.TemporaryDirectory(dir=Path.cwd()) as temporary:
            temp = Path(temporary)
            output = temp.relative_to(Path.cwd()) / "packages"

            def build_gem(*argv, cwd):
                artifact = Path(argv[argv.index("--output") + 1])
                self.assertTrue(artifact.is_absolute())
                artifact.parent.mkdir(parents=True, exist_ok=True)
                artifact.write_bytes(b"gem fixture")

            with (
                patch("scripts.package_bindings.validate_manifest", return_value={"target": "x86_64-pc-windows-msvc"}),
                patch("scripts.package_bindings.install_bundle"),
                patch("scripts.package_bindings._tool", side_effect=build_gem),
                patch("scripts.package_bindings._check_archive_members"),
            ):
                artifact = _pack_ruby_gem(temp / "bundle", output, "1.1.0")

            self.assertEqual(artifact, (Path.cwd() / output / "grafikr-ui-1.1.0-x64-mingw-ucrt.gem").resolve())

    def test_php_sdk_package_includes_the_windows_pipe_reader(self):
        paths = {member for _, member in _source_sdk_files("php", "1.1.0")}
        self.assertIn("bin/grafikr-reader.php", paths)

    def test_runtime_installer_sdks_require_their_source_package(self):
        for sdk in ("swift", "dart", "lua"):
            with self.subTest(sdk=sdk):
                pending = _pending_sdk_targets([{"name": sdk}], {}, [])
                self.assertEqual([item["target"] for item in pending], ["source"])

    def test_jvm_publication_requires_both_jar_and_maven_pom(self):
        packages = [
            {"sdk": "java", "target": "source", "path": "repo/grafikr-jvm-1.1.0.jar"},
            {"sdk": "java", "target": "source", "path": "repo/grafikr-jvm-1.1.0.pom"},
        ]
        _validate_jvm_publications(packages, "1.1.0")
        for package in packages:
            _check_package_filename(package, "1.1.0")
        with self.assertRaisesRegex(ValueError, "Maven POM"):
            _validate_jvm_publications(packages[:1], "1.1.0")

    def test_external_consumers_use_the_bundle_for_the_current_runner(self):
        host = "x86_64-pc-windows-msvc"
        bundles = {"aarch64-apple-darwin": Path("macos"), host: Path("windows")}
        with patch("scripts.package_bindings._host_triple", return_value=host):
            self.assertEqual(_consumer_runtime_bundle(bundles), Path("windows"))
        with patch("scripts.package_bindings._host_triple", return_value="aarch64-unknown-linux-gnu"):
            with self.assertRaisesRegex(RuntimeError, "native runner target"):
                _consumer_runtime_bundle(bundles)

    def test_foreign_runtime_packages_are_not_executed_on_the_package_runner(self):
        host = "x86_64-unknown-linux-gnu"
        self.assertTrue(_is_native_target(host, host))
        self.assertFalse(_is_native_target("aarch64-pc-windows-msvc", host))

    def test_ruby_platform_and_php_runtime_names_cover_every_native_target(self):
        expected = {
            "x86_64-pc-windows-msvc": ("x64-mingw-ucrt", "windows-x64"),
            "aarch64-pc-windows-msvc": ("arm64-mingw-ucrt", "windows-arm64"),
            "x86_64-unknown-linux-gnu": ("x86_64-linux-gnu", "linux-x64"),
            "aarch64-unknown-linux-gnu": ("aarch64-linux-gnu", "linux-arm64"),
            "x86_64-apple-darwin": ("x86_64-darwin-23", "macos-x64"),
            "aarch64-apple-darwin": ("arm64-darwin-23", "macos-arm64"),
        }
        for target, (platform, slug) in expected.items():
            with self.subTest(target=target):
                self.assertEqual(_ruby_platform_for_target(target), platform)
                self.assertEqual(_php_runtime_slug(target), slug)
                _check_package_filename(
                    {"sdk": "ruby", "target": target, "path": f"grafikr-ui-1.1.0-{platform}.gem"},
                    "1.1.0",
                )
                _check_package_filename(
                    {"sdk": "php", "target": target, "path": f"grafikr-php-runtime-1.1.0-{slug}.zip"},
                    "1.1.0",
                )

    def test_php_runtime_archive_verifies_embedded_bundle_file_hashes(self):
        with tempfile.TemporaryDirectory() as temporary:
            artifact = Path(temporary) / "runtime.zip"
            contents = b"verified host"
            manifest = {
                "engine_version": "1.1.0",
                "target": "x86_64-pc-windows-msvc",
                "development_fixture": False,
                "files": [{
                    "path": "bin/grafikr-host.exe",
                    "size": len(contents),
                    "sha256": hashlib.sha256(contents).hexdigest(),
                }],
            }
            with zipfile.ZipFile(artifact, "w") as archive:
                archive.writestr("composer.json", json.dumps({
                    "name": "grafikr/runtime-windows-x64",
                    "version": "1.1.0",
                    "extra": {"grafikr": {"runtime_target": "x86_64-pc-windows-msvc"}},
                }))
                archive.writestr("runtime/manifest.json", json.dumps(manifest))
                archive.writestr("runtime/bin/grafikr-host.exe", contents)
            _check_archive_members(
                artifact, "1.1.0", "x86_64-pc-windows-msvc", "php"
            )
            with zipfile.ZipFile(artifact, "w") as archive:
                archive.writestr("composer.json", json.dumps({
                    "name": "grafikr/runtime-windows-x64",
                    "version": "9.9.9",
                    "extra": {"grafikr": {"runtime_target": "x86_64-pc-windows-msvc"}},
                }))
                archive.writestr("runtime/manifest.json", json.dumps(manifest))
                archive.writestr("runtime/bin/grafikr-host.exe", contents)
            with self.assertRaisesRegex(ValueError, "Composer package metadata"):
                _check_archive_members(
                    artifact, "1.1.0", "x86_64-pc-windows-msvc", "php"
                )
            with zipfile.ZipFile(artifact, "w") as archive:
                archive.writestr("composer.json", json.dumps({
                    "name": "grafikr/runtime-windows-x64",
                    "version": "1.1.0",
                    "extra": {"grafikr": {"runtime_target": "x86_64-pc-windows-msvc"}},
                }))
                archive.writestr("runtime/manifest.json", json.dumps(manifest))
                archive.writestr("runtime/bin/grafikr-host.exe", b"tampered")
            with self.assertRaisesRegex(ValueError, "runtime checksum"):
                _check_archive_members(
                    artifact, "1.1.0", "x86_64-pc-windows-msvc", "php"
                )

    def test_ruby_gem_archive_verifies_files_in_nested_data_tar(self):
        with tempfile.TemporaryDirectory() as temporary:
            artifact = Path(temporary) / "grafikr-ui-1.1.0-x64-mingw-ucrt.gem"
            host = b"verified host"
            manifest = {
                "engine_version": "1.1.0",
                "target": "x86_64-pc-windows-msvc",
                "development_fixture": False,
                "files": [{"path": "bin/grafikr-host.exe", "size": len(host), "sha256": hashlib.sha256(host).hexdigest()}],
            }
            payload = io.BytesIO()
            with tarfile.open(fileobj=payload, mode="w:gz") as archive:
                for name, contents in (
                    ("runtime/manifest.json", json.dumps(manifest).encode()),
                    ("runtime/bin/grafikr-host.exe", host),
                ):
                    info = tarfile.TarInfo(name)
                    info.size = len(contents)
                    archive.addfile(info, io.BytesIO(contents))
            with tarfile.open(artifact, "w") as archive:
                for name, contents in (("metadata.gz", b"metadata"), ("data.tar.gz", payload.getvalue())):
                    info = tarfile.TarInfo(name)
                    info.size = len(contents)
                    archive.addfile(info, io.BytesIO(contents))
            _check_archive_members(artifact, "1.1.0", "x86_64-pc-windows-msvc", "ruby")

    def test_pending_packages_preserve_the_actual_build_failure_reason(self):
        pending = _pending_sdk_targets(
            [{"name": "swift"}],
            {},
            [],
            {("swift", "source"): "Swift toolchain is unavailable"},
        )
        self.assertEqual(pending, [{
            "sdk": "swift",
            "target": "source",
            "reason": "Swift toolchain is unavailable",
        }])

    def test_dotnet_pack_verifier_reads_version_from_project_metadata(self):
        with tempfile.TemporaryDirectory() as temporary:
            project = Path(temporary) / "Grafikr.Client.csproj"
            project.write_text("<Project><PropertyGroup><Version>1.1.0</Version></PropertyGroup></Project>", encoding="utf-8")
            self.assertEqual(project_version(project), "1.1.0")

    def test_dotnet_runtime_package_path_keeps_extensionless_files_at_bundle_paths(self):
        project = ET.parse(DOTNET_PROJECT).getroot()
        runtime_files = next(
            item for item in project.iter("None") if "**" in item.get("Include", "")
        )
        self.assertEqual(
            runtime_files.get("PackagePath"),
            "runtimes/$(RuntimeIdentifier)/native/",
        )

    def test_jvm_consumer_verifier_reads_version_from_gradle_metadata(self):
        with tempfile.TemporaryDirectory() as temporary:
            project = Path(temporary) / "build.gradle.kts"
            project.write_text('group = "dev.grafikr"\nversion = "1.1.0"\n', encoding="utf-8")
            self.assertEqual(jvm_project_version(project), "1.1.0")


if __name__ == "__main__":
    unittest.main()
