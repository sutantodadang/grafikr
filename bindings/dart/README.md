# Grafikr Dart VM SDK

Dart 3.10+ CLI desktop package. It starts only an explicitly supplied absolute
host path with `Process.start(runInShell: false)` in a reader isolate, negotiates
protocol 1, bounds frames, correlates async requests, and isolates event
callback exceptions. Reader output and command SendPort queues use count and
byte credits, capped at 1,024 messages and 16 MiB. A slow synchronous callback
does not stop the reader from draining host output. `close()` releases pending
requests and kills a child that does not exit within five seconds. This package
targets Dart VM desktop applications, not Flutter widgets.

`App.createCollectionProvider(node, metadata)` subscribes before sending
collection metadata. Await `nextRequest(timeout:)` for visible ranges, answer
with `provide(request, rows)`, and send metadata changes with `update(...)`.
`close()` unsubscribes and resolves a waiting read with `null`.

Pass `onError:` to `App.start` to receive event-handler exceptions. Unsubscribing removes the local callback before waiting for the host acknowledgement, so already queued events are ignored.
Event handlers may return a `Future`; the app awaits them in order while the reader isolate continues draining bounded output.

Install an unpacked runtime bundle explicitly with `dart run bin/grafikr_runtime.dart <bundle> <cache>`; the command verifies manifest paths, file sizes, and SHA-256 values and prints the cached host path. It does not download runtimes.
