from __future__ import annotations

import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from scripts.run_cmake_binding_tests import _validate_native_parity


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


if __name__ == "__main__":
    unittest.main()
