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

The full evidence gate still has no accepted desktop GUI, physical
hardware/accessibility, or native-versus-IPC benchmark records. A local full
audit reports 935 evidence records (609 pass, 326 fail) and explicitly lists
336 missing GUI scenarios, eight hardware/accessibility cells, and eight
native-versus-IPC benchmark cells. Headless installed SDK scenarios and Xvfb
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
- No native desktop display cell has been accepted yet. In particular, Xvfb and
  software Vulkan do not establish physical GPU, Wayland, or accessibility
  behavior.

Until those gates pass, label this work as an implementation checkpoint. Do not
claim complete cross-language platform support or public package availability.
