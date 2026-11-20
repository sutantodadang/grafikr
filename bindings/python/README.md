# Grafikr Python SDK

Python 3.12+ client. The platform wheel includes the matching verified host
runtime, so `App()` selects it from the installed package. You can pass an
unpacked verified runtime bundle directory to `App(bundle_path)` to use a
separately installed build. Importing this package does not download or build
binaries.

Build a local wheel from a verified runtime bundle with:

```powershell
rtk uv run --project bindings/python --group dev --with-requirements scripts/requirements-bindings.txt python -m scripts.package_python --bundle target/runtime/current --out target/packages/python
```

The wheel tag and embedded runtime target come from the bundle manifest. Each
target bundle must be built natively; a cross-built bundle is not runtime
verification. The local wheel is not a published package.

```python
from grafikr import App, Node, State, Window

with App() as app:
    app.set_error_handler(lambda error, event: print(event["event"], error))
    window = Window.create(app, "Hello", 640, 400)
    label = Node.create(app, window, "Text", text="Ready")
    label.attach(window)
    count = State.create(app, 0)
    count.set(count.get() + 1)
```

Callbacks run serially on the SDK event thread, while the reader resolves request
responses independently. A callback may call `request`; callback exceptions
are passed to the optional error handler, and later callbacks continue. The
`request_async` variant returns a `concurrent.futures.Future` for overlap and
nested callback requests. The callback queue is bounded to 1,024 messages and
16 MiB; overflow fails the
session and terminates its host. Non-finite floats are rejected before a
request is registered or written. Errors are `BridgeError` values with `code`
and optional `field`. `State` maps Python `bool`, `int`, finite `float`, and
`str` values to protocol scalars; integer state is limited to signed 64-bit.
Context manager exit requests shutdown, waits at most five seconds, then
terminates only this child. The current package provides core typed wrappers
and generic `request` access; see the protocol guide for operation coverage and
runtime limits. Runtime lookup verifies target, minimum OS, file size and
SHA-256 before starting the host.

For `VirtualList` and `DataGrid`, `App.create_collection_provider(node, metadata)`
subscribes before creating the collection. A worker can block on
`provider.next_request()`, answer each revision-tagged range with
`provider.provide(request, rows)`, call `provider.update(metadata)` when the
dataset changes, and use `provider.close()` to wake the worker and unsubscribe.
