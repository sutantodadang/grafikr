import base64
import json
import os
import threading
import subprocess
import sys
import time
import unittest
from concurrent.futures import Future
from pathlib import Path
from unittest.mock import patch

from grafikr.client import (
    App, BridgeError, CollectionMetadata, CollectionProvider, MAX_CALLBACK_BYTES,
    MAX_CALLBACK_EVENTS, MAX_OUTSTANDING_REQUESTS, Node, State, Subscription, Window,
)


class FakeProcess:
    def __init__(self):
        self.stdin = self
        self.stdout = self
        self.stderr = self.EmptyPipe()
        self.lines = []
        self.written = []
        self.terminate_calls = 0
        self._event = threading.Event()

    class EmptyPipe:
        def readline(self): return b""

    def write(self, data):
        self.written.append(data)
        request = json.loads(data)
        result = {"protocol": {"major": 1}} if request["op"] == "hello" else {}
        self.lines.append(json.dumps({"kind": "response", "id": request["id"], "ok": True, "result": result}).encode() + b"\n")
        self._event.set()

    def flush(self): pass

    def readline(self, _limit=None):
        if not self.lines:
            self._event.wait(1)
        self._event.clear()
        return self.lines.pop(0) if self.lines else b""

    def wait(self, timeout=None): return 0
    def close(self): self._event.set()
    def terminate(self): self.terminate_calls += 1
    def kill(self): pass


class TimeoutProcess:
    class Pipe:
        def close(self): pass

    def __init__(self):
        self.stdin = self.Pipe()
        self.running = True
        self.wait_timeouts = []

    def wait(self, timeout=None):
        self.wait_timeouts.append(timeout)
        if self.running:
            raise subprocess.TimeoutExpired("fixture-host", timeout)
        return 0

    def terminate(self): pass
    def kill(self): self.running = False


class TrackedPipe:
    def __init__(self, reader):
        self.reader = reader
        self.closed = False

    def readline(self, limit=None):
        return self.reader(limit) if limit is not None else self.reader()

    def close(self):
        self.closed = True


class ClientTests(unittest.TestCase):
    def test_close_closes_host_output_pipes(self):
        process = FakeProcess()
        stdout = TrackedPipe(process.readline)
        stderr = TrackedPipe(process.stderr.readline)
        process.stdout = stdout
        process.stderr = stderr
        app = object.__new__(App)
        app._init_callback_dispatch()
        app._closed = False
        app._lock = threading.Lock()
        app._failure = None
        app._pending = {}
        app._process = process
        app.request = lambda *_args, **_kwargs: {}

        app.close()

        self.assertTrue(stdout.closed)
        self.assertTrue(stderr.closed)

    @unittest.skipUnless(
        os.environ.get("GRAFIKR_FIXTURE_WIDGET_INVENTORY"),
        "requires isolated widget inventory fixture mode",
    )
    def test_creates_every_shared_widget_and_receives_declared_events(self):
        host = Path(os.environ["GRAFIKR_FIXTURE_HOST"])
        inventory = json.loads(
            Path("bindings/protocol/v1/widgets.json").read_text(encoding="utf-8")
        )["widgets"]
        app = None
        with patch("grafikr.client.runtime_host_path", return_value=host):
            app = App()
        try:
            window = Window.create(app, "widget inventory", 640, 480)
            state = State.create(app, 1)
            upload = app.request(
                "image.begin", {"width": 1, "height": 1, "byte_length": 4}
            )["upload"]
            app.request(
                "image.chunk",
                {"upload": upload, "offset": 0, "data": base64.b64encode(bytes([0, 0, 0, 255])).decode()},
            )
            image = app.request("image.finalize", {"upload": upload})["image"]

            for widget, spec in inventory.items():
                properties = {
                    name: _sample_widget_value(widget, name, kind, state.handle, image)
                    for name, kind in spec["properties"].items()
                }
                node = Node.create(app, window, widget, **properties)
                node.attach(window)
                for event in spec.get("events", []):
                    received = threading.Event()
                    message = []

                    def on_event(value):
                        message.append(value)
                        received.set()

                    subscription = app.on(node.handle, event, on_event)
                    self.assertTrue(received.wait(2), f"{widget}@{event}")
                    self.assertEqual(message[0]["source"], node.handle)
                    self.assertEqual(message[0]["event"], event)
                    subscription.close()
                node.destroy()

            app.request("image.destroy", {"image": image})
            state.close()
            window.close()
        finally:
            app.close()

    def test_collection_provider_subscribes_first_and_supplies_revisioned_rows(self):
        app = object.__new__(App)
        calls = []
        callbacks = {}

        class StubSubscription:
            def close(self):
                calls.append(("event.unsubscribe", {}))

        def subscribe(node, event, callback):
            calls.append(("event.subscribe", {"node": node, "event": event}))
            callbacks[event] = callback
            return StubSubscription()

        def request(op, args=None, *, timeout=30):
            args = args or {}
            calls.append((op, args))
            if op == "collection.create":
                callbacks["data.request"]({
                    "kind": "event", "event": "data.request", "source": "7",
                    "payload": {"revision": "3", "request": "9", "start": 4, "count": 2},
                })
            return {}

        app.on = subscribe
        app.request = request
        node = Node(app, "7")
        provider = CollectionProvider.create(
            node, CollectionMetadata(row_count=20, row_height=24, columns=("label",))
        )

        self.assertEqual([call[0] for call in calls[:2]], ["event.subscribe", "collection.create"])
        data_request = provider.next_request(timeout=1)
        self.assertEqual((data_request.revision, data_request.request, data_request.start, data_request.count), ("3", "9", 4, 2))
        provider.provide(data_request, [["four"], ["five"]])
        provider.update(CollectionMetadata(row_count=2, row_height=30, columns=("label",)))
        provider.close()
        provider.close()
        self.assertEqual(calls[-3][0], "data.provide")
        self.assertEqual(calls[-2][0], "collection.update")
        self.assertEqual(calls[-1][0], "event.unsubscribe")
        self.assertIsNone(provider.next_request(timeout=0.1))

    def test_collection_provider_serves_visible_rows_over_fixture_ipc(self):
        host_name = "fixture-host.exe" if sys.platform == "win32" else "fixture-host"
        host = (Path("target/debug") / host_name).resolve()
        if not host.is_file():
            self.skipTest("fixture host has not been built")
        app = None
        provider = None
        try:
            with patch("grafikr.client.runtime_host_path", return_value=host):
                app = App()
            window = Window.create(app, "rows", 320, 240)
            node = Node.create(app, window, "VirtualList", row_count=100, row_height=24, height=120)
            node.attach(window)
            provider = app.create_collection_provider(
                node, CollectionMetadata(row_count=100, row_height=24, columns=["label"])
            )
            request = provider.next_request(timeout=2)
            self.assertIsNotNone(request)
            self.assertGreater(request.count, 0)
            rows = [[f"row-{request.start + index}"] for index in range(request.count)]
            provider.provide(request, rows)
            provider.update(CollectionMetadata(row_count=2, row_height=30, columns=["label"]))
        finally:
            if provider is not None:
                provider.close()
            if app is not None:
                app.close()

    def test_outstanding_request_count_is_capped(self):
        app = object.__new__(App)
        app._ids = iter(range(MAX_OUTSTANDING_REQUESTS + 2))
        app._pending = {str(index): Future() for index in range(MAX_OUTSTANDING_REQUESTS)}
        app._lock = threading.Lock()
        app._closed = False
        app._failure = None
        app._process = FakeProcess()

        with self.assertRaises(BridgeError) as raised:
            app.request_async("state.get", {"state": "1"})

        self.assertEqual(raised.exception.code, "queue_full")
        self.assertEqual(app._process.written, [])

    def test_request_is_correlated(self):
        app = object.__new__(App)
        process = FakeProcess()
        app._process = process
        app._ids = iter(range(1, 10))
        app._pending = {}
        app._lock = threading.Lock()
        app._init_callback_dispatch()
        app._closed = False
        app._failure = None
        reader = threading.Thread(target=app._read, daemon=True)
        reader.start()
        self.assertEqual(app.request("state.get", {"state": "2"}), {})
        process.close()
        reader.join(timeout=1)

    def test_async_request_returns_correlated_future(self):
        app = object.__new__(App)
        app._process = FakeProcess()
        app._ids = iter(("1",))
        app._pending = {}
        app._lock = threading.Lock()
        app._init_callback_dispatch()
        app._closed = False
        app._failure = None

        future = app.request_async("state.get", {"state": "2"})
        self.assertIsInstance(future, Future)
        self.assertFalse(future.done())
        app._read()
        self.assertEqual(future.result(), {})

    def test_async_requests_correlate_out_of_order_responses(self):
        app = object.__new__(App)
        process = FakeProcess()
        app._process = process
        app._ids = iter(("first", "second"))
        app._pending = {}
        app._lock = threading.Lock()
        app._init_callback_dispatch()
        app._closed = False
        app._failure = None

        def response_for(data):
            request = json.loads(data)
            process.written.append(data)
            process.lines.append(
                json.dumps({"kind": "response", "id": request["id"], "ok": True, "result": {"id": request["id"]}}).encode()
                + b"\n"
            )
            process._event.set()

        process.write = response_for

        first = app.request_async("state.get", {"state": "1"})
        second = app.request_async("state.get", {"state": "2"})
        process.lines.reverse()
        app._read()
        self.assertEqual(first.result(), {"id": "first"})
        self.assertEqual(second.result(), {"id": "second"})

    def test_reader_preserves_split_utf8_and_escaped_newlines_in_large_frame(self):
        text = "line one\nline two 雪 😀" + ("x" * 102_400)
        frame = json.dumps(
            {"kind": "response", "id": "1", "ok": True, "result": {"text": text}},
            ensure_ascii=False,
        ).encode("utf-8") + b"\n"
        read_fd, write_fd = os.pipe()
        reader = os.fdopen(read_fd, "rb")
        writer = os.fdopen(write_fd, "wb")
        process = FakeProcess()
        process.stdout = reader
        app = object.__new__(App)
        pending = Future()
        app._pending = {"1": pending}
        app._lock = threading.Lock()
        app._init_callback_dispatch()
        app._failure = None
        app._process = process

        def write_fragments():
            try:
                for offset in range(0, len(frame), 127):
                    writer.write(frame[offset:offset + 127])
                    writer.flush()
            finally:
                writer.close()

        thread = threading.Thread(target=write_fragments)
        thread.start()
        try:
            app._read()
            thread.join(timeout=2)
            self.assertFalse(thread.is_alive())
            self.assertEqual(pending.result()["text"], text)
        finally:
            reader.close()

    def test_eof_rejects_all_pending_requests_once(self):
        process = FakeProcess()
        process.stdout = process.stderr
        app = object.__new__(App)
        pending = {str(index): Future() for index in range(256)}
        app._pending = pending.copy()
        app._lock = threading.Lock()
        app._init_callback_dispatch()
        app._failure = None
        app._process = process

        app._read()

        self.assertEqual(app._pending, {})
        for future in pending.values():
            self.assertTrue(future.done())
            with self.assertRaises(BridgeError) as raised:
                future.result()
            self.assertEqual(raised.exception.code, "connection_closed")

    def test_nested_request_from_callback_completes_on_separate_reader(self):
        app = object.__new__(App)
        app._process = FakeProcess()
        app._ids = iter(range(1, 10))
        app._pending = {}
        app._lock = threading.Lock()
        app._init_callback_dispatch()
        app._closed = False
        app._failure = None
        reader = threading.Thread(target=app._read, daemon=True)
        callbacks = threading.Thread(target=app._dispatch_events, daemon=True)
        reader.start()
        callbacks.start()
        finished = threading.Event()
        results = []

        def handler(_event):
            results.append(app.request("state.set", {"state": "2", "value": {"type": "i64", "value": "1"}}, timeout=1))
            finished.set()

        app._handlers[("7", "click")] = [handler]
        app._queue_event({"kind": "event", "event": "click", "source": "7", "payload": {}}, 128)
        self.assertTrue(finished.wait(1))
        self.assertEqual(results, [{}])
        app._stop_callback_dispatch()
        app._process.close()
        reader.join(timeout=1)
        callbacks.join(timeout=1)

    def test_non_finite_json_is_rejected_before_queueing_pending_request(self):
        app = object.__new__(App)
        app._ids = iter(("1",))
        app._pending = {}
        app._lock = threading.Lock()
        app._closed = False
        app._failure = None
        app._process = FakeProcess()

        with self.assertRaises(BridgeError) as raised:
            app.request("state.create", {"value": {"type": "f64", "value": float("nan")}}, timeout=0)

        self.assertEqual(raised.exception.code, "invalid_value")
        self.assertEqual(app._pending, {})
        self.assertEqual(app._process.written, [])

    def test_typed_state_preserves_i64_boundaries_and_rejects_overflow(self):
        class StubApp:
            def __init__(self):
                self.requests = []
                self.get_value = {"type": "i64", "value": "-9223372036854775808"}

            def request(self, op, args):
                self.requests.append((op, args))
                return {"state": "s-1"} if op == "state.create" else self.get_value

        app = StubApp()
        state = State.create(app, -(2**63))
        self.assertEqual(app.requests[-1], ("state.create", {"value": {"type": "i64", "value": "-9223372036854775808"}}))
        self.assertEqual(state.get(), -(2**63))
        state.set(2**63 - 1)
        self.assertEqual(app.requests[-1][1]["value"], {"type": "i64", "value": "9223372036854775807"})
        request_count = len(app.requests)
        with self.assertRaises(BridgeError) as raised:
            state.set(2**63)
        self.assertEqual(raised.exception.code, "invalid_value")
        self.assertEqual(len(app.requests), request_count)
        with self.assertRaises(BridgeError):
            state.set(float("inf"))
        self.assertEqual(len(app.requests), request_count)

    def test_host_launch_uses_argument_list_without_a_console_window(self):
        process = FakeProcess()
        bundle = Path("runtime bundle ünicode")
        host = bundle / "bin" / "grafikr-host.exe"
        with patch("grafikr.client.runtime_host_path", return_value=host.resolve()) as locate, patch(
            "grafikr.client.subprocess.Popen", return_value=process
        ) as start:
            app = App(bundle, sdk_version="test")
            app.close()

        args, kwargs = start.call_args
        locate.assert_called_once_with(bundle)
        self.assertEqual(args[0], [str(host.resolve())])
        self.assertFalse(kwargs.get("shell", False))
        self.assertEqual(kwargs["creationflags"], getattr(subprocess, "CREATE_NO_WINDOW", 0))

    def test_app_without_host_uses_the_verified_wheel_runtime(self):
        process = FakeProcess()
        host = Path("runtime bundle ünicode/grafikr-host.exe").resolve()
        with patch("grafikr.client.runtime_host_path", return_value=host), patch(
            "grafikr.client.subprocess.Popen", return_value=process
        ) as start:
            app = App()
            app.close()
        self.assertEqual(start.call_args.args[0], [str(host)])

    def test_app_rejects_unverified_executable_paths(self):
        with patch("grafikr.client.subprocess.Popen") as start:
            with self.assertRaises(BridgeError) as raised:
                App(Path("grafikr-host.exe"))
        self.assertEqual(raised.exception.code, "runtime_invalid")
        start.assert_not_called()

    def test_handler_exceptions_reach_error_hook_and_do_not_stop_later_handlers(self):
        app = object.__new__(App)
        app._lock = threading.Lock()
        app._init_callback_dispatch()
        reported = []
        delivered = threading.Event()
        app.set_error_handler(lambda error, event: reported.append((error, event)))

        def fail(_event):
            raise ValueError("callback failed")

        app._handlers.setdefault(("7", "click"), []).extend((fail, lambda _event: delivered.set()))
        event = {"kind": "event", "event": "click", "source": "7", "payload": {}}
        app._queue_event(event, 128)
        app._stop_callback_dispatch()
        app._dispatch_events()

        self.assertTrue(delivered.is_set())
        self.assertEqual(len(reported), 1)
        self.assertIsInstance(reported[0][0], ValueError)
        self.assertEqual(reported[0][1], event)

    def test_blocked_callback_queue_fails_at_count_quota(self):
        app = object.__new__(App)
        app._lock = threading.Lock()
        app._init_callback_dispatch()
        started = threading.Event()
        release = threading.Event()

        def blocked(_event):
            started.set()
            release.wait(2)

        app._handlers.setdefault(("7", "click"), []).append(blocked)
        event = {"kind": "event", "event": "click", "source": "7", "payload": {}}
        worker = threading.Thread(target=app._dispatch_events, daemon=True)
        worker.start()
        app._queue_event(event, 1024)
        self.assertTrue(started.wait(1))
        for _ in range(MAX_CALLBACK_EVENTS):
            app._queue_event(event, 1024)
        with self.assertRaises(BridgeError) as raised:
            app._queue_event(event, 1024)
        self.assertEqual(raised.exception.code, "queue_full")
        self.assertEqual(app._callback_count, MAX_CALLBACK_EVENTS)
        self.assertLessEqual(app._callback_bytes, MAX_CALLBACK_BYTES)
        release.set()
        app._stop_callback_dispatch()
        worker.join(timeout=2)
        self.assertFalse(worker.is_alive())

    def test_reader_terminates_host_when_callback_backpressure_is_exhausted(self):
        class Stream:
            def __init__(self, lines):
                self.lines = list(lines)

            def readline(self, _limit=None):
                return self.lines.pop(0) if self.lines else b""

        process = FakeProcess()
        event = {"kind": "event", "event": "click", "source": "7", "payload": {}}
        line = json.dumps(event).encode() + b"\n"
        process.stdout = Stream([line] * (MAX_CALLBACK_EVENTS + 1))
        app = object.__new__(App)
        app._init_callback_dispatch()
        app._pending = {}
        app._lock = threading.Lock()
        app._failure = None
        app._process = process

        app._read()

        self.assertEqual(app._failure.code, "queue_full")
        self.assertEqual(process.terminate_calls, 1)

    def test_callback_queue_fails_at_byte_quota(self):
        app = object.__new__(App)
        app._init_callback_dispatch()
        event = {"kind": "event", "event": "click", "source": "7", "payload": {}}
        max_frame_bytes = 8 * 1024 * 1024
        app._queue_event(event, max_frame_bytes)
        app._queue_event(event, max_frame_bytes)
        with self.assertRaises(BridgeError) as raised:
            app._queue_event(event, 1)
        self.assertEqual(raised.exception.code, "queue_full")
        self.assertEqual(app._callback_bytes, MAX_CALLBACK_BYTES)
        app._stop_callback_dispatch()

    def test_unsubscribe_suppresses_already_queued_event(self):
        app = object.__new__(App)
        app._lock = threading.Lock()
        app._init_callback_dispatch()
        app.request = lambda *_args, **_kwargs: {}
        called = []
        key = ("7", "click")
        callback = lambda _event: called.append(True)
        app._handlers[key] = [callback]
        event = {"kind": "event", "event": "click", "source": "7", "payload": {}}
        app._queue_event(event, 128)
        Subscription(app, "sub-1", key, callback).close()
        app._stop_callback_dispatch()
        app._dispatch_events()
        self.assertEqual(called, [])

    def test_subscription_handler_is_registered_before_subscribe_request(self):
        app = object.__new__(App)
        app._lock = threading.Lock()
        app._init_callback_dispatch()
        event = {"kind": "event", "event": "click", "source": "7", "payload": {}}
        called = []

        def request(_op, _args):
            app._queue_event(event, 128)
            app._stop_callback_dispatch()
            app._dispatch_events()
            return {"subscription": "sub-1"}

        app.request = request

        app.on("7", "click", lambda _event: called.append(True))

        self.assertEqual(called, [True])

    def test_close_never_waits_longer_than_five_seconds(self):
        app = object.__new__(App)
        app._init_callback_dispatch()
        app._closed = False
        app._lock = threading.Lock()
        app._failure = None
        sentinel = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(30)"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        process = TimeoutProcess()
        app._process = process
        app._pending = {}
        app.request = lambda *_args, **_kwargs: {}
        started = time.monotonic()
        try:
            app.close(timeout=30)
            self.assertLess(time.monotonic() - started, 5)
            self.assertTrue(process.wait_timeouts)
            self.assertTrue(all(timeout <= 4.9 for timeout in process.wait_timeouts))
            self.assertFalse(process.running)
            self.assertIsNone(sentinel.poll())
        finally:
            if sentinel.poll() is None:
                sentinel.terminate()
            sentinel.wait(timeout=1)

    def test_close_from_callback_and_repeated_close_are_safe(self):
        app = object.__new__(App)
        app._init_callback_dispatch()
        app._closed = False
        app._lock = threading.Lock()
        app._failure = None
        app._pending = {}
        app._process = FakeProcess()
        app.request = lambda *_args, **_kwargs: {}
        finished = threading.Event()

        def close_from_callback(_event):
            app.close()
            finished.set()

        app._handlers[("7", "click")] = [close_from_callback]
        worker = threading.Thread(target=app._dispatch_events, daemon=True)
        worker.start()
        app._queue_event({"kind": "event", "event": "click", "source": "7", "payload": {}}, 128)
        self.assertTrue(finished.wait(1))
        worker.join(timeout=1)
        self.assertFalse(worker.is_alive())
        self.assertTrue(app._closed)
        app.close()


def _sample_widget_value(widget, name, kind, state, image):
    if widget == "Radio" and name == "group":
        return state
    if widget == "TreeView" and name == "selected":
        return "root"
    if widget == "Slider":
        return {"min": 0.0, "max": 10.0, "width": 120.0}.get(name, 1.0)
    if widget == "Spinner" and name == "size":
        return 16.0
    if name == "row_count":
        return 1
    if name == "row_height":
        return 24.0
    return {
        "number": 1.0,
        "f32": 1.0,
        "u16": 1,
        "u64": 0,
        "string": "sample",
        "color": "#336699",
        "transform": {"x": 0.0, "y": 0.0, "scale": 1.0},
        "boolean": False,
        "primary|secondary|icon": "primary",
        "state": state,
        "GridColumn[]": [{"title": "Column", "width": 120.0}],
        "string[]": ["One", "Two"],
        "TreeNode[]": [{"key": "root", "label": "Root", "children": []}],
        "CanvasCommand[]": [{
            "kind": "rect",
            "rect": {"x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0},
            "color": "#336699",
            "radius": 0.0,
        }],
        "ImageHandle": image,
    }[kind]


if __name__ == "__main__":
    unittest.main()
