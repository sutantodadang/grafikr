# Cross-language support evidence

Snapshot: 2026-11-20. This is local implementation evidence, not a release
support promise. No package registry has been published.

## Contract and unit checks

The earlier Windows x64 aggregate contract report is
`.artifacts/bindings/continuation-2026-11-20-contract-current.json`.
Thirteen of fourteen SDK rows pass: C, C++, Python, JavaScript, TypeScript,
.NET, Go, Java, Kotlin, Dart, Ruby, PHP, and Lua. Swift is the failing row; the
Windows Swift toolchain cannot find `stdlib.h`. Its installed C++ standard
headers are incomplete, and an explicit include-path workaround crashes the
Swift frontend.

After the callback-limit fixture changed to seventeen 1 MiB events, the fresh
aggregate run at
`.artifacts/bindings/continuation-2026-11-20-contract-after-1m-burst.json`
has twelve passing SDK rows. Swift has the same Swift Crypto build failure;
Lua ran without its configured Lua 5.4 executable and module path, so its row
is not a valid Lua 5.4 result. A separate Windows direct-source harness
compiled the current Swift SDK and passed all fifteen `ProtocolTests`,
including count and byte queue overflow. It used a local XCTest shim and is not
SwiftPM package-build evidence.

The corresponding unit aggregate is
`.artifacts/bindings/continuation-2026-11-20-unit-final-windows.json`. It also
passes thirteen of fourteen rows, with the same Swift failure. Both runs used a
fresh Windows x64 runtime bundle built from the current working tree; the bundle
is source-dirty and is not a release artifact. Lua uses `GRAFIKR_LUA54` for the
Lua 5.4 executable and `GRAFIKR_LUA54_MODULES` for the matching LuaRocks tree.
The runner sets module and native-library paths for both the SDK and reader
child. Ruby's fixture-host unit row receives `GRAFIKR_FIXTURE_HOST` explicitly.

C and C++ contract rows include deterministic mock-ABI tests for reversed
responses, early event delivery, unsubscribe suppression, shutdown with 256
pending requests, and event-queue bounds while a callback blocks. These tests
exercise the SDK transport/client layer; they do not prove native window or GPU
behavior. The corpus excludes `callbacks.exception_hook` for C because C has no
language exception mechanism, and excludes `lifecycle.close_timeout_sentinel`
for native C/C++ because those SDKs do not own a host child process. Each
exclusion has a reason checked by the coverage runner.

## Runtime and package artifacts

Windows x64 and Linux x64 runtime bundles are present and their headless native
build/CTest checks pass. Linux x64 was built on Ubuntu 22.04/glibc 2.35 under
Xvfb/Openbox/lavapipe; this does not establish Wayland, physical GPU, or desktop
accessibility behavior.

A fresh Windows x64 development-fixture bundle was built at
`target/runtime/continuation-current-windows-x64`. Its manifest verifies as
`x86_64-pc-windows-msvc`, and the bundled host reports engine 1.1.0/protocol
1.0. The C and C++ consumers linked against its copied static FFI library and
passed seven CTest cases. The bundle records `source_dirty=true` and contains
the fixture host, so it is local development evidence only.

The newest local package index is
`target/packages/continuation-windows-only/package-index.json`. It contains
28 validated artifacts for Windows x64 and has 51 pending entries. A prior
candidate at
`target/packages/release-after-mock-transport-two-targets/package-index.json`
contains 40 artifacts for Windows x64/Linux x64 with 41 pending entries. Four
required runtime targets are missing:

- `aarch64-pc-windows-msvc`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

The source tree is dirty, so these artifacts are not release-clean. Clean
consumer checks pass for NuGet, Go, Java, Kotlin, Node/TypeScript, and Python.
The Windows x64 C/C++ consumers also pass CTest while linking the copied runtime
library. A separate local C/C++ installed-consumer harness passed counter/form,
dashboard scrolling/selection, and modal multi-window scenarios through X11
input under Xvfb. That run used a temporary mixed bundle with a Linux FFI built
on Ubuntu 26.04/glibc 2.43, so it is not evidence for the packaged Ubuntu 22.04
runtime or a required desktop display cell. Python, JavaScript, TypeScript, .NET,
Go, Java, Kotlin, Ruby, PHP, Dart, and Lua installed-suite rows pass the headless
counter/form, dashboard, and multi-window scenarios against the fixture host;
those runs do not count as GUI evidence. The package index validator accepts these as incomplete local candidates;
`package_bindings --check` rejects them because required runtime targets are
missing and the source tree is dirty.
Ruby's installed gem scenario is recorded at
`.artifacts/bindings/continuation-windows-only-ruby-installed.json`. The PHP
installed Composer consumer row is recorded at
`.artifacts/bindings/continuation-windows-only-php-installed.json`; it used a
locally SHA-256-verified Composer PHAR and PHP 8.4 OpenSSL/ZIP modules under
`target/` because the system PHP build omits those modules. Dart's installed
source-package scenarios are recorded at
`.artifacts/bindings/continuation-windows-only-dart-installed.json`.
Lua's source-rock consumer run is recorded at
`.artifacts/bindings/continuation-windows-only-lua-installed.json`.
The Swift source-package consumer is configured for the Linux package-matrix
workflow. The local Windows run is unverified because SwiftPM cannot compile
swift-crypto without the missing MSVC C headers; the Swift scenarios do pass
Swift type-checking against the SDK source.

## Open release gates

- Six native runtime targets and package artifacts are required; only Windows
  x64 and Linux x64 bundles are currently in the local index.
- Python, JavaScript, TypeScript, .NET, Go, Java, Kotlin, Ruby, PHP, Dart, and Lua
  have passing headless installed-scenario rows. C/C++ also pass local Xvfb input
  scenarios, but need clean packaged-target and desktop display evidence; Swift
  needs a successful installed-consumer run.
- The full GUI matrix requires 336 installed application scenarios across the
  SDK/display cells, using real OS input rather than synthetic `UiEvent`s.
- Eight hardware/accessibility cells and the native-versus-IPC benchmark
  workloads need current evidence from the required display and GPU targets.
- Windows ARM64, Linux ARM64, both macOS architectures, Linux Wayland, and
  physical GPU/accessibility behavior remain unverified.
- Swift's Windows SDK tests remain blocked by the missing C standard headers.

Until those gates pass, label this work as an implementation checkpoint. Do not
claim complete cross-language platform support or public package availability.
