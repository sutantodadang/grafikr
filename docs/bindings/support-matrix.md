# Cross-language support evidence

Snapshot: 2026-11-24. This is implementation evidence, not a release support promise.
No package registry has been published.

## Contract and unit checks

CI run `45312336353` on commit `bf3af3a` passed quality, benchmark, and tests on
Windows, Linux/Xvfb, and macOS. Runtime workflow run `45312336399` on the same
commit passed all six native-bundle target jobs, including SDK unit and contract
checks. One initial Windows ARM64 Lua protocol timeout did not reproduce on the
same-runner retry; the final target job passed. This does not remove the need for
the desktop evidence gates below.

C and C++ contract rows include deterministic mock-ABI tests for reversed
responses, early event delivery, unsubscribe suppression, shutdown with 256
pending requests, and event-queue bounds while a callback blocks. These tests
exercise the SDK transport/client layer; they do not prove native window or GPU
behavior. The corpus excludes `callbacks.exception_hook` for C because C has no
language exception mechanism, and excludes `lifecycle.close_timeout_sentinel`
for native C/C++ because those SDKs do not own a host child process. Each
exclusion has a reason checked by the coverage runner.

## Runtime and package artifacts

All six runtime bundles passed the native-bundle CI jobs in run `45312336399`.
Linux x64 was built on Ubuntu 22.04/glibc 2.35 under Xvfb/Openbox/lavapipe;
this does not establish Wayland, physical GPU, or desktop accessibility
behavior.

## Native desktop evidence collected locally

On a Windows 11 x64 interactive desktop (build 26200), the C and C++ installed
scenario harnesses were run against a clean local runtime bundle built from
commit `a3db248`, with `WGPU_BACKEND=dx12` and real `SendInput` mouse/keyboard
events. The local evidence directory is
`.artifacts/bindings/windows-x64-verified`; its records identify source and
runtime build SHA `a3db248`, and the manifest has `source_dirty=false`.

The C and C++ dashboard scenarios passed, including the 50,000-row range,
selection, canvas/image, and cleanup checks. Their counter/form and multi-window
records remain `UNVERIFIED`: IME/focus preservation and modal focus trapping
were not exercised. This leaves 334 of 336 GUI scenario cells incomplete. The
Windows x64 hardware/accessibility cell passed: the application reported its
actual D3D12 adapter as a discrete NVIDIA RTX 4070 SUPER, and Windows UI
Automation exposed a named, keyboard-focusable Increment button and the Unicode
text-field value; OS keyboard input updated the bound state. Seven
hardware/accessibility cells remain open. No native-versus-IPC workload
benchmark evidence was collected; all eight benchmark cells remain open.

An earlier Windows attempt used the CI bundle from `bf3af3a`. Diagnostics showed
that it selected Vulkan despite the test harness setting `WGPU_BACKEND=dx12`,
because the shell used wgpu's non-environment instance constructor. Those early
records are superseded and excluded. The shell now applies the backend variable
and reports the selected adapter only when test diagnostics are explicitly
enabled.

A fresh Windows x64 development-fixture bundle was built at
`target/runtime/continuation-current-windows-x64`. Its manifest verifies as
`x86_64-pc-windows-msvc`, and the bundled host reports engine 1.1.0/protocol
1.0. The C and C++ consumers linked against its copied static FFI library and
passed seven CTest cases. The bundle records `source_dirty=true` and contains
the fixture host, so it is local development evidence only.

The successful Linux package-matrix job in run `45312336399` produced a clean
index with 88 artifacts for all six runtime targets, zero pending entries, and
`source_dirty=false`. Hash and target coverage checks passed. Clean Ruby/PHP,
Dart, Swift, and Lua package consumers passed; C/C++ installed input scenarios
passed under Xvfb/X11. The six native target jobs also passed their target-local
SDK and package consumer checks. These are build and installed-consumer results,
not registry publication or native desktop support evidence.

The full evidence gate still has no complete desktop display cell and no
native-versus-IPC benchmark records. The local Windows run contributes two
passing GUI scenarios and one hardware/accessibility cell; it does not make the
Windows x64 display cell complete. Headless installed SDK scenarios and Xvfb
input tests do not count as native desktop GUI evidence.

## Open release gates

- The package matrix is green in CI for all six runtime targets, with 88
  checksummed artifacts and zero pending targets. No package registry has been
  published.
- Installed headless package scenarios pass for the SDKs exercised by the CI
  workflow. C/C++ input scenarios also pass under Xvfb/X11. These results still
  need the required per-SDK desktop display evidence.
- The full GUI matrix requires 336 installed application scenarios across the
  SDK/display cells, using real OS input rather than synthetic `UiEvent`s.
- Eight hardware/accessibility cells and eight native-versus-IPC benchmark
  cells remain missing. Linux Wayland and physical GPU/accessibility behavior
  are unverified.
- Windows x64 has two locally passing dashboard scenarios and four unverified
  counter/multi-window scenarios across C and C++, plus one passing
  hardware/accessibility cell. No display cell is complete.
- Xvfb and software Vulkan do not establish physical GPU, Wayland, or
  accessibility behavior.

Until those gates pass, label this work as an implementation checkpoint. Do not
claim complete cross-language platform support or public package availability.
