# Grafikr Swift SDK

Swift 6 package for Windows, Linux, and macOS. Swift Crypto supplies portable
SHA-256 verification for the runtime installer. `Runtime.install(bundle:cache:)`
validates an unpacked runtime manifest and SHA-256 file digests, installs into
an OS/architecture-specific cache, and returns the absolute host URL. The
`grafikr-runtime <bundle-dir> <cache-dir>` executable performs the same install
for setup scripts. Neither path downloads a runtime implicitly.

The SDK uses Foundation process/JSON support, argument arrays, bounded LF
frames, async request correlation, and explicit close and subscription
lifecycle. Windows/Linux builds require the Swift 6.4 platform toolchains in
the release matrix; those are not available in this workspace.

`App.onError` receives thrown event-handler errors without stopping later
callbacks. `Subscription.close()` removes the local handler before waiting for
the host unsubscribe response, so already queued events are dropped.

The source package includes the Grafikr license and the pinned Swift Crypto and
Swift ASN.1 license texts under `ThirdPartyNotices/`.

`App.createCollectionProvider(node:metadata:)` subscribes before creating the
collection. Await `nextRequest()` for a visible range, answer it with
`provide(_:rows:)`, update metadata with `update(_:)`, and call `close()` to
unsubscribe and wake a waiting reader.

[`Examples/Counter/main.swift`](Examples/Counter/main.swift) shows a minimal
async app that opens a window, creates a button, and reports click events. Add
the file to an executable target that depends on this Swift package.
