# Grafikr C++ interface

`grafikr.hpp` provides move-only RAII wrappers for the application and typed
JSON values. Values support object/array construction, exact signed 64-bit
storage, checked getters, child ownership transfer, and compact serialization.
`Application::run` accepts a setup callback; `Client::request` waits for
responses and throws `BridgeError` with the remote error fields. Callback
subscriptions use move-only RAII `Subscription` values; handlers run on the
setup/event worker and may make synchronous requests. The C trampoline catches
handler exceptions before they cross the ABI. Set `Client::on_error(handler)`
to receive a handler's `std::exception_ptr` and borrowed event value; exceptions
from the error handler are also contained. The CMake build includes
`grafikr_cpp_counter_form`, `grafikr_cpp_dashboard`, and
`grafikr_cpp_multi_window`. Run them from a desktop session; each exits when its
last window closes. The runtime bundle exports `Grafikr::cpp` and propagates
the C++17 requirement. The collection provider scenario passed on the Windows
x64 native runtime; the other example input flows remain unverified.
The runtime bundle places the three executables under
`share/grafikr/examples/`.

`CollectionProvider::create(client, node, metadata)` subscribes to
`data.request` before creating the collection. It keeps one bounded range;
`next_request(timeout_ms)` waits for a request, `provide(request, rows)` supplies
rows, and `update(metadata)` changes metadata. The provider is move-only RAII;
call the blocking `next_request` from a worker thread, not the UI thread.
