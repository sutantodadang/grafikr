"""Run one SDK suite without a shell and write failure-complete evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MATRIX = ROOT / "bindings/sdk-matrix.json"
MAX_CAPTURED_OUTPUT_CHARS = 16_000
WIDGETS = ROOT / "bindings/protocol/v1/widgets.json"
CASES = ROOT / "bindings/protocol/v1/fixtures/sdk-cases.json"
RUNTIME_CLASSIFIERS = {
    "x86_64-pc-windows-msvc": "windows-x64",
    "aarch64-pc-windows-msvc": "windows-arm64",
    "x86_64-unknown-linux-gnu": "linux-x64",
    "aarch64-unknown-linux-gnu": "linux-arm64",
    "x86_64-apple-darwin": "macos-x64",
    "aarch64-apple-darwin": "macos-arm64",
}


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def digest_file(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def digest_tree(directory: Path) -> str:
    hasher = hashlib.sha256()
    for path in sorted(directory.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"bundle contains a symlink: {path.relative_to(directory)}")
        if path.is_file():
            relative = path.relative_to(directory).as_posix().encode("utf-8")
            hasher.update(len(relative).to_bytes(4, "big"))
            hasher.update(relative)
            hasher.update(bytes.fromhex(digest_file(path)))
    return hasher.hexdigest()


def widget_coverage(widgets_path: Path) -> set[str]:
    inventory = json.loads(widgets_path.read_text(encoding="utf-8"))
    found = set()
    for kind, spec in inventory["widgets"].items():
        found.add(kind)
        found.update(f"{kind}.{name}" for name in spec.get("properties", {}))
        found.update(f"{kind}@{event}" for event in spec.get("events", []))
    return found


def _write_report(path: Path, report: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", dir=path.parent, delete=False
    ) as temporary:
        json.dump(report, temporary, indent=2, ensure_ascii=False)
        temporary.write("\n")
        temp_path = Path(temporary.name)
    os.replace(temp_path, path)


def run_binding_checks(
    sdk_name: str,
    suite_name: str,
    bundle_path: Path,
    out_path: Path,
    matrix_path: Path = DEFAULT_MATRIX,
    repo_root: Path = ROOT,
    fixture_host: Path | None = None,
    package_root: Path | None = None,
) -> bool:
    started_at = utc_now()
    errors: list[str] = []
    coverage_errors: list[str] = []
    results: list[dict] = []
    sdk: dict = {}
    suite: dict = {}
    bundle: Path | None = None
    fixture: Path | None = None
    package: Path | None = None
    artifact_hash = None
    engine_version = None
    source_sha = None
    runtime_target = None
    repo_root = repo_root.resolve()

    try:
        matrix = json.loads(matrix_path.read_text(encoding="utf-8"))
        if not isinstance(matrix, dict) or not isinstance(matrix.get("sdks"), list):
            raise ValueError("matrix must contain an sdks array")
    except (OSError, ValueError, TypeError, AttributeError) as error:
        errors.append(f"SDK matrix is missing or invalid: {error}")
        matrix = {}
    else:
        sdk = next(
            (row for row in matrix["sdks"] if isinstance(row, dict) and row.get("name") == sdk_name),
            None,
        )
        if sdk is None:
            errors.append(f"SDK {sdk_name!r} is not in the matrix")

    if sdk:
        suite = sdk.get("suites", {}).get(suite_name) or {}
        if not suite:
            errors.append(f"{suite_name} suite is missing for SDK {sdk_name}")
        elif not isinstance(suite.get("checks"), list) or not suite["checks"]:
            errors.append(f"{suite_name} suite has no runnable checks")

    if not bundle_path.is_absolute():
        errors.append("bundle path must be absolute")
    elif not bundle_path.is_dir():
        errors.append("bundle directory does not exist")
    else:
        bundle = bundle_path.resolve()
        try:
            artifact_hash = digest_tree(bundle)
            manifest_path = bundle / "manifest.json"
            if manifest_path.is_file():
                manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
                engine_version = manifest.get("engine_version")
                source_sha = manifest.get("build_sha")
                runtime_target = manifest.get("target")
        except (OSError, ValueError, json.JSONDecodeError) as error:
            errors.append(f"bundle cannot be read safely: {error}")

    if fixture_host is not None:
        if not fixture_host.is_absolute() or not fixture_host.is_file():
            errors.append("fixture-host path must be an existing absolute file")
        else:
            fixture = fixture_host.resolve()
    elif suite.get("requires_fixture_host"):
        errors.append("this suite requires an explicit fixture-host path")

    if package_root is not None:
        if not package_root.is_absolute() or not package_root.is_dir():
            errors.append("package-root path must be an existing absolute directory")
        else:
            package = package_root.resolve()
    elif suite.get("requires_package_root"):
        errors.append("this suite requires an explicit package-root path")

    if suite and not errors:
        cwd_value = suite.get("cwd")
        try:
            cwd = (repo_root / cwd_value).resolve()
            cwd.relative_to(repo_root)
            if not cwd.is_dir():
                errors.append(f"working directory does not exist: {cwd_value}")
        except (TypeError, ValueError):
            cwd = repo_root
            errors.append("working directory must stay inside the repository")

        helper_paths = suite.get("helper_paths", [])
        if not isinstance(helper_paths, list):
            errors.append("helper_paths must be an array")
            helper_paths = []
        for helper in helper_paths:
            try:
                helper_path = (repo_root / helper).resolve()
                helper_path.relative_to(repo_root)
                if not helper_path.is_file():
                    errors.append(f"coverage helper is missing: {helper}")
            except (TypeError, ValueError):
                errors.append(f"coverage helper must stay inside repository: {helper}")

        if suite_name in {"contract", "installed"}:
            requirements = matrix.get("coverage_requirements", {})
            coverage = suite.get("coverage", {})
            cases_path = repo_root / requirements.get("cases_file", CASES.relative_to(ROOT))
            widgets_path = repo_root / requirements.get("widgets_file", WIDGETS.relative_to(ROOT))
            try:
                corpus = json.loads(cases_path.read_text(encoding="utf-8"))["cases"]
                all_cases = {row["id"] for row in corpus}
                required_cases = {
                    row["id"]
                    for row in corpus
                    if (row.get("area") == "installed") == (suite_name == "installed")
                    and sdk_name not in row.get("exclude_sdks", [])
                }
                known_sdks = {row["name"] for row in matrix["sdks"] if isinstance(row, dict)}
                if any(
                    not isinstance(row.get("exclude_sdks", []), list)
                    or not all(
                        isinstance(name, str) and name in known_sdks
                        for name in row.get("exclude_sdks", [])
                    )
                    or (row.get("exclude_sdks") and not row.get("exclusion_reason"))
                    for row in corpus
                ):
                    raise ValueError("case SDK exclusions must name known SDKs and include a reason")
                if required_cases - all_cases:
                    raise ValueError("matrix names cases absent from the shared corpus")
                required_widgets = widget_coverage(widgets_path) if suite_name == "contract" else set()
            except (OSError, ValueError, KeyError) as error:
                errors.append(f"coverage requirements are invalid: {error}")
                required_cases, required_widgets = set(), set()
            if not isinstance(coverage, dict):
                errors.append("coverage must be an object")
                coverage = {}
            try:
                declared_cases = set(coverage.get("cases", []))
                declared_widgets = set(coverage.get("widgets", []))
            except TypeError:
                errors.append("coverage cases and widgets must be arrays of strings")
                declared_cases, declared_widgets = set(), set()
            if not all(isinstance(item, str) for item in declared_cases | declared_widgets):
                errors.append("coverage cases and widgets must contain strings")
            missing_cases = sorted(required_cases - declared_cases)
            missing_widgets = sorted(required_widgets - declared_widgets)
            if missing_cases:
                coverage_errors.append(f"coverage omits scenarios: {', '.join(missing_cases)}")
            if missing_widgets:
                coverage_errors.append(f"coverage omits widget coverage: {', '.join(missing_widgets)}")

    if suite and not errors:
        environment = os.environ.copy()
        environment["GRAFIKR_RUNTIME_DIR"] = str(bundle)
        if package is not None:
            environment["GRAFIKR_PACKAGE_ROOT"] = str(package)
        if fixture is not None:
            environment["GRAFIKR_FIXTURE_HOST"] = str(fixture)
        for check in suite["checks"]:
            name = check.get("name", "unnamed")
            argv = check.get("argv")
            check_started = utc_now()
            check_result = {
                "name": name,
                "argv": argv,
                "cwd": str(cwd),
                "started_at": check_started,
                "exit_code": None,
                "status": "FAIL",
                "stdout": "",
                "stderr": "",
            }
            if not isinstance(argv, list) or not argv or any(not isinstance(arg, str) or not arg for arg in argv):
                check_result["stderr"] = "argv must be a non-empty array of non-empty strings"
            else:
                try:
                    replacements = {"{runtime_dir}": str(bundle)}
                    if "{package_root}" in " ".join(argv):
                        if package is None:
                            raise ValueError("package-root path is required by this check")
                        replacements["{package_root}"] = str(package)
                    if "{runtime_classifier}" in " ".join(argv):
                        classifier = RUNTIME_CLASSIFIERS.get(runtime_target)
                        if classifier is None:
                            raise ValueError("runtime classifier cannot be resolved from bundle target")
                        replacements["{runtime_classifier}"] = classifier
                    for marker, value in replacements.items():
                        argv = [argument.replace(marker, value) for argument in argv]
                    lua_executable = None
                    if argv[0] == "lua":
                        lua_executable = environment.get("GRAFIKR_LUA54", "lua")
                        argv[0] = lua_executable
                    if argv[0] in {"python", "python3"}:
                        argv[0] = sys.executable
                    check_result["argv"] = argv
                    timeout = float(check.get("timeout_seconds", 300))
                    if not 0 < timeout <= 3600:
                        raise ValueError("timeout must be greater than zero and at most 3600 seconds")
                    check_environment = check.get("env", {})
                    if not isinstance(check_environment, dict) or not all(
                        isinstance(key, str) and isinstance(value, str)
                        for key, value in check_environment.items()
                    ):
                        raise ValueError("check env must map string names to string values")
                    child_environment = environment | check_environment
                    if lua_executable:
                        child_environment["LUA"] = lua_executable
                        lua_modules = environment.get("GRAFIKR_LUA54_MODULES")
                        if lua_modules:
                            module_root = Path(lua_modules)
                            if not module_root.is_absolute():
                                module_root = repo_root / module_root
                            module_root = module_root.resolve()
                            lua_share = module_root / "share" / "lua" / "5.4"
                            lua_lib = module_root / "lib" / "lua" / "5.4"
                            child_environment["LUA_PATH"] = (
                                f"{lua_share / '?.lua'};{lua_share / '?/init.lua'};;"
                            )
                            child_environment["LUA_CPATH"] = ";".join(
                                [*(str(lua_lib / f"?.{extension}") for extension in ("dll", "so", "dylib")), "", ""]
                            )
                            if os.name == "nt":
                                child_environment["PATH"] = str(lua_lib) + os.pathsep + child_environment.get("PATH", "")
                            elif sys.platform == "darwin":
                                key = "DYLD_LIBRARY_PATH"
                                child_environment[key] = str(lua_lib) + os.pathsep + child_environment.get(key, "")
                            else:
                                key = "LD_LIBRARY_PATH"
                                child_environment[key] = str(lua_lib) + os.pathsep + child_environment.get(key, "")
                    completed = subprocess.run(
                        argv,
                        cwd=cwd,
                        env=child_environment,
                        shell=False,
                        capture_output=True,
                        text=True,
                        encoding="utf-8",
                        errors="replace",
                        timeout=timeout,
                        check=False,
                    )
                    check_result.update(
                        exit_code=completed.returncode,
                        status="PASS" if completed.returncode == 0 else "FAIL",
                        stdout=completed.stdout[-MAX_CAPTURED_OUTPUT_CHARS:],
                        stderr=completed.stderr[-MAX_CAPTURED_OUTPUT_CHARS:],
                    )
                except FileNotFoundError as error:
                    check_result["stderr"] = f"tool or test command is missing: {error}"
                except (TypeError, ValueError) as error:
                    check_result["stderr"] = str(error)
                except subprocess.TimeoutExpired as error:
                    stdout = error.stdout.decode("utf-8", "replace") if isinstance(error.stdout, bytes) else error.stdout or ""
                    stderr = error.stderr.decode("utf-8", "replace") if isinstance(error.stderr, bytes) else error.stderr or ""
                    check_result["stdout"] = stdout[-MAX_CAPTURED_OUTPUT_CHARS:]
                    check_result["stderr"] = f"check timed out: {error.timeout} seconds\n{stderr[-MAX_CAPTURED_OUTPUT_CHARS:]}"
            check_result["finished_at"] = utc_now()
            results.append(check_result)

    errors.extend(
        f"check failed: {result['name']} (exit={result['exit_code']})"
        for result in results
        if result["status"] != "PASS"
    )
    errors.extend(coverage_errors)

    exit_codes = [result["exit_code"] for result in results]
    failed_codes = [code for code in exit_codes if code not in (None, 0)]
    aggregate_exit_code = (
        failed_codes[0]
        if failed_codes
        else 0
        if results and all(code == 0 for code in exit_codes)
        else None
    )
    report = {
        "schema_version": 1,
        "stage": suite_name,
        "sdk": sdk_name,
        "consumer_language": sdk.get("consumer_language"),
        "engine_version": engine_version,
        "sdk_version": sdk.get("sdk_version"),
        "protocol_version": matrix.get("protocol_version"),
        "runtime_version": sdk.get("runtime_version"),
        "target": f"{platform.system()}-{platform.machine()}",
        "backend": "headless",
        "gpu_kind": None,
        "gpu_vendor": None,
        "source_sha": source_sha,
        "runner_sha": digest_file(Path(__file__).resolve()),
        "artifact_sha256": artifact_hash,
        "command": [
            {"name": result["name"], "argv": result["argv"]}
            for result in results
            if isinstance(result["argv"], list)
            and result["argv"]
            and all(isinstance(arg, str) for arg in result["argv"])
        ],
        "coverage": suite.get("coverage", {}),
        "helper_paths": suite.get("helper_paths", []),
        "exit_code": aggregate_exit_code,
        "status": "PASS" if not errors and results else "FAIL",
        "started_at": started_at,
        "finished_at": utc_now(),
        "attachments": [],
        "reason": None if not errors else "; ".join(errors),
        "errors": errors,
        "checks": results,
    }
    _write_report(out_path, report)
    return report["status"] == "PASS"


def run_all_binding_checks(
    suite_name: str,
    bundle_path: Path,
    out_path: Path,
    matrix_path: Path = DEFAULT_MATRIX,
    repo_root: Path = ROOT,
    fixture_host: Path | None = None,
    package_root: Path | None = None,
) -> bool:
    """Run one suite for every matrix SDK and retain per-SDK evidence."""
    started_at = utc_now()
    errors: list[str] = []
    try:
        matrix = json.loads(matrix_path.read_text(encoding="utf-8"))
        rows = matrix["sdks"]
        names = [row["name"] for row in rows]
        if not rows or len(names) != len(set(names)) or any(not re.fullmatch(r"[a-z0-9_-]+", name) for name in names):
            raise ValueError("matrix SDK names must be unique lowercase identifiers")
    except (OSError, ValueError, TypeError, KeyError) as error:
        rows = []
        errors.append(f"SDK matrix is missing or invalid: {error}")

    results = []
    for row in rows:
        name = row["name"]
        evidence_path = out_path.parent / f"{out_path.stem}.{name}.{suite_name}{out_path.suffix}"
        passed = run_binding_checks(
            name, suite_name, bundle_path, evidence_path, matrix_path, repo_root, fixture_host, package_root
        )
        evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
        results.append(
            {
                "sdk": name,
                "status": evidence["status"],
                "evidence": evidence_path.relative_to(out_path.parent).as_posix(),
                "artifact_sha256": evidence["artifact_sha256"],
                "reason": evidence["reason"],
            }
        )
        if not passed:
            errors.append(f"{name}: {evidence['reason'] or 'suite failed'}")

    report = {
        "schema_version": 1,
        "stage": suite_name,
        "sdk": "all",
        "target": f"{platform.system()}-{platform.machine()}",
        "status": "PASS" if results and not errors else "FAIL",
        "started_at": started_at,
        "finished_at": utc_now(),
        "results": results,
        "errors": errors,
    }
    _write_report(out_path, report)
    return report["status"] == "PASS"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", required=True)
    parser.add_argument("--suite", required=True, choices=("unit", "contract", "installed"))
    parser.add_argument("--bundle", required=True, type=Path)
    parser.add_argument("--fixture-host", type=Path)
    parser.add_argument("--package-root", type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--matrix", type=Path, default=DEFAULT_MATRIX)
    arguments = parser.parse_args(argv)
    bundle_path = arguments.bundle.resolve()
    fixture_host = arguments.fixture_host.resolve() if arguments.fixture_host else None
    package_root = arguments.package_root.resolve() if arguments.package_root else None
    if arguments.sdk == "all":
        passed = run_all_binding_checks(
            arguments.suite,
            bundle_path,
            arguments.out,
            arguments.matrix,
            ROOT,
            fixture_host,
            package_root,
        )
        report = json.loads(arguments.out.read_text(encoding="utf-8"))
        print(f"all/{arguments.suite}: {report['status']}")
        if report["errors"]:
            print("\n".join(report["errors"]), file=sys.stderr)
        return 0 if passed else 1
    passed = run_binding_checks(
        arguments.sdk,
        arguments.suite,
        bundle_path,
        arguments.out,
        arguments.matrix,
        ROOT,
        fixture_host,
        package_root,
    )
    report = json.loads(arguments.out.read_text(encoding="utf-8"))
    print(f"{arguments.sdk}/{arguments.suite}: {report['status']}")
    if report["reason"]:
        print(report["reason"], file=sys.stderr)
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
