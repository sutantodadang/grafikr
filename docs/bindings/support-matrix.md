# Cross-language support evidence

Snapshot: 2026-11-22. This is implementation evidence, not a release support
promise. No package registry has been published.

## Contract and unit checks

Workflow run `44900304839` on commit `bd88324` passed all six native-bundle
target jobs, including SDK unit and contract checks. Its package-matrix job
failed with 53 artifacts and 16 pending targets; its index was not retained.
Workflow run `45008660730` is rebuilding all six target jobs after adding
pending-reason logging and failure-time package-index upload. The Windows Swift
toolchain still cannot find `stdlib.h`; Swift package checks pass on Linux and
macOS runners.

C and C++ contract rows include deterministic mock-ABI tests for reversed
responses, early event delivery, unsubscribe suppression, shutdown with 256
pending requests, and event-queue bounds while a callback blocks. These tests
exercise the SDK transport/client layer; they do not prove native window or GPU
behavior. The corpus excludes `callbacks.exception_hook` for C because C has no
language exception mechanism, and excludes `lifecycle.close_timeout_sentinel`
for native C/C++ because those SDKs do not own a host child process. Each
exclusion has a reason checked by the coverage runner.

## Runtime and package artifacts

All six runtime bundles pass the native-bundle CI jobs in run `44900304839`.
Linux x64 was built on Ubuntu 22.04/glibc 2.35 under Xvfb/Openbox/lavapipe;
this does not establish Wayland, physical GPU, or desktop accessibility
behavior.

A fresh Windows x64 development-fixture bundle was built at
`target/runtime/continuation-current-windows-x64`. Its manifest verifies as
`x86_64-pc-windows-msvc`, and the bundled host reports engine 1.1.0/protocol
1.0. The C and C++ consumers linked against its copied static FFI library and
passed seven CTest cases. The bundle records `source_dirty=true` and contains
the fixture host, so it is local development evidence only.

The local package candidate at `target/package-debug-output/package-index.json`
contains 88 artifacts for all six runtime targets, has zero pending entries,
and records `source_dirty=false`. The package builder also installed offline Go,
Java/Kotlin, and NuGet consumers and passed signed i64 IPC round trips. This is
local build evidence, not the missing Linux package-matrix CI result or a
published package. CI run `45008660730` will retain its package index even if
the package build fails.

The local evidence gate currently reports 935 records (609 pass, 326 fail).
These records do not fill any of the required GUI, physical hardware, or
native-versus-IPC benchmark cells. Headless installed SDK scenarios and Xvfb
input tests do not count as native desktop GUI evidence.

## Open release gates

- The package matrix passed locally for all six runtime targets, but the Linux
  CI package-matrix result is not yet green.
- Python, JavaScript, TypeScript, .NET, Go, Java, Kotlin, Ruby, PHP, Dart, and Lua
  have passing headless installed-scenario rows. C/C++ also pass local Xvfb input
  scenarios, but need clean packaged-target and desktop display evidence; Swift
  needs a successful installed-consumer run.
- The full GUI matrix requires 336 installed application scenarios across the
  SDK/display cells, using real OS input rather than synthetic `UiEvent`s.
- Eight hardware/accessibility cells and eight native-versus-IPC benchmark
  cells remain missing. Linux Wayland and physical GPU/accessibility behavior
  are unverified.
- Swift's Windows package test remains blocked by missing C standard headers;
  Swift checks on Linux and macOS do not establish Windows support.

Until those gates pass, label this work as an implementation checkpoint. Do not
claim complete cross-language platform support or public package availability.
