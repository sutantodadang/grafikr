# Grafikr Ruby SDK

Ruby 3.4 platform gem using `Open3.popen3` with an argument array and no shell.
`App.start` resolves and verifies the host bundled in the matching platform
gem; `App.new(host)` accepts an explicit absolute host path. Reads are bounded,
request IDs are correlated, callbacks run on a separate thread, and
`App#close` shuts down only the child process it owns with a five-second
deadline. The callback queue is capped at 1,024 events and 16 MiB; overflow
closes the session. Host stderr is drained in fixed-size chunks. Register
`App#on_error` to receive callback exceptions.
Local handlers are removed before unsubscribe is sent, suppressing already
queued events for that subscription.

`App#create_collection_provider(node, row_count:, row_height:, columns:)`
subscribes to `data.request` before creating collection metadata. Read visible
ranges with `next_request(timeout:)`, answer them with `provide(request, rows)`,
and publish metadata changes with `update(...)`. `close` unsubscribes and wakes
a blocked `next_request` with `nil`.
