# Grafikr C interface

`include/grafikr.h` is the versioned C ABI. `grafikr_client.h` provides an
opaque-session wrapper and an owned JSON value tree with exact signed 64-bit
numbers. Object/array insertion transfers ownership only on success; value
getters return borrowed children. `gf_value_get_i64` preserves the full range
for values built with `gf_value_i64`; decoded JSON numbers are accepted only in
the exactly representable integer range. The protocol uses decimal strings for
state i64 values and handles. `gf_client_run` performs the protocol hello,
runs setup on a worker, and runs the native UI on the creating thread;
`gf_request` synchronously returns a response result or structured bridge error.
`gf_on` registers before sending `event.subscribe`; callbacks run on the setup/
event worker and may call `gf_request`. `gf_off` unsubscribes and suppresses later
queued callbacks. Subscription handles retain the client control block, so they
may be released after `gf_client_destroy`.

`gf_collection_provider_create` registers `data.request` before sending
`collection.create`. Metadata requires a finite positive row height and 1–256
non-null column names. `gf_collection_provider_next` waits for one range request
and returns `GRAFIKR_TIMEOUT` when its deadline expires; only one thread may
wait on a provider at a time. The range strings are copied into fixed 65-byte
buffers. Pass an array of rows to `gf_collection_provider_provide`; update
metadata with `gf_collection_provider_update`. Close every provider before
destroying its client, then release it with `gf_collection_provider_destroy`.
The provider has a one-request queue; a second undelivered request reports
`GRAFIKR_QUEUE_FULL` on the next call to `next`. The
`grafikr_collection_provider_scenario` CMake target exercises the visible-range
callback and row reply; it passed on the Windows x64 native runtime.
Raw frame submission and event reads remain available in low-level mode. Build
with CMake by supplying the absolute `GRAFIKR_FFI_LIBRARY` path and, on Windows,
its import library.

The CMake build includes `grafikr_counter_form`, `grafikr_dashboard`, and
`grafikr_multi_window`. Run them from a desktop session; each exits when its
last window closes. The runtime bundle exports the imported `Grafikr::client`
target for C consumers. Counter/form and multi-window input scenarios remain
unverified on a real desktop.
The runtime bundle places the three executables under
`share/grafikr/examples/`.
