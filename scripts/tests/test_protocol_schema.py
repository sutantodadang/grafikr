import json
import unittest
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[2]


class ProtocolSchemaTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.schema = json.loads((ROOT / "bindings/protocol/v1/schema.json").read_text(encoding="utf-8"))
        Draft202012Validator.check_schema(cls.schema)
        cls.validator = Draft202012Validator(cls.schema)
        cls.wire_validator = Draft202012Validator({"$defs": cls.schema["$defs"], "$ref": "#/$defs/wire_message"})

    def test_valid_fixture_corpus(self):
        lines = (ROOT / "bindings/protocol/v1/fixtures/valid.jsonl").read_text(encoding="utf-8").splitlines()
        for line in lines:
            with self.subTest(line=line):
                self.validator.validate(json.loads(line))

    def test_native_ipc_parity_fixture_matches_wire_schema(self):
        lines = (ROOT / "bindings/protocol/v1/fixtures/parity.jsonl").read_text(encoding="utf-8").splitlines()
        for line in lines:
            with self.subTest(line=line):
                self.validator.validate(json.loads(line))

    def test_invalid_fixture_corpus(self):
        lines = (ROOT / "bindings/protocol/v1/fixtures/invalid.jsonl").read_text(encoding="utf-8").splitlines()
        for line in lines:
            with self.subTest(line=line):
                self.assertTrue(list(self.validator.iter_errors(json.loads(line))))

    def test_rejects_wrong_operation_arguments(self):
        request = {"id":"x","op":"shutdown","args":{"force":True}}
        self.assertTrue(list(self.validator.iter_errors(request)))

    def test_outbound_messages_match_wire_schema(self):
        messages = [
            {"kind":"response","id":"1","ok":True,"result":{}},
            {"kind":"response","id":"2","ok":False,"error":{"code":"invalid_value","message":"bad"}},
            {"kind":"event","sequence":"1","event":"click","source":"7","payload":{}},
        ]
        for message in messages:
            with self.subTest(message=message):
                self.wire_validator.validate(message)


if __name__ == "__main__":
    unittest.main()
