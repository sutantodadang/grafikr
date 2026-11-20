"""Validation helpers for machine-readable Grafikr implementation evidence."""

from __future__ import annotations

import argparse
import json
import stat
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator, FormatChecker

from scripts.build_runtime import _safe_member_path, _sha256

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE_SCHEMA_PATH = ROOT / "bindings/runtime/evidence.schema.json"


def validate_evidence(evidence: dict[str, Any], attachment_root: Path | None = None) -> None:
    schema = json.loads(EVIDENCE_SCHEMA_PATH.read_text(encoding="utf-8"))
    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    errors = list(validator.iter_errors(evidence))
    if errors:
        error = min(errors, key=lambda item: len(item.absolute_path))
        raise ValueError(f"invalid evidence at {list(error.absolute_path)}: {error.message}")

    attachments = evidence["attachments"]
    if attachments and attachment_root is None:
        raise ValueError("evidence attachments require a directory for verification")
    if not attachments:
        return

    root = Path(attachment_root)
    if root.is_symlink() or root.is_junction() or not root.is_dir():
        raise ValueError("evidence attachment root must be a regular directory")
    root = root.resolve()
    for attachment in attachments:
        relative = _safe_member_path(attachment["path"], allow_manifest=True)
        member = root.joinpath(*relative.parts)
        current = root
        for part in relative.parts:
            current /= part
            if current.is_symlink() or current.is_junction():
                raise ValueError(f"evidence attachment is a symlink: {attachment['path']}")
        try:
            metadata = member.stat(follow_symlinks=False)
        except OSError as error:
            raise ValueError(f"evidence attachment is missing: {attachment['path']}") from error
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise ValueError(f"evidence attachment is not a regular unlinked file: {attachment['path']}")
        if _sha256(member) != attachment["sha256"]:
            raise ValueError(f"evidence attachment hash mismatch: {attachment['path']}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", type=Path)
    parser.add_argument("--attachments", type=Path, help="directory containing evidence attachment files")
    args = parser.parse_args(argv)
    try:
        evidence = json.loads(args.evidence.read_text(encoding="utf-8"))
        validate_evidence(evidence, args.attachments)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        parser.exit(1, f"invalid runtime evidence: {error}\n")
    print(f"runtime evidence verified: {args.evidence}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
