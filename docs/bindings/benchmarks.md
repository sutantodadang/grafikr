# Binding benchmark evidence

`scripts/benchmark_bindings.py` compares persistent C ABI and IPC `state.get`
requests on one machine. Each sample times from request write/flush through a
complete matching response. It performs 100 warmups and records 1,000 raw
samples per endpoint by default. Both endpoints use the same runtime bundle;
the report verifies the host and the native harness's linked FFI library
against its manifest.

Run it with the C scenario harness built from the selected runtime bundle:

```powershell
rtk uv run --with-requirements scripts/requirements-bindings.txt python -m scripts.benchmark_bindings `
  --native target/bindings-parity/Release/grafikr_native_scenario.exe `
  --host target/runtime/windows-x64-native-examples/bin/grafikr-host.exe `
  --out .artifacts/bindings/benchmark-headless-windows
```

The output directory contains `native-samples.json`, `ipc-samples.json`, and
`report.json`. The report records the endpoint, workload, hardware, runtime
manifest/build identity, binary hashes, commands, timestamps, sample counts,
and p50/p95 latency. `.artifacts/` is local evidence and is not committed.

This benchmark is headless request latency evidence only. It does not measure
label updates, scrolling 50,000 rows, resource create/destroy cycles, RSS,
physical GPU behavior, or idle redraws, and it does not establish GUI or
cross-platform support. Those D03 workloads still require installed desktop
and physical-hardware evidence on the required display cells.
