# Grafikr Lua SDK

Lua 5.4 module uses luv for bidirectional pipes and child lifecycle, dkjson for
protocol JSON, and luaossl is reserved for explicit runtime-bundle checksum
verification. It does not use `io.popen` or a shell. API operations are
callback-based, and callbacks can submit more requests asynchronously. A
bundled Lua reader process continues reading host frames while an application
callback runs; its unacknowledged queue is limited to 1,024 messages and 16 MiB,
then the session fails with `queue_full`. Application callbacks still run on
the caller's libuv loop. `app:on_error(fn)`
receives event-handler exceptions together with the event; exceptions from the
error hook are isolated. Run the included example with an installed absolute
host path.

The binding-check runner can select a Lua 5.4 executable and its LuaRocks tree
with `GRAFIKR_LUA54` and `GRAFIKR_LUA54_MODULES`. It derives `LUA_PATH`,
`LUA_CPATH`, and the native library search path from the module tree, so test
reader processes use the same Lua 5.4 modules as the parent process.

Use `grafikr.runtime.install(bundle_dir, cache_dir)` to validate an unpacked
runtime manifest and SHA-256 hashes before copying the bundle into the local
cache. The installer rejects development fixtures and unsafe paths, and keeps
the previous cache intact if validation or staging fails. Pass the returned
absolute host path to `grafikr.start(host, on_ready)`.

`app:create_collection_provider(node, metadata, callback)` registers the
`data.request` handler before creating collection metadata. The provider's
`next_request(timeout_ms, callback)` waits without blocking `uv.run`, while
`provide` and `update` answer ranges and refresh metadata. `close(callback)`
removes the subscription and completes a waiting read with no request.
