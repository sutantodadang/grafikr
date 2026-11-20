# SDK acceptance contract

`bindings/sdk-matrix.json` is the local source of truth for each consumer
language's runtime floor, unit/contract/installed checks, and test helper paths.
The shared transport cases live in
`bindings/protocol/v1/fixtures/sdk-cases.json`; the widget inventory comes from
`bindings/protocol/v1/widgets.json`.

The wire contract remains protocol 1.0 and the native C ABI version is 1.0
(`grafikr_abi_version() == 0x00010000`). The engine and SDK package version is
1.1.0; changing it does not imply a wire or ABI version change.

Run one suite with an absolute runtime bundle directory:

```powershell
rtk uv run --python 3.12 --with-requirements scripts/requirements-bindings.txt python -m scripts.run_binding_checks --sdk python --suite unit --bundle C:\path\to\grafikr-runtime --out .artifacts\bindings\python-unit.json
```

The runner executes matrix argv directly with `shell=False`, the configured
working directory, and `GRAFIKR_RUNTIME_DIR`. Contract checks also receive an
explicit `--fixture-host <absolute-path>` through `GRAFIKR_FIXTURE_HOST`; this
is reserved for headless conformance evidence. It is not a desktop GUI pass.

Every invocation writes an evidence file, including failures. A missing suite,
tool, helper file, required scenario, widget/property/event case, or a nonzero
child exit produces `FAIL`. Output text cannot turn a failed command into a
pass. Contract coverage is checked against the shared scenario corpus and the
complete widget inventory before its test commands run.

The C and C++ unit/contract suites use bundle-aware CMake/CTest runners. The
Lua row can select a specific Lua 5.4 executable with `GRAFIKR_LUA54` and its
LuaRocks tree with `GRAFIKR_LUA54_MODULES`; the runner configures module and
native library lookup for the SDK and its reader child process. The Swift
Windows row uses a separate scratch directory to avoid reusing another target's
SwiftPM products.

SDK-specific scenario exclusions are declared in the shared case corpus with a
reason. C does not have a language exception hook, and native C/C++ do not own a
host child process; neither exclusion removes a test requirement that applies
to that transport.

Contract rows currently pass for thirteen of fourteen SDKs on Windows x64.
Swift is still unverified there because its Windows toolchain cannot find
`stdlib.h`. Installed-package suites remain incomplete for the full SDK and GUI
matrix. A missing compiler or package tool remains a failed check, never a pass.
