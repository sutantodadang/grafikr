from __future__ import annotations

import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from scripts.run_cmake_binding_tests import _parse_adapter_report, _validate_native_parity, _write_gui_evidence


def parity_output() -> str:
    responses = []
    for request_id in range(1, 17):
        response = {"id": str(request_id), "kind": "response", "ok": True, "result": {}}
        if request_id == 3:
            response["result"] = {"type": "i64", "value": "-9223372036854775808"}
        elif request_id == 5:
            response["result"] = {"type": "i64", "value": "9223372036854775807"}
        elif request_id in (7, 10):
            response = {"id": str(request_id), "kind": "response", "ok": False, "error": {"code": "invalid_handle"}}
        elif request_id == 16:
            response["result"] = {"stopped": True}
        responses.append(json.dumps(response))
    return "\n".join(responses) + "\n"


class NativeParityTests(unittest.TestCase):
    def test_accepts_all_expected_state_and_error_responses(self) -> None:
        _validate_native_parity(parity_output())

    def test_rejects_a_missing_or_corrupt_response(self) -> None:
        output = parity_output().replace('"9223372036854775807"', '"9223372036854775806"')
        with self.assertRaisesRegex(ValueError, "signed i64"):
            _validate_native_parity(output)

    def test_fixture_response_ids_and_shutdown_are_not_hardcoded(self) -> None:
        with TemporaryDirectory() as temporary:
            fixture = Path(temporary) / "scenario.jsonl"
            fixture.write_text(
                '{"id":"create","op":"state.create","args":{"value":{"type":"f64","value":0.0}}}\n'
                '{"id":"before","op":"state.get","args":{"state":"8"}}\n'
                '{"id":"transition","op":"transition.start","args":{"state":"8","to":{"type":"f64","value":1.0},"duration_ms":0,"easing":"linear"}}\n'
                '{"id":"after","op":"state.get","args":{"state":"8"}}\n'
                '{"id":"stop-request","op":"shutdown","args":{}}\n',
                encoding="utf-8",
            )
            output = (
                '{"id":"create","kind":"response","ok":true,"result":{"state":"8"}}\n'
                '{"id":"before","kind":"response","ok":true,"result":{"type":"f64","value":0.0}}\n'
                '{"id":"transition","kind":"response","ok":true,"result":{}}\n'
                '{"id":"after","kind":"response","ok":true,"result":{"type":"f64","value":1.0}}\n'
                '{"id":"stop-request","kind":"response","ok":true,"result":{"stopped":true}}\n'
            )
            _validate_native_parity(output, fixture)
            with self.assertRaisesRegex(ValueError, "wrong state value"):
                _validate_native_parity(output.replace('"value":1.0}', '"value":2.0}'), fixture)


class NativeGuiEvidenceTests(unittest.TestCase):
    def test_dashboard_record_includes_hashed_screenshot_and_state(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            attachments = root / "attachments"
            attachments.mkdir()
            screenshot = attachments / "cpp-dashboard.bmp"
            screenshot.write_bytes(b"test screenshot bytes")
            executable = root / "cpp-installed-scenarios.exe"
            executable.write_bytes(b"consumer executable")
            manifest = {
                "target": "x86_64-pc-windows-msvc",
                "engine_version": "1.1.0",
                "protocol": {"major": 1, "minor": 0},
                "build_sha": "a" * 40,
            }
            _write_gui_evidence(
                root, "cpp", "dashboard", executable, manifest,
                ["dashboard.initial-range", "dashboard.selection", "dashboard.scroll-range"],
                screenshot, "2026-11-24T00:00:00+00:00",
                "grafikr-adapter-v1\tbackend=Dx12\tdevice_type=DiscreteGpu\tvendor=10de\tdevice=2783\tname=NVIDIA GeForce RTX 4070 SUPER",
            )
            record = json.loads((root / "gui-cpp-dashboard.json").read_text(encoding="utf-8"))
            self.assertEqual(record["status"], "PASS")
            self.assertEqual(record["input_method"], "windows_sendinput")
            self.assertEqual({attachment["kind"] for attachment in record["attachments"]}, {"screenshot", "client_state"})
            self.assertEqual(record["runtime_build_sha"], "a" * 40)

    def test_counter_record_remains_unverified_without_ime_and_focus_checks(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            attachments = root / "attachments"
            attachments.mkdir()
            screenshot = attachments / "c-counter.bmp"
            screenshot.write_bytes(b"test screenshot bytes")
            executable = root / "c-installed-scenarios.exe"
            executable.write_bytes(b"consumer executable")
            manifest = {
                "target": "x86_64-pc-windows-msvc",
                "engine_version": "1.1.0",
                "protocol": {"major": 1, "minor": 0},
                "build_sha": "b" * 40,
            }
            _write_gui_evidence(
                root, "c", "counter", executable, manifest,
                ["counter.click-state-update", "counter.unicode-binding"],
                screenshot, "2026-11-24T00:00:00+00:00",
                "grafikr-adapter-v1\tbackend=Dx12\tdevice_type=DiscreteGpu\tvendor=10de\tdevice=2783\tname=NVIDIA GeForce RTX 4070 SUPER",
            )
            record = json.loads((root / "gui-c-counter_form.json").read_text(encoding="utf-8"))
            self.assertEqual(record["status"], "UNVERIFIED")
            self.assertIn("IME composition", record["reason"])

    def test_adapter_report_requires_explicit_backend_and_device_identity(self) -> None:
        report = _parse_adapter_report(
            "noise\n"
            "grafikr-adapter-v1\tbackend=Dx12\tdevice_type=DiscreteGpu\tvendor=10de\tdevice=2783\tname=NVIDIA GeForce RTX 4070 SUPER\n"
        )
        self.assertEqual(report["backend"], "Dx12")
        self.assertEqual(report["device_type"], "DiscreteGpu")
        self.assertEqual(report["vendor"], "10de")
        self.assertEqual(_parse_adapter_report("no renderer diagnostics"), {})


if __name__ == "__main__":
    unittest.main()
