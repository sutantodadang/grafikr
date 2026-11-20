# Cross-language implementation evidence

Snapshot: 2026-11-20, Windows 11 build 26200, `x86_64-pc-windows-msvc`.
This is an implementation checkpoint, not a support or release claim.

- Linux x64: the current source builds and validates
  `target/runtime/linux-container-output/x86_64-unknown-linux-gnu-frame-redraw`
  on Ubuntu 22.04/glibc 2.35. Bundle CTest, host version, C/C++ CMake/CTest,
  parity fixture, widget inventory, collection-provider, and installed-consumer
  checks pass under Xvfb/Openbox/lavapipe. Dynamic widget patches and child
  changes now request a shell frame; regressions cover collection creation after
  a cleared frame request and child changes. The manifest explicitly lists
  `libxkbcommon-x11.so.0`, which `ldd` cannot discover because winit loads it
  with `dlopen`. This is virtual-display evidence, not physical desktop/GPU or
  Wayland evidence.
- Windows x64: `target/runtime/windows-frame-redraw` was rebuilt from the same
  source. C and C++ CMake/CTest, parity fixture, native collection provider, and
  full widget-inventory checks pass against that bundle.
- C/C++ installed-scenario harnesses now pass counter/form, a 50,000-row
  dashboard with selection, and modal multi-window Tab/Escape flows under local
  Xvfb/X11 automation. The test used a temporary mixed bundle with a Linux FFI
  built on Ubuntu 26.04/glibc 2.43; it is not a clean installed artifact or one
  of the required native desktop evidence cells.

## Verified locally

- Rust: the latest full workspace test run passes; `grafikr-bridge` and
  `grafikr-shell` pass Clippy with `-D warnings`, and workspace formatting ran.
  WSL Ubuntu 26.04 on x86_64 passes
  `cargo +1.88.0 check --workspace --all-targets --locked`. The same toolchain
  passes `cargo +1.88.0 check --target aarch64-unknown-linux-gnu -p grafikr-host -p grafikr-ffi`;
  this cross-target compile is not native runtime evidence.
- Runtime: `hello` now reports every implemented capability group and the
  active frame, queue, handle, image, canvas, model, collection, and tree limits;
  state, binding, and subscription handle quotas are counted per kind. The
  runtime also includes bounded JSONL protocol, retained document/tree operations,
  one-way/two-way binding machinery, filtered subscriptions, image upload,
  Canvas rect/bordered-rect/shadow/text/image painting and resize events,
  finite `f64` state transitions with elapsed-time shell scheduling and
  reduced-motion snapping (`window.create`/`window.update`), bounded collection
  cache, controlled shell, and Windows host executable are
  present. VirtualList and DataGrid request visible ranges, paint returned rows,
  and refresh on collection metadata changes. TreeView creates from stable-key
  snapshots, emits key-based selection events, and accepts root/selection updates
  while preserving expansion and surviving selection by key; bridge integration
  coverage exercises a root reorder followed by selection.
- Host process: subprocess tests cover version output, split UTF-8 request,
  hello gating/version mismatch, accepted-request draining at EOF, malformed
  and oversized terminal frames, and clean EOF shutdown. These cases do not
  open a window.
- Native: Rust C ABI tests pass; C and C++ headers and thin clients compile with
  MSVC. C/C++ now include owned JSON value trees, exact signed-64-bit number
  serialization, and strict cJSON whitespace handling; their static-library
  CTest rows pass (`.artifacts/bindings/c-unit-after-values.json`,
  `.artifacts/bindings/cpp-unit-after-values.json`).
  The native C/C++ transport contract targets additionally use a deterministic
  mock ABI to check reversed response correlation, early event delivery, and stale-event
  suppression after unsubscribe; both targets pass MSVC CTest on the Windows x64 bundle.

  Stop-before-run harnesses link and pass against both the Windows DLL/import
  library and Rust static library. FFI run completion now releases its
  running/thread ownership state, allowing session destruction afterward.
  This does not exercise callbacks or a running native GUI.
- Python 3.12: 25 client/runtime tests pass. A `CollectionProvider` subscribes
  before collection creation, serves visible revision/range requests over the
  fixture IPC host, updates metadata, and closes its bounded request stream.
  The SDK runner including its external wheel consumer passes in
  `.artifacts/bindings/python-unit-after-collection-provider.json`. The platform
  wheel builder verifies
  the selected bundle and emits a target-tagged wheel containing the host,
  manifest, native libraries, headers, CMake config, licenses, and native
  examples. A clean
  venv installed the Windows x64 wheel from a separate working directory;
  `App()` resolved the site-packages host and completed an i64-minimum
  state round-trip. The matrix also builds a fresh wheel, installs it offline
  in a clean venv, verifies import starts no process, and performs an IPC state
  round-trip. This covers package loading and IPC smoke only, not the three
  installed GUI scenarios.
- Node 24 / TypeScript 5.8: TypeScript build and eleven node:test checks pass.
  The worker owns process I/O and framing, queues callbacks sequentially, and
  enforces a 256-request outstanding cap plus event count/byte quotas. Tests
  cover child crashes,
  early events,
  nested async requests behind a synchronous callback, JSON BigInt rejection,
  callback-error-hook delivery, blocked-handler credit exhaustion,
  request-credit exhaustion, late responses after close, and Unicode
  bundle paths with SHA-256 tamper detection. The Node runtime
  now includes a bounded `CollectionProvider` that subscribes before metadata
  creation, streams revision-tagged visible ranges, provides rows, updates
  metadata, and closes waiting reads on disposal. JavaScript and TypeScript
  unit/package runners pass in
  `.artifacts/bindings/javascript-unit-after-collection-provider.json` and
  `.artifacts/bindings/typescript-unit-after-collection-provider.json`.
  The Node runtime
  packager verifies target bundle contents, and clean offline JavaScript and
  compiled TypeScript consumers install separate SDK/runtime tarballs outside
  the source tree and pass signed i64 round trips. All six target runtime
  packages are declared as optional dependencies and the packed SDK metadata
  is checked; a registry install that resolves the matching optional package
  has not run. Complete
  protocol/widget coverage and all three installed scenarios remain open.
  Six-target packaging/smoke CI is configured but has not run.
- Go 1.26: package tests, vet, and `-race` pass, including an external-package early-event
  test against the shared fixture, request/event queue count and byte limits,
  256 outstanding requests, and a Unicode-path runtime install/tamper test.
  `grafikr-runtime` now verifies and atomically caches an unpacked target
  bundle. An external consumer now
  installs the versioned module from a local offline Go proxy, installs the
  verified Windows x64 runtime, and passes signed i64 minimum/maximum IPC
  round trips. The Go unit matrix passes with this package check; evidence is
  `.artifacts/bindings/go-unit-after-collection-provider.json`. Go exposes typed Canvas and
  Image facades; fixture-host IPC tests update Canvas commands, upload a
  multi-chunk RGBA image, attach it to a widget, and release its resource.
  Go now exposes a `CollectionProvider` that subscribes before metadata
  creation, streams bounded revision/range requests to a caller-owned worker,
  provides rows, updates metadata, and closes its request stream on disposal.
  Its fixture-host test passes through the full Go package runner, including
  package consumer verification and `go vet`, with evidence in
  `.artifacts/bindings/go-unit-after-collection-provider.json`. `go test -race`
  also passes. Complete widget inventory remains open.
- .NET 10: client build, dependency-free harness, and NuGet pack pass. A clean
  external consumer installed the local NuGet package, launched the verified
  Windows host from its bundled RID package, verified runtime file hashes, and
  round-tripped signed i64 minimum/maximum values. Windows x64 passed locally;
  the package verifier checks every manifest-listed runtime file, including
  nested licenses and the C/C++ examples. Other RID package builds and the full
  callback/cancellation/contract suites remain open. The shared fixture can now emit a subscription event before its
  reply; the .NET harness passes this early-delivery case through the matrix
   runner. Pending requests are capped at 256; the callback queue enforces
   1,024 events and 16 MiB. .NET now includes a bounded `CollectionProvider`
   with strict revision/range parsing, early subscription, row replies, metadata
    updates, and reader wakeup on disposal; its harness and clean NuGet consumer
    pass against the fresh Windows x64 bundle in
    `.artifacts/bindings/dotnet-unit-after-collection-provider-fresh-bundle.json`.
    The older `target/runtime/current` bundle remains stale and lacks
    `system_requirements`.
- Java 21 / Kotlin 2.2: the Gradle 9.8.1 wrapper is checked in with its
  distribution SHA-256 pin. JUnit tests pass for early event delivery and
  signed i64 boundaries; Kotlin compiles through the same wrapper. Clean Java
  and Kotlin Maven consumers install the local artifact and pass signed i64 IPC
  round trips against the Windows bundle. The Java client caps pending requests
   at 256; per-RID runtime classifiers remain open. Java now also exposes a
   bounded `CollectionProvider` that subscribes before metadata creation,
   validates revision-tagged ranges, answers row requests, updates metadata,
   and wakes waiting reads on close; Gradle tests pass with the local runtime
   classifier configured.
- Swift, Dart, Ruby, PHP, and Lua now register event handlers before sending
  `event.subscribe`, preventing an event delivered ahead of its response from
  being lost; provisional registrations are removed when subscription fails.
  Each SDK now has a fixture-host early-delivery integration check, and the
  matrix builds that fixture and enables its early-event mode. Ruby also tests
  provisional-handler cleanup on subscribe failure. Ruby 3.4 and PHP 8.4 now
  pass their local unit runners; Ruby's callback queue fails the session at
   1,024 events or 16 MiB and now has count/byte overflow tests. Dart 3.14.0
   now passes 17 unit scenarios plus dedicated out-of-order-response and
   shutdown-timeout checks. Analyzer, `pub publish --dry-run`, and an offline
   clean-consumer run of its state example against the selected release bundle
   also pass. `Runtime.install` validates
   unpacked manifests and file hashes before atomically caching the host. Its
   process reader runs in a dedicated isolate, with bounded SendPort credits
   capped at 1,024 messages/16 MiB for event output and outstanding writes.
   Fixture tests prove the reader drains a 12 MiB host burst while a synchronous
   callback blocks, and enforce count/byte overflow. Dart also reports handler
   errors through `App.start(onError:)`, awaits async handlers sequentially,
   correlates out-of-order replies by request ID, round-trips a 100 KiB UTF-8
   frame, allows nested async requests, rejects non-finite JSON values before
   writing, and suppresses queued events on unsubscribe. `App.close()` releases
   pending requests with `connection_closed` and kills a child stalled after
   shutdown without affecting a sentinel process.
   Its tests cover isolate startup failure and exact signed i64 boundaries.
   Evidence is `.artifacts/bindings/dart-unit-s12-progress.json`. Swift has a bounded async
  `CollectionProvider` and an integration test wired into the matrix. Swift
  6.4.2 on Linux x64 passes the SDK unit runner
  (`.artifacts/bindings/swift-unit-linux-x64.json`), and a clean consumer builds
  against the refreshed Swift source archive from
  `target/swift-close-consumer`. `App.close()` also resolves pending requests
  with `connection_closed`. Lua now has a callback-based bounded provider
   and a live-range fixture scenario wired into its matrix. Its isolated
  collection self-test previously passed under Lua 5.1. After installing Lua
  5.4 and the declared rocks in the Ubuntu builder, the full Lua unit runner
  passes against the Linux x64 fixture host; evidence is
  `.artifacts/bindings/lua-unit-linux-x64.json`. The Lua source rock from
  `target/packages/cmake-link-fix-two-targets` installs
  with its dependency closure into the isolated
  `target/lua-install-smoke` tree. Running the protocol example from `/tmp`
  loads that installed module and passes early-subscription and collection
  flows against the absolute Linux fixture-host path. The shared fixture emits a
  revision-tagged range from an attached, visible list, and the Ruby test
   exercises that path. Ruby callback exceptions reach `App#on_error`,
  and unsubscribe removes the local handler before the request so queued stale
  events are suppressed. Host stderr drains in fixed-size chunks. The fixture
  host also starts from a copied path with spaces and Unicode using the
  explicit executable/argv form.
- Protocol 1 schema validates the checked-in valid/invalid corpus and outbound
  response/event examples with jsonschema 4.25.1. The corpus now includes
  TreeView create, select subscription, and stable-key root replacement.
- Runtime distribution: a native Windows x86_64 bundle builds and validates
   locally; the bundled host reports engine 1.1.0/protocol 1.0 and the build
  includes passing static C/C++ CTest harnesses. Its manifest verifies all
  sixteen shipped file hashes, quota values, target minima, and the actual PE
  architecture of the host and six examples. Its declared Windows prerequisite matches the `VCRUNTIME140.dll`
  import observed in the host and FFI DLL. Evidence schema validation checks provenance/status and
  verifies attachment paths and hashes. The artifact is marked source-dirty
  and is local build evidence, not a release artifact. Linux ARM64 now passes a
  Rust 1.88 cross-target check from x86_64, but still lacks a native bundle or
  runtime. Linux x86_64 also builds in an Ubuntu
  22.04/glibc 2.35 container with Rust 1.88.0; its manifest validates, the host
  reports engine 1.1.0/protocol 1.0, and `ldd` found no unresolved direct
  dependencies. C/C++ static CTest passes under Xvfb. This is headless build and
  event-loop evidence only; it does not count as desktop, GPU, or accessibility
  verification. The refreshed bundle at
  `target/runtime/releases-linux-dependency-audit/x86_64-unknown-linux-gnu`
  validates with eleven external ELF sonames recorded in `system_requirements`;
  `ldd` found no unresolved dependencies for host or FFI, and the bundle build's
  C/C++ CTest passed under Xvfb. The renderer's offscreen golden scenes also
  pass on lavapipe. The manifest validator also passed on Windows.
  The exported Linux CMake target now propagates fontconfig, `m`, `dl`, and
  `Threads::Threads`; this fixes downstream static C/C++ link failures. A
  regenerated local Linux bundle copy passes clean installed C and C++ CMake
  consumers, and that fixed export is hash-verified in the native Linux zip at
  `target/packages/cmake-link-fix-two-targets`. Dynamically loaded X11/Wayland
  and GPU requirements remain unaudited. macOS and ARM64 remain
  unbuilt. A separate development-only fixture host also builds
  and exercises the same protocol dispatcher against the CPU memory backend;
  it does not prove GUI behavior and the release workflow does not bundle it.
- The SDK check runner now supports `--sdk all`, writes per-SDK and aggregate
  evidence, normalizes CLI-relative bundle paths, resolves target runtime
  classifiers from the selected manifest, and emits evidence matching the
  runtime evidence schema. The refreshed Windows x64 unit matrix passes 13 of 14
  rows: C, C++, Python, JavaScript, TypeScript, .NET, Go, Java, Kotlin, Dart,
  Ruby, PHP, and Lua 5.4. Evidence is
  `.artifacts/bindings/all-unit-windows-x64-radio-widgets.json`; the Swift row
  fails because the installed Windows toolchain lacks its target standard
  library. Linux x64 Swift unit and packaged-consumer checks pass. The earlier
  Windows aggregate report is stale. Missing SDK tools generate explicit failed
  evidence instead of being reported as passing.
- The release evidence gate now has `local` and `full` modes. Local mode
  validates and summarizes records as `LOCAL_ONLY`; full mode requires all 336
  installed GUI scenario cells, eight independent hardware/accessibility cells,
  and eight native/IPC benchmark cells with matching request-to-response
  endpoints, raw sample attachments, workload inventory, and 1,000 samples.
  It checks source provenance, backend, OS input method, and attachment hashes.
  The local audit is `.artifacts/bindings/local-evidence-report.json`. Full
  mode correctly fails because no native GUI or hardware cells are recorded.
- SDK acceptance foundation: the matrix and machine-readable transport/installed
  scenario corpus cover all 14 consumer languages. The no-shell runner writes
  failure-complete evidence and checks required case/widget coverage. Its 12
  runner self-tests pass. The full tooling suite now has 93 tests (92 passed;
  one Windows symlink test is skipped by account permissions). Two S12 checks
  prove that missing transport cases,
  widget events, installed scenarios, or consumer helpers fail before SDK test
  execution. Python, JavaScript, Go, and .NET unit suites pass through the
  runner. Python callback errors reach an error hook, its callback queue is
  count/byte bounded, unsubscribe suppresses queued callbacks, and close
  respects the five-second deadline; callback backpressure terminates the host.
  It provides a Future-returning request path for nested callback operations.
  Typed Python State preserves signed 64-bit boundaries and rejects overflow
  and non-finite values locally. The client also rejects a 257th outstanding
  request before writing it; its unit test confirms the host receives no
  overflow request.
  Its wheel build uses no Cargo invocation, and an installed-package probe
  confirms import does not start a process. The Python matrix now runs this
  wheel/install smoke as part of its unit suite. Python and .NET contract suites
  pass; most SDK contract and all installed-package scenario gates remain open.
- The release packer builds Swift, Dart, PHP, and Lua source archives from
  explicit allowlists, target-specific PHP Composer runtime packages, and
  target-platform Ruby gems. Ruby startup verifies its bundled runtime; PHP
  resolves an explicitly installed Composer runtime package and verifies its
  file hashes. Maven API/runtime packages include their POM files. A Windows
  x64 smoke produced 27 artifacts and passed external NuGet, Go, Java, and
  Kotlin consumer checks. The Composer source and Windows runtime packages
  passed archive/hash checks. A temporary PHP 8.4.27 x64 toolchain matched the
  official archive SHA-256; the PHP SDK unit runner passes, including the
  Windows pipe relay protocol, early event delivery, nested callback requests,
  and callback exception delivery to `App::onError`
    (`.artifacts/bindings/php-unit-after-live-collection-provider.json`). PHP now
   exposes a bounded collection provider that pumps host frames while awaiting
   range requests, validates row ranges, and supports provide/update/close.
   Composer strict
  validation passes with `--no-check-version` because the package version is
  required by the artifact repository. A clean Composer artifact consumer
  installs the source and Windows runtime packages and starts/closes the
  fixture host using the installed SDK. Ruby 3.4.11 was extracted under
  `target/` after its official release asset hash matched; the Ruby unit runner
   passes (`.artifacts/bindings/ruby-unit-after-live-collection-test.json`). Ruby
  exposes a bounded `CollectionProvider` that subscribes before collection
  creation, serves revision-tagged ranges, updates metadata, and wakes blocked
  reads when closed. The
  Windows platform gem builds, installs into an isolated GEM_HOME, verifies
  its embedded runtime, and starts/closes the installed host after the queue
  change. A later full package pass timed out before writing an index; its
  staging directory contained the Ruby gem and PHP source/runtime artifacts.
  The newest two-target package index is validated below, but remains
  source-dirty and incomplete. The full tooling suite has 96 tests (95 pass,
  one Windows symlink test skipped).

- Local packaging consumes verified Windows x64 and Linux x64 bundles and
  produced 40 artifacts in
  `target/packages/release-after-mock-transport-two-targets`. Its index has 41
  pending entries: four runtime targets are absent across the SDKs and the
  release source tree is dirty. Package-index structure and all artifact hashes
  validate; recorded runtime manifest hashes match both bundles. The Linux native
  zip includes the corrected CMake export with a matching manifest hash. NuGet,
  Go, Java, and Kotlin clean consumers pass. The package check correctly refuses
  the incomplete index because all six runtime targets are required for release.
  A fresh Windows-only package run from the current non-fixture bundle produced
  28 artifacts in `target/packages/windows-handshake-refresh`; NuGet, Go, Java,
  and Kotlin clean consumers passed. Its check correctly remains FAIL with 51
  pending entries because five runtime targets are absent and the source tree
  is dirty.
  D01 also built the Windows x64 development bundle at
  `target/runtime/hello-limit-update`; its manifest validates, `grafikr-host --version`
  reports engine 1.1.0/protocol 1.0, C/C++ installed consumers and
  the native C contract harness pass, and PE dependency inspection records the
  VC++ runtime requirement. This is a development-fixture bundle with a dirty
  source tree, not a release artifact. Runtime validation now rejects directory
  junctions as well as symlinks and hard links; the full tooling suite passes
  with one symlink-creation check skipped by the Windows account.

## Still required by the approved plan

- Protocol conformance coverage for every operation/widget, plus native/IPC
  transcript parity. TreeView now has widget-level regressions for stable-key
  selection and expansion across root reorder, plus explicit selection patches.
  `check-parity` now launches the compiled C ABI scenario harness and the
  Windows IPC host against a 49-request fixture covering signed-i64 and bool
  state, transitions, window/node lifecycle, property updates, image upload,
  Canvas, TreeView, collection metadata, stale data, and invalid handles.
  The handshake now advertises the complete capability list and protocol limits,
  and handle quotas are independent per object kind. Transcripts match in
  `.artifacts/bindings/parity-handshake-limits.json`. Widget input events and
  full operation parity remain open. The CPU
  MemoryBackend test also verifies a bound state update changes rendered text
  and a synthetic button click publishes the subscribed event.
- Complete C/C++ APIs: synchronous request correlation, subscriptions, callback
  scheduling, all three real examples, installed CMake exports, and running-
  thread ownership tests. The C API now has a sole response reader, bounded
  pending request tracking, synchronous `gf_request`, structured bridge errors,
  `gf_on`/`gf_off`, and worker-thread callback dispatch. C exposes a bounded
  collection provider that subscribes before metadata creation, queues one
  range request, and supports timed reads/provide/update/close. Its C harness
  verifies invalid metadata, lifecycle rejection, and parsed JSON integer
  handling. The `grafikr_collection_provider_scenario` executable was run
  against the fresh Windows x64 native runtime and successfully received and
  supplied a visible range. C++ exposes
  `Client::on` and move-only RAII `Subscription`; its C trampoline catches
  exceptions. C++ now has a bounded RAII `CollectionProvider` that subscribes
  before metadata creation and supports range wait/provide/update/close. The
  C++ unit/package runner passes parser and metadata validation in
  `.artifacts/bindings/cpp-unit-current-runtime.json`; its
  `grafikr_cpp_collection_provider_scenario` also passed against the fresh
  Windows x64 native runtime. Both C and C++ now test the native request queue
  at its 1,024-message and exact 16 MiB boundaries; their unit runners pass in
  `.artifacts/bindings/c-unit-after-queue-quotas.json` and
  `.artifacts/bindings/cpp-unit-after-callback-errors.json`. C++ now exposes
  `Client::on_error`; its unit check verifies forwarding and containment when
  the error hook itself throws. `Subscription::dispatch` retains its callback
  context for the duration of dispatch, including self-unsubscribe. Their
  contract runners now launch the 50,000-row
  native collection scenarios: C verifies a nested request from `data.request`,
  and C++ also verifies the error hook receives a thrown callback exception.
  Their current native CMake/CTest checks pass. The contract evidence reports
  `.artifacts/bindings/c-contract-expanded-parity.json` and
  `.artifacts/bindings/cpp-contract-expanded-parity.json` record native
  check exit code 0; each overall row remains FAIL because shared cases are
  still uncovered.
  Seven shared cases remain uncovered for C and six for C++.
  Both CMake
  package-consumer checks pass against
  that bundle (`--installed-consumer-only`). Linux clean C and C++ installed
  CMake consumers now pass from the refreshed local bundle after fixing
  transitive static-link dependencies; the corrected CMake export is included
  in the hash-validated Linux native package. The SDK `installed` matrix rows
  still fail coverage validation for the three OS-driven app scenarios. The
  Linux C and C++ collection-provider scenarios now both receive visible-range
  rows and exit successfully under Xvfb with Openbox and lavapipe. This is
  headless Xvfb/lavapipe event-loop evidence, not physical-GPU or interactive
  desktop verification. Counter-form captures show either a black frame or only
  the TextField; forcing Mesa GL produces the same partial capture. These
  scenarios do not prove complete visible widget rendering or OS-input handling.
  C/C++ provider scenarios verify successful `data.request` event delivery and
  response requests from provider-reader threads on Windows and Linux headless.
  Failed-subscription cleanup and request flow are also tested. Generic widget
  event callbacks, unsubscribe suppression during queued dispatch,
  callback-nested requests, and off-after-destroy remain unverified. The three
  C and three C++ example applications compile and ship in the Windows bundle;
  their PE architectures are verified against the manifest target. The runtime
  bundle exports `Grafikr::client` and `Grafikr::cpp`; clean installed-package
  consumers link and run. C/C++ matrix rows pass in
  `.artifacts/bindings/c-unit-current-runtime.json` and
  `.artifacts/bindings/cpp-unit-current-runtime.json` against the refreshed
  Windows x64 runtime.
  Real GUI examples
  and running-thread ownership tests remain open.
- Extend native/IPC transcript parity to the remaining operations, error paths,
  widget input events, and live collection request/provide cycle.
- Installed-package consumers and counter/form, dashboard, and multi-window
  scenarios for all SDKs. Current examples/tests are partial and often use a
  fake host for transport-only checks.
- Full D03 release gate: all 336 installed GUI scenario cells, eight physical
  GPU/accessibility cells, remaining GUI/hardware benchmark workloads, and
  screenshots/client-state evidence.
  The checker is implemented and its full-mode failure is expected until those
  real desktop records exist.
- Headless C ABI versus IPC `state.get` request-to-response benchmarking is
  implemented in `scripts/benchmark_bindings.py`, including 100 warmups,
  1,000 raw samples per endpoint, manifest/library hash checks, and p50/p95.
  A fresh Windows x64 run using the capability/limit handshake bundle is
  recorded in `.artifacts/bindings/benchmark-handshake-windows/report.json`
  with 1,000 raw samples per endpoint. It uses a dirty development-fixture
  bundle and measures state reads only; it does not cover label updates,
  50,000-row scrolling, resource lifecycle, RSS,
  physical GPU behavior, or idle redraw, so it cannot satisfy the full benchmark
  gate. See `docs/bindings/benchmarks.md`.
- Complete SDK contract-suite coverage for every protocol case and widget
  property/event, plus the installed-package runner commands for all consumers.
- Runtime bundle generation for non-Windows/ARM targets, checksum-verified
  SDK installation, and packaging the matching runtime in package-manager
  artifacts. The new six-runner CI matrix is configuration only and has not
  executed; Windows DLL and
  Linux/macOS dynamic loader/system dependency audits, third-party notices,
  and non-Windows bundles remain open. The bundle builder rejects Linux
  builds above the declared glibc 2.35 baseline and pins the macOS deployment
  target to 14.0, but those paths remain unexecuted here.
- Linux x86_64 has headless build, C/C++ CTest, runtime-manifest, and installed
  CMake-consumer evidence. Linux ARM64 passes a Rust 1.88 cross-target
  `cargo check` in WSL x86_64, but no native ARM64 bundle/runtime evidence
  exists. Interactive Linux X11/Wayland, Linux ARM64, both
  macOS architectures, and Windows ARM64 builds and GUI runs remain open.
- Real desktop input, GPU, accessibility, 50,000-row, resource-leak, and
  native-vs-IPC performance evidence.
- Earlier Swift Linux x64 unit and packaged-source checks passed with Swift
  6.4.2 before adding the portable SHA-256 runtime installer. The current
  package manifest and Swift sources type-check with local API shims, but the
  real package test is pending. Windows Swift 6.4.2 cannot build Swift Crypto's
  C target because `stdlib.h` is not found, even with the Visual Studio
  developer environment. macOS runtime, native GUI, and complete
  protocol-contract coverage remain open.
  Lua unit and contract checks pass on Linux x64 and Windows x64 with Lua 5.4;
  the Windows `luv` rock was built with the RubyInstaller UCRT toolchain. Its child reader
  process continues to drain host output while a callback blocks, bounds
  unacknowledged frames at 1,024/16 MiB, and fails with `queue_full` on overflow.
  Lua unit and full 12-case/123-ID contract checks pass in
  `.artifacts/bindings/lua-unit-after-reader-process.json` and
  `.artifacts/bindings/lua-contract-after-reader-process.json`. The source
  package test verifies the relay helper ships in the Lua rock; all 33
  `scripts.tests.test_package_bindings` checks pass. A LuaRocks install into the
  local target tree and a state round-trip from a temporary consumer directory
  also pass with dependency checks disabled; `luaossl` is unavailable here, so
  the full install/package gates remain open.
  Dart 3.13.7 analyze, all unit
  checks (including the unresponsive-host shutdown deadline), runtime install,
  clean-consumer, and package dry-run checks pass locally. The Dart reader now
  serializes large stdin writes around flushes; a paused-host stress check proves
  the 16 MiB command-byte cap rejects excess work and close releases accepted
  requests. The Dart widget-inventory contract creates every shared widget and
  subscribes to its declared events; its six contract checks cover all 123
  shared widget/property/event IDs and declared protocol cases. Python, JavaScript,
  and TypeScript now also have runnable contracts that exercise all 123 shared
  widget/property/event IDs and all 12 non-installed protocol cases; their
  individual contract reports pass. Go and Ruby now pass the same widget and
  case coverage. PHP has runnable protocol and inventory checks for all 123
  widget/property/event IDs and now passes all 12 non-installed cases. PHP
  `App::request()` suspends inside Fibers while the application pumps responses;
  its fixture-driven EOF check observes host EOF with 256 requests pending and
  confirms every suspended request is rejected once. Fresh PHP unit and
  contract runners pass in `.artifacts/bindings/php-unit-after-fiber-fix.json`
  and `.artifacts/bindings/all-contract-windows-x64-after-php-eof-correct-toolchains.php.contract.json`.
  .NET passes its protocol and widget inventory contract, including the clean
  NuGet consumer. Java and Kotlin now pass their 12-case/123-ID contracts; the
  Java unit matrix also passes. The JVM reader now completes pending requests on
  normal EOF, maps transport failures to `connection_closed`, rejects non-finite
  values, caps pending request bytes, reports callback errors, and suppresses
  queued events after unsubscribe. Lua now has runnable live protocol and
  complete 123-ID widget inventory checks; its checked cases cover out-of-order
  responses, nested requests, callback errors, early events, signed i64 boundaries,
  non-finite rejection, a 100 KiB UTF-8 value, 256-pending EOF, command-byte quota,
  stale unsubscribe, and an unrelated-process shutdown sentinel. The fixture-backed
  blocked-handler test now covers both message-count and byte overflow while a
  pending request receives `queue_full`. PHP's Fiber-based pending request test
  and all three PHP contract checks pass. The earlier aggregate
  `.artifacts/bindings/all-contract-windows-x64-after-php-fibers.json` has eleven
  passing SDK rows: Python, JavaScript, TypeScript, .NET, Go, Java, Kotlin, Dart,
  Ruby, PHP, and Lua. The current aggregate Windows x64 contract report,
  .artifacts/bindings/all-contract-windows-x64-after-mock-transport.json, has
  13 passing SDK rows: C, C++, Python, JavaScript, TypeScript, .NET, Go, Java,
  Kotlin, Dart, Ruby, PHP, and Lua. Swift fails to compile because its Windows toolchain
  lacks a usable standard library. Native C/C++ CTest additionally
  verifies out-of-order response correlation, early event delivery, unsubscribe
  suppression, 256 pending requests rejected on session stop, and bounded output
  while a callback blocks. Per-SDK reports:
  .artifacts/bindings/c-contract-after-mock-transport.json and
  .artifacts/bindings/cpp-contract-after-mock-transport.json. The coverage corpus
  records why C has no exception-hook case and native C/C++ have no owned-child
  shutdown-timeout case; the runner validates these exclusions.
  The Windows x64 unit aggregate at
  .artifacts/bindings/all-unit-windows-x64-after-mock-transport.json also passes
  13 of 14 rows; Swift is the only failure for the same unavailable Windows
  standard library.

   Lua's aggregate row passes when the runner selects the installed Lua 5.4
   executable and its verified `luv` module path; the runner passes that same
   executable to the reader child instead of resolving a different Lua from PATH.
   The Swift callback-queue check is wired. The Windows SwiftPM contract row
   still cannot compile because Swift Crypto cannot find `stdlib.h`; WSL has no
   Swift toolchain. A direct-source Windows harness using a local XCTest shim
   compiled the current SDK and passed all fifteen protocol tests, including
   callback count and byte overflow. This is not package-build evidence; see
   `.artifacts/bindings/continuation-2026-11-20-contract-after-1m-burst.json`
   and the per-SDK report.
  Python `App.close()`
  now joins its reader/drainer within the shutdown deadline and closes both host
  output pipes; its contract suite checks the pipe cleanup. Dart installed-package
  scenarios remain open. Ruby now tracks pending request bytes and maps invalid
  non-finite JSON values to `invalid_value`; its full unit and contract suites
  pass. PHP 8.4 protocol checks pass; `App.close()` now uses one shared
  five-second deadline for shutdown and relay cleanup, measured at 5.01 seconds
  against a fixture host delaying exit for ten seconds. Go now removes callbacks before
  waiting for unsubscribe and reports callback panics through `SetErrorHandler`;
  its race-enabled suite passes. Windows x64 Radio `group` state binding is
  implemented and has a Rust regression test; `ScrollView.grow` is now accepted
  by protocol property validation. The focused Rust regressions and full
  workspace tests passed before the latest matrix refresh.
  Java/Kotlin checks run through the checked-in Gradle wrapper.
- The 256-outstanding-request cap is now implemented in all ten high-level
  IPC SDKs. Overflow tests are checked in for Go, Node, Python, Ruby, PHP, and
  Lua. Java and .NET now also have direct rejection-at-cap checks, and both
  suites pass locally; their package-matrix evidence is
  `.artifacts/bindings/java-unit-after-transition-caps.json` and
  `.artifacts/bindings/dotnet-unit-after-transition-caps.json`. Go, Node,
  Python, Java, and .NET overflow checks ran;
  Lua now passes Windows unit and contract suites with the explicit Lua 5.4
  runtime override; its installed-package gate remains open. Swift exercises its 256-request cap with 257 concurrent
  requests and verifies that close releases pending requests. Dart now tests
  the same cap and verifies close releases 256 requests. Its reader-isolate
  overflow tests also verify the 1,024-message/16 MiB output gate. Evidence is
  `.artifacts/bindings/dart-unit-after-reader-isolate.json`. C/C++ expose the native bounded session queue and raw protocol frames rather
  than an SDK request-correlation map. The latest full matrix report predates
  the new Ruby/PHP/Lua/Swift/Dart cap guards and the separate Linux x64 Swift
  and Lua unit runs.
- The six-target package workflow is configured to collect the runtime bundles,
  run the complete pack/check gate, and install/test the native Ruby and PHP
  packages on Linux x64. Ruby/PHP SDK unit checks are also configured for the
  native target jobs except Windows ARM64. None of those workflow jobs has run
  in CI. Ruby and PHP unit runners and clean local package consumers pass, but
  their CI jobs remain unrun. Swift, Dart, and Lua use explicit runtime-bundle
  installers. Swift and Lua Linux x64 unit checks pass locally; Swift's Windows
  toolchain and Lua's full platform/package gates remain open.

No registry package has been published. No full language or platform support
claim follows from this checkpoint.

