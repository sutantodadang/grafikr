# Grafikr PHP CLI SDK

PHP 8.4+ CLI package. `Client::start()` launches an absolute host executable with
`proc_open`'s argument-array form and `bypass_shell`; when no host path is
passed, it resolves and verifies the explicitly installed Composer runtime
package for the current OS and architecture. Install the matching
`grafikr/runtime-<platform>` artifact from the release package directory; the
SDK never downloads a runtime. The client bounds LF frames, correlates
requests, relays host output through its bundled PHP reader process (including
on Windows, where `stream_select` does not support `proc_open` pipes), queues
events away from the reader, and forwards callback exceptions to `App::onError`.
It closes only its own child processes with a five-second deadline. This is for
desktop CLI applications, not PHP web hosting.

`App::request()` remains synchronous in ordinary PHP code. When called from a
PHP `Fiber`, it writes the request and suspends that fiber; the application
must call `pumpEvents()` to receive responses and resume waiting fibers. The
client enforces the same 256 outstanding-request and 16 MiB pending-byte caps
for both request styles. EOF and close reject each suspended request once.

`App::createCollectionProvider($node, $metadata)` subscribes before publishing
collection metadata. `nextRequest($timeoutSeconds)` pumps host frames while it
waits, `provide($request, $rows)` supplies requested rows, and `update($metadata)`
changes collection metadata. Queue overflow is reported by the next read.
