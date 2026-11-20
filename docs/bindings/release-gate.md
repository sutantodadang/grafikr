# Release evidence gate

Run the local evidence audit after collecting checks:

```powershell
rtk uv run --with-requirements scripts/requirements-bindings.txt python -m scripts.check_bindings_evidence --root .artifacts/bindings --mode local --out .artifacts/bindings/local-evidence-report.json
```

Local mode validates recorded evidence and reports `LOCAL_ONLY`. It does not
claim platform or SDK support. Full mode requires passing evidence for every
SDK/display/scenario cell, current source SHA, every GPU/accessibility display
cell, and a native/IPC benchmark record per display cell. Benchmark records
must identify physical hardware, include hashed raw samples, cover each workload
listed in `bindings/runtime/gui-cases.json`, and use the same complete
request-to-response endpoint with at least 1,000 samples. Missing toolchains,
displays, or hardware leave the gate failed; they are not converted to passes.

Build and verify package-manager artifacts only from a clean committed source
tree with all six runtime bundles available:

```powershell
rtk uv run --with-requirements scripts/requirements-bindings.txt python -m scripts.package_bindings --version 1.1.0 --runtime-root target/runtime/releases --out target/packages/release-1.1
rtk uv run --with-requirements scripts/requirements-bindings.txt python -m scripts.package_bindings --version 1.1.0 --runtime-root target/runtime/releases --out target/packages/release-1.1 --check
rtk uv run --with-requirements scripts/requirements-bindings.txt python -m scripts.verify_ruby_php_packages --package-root target/packages/release-1.1
```

The pack command writes a package index even when incomplete and exits nonzero.
The check command requires every SDK/target package declared by the package
matrix, all six matching runtime bundles, and verifies artifact hashes and
archive contents. Run the final consumer command on a runner matching a
published runtime target; it installs the Ruby gem and PHP Composer source and
runtime packages into clean consumers, launches the bundled host, and shuts it
down. A locally built source-dirty or partial package
index is diagnostic evidence only and cannot pass the release check.
