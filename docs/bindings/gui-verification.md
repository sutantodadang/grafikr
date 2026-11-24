# Installed GUI verification

GUI evidence is collected from a clean installed consumer using one of the
three packaged examples: counter/form, dashboard, or multi-window. The required
cells and scenario names are in `bindings/runtime/gui-cases.json`.

Each GUI record identifies the SDK, scenario, display cell, target, backend,
source SHA, tested artifact hash, and OS input method. Accepted input methods
are native OS automation (`windows_sendinput`, `xdotool`, or `osascript`) and
supervised manual testing (`manual_supervised`). Direct `UiEvent` injection is
headless engine evidence only. Attach a screenshot and the consumer's state
report; keep GPU and accessibility reports in separate hardware evidence.

The counter/form scenario checks mouse activation, Unicode keyboard input,
state updates, Tab navigation, and IME composition. Dashboard checks scrolling,
visible data requests, selection, and canvas/image updates. Multi-window checks
modal focus, Escape, one-window close, and final shutdown. The C and C++ installed
scenario harnesses pass those input flows under local Xvfb/X11, but use a
temporary mixed Linux bundle and do not populate the required desktop display
cells.

The harness also supports native Windows x64 desktop runs. Run it separately for
C and C++ with a verified runtime bundle and a fresh evidence directory:

```powershell
rtk proxy uv run --no-project --with-requirements scripts/requirements-bindings.txt python -m scripts.run_cmake_binding_tests `
  --project c --runtime target/runtime/windows-x64 --installed-consumer-only --installed-scenarios `
  --evidence-dir .artifacts/bindings/windows-x64-local

rtk proxy uv run --no-project --with-requirements scripts/requirements-bindings.txt python -m scripts.run_cmake_binding_tests `
  --project cpp --runtime target/runtime/windows-x64 --installed-consumer-only --installed-scenarios `
  --evidence-dir .artifacts/bindings/windows-x64-local
```

This uses `SendInput`, forces the D3D12 backend, captures each application
window, and writes hashed client-state/evidence records. In the local Windows
build 26200 run, both dashboard records passed. The counter/form records remain
unverified because the harness does not test IME composition/focus preservation;
the multi-window records remain unverified because it does not test modal focus
trapping. Those records are correctly excluded from the full GUI pass count.

Run the manual `Cross-language release evidence gate` workflow with the tested
source commit, the Actions run ID holding the evidence artifact, and its artifact
name. The artifact must preserve the evidence JSON and all attachment files at
the relative paths named by each record. The workflow checks that every record
matches the supplied source SHA and uploads the audit report, including when
the gate fails. It consumes captured evidence; it does not produce GUI,
physical-GPU, accessibility, or benchmark evidence itself.
