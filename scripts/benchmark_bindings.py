"""Compare headless C ABI and IPC request latency for one protocol operation."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from scripts.build_runtime import validate_manifest


SCOPE = "headless native/IPC request and label update latency"
ENDPOINT = "request_to_response"
WORKLOADS = {"request_latency": "state.get", "label_update": "property.update"}
# shortcut: desktop workloads are unavailable in this headless run; add them when native GUI runners can collect real OS input.


def _percentile(samples: list[int], percentile: float) -> int:
    ordered = sorted(samples)
    return ordered[max(0, math.ceil(percentile * len(ordered)) - 1)]


def _summary(run: dict[str, Any]) -> dict[str, Any]:
    samples = run.get("samples_ns")
    if not isinstance(samples, list) or not samples or any(type(value) is not int or value < 0 for value in samples):
        raise ValueError("each benchmark endpoint requires non-empty non-negative integer raw samples")
    if len(samples) != run.get("measured_count"):
        raise ValueError("raw sample count does not match measured_count")
    return {"sample_count": len(samples), "p50_ns": _percentile(samples, 0.50), "p95_ns": _percentile(samples, 0.95)}


def build_report(native: dict[str, Any], ipc: dict[str, Any], hardware: dict[str, Any]) -> dict[str, Any]:
    native_runs = {native["workload"]: native} if "workload" in native else native
    ipc_runs = {ipc["workload"]: ipc} if "workload" in ipc else ipc
    if not isinstance(native_runs, dict) or not isinstance(ipc_runs, dict) or not native_runs:
        raise ValueError("native and IPC workloads must be non-empty mappings")
    if set(native_runs) != set(ipc_runs) or set(native_runs) - set(WORKLOADS):
        raise ValueError("native and IPC workloads must contain the same supported workload names")
    for workload in native_runs:
        for label, run in (("native", native_runs[workload]), ("IPC", ipc_runs[workload])):
            if run.get("endpoint") != ENDPOINT:
                raise ValueError(f"{label} benchmark endpoint must be {ENDPOINT}")
            if run.get("workload") != workload:
                raise ValueError(f"{label} benchmark workload does not match {workload}")
            _summary(run)
            if type(run.get("warmup_count")) is not int or run["warmup_count"] < 0 or type(run.get("measured_count")) is not int or run["measured_count"] < 1:
                raise ValueError("benchmark sample plan must have non-negative warmups and positive measured samples")
        native_run, ipc_run = native_runs[workload], ipc_runs[workload]
        if native_run["warmup_count"] != ipc_run["warmup_count"] or native_run["measured_count"] != ipc_run["measured_count"]:
            raise ValueError("native and IPC sample plans do not match")
    required_hardware = ("os", "architecture", "cpu", "logical_cpu_count")
    if not isinstance(hardware, dict) or any(
        not isinstance(hardware.get(key), str)
        or not hardware[key].strip()
        or hardware[key].lower() == "unknown"
        for key in required_hardware[:-1]
    ):
        raise ValueError("hardware metadata must include OS, architecture, CPU, and logical CPU count")
    if type(hardware["logical_cpu_count"]) is not int or hardware["logical_cpu_count"] < 1:
        raise ValueError("hardware logical_cpu_count must be a positive integer")
    return {
        "format": "grafikr-bindings-benchmark-v1",
        "scope": SCOPE,
        "endpoint": ENDPOINT,
        "workloads_measured": sorted(native_runs),
        "hardware": hardware,
        "warmup_count": native_runs["request_latency"]["warmup_count"] if "request_latency" in native_runs else None,
        "measured_count": native_runs["request_latency"]["measured_count"] if "request_latency" in native_runs else None,
        "workloads": {
            workload: {
                endpoint: {
                    **_summary(run),
                    "endpoint": run["endpoint"],
                    "samples_ns": run["samples_ns"],
                }
                for endpoint, runs in (("native", native_runs), ("ipc", ipc_runs))
                for run in (runs[workload],)
            }
            for workload in native_runs
        },
        "native": _summary(native_runs["request_latency"]) if "request_latency" in native_runs else None,
        "ipc": _summary(ipc_runs["request_latency"]) if "request_latency" in ipc_runs else None,
        "notice": "Headless request and label update latency only; this does not measure GUI, GPU, scroll, resource lifecycle, RSS, or idle redraw workloads.",
    }


def _request(request_id: str, op: str, args: dict[str, Any]) -> bytes:
    return (json.dumps({"id": request_id, "op": op, "args": args}, separators=(",", ":")) + "\n").encode("utf-8")


def _exchange(process: subprocess.Popen[bytes], request_id: str, op: str, args: dict[str, Any]) -> dict[str, Any]:
    if process.stdin is None or process.stdout is None:
        raise RuntimeError("benchmark process pipes are unavailable")
    process.stdin.write(_request(request_id, op, args))
    process.stdin.flush()
    line = process.stdout.readline()
    if not line:
        code = process.poll()
        raise RuntimeError(f"benchmark process ended before response {request_id!r} (exit={code})")
    try:
        response = json.loads(line)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"invalid response for {request_id!r}: {line[:160]!r}") from error
    if response.get("id") != request_id or response.get("kind", "response") != "response" or response.get("ok") is not True:
        raise RuntimeError(f"request {request_id!r} failed: {response}")
    return response


def _run_endpoint(command: list[str], label: str, workload: str, warmups: int, count: int) -> dict[str, Any]:
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        _exchange(process, f"{label}-hello", "hello", {"protocol_major": 1, "protocol_minor": 0, "sdk_version": "benchmark"})
        operation = WORKLOADS[workload]
        if workload == "request_latency":
            state = _exchange(process, f"{label}-create", "state.create", {"value": {"type": "i64", "value": "1"}})["result"]["state"]
            args = {"state": state}
        else:
            window = _exchange(process, f"{label}-window", "window.create", {"title": "Grafikr benchmark", "width": 320, "height": 200})["result"]["window"]
            node = _exchange(process, f"{label}-node", "node.create", {"window": window, "widget": "Text", "properties": {"text": "benchmark warmup"}})["result"]["node"]
            args = {"node": node, "properties": {"text": "benchmark update 0"}}
        for index in range(warmups):
            if workload == "label_update":
                args["properties"]["text"] = f"benchmark update {index}"
            _exchange(process, f"{label}-warm-{index}", operation, args)
        samples: list[int] = []
        for index in range(count):
            if workload == "label_update":
                args["properties"]["text"] = f"benchmark update {warmups + index}"
            start = time.perf_counter_ns()
            _exchange(process, f"{label}-sample-{index}", operation, args)
            samples.append(time.perf_counter_ns() - start)
        return {"endpoint": ENDPOINT, "workload": workload, "operation": operation, "warmup_count": warmups, "measured_count": count, "samples_ns": samples}
    finally:
        if process.stdin is not None:
            try:
                process.stdin.close()
            except OSError:
                pass
        try:
            code = process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            raise RuntimeError(f"{label} benchmark process did not stop after stdin EOF")
        stderr = process.stderr.read() if process.stderr is not None else b""
        if code != 0:
            raise RuntimeError(f"{label} benchmark process exited {code}: {stderr.decode('utf-8', errors='replace')[-2000:]}")


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _hardware() -> dict[str, Any]:
    cpu = platform.processor() or os.environ.get("PROCESSOR_IDENTIFIER")
    if not cpu:
        raise ValueError("could not identify the benchmark CPU")
    return {
        "os": f"{platform.system()} {platform.release()}",
        "architecture": platform.machine(),
        "cpu": cpu,
        "logical_cpu_count": os.cpu_count() or 0,
    }


def _runtime_metadata(native: Path, host: Path) -> dict[str, Any]:
    bundle = host.parent.parent.resolve()
    manifest_path = bundle / "manifest.json"
    manifest = validate_manifest(bundle)
    entries = {entry["path"]: entry for entry in manifest["files"]}
    host_relative = host.resolve().relative_to(bundle).as_posix()
    if host_relative not in entries or _sha256(host) != entries[host_relative]["sha256"]:
        raise ValueError("host executable does not match its runtime manifest")

    cache = next((parent / "CMakeCache.txt" for parent in native.resolve().parents if (parent / "CMakeCache.txt").is_file()), None)
    if cache is None:
        raise ValueError("native harness must be built from a CMake build with a readable CMakeCache.txt")
    prefix = "GRAFIKR_FFI_LIBRARY:"
    library_line = next((line for line in cache.read_text(encoding="utf-8").splitlines() if line.startswith(prefix)), None)
    if library_line is None or "=" not in library_line:
        raise ValueError("native harness CMake cache does not record GRAFIKR_FFI_LIBRARY")
    library = Path(library_line.split("=", 1)[1]).resolve()
    try:
        library_relative = library.relative_to(bundle).as_posix()
    except ValueError as error:
        raise ValueError("native harness FFI library is outside the host runtime bundle") from error
    if library_relative not in entries or _sha256(library) != entries[library_relative]["sha256"]:
        raise ValueError("native harness FFI library does not match the host runtime manifest")
    return {
        "target": manifest["target"],
        "engine_version": manifest["engine_version"],
        "protocol_version": manifest["protocol"],
        "runtime_build_sha": manifest["build_sha"],
        "runtime_source_dirty": manifest["source_dirty"],
        "runtime_development_fixture": manifest["development_fixture"],
        "runtime_manifest_sha256": _sha256(manifest_path),
        "native_ffi_library": library_relative,
        "native_ffi_library_sha256": _sha256(library),
        "native_binary_sha256": _sha256(native),
        "host_binary_sha256": _sha256(host),
        "native_command": [str(native), "--stream"],
        "ipc_command": [str(host)],
        "python": sys.version.split()[0],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native", type=Path, required=True, help="C ABI grafikr_native_scenario executable")
    parser.add_argument("--host", type=Path, required=True, help="IPC grafikr-host executable")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--warmups", type=int, default=100)
    parser.add_argument("--samples", type=int, default=1000)
    args = parser.parse_args(argv)
    if args.warmups < 0 or args.samples < 1:
        parser.error("--warmups must be non-negative and --samples must be positive")
    native, host = args.native.resolve(), args.host.resolve()
    if not native.is_file() or not host.is_file():
        parser.error("--native and --host must name existing executables")
    try:
        args.out.mkdir(parents=True, exist_ok=True)
        started_at = datetime.now(timezone.utc).isoformat()
        runtime = _runtime_metadata(native, host)
        native_runs = {
            workload: _run_endpoint([str(native), "--stream"], f"native-{workload}", workload, args.warmups, args.samples)
            for workload in WORKLOADS
        }
        ipc_runs = {
            workload: _run_endpoint([str(host)], f"ipc-{workload}", workload, args.warmups, args.samples)
            for workload in WORKLOADS
        }
        report = build_report(native_runs, ipc_runs, _hardware())
        report["runtime"] = runtime
        report["started_at"] = started_at
        report["finished_at"] = datetime.now(timezone.utc).isoformat()
        (args.out / "native-samples.json").write_text(json.dumps(native_runs, indent=2) + "\n", encoding="utf-8")
        (args.out / "ipc-samples.json").write_text(json.dumps(ipc_runs, indent=2) + "\n", encoding="utf-8")
        raw = {
            "format": "grafikr-bindings-benchmark-evidence-v1",
            "workloads": report["workloads"],
            "notice": "Only request_latency and label_update are collected by this headless runner. The release evidence gate requires GUI and resource workload collectors too.",
        }
        (args.out / "benchmark-raw.json").write_text(json.dumps(raw, indent=2) + "\n", encoding="utf-8")
        (args.out / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as error:
        parser.exit(1, f"benchmark failed: {error}\n")
    print(f"{SCOPE}: {len(report['workloads'])} workloads; native request p50={report['native']['p50_ns']}ns; IPC request p50={report['ipc']['p50_ns']}ns")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
