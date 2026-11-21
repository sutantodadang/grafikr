import hashlib
import json
import os
import tempfile
import unittest
from unittest import mock
from pathlib import Path

from jsonschema import Draft202012Validator

from scripts.build_runtime import _cmake_config, _linux_system_requirements, _tool, install_bundle, validate_manifest
from scripts.runtime_evidence import validate_evidence

ROOT = Path(__file__).resolve().parents[2]


def fake_pe(machine: int = 0x8664) -> bytes:
    data = bytearray(0x88)
    data[:2] = b"MZ"
    data[0x3C:0x40] = (0x80).to_bytes(4, "little")
    data[0x80:0x84] = b"PE\0\0"
    data[0x84:0x86] = machine.to_bytes(2, "little")
    return bytes(data)


def write_fixture_bundle(root: Path, development_fixture: bool = False) -> dict:
    (root / "bin").mkdir(parents=True, exist_ok=True)
    host = root / "bin/grafikr-host.exe"
    host.write_bytes(fake_pe())
    fixture_host = root / "bin/fixture-host.exe"
    if development_fixture:
        fixture_host.write_bytes(fake_pe())
    manifest = {
        "format_version": 1,
        "engine_version": "1.0.0",
        "protocol": {"major": 1, "minor": 0},
        "abi": {"major": 1, "minor": 0},
        "target": "x86_64-pc-windows-msvc",
        "min_os": "Windows 11 24H2 build 26100",
        "min_glibc": None,
        "build_sha": "a" * 40,
        "build_config_sha256": "b" * 64,
        "source_dirty": False,
        "development_fixture": development_fixture,
        "system_requirements": ["Microsoft Visual C++ 2015-2022 Redistributable (x64)"],
        "limits": {
            "frame_bytes": 8388608,
            "json_depth": 64,
            "command_queue_messages": 1024,
            "command_queue_bytes": 16777216,
            "output_queue_messages": 1024,
            "output_queue_bytes": 16777216,
            "windows": 16,
            "nodes": 100000,
            "states": 100000,
            "subscriptions": 100000,
            "image_bytes": 67108864,
            "image_session_bytes": 268435456,
            "image_chunk_bytes": 1048576,
            "canvas_commands": 10000,
            "canvas_bytes": 8388608,
            "model_bytes": 134217728,
            "collection_rows_per_node": 2048,
            "collection_bytes_per_node": 8388608,
            "collection_bytes_session": 67108864,
            "collection_request_rows": 512,
            "tree_items": 50000,
            "tree_depth": 64,
            "host_shutdown_seconds": 5,
        },
        "files": [
            {
                "path": "bin/grafikr-host.exe",
                "size": host.stat().st_size,
                "sha256": hashlib.sha256(host.read_bytes()).hexdigest(),
                "role": "host",
            }
        ],
    }
    if development_fixture:
        manifest["files"].append(
            {
                "path": "bin/fixture-host.exe",
                "size": fixture_host.stat().st_size,
                "sha256": hashlib.sha256(fixture_host.read_bytes()).hexdigest(),
                "role": "fixture_host",
            }
        )
    (root / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
    return manifest


class RuntimeBundleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.manifest_validator = Draft202012Validator(
            json.loads((ROOT / "bindings/runtime/manifest.schema.json").read_text(encoding="utf-8"))
        )

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def test_command_failure_includes_stdout_and_stderr(self):
        result = mock.Mock(returncode=1, stdout="CTest case failed", stderr="Errors while running CTest")
        with mock.patch("scripts.build_runtime.shutil.which", return_value=None), mock.patch(
            "scripts.build_runtime.subprocess.run", return_value=result
        ):
            with self.assertRaises(RuntimeError) as error:
                _tool("ctest")
        self.assertIn("CTest case failed", str(error.exception))
        self.assertIn("Errors while running CTest", str(error.exception))

    def test_linux_dependency_audit_records_external_sonames_and_rejects_missing(self):
        binary = self.root / "grafikr-host"
        binary.touch()
        output = """\
linux-vdso.so.1 (0x00007fff)
libfontconfig.so.1 => /lib/x86_64-linux-gnu/libfontconfig.so.1 (0x0000)
libc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x0000)
libmissing.so.1 => not found
"""
        with mock.patch("scripts.build_runtime._tool", return_value=mock.Mock(stdout=output)):
            with self.assertRaisesRegex(RuntimeError, "libmissing.so.1"):
                _linux_system_requirements([binary], self.root)

        output = output.replace("libmissing.so.1 => not found\n", "")
        with mock.patch("scripts.build_runtime._tool", return_value=mock.Mock(stdout=output)):
            self.assertEqual(
                _linux_system_requirements([binary], self.root),
                ["libc.so.6", "libfontconfig.so.1", "libxkbcommon-x11.so.0"],
            )

    def test_linux_cmake_package_exports_static_ffi_link_dependencies(self):
        _cmake_config(self.root, "linux")
        config = (self.root / "cmake/GrafikrConfig.cmake").read_text(encoding="utf-8")
        self.assertIn("find_package(Threads REQUIRED)", config)
        self.assertIn("find_library(GRAFIKR_FONTCONFIG_LIBRARY NAMES fontconfig REQUIRED)", config)
        self.assertIn(
            'INTERFACE_LINK_LIBRARIES "${GRAFIKR_FONTCONFIG_LIBRARY};m;dl;Threads::Threads"',
            config,
        )

    def test_macos_cmake_package_exports_framework_and_objc_dependencies(self):
        _cmake_config(self.root, "macos")
        config = (self.root / "cmake/GrafikrConfig.cmake").read_text(encoding="utf-8")
        self.assertIn("foreach(GRAFIKR_FRAMEWORK AppKit Foundation CoreFoundation CoreGraphics CoreVideo CoreText Metal Carbon)", config)
        self.assertIn("find_library(GRAFIKR_${GRAFIKR_FRAMEWORK}_FRAMEWORK NAMES ${GRAFIKR_FRAMEWORK} REQUIRED)", config)
        self.assertIn('list(APPEND GRAFIKR_APPLE_FRAMEWORKS "${GRAFIKR_${GRAFIKR_FRAMEWORK}_FRAMEWORK}")', config)
        self.assertIn("find_library(GRAFIKR_OBJC_LIBRARY NAMES objc REQUIRED)", config)
        self.assertIn('INTERFACE_LINK_LIBRARIES "${GRAFIKR_APPLE_FRAMEWORKS}"', config)

    def test_manifest_rejects_traversal_absolute_unc_duplicates_and_hash_mismatch(self):
        invalid_paths = ["../outside", "C:/outside", "//server/share/file", "bin/../../outside"]
        for index, path in enumerate(invalid_paths):
            with self.subTest(path=path):
                bundle = self.root / f"invalid-{index}"
                bundle.mkdir()
                manifest = write_fixture_bundle(bundle)
                manifest["files"][0]["path"] = path
                (bundle / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
                with self.assertRaises(ValueError):
                    validate_manifest(bundle)

        duplicate = self.root / "duplicate"
        duplicate.mkdir()
        manifest = write_fixture_bundle(duplicate)
        manifest["files"].append(dict(manifest["files"][0]))
        (duplicate / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaises(ValueError):
            validate_manifest(duplicate)

        mismatch = self.root / "mismatch"
        mismatch.mkdir()
        manifest = write_fixture_bundle(mismatch)
        manifest["files"][0]["sha256"] = "0" * 64
        (mismatch / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaises(ValueError):
            validate_manifest(mismatch)

    def test_manifest_rejects_junction_bundle_root(self):
        bundle = self.root / "junction-root"
        bundle.mkdir()
        with mock.patch.object(type(bundle), "is_junction", autospec=True, return_value=True, create=True):
            with self.assertRaisesRegex(ValueError, "regular directory"):
                validate_manifest(bundle)

    def test_manifest_rejects_junction_in_manifest_path(self):
        bundle = self.root / "junction-member"
        bundle.mkdir()
        write_fixture_bundle(bundle)
        with mock.patch.object(
            type(bundle),
            "is_junction",
            autospec=True,
            side_effect=lambda path: path == bundle.resolve() / "bin",
            create=True,
        ):
            with self.assertRaisesRegex(ValueError, "link or junction"):
                validate_manifest(bundle)

    def test_manifest_rejects_symlink_entries(self):
        bundle = self.root / "linked"
        bundle.mkdir()
        manifest = write_fixture_bundle(bundle)
        original = bundle / "bin/grafikr-host.exe"
        linked = bundle / "bin/linked-host.exe"
        try:
            linked.symlink_to(original)
        except OSError:
            self.skipTest("symlink creation is unavailable on this Windows account")
        manifest["files"][0]["path"] = "bin/linked-host.exe"
        (bundle / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaises(ValueError):
            validate_manifest(bundle)

    def test_manifest_rejects_hardlink_entries(self):
        hardlink = self.root / "hardlinked"
        hardlink.mkdir()
        manifest = write_fixture_bundle(hardlink)
        try:
            os.link(hardlink / "bin/grafikr-host.exe", hardlink / "bin/second-name.exe")
        except OSError:
            self.skipTest("hard links are unavailable on this volume")
        manifest["files"].append(
            {
                "path": "bin/second-name.exe",
                "size": (hardlink / "bin/grafikr-host.exe").stat().st_size,
                "sha256": hashlib.sha256((hardlink / "bin/grafikr-host.exe").read_bytes()).hexdigest(),
                "role": "host-copy",
            }
        )
        (hardlink / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaises(ValueError):
            validate_manifest(hardlink)

    def test_install_copies_only_verified_files_and_keeps_previous_cache_on_failure(self):
        source = self.root / "source"
        source.mkdir()
        write_fixture_bundle(source)
        (source / "unlisted-secret.txt").write_text("do not copy", encoding="utf-8")
        output = self.root / "cache/runtime"

        install_bundle(source, output)

        self.assertTrue((output / "bin/grafikr-host.exe").is_file())
        self.assertFalse((output / "unlisted-secret.txt").exists())
        original = (output / "bin/grafikr-host.exe").read_bytes()
        (source / "bin/grafikr-host.exe").write_bytes(b"tampered")
        with self.assertRaises(ValueError):
            install_bundle(source, output)
        self.assertEqual((output / "bin/grafikr-host.exe").read_bytes(), original)
        validate_manifest(output)

    @unittest.skipIf(os.name == "nt", "Unix executable mode bits are not available on Windows")
    def test_install_preserves_host_executable_mode(self):
        source = self.root / "executable-source"
        source.mkdir()
        write_fixture_bundle(source)
        host = source / "bin/grafikr-host.exe"
        host.chmod(0o755)

        output = install_bundle(source, self.root / "cache/executable-runtime")

        self.assertTrue(os.access(output / "bin/grafikr-host.exe", os.X_OK))

    def test_install_rejects_junction_destination(self):
        source = self.root / "source-for-junction"
        source.mkdir()
        write_fixture_bundle(source)
        destination = self.root / "cache" / "runtime"
        destination.parent.mkdir()
        with mock.patch.object(
            type(destination),
            "is_junction",
            autospec=True,
            side_effect=lambda path: path == destination.parent.resolve() / destination.name,
            create=True,
        ):
            with self.assertRaisesRegex(ValueError, "link or junction"):
                install_bundle(source, destination)
        self.assertFalse(destination.exists())

    def test_manifest_schema_rejects_missing_quota_version_and_unknown_target(self):
        bundle = self.root / "schema"
        bundle.mkdir()
        manifest = write_fixture_bundle(bundle)
        self.assertFalse(list(self.manifest_validator.iter_errors(manifest)))
        for damaged in (
            {**manifest, "protocol": None},
            {**manifest, "limits": {"frame_bytes": 1}},
            {**manifest, "target": "mystery-architecture"},
        ):
            with self.subTest(damaged=damaged):
                self.assertTrue(list(self.manifest_validator.iter_errors(damaged)))

    def test_actual_binary_architecture_must_match_target_label(self):
        bundle = self.root / "wrong-arch"
        bundle.mkdir()
        manifest = write_fixture_bundle(bundle)
        host = bundle / "bin/grafikr-host.exe"
        wrong_arch = fake_pe(0xAA64)
        host.write_bytes(wrong_arch)
        manifest["files"][0]["size"] = len(wrong_arch)
        manifest["files"][0]["sha256"] = hashlib.sha256(wrong_arch).hexdigest()
        (bundle / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaises(ValueError):
            validate_manifest(bundle)

    def test_native_example_architecture_must_match_target_label(self):
        bundle = self.root / "wrong-example-arch"
        bundle.mkdir()
        manifest = write_fixture_bundle(bundle)
        example = bundle / "share/grafikr/examples/grafikr-counter.exe"
        example.parent.mkdir(parents=True)
        example.write_bytes(fake_pe(0xAA64))
        manifest["files"].append(
            {
                "path": "share/grafikr/examples/grafikr-counter.exe",
                "size": example.stat().st_size,
                "sha256": hashlib.sha256(example.read_bytes()).hexdigest(),
                "role": "native_example",
            }
        )
        (bundle / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaises(ValueError):
            validate_manifest(bundle)

    def test_development_fixture_marker_requires_matching_binary(self):
        bundle = self.root / "fixture-marker"
        bundle.mkdir()
        manifest = write_fixture_bundle(bundle)
        manifest["development_fixture"] = True
        (bundle / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaises(ValueError):
            validate_manifest(bundle)

        hidden = self.root / "hidden-fixture"
        hidden.mkdir()
        manifest = write_fixture_bundle(hidden)
        fixture_host = hidden / "bin/fixture-host.exe"
        fixture_host.write_bytes(fake_pe())
        manifest["files"].append(
            {
                "path": "bin/fixture-host.exe",
                "size": fixture_host.stat().st_size,
                "sha256": hashlib.sha256(fixture_host.read_bytes()).hexdigest(),
                "role": "native_client",
            }
        )
        (hidden / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaises(ValueError):
            validate_manifest(hidden)

        development = self.root / "fixture-bundle"
        development.mkdir()
        write_fixture_bundle(development, development_fixture=True)
        self.assertTrue(validate_manifest(development)["development_fixture"])

    def test_evidence_schema_rejects_false_pass_and_missing_provenance(self):
        evidence = {
            "stage": "unit",
            "sdk": "python",
            "consumer_language": "Python",
            "engine_version": "1.0.0",
            "sdk_version": "1.0.0",
            "protocol_version": {"major": 1, "minor": 0},
            "runtime_version": "3.12.0",
            "target": "Windows-AMD64",
            "backend": "headless",
            "gpu_kind": None,
            "gpu_vendor": None,
            "source_sha": "a" * 40,
            "runner_sha": "b" * 64,
            "artifact_sha256": "c" * 64,
            "command": [{"name": "unit", "argv": ["python", "-m", "unittest"]}],
            "exit_code": 0,
            "status": "PASS",
            "started_at": "2026-11-14T00:00:00Z",
            "finished_at": "2026-11-14T00:00:01Z",
            "attachments": [],
            "reason": None,
        }
        validate_evidence(evidence)
        for damaged in (
            {**evidence, "exit_code": 1},
            {**evidence, "artifact_sha256": None},
            {**evidence, "target": ""},
        ):
            with self.subTest(damaged=damaged):
                with self.assertRaises(ValueError):
                    validate_evidence(damaged)

    def test_evidence_requires_verifiable_relative_attachments(self):
        evidence = {
            "stage": "unit", "sdk": "python", "consumer_language": "Python",
            "engine_version": "1.0.0", "sdk_version": "1.0.0",
            "protocol_version": {"major": 1, "minor": 0}, "runtime_version": "3.12.0",
            "target": "Windows-AMD64", "backend": "headless", "gpu_kind": None,
            "gpu_vendor": None, "source_sha": "a" * 40, "runner_sha": "b" * 64,
            "artifact_sha256": "c" * 64,
            "command": [{"name": "unit", "argv": ["python", "-m", "unittest"]}],
            "exit_code": 0, "status": "PASS", "started_at": "2026-11-14T00:00:00Z",
            "finished_at": "2026-11-14T00:00:01Z", "attachments": [], "reason": None,
        }
        with self.assertRaises(ValueError):
            validate_evidence({**evidence, "attachments": [{"path": "result.txt", "sha256": "d" * 64}]})
        attachment_root = self.root / "evidence"
        attachment_root.mkdir()
        attachment = attachment_root / "result.txt"
        attachment.write_text("evidence", encoding="utf-8")
        digest = hashlib.sha256(attachment.read_bytes()).hexdigest()
        verify = {**evidence, "attachments": [{"path": "result.txt", "sha256": digest}]}
        validate_evidence(verify, attachment_root)
        for path in ("../outside.txt", "C:/outside.txt", "//server/share/file"):
            with self.subTest(path=path):
                with self.assertRaises(ValueError):
                    validate_evidence(
                        {**evidence, "attachments": [{"path": path, "sha256": digest}]}, attachment_root
                    )
        with self.assertRaises(ValueError):
            validate_evidence(
                {**evidence, "attachments": [{"path": "result.txt", "sha256": "0" * 64}]},
                attachment_root,
            )


if __name__ == "__main__":
    unittest.main()
