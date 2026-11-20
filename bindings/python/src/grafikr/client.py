from __future__ import annotations

import itertools
import json
import math
import queue
import subprocess
import threading
import time
from concurrent.futures import Future, TimeoutError
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Generic, TypeVar

from .runtime import runtime_host_path

MAX_FRAME = 8 * 1024 * 1024
MAX_OUTSTANDING_REQUESTS = 256
MAX_CALLBACK_EVENTS = 1024
MAX_CALLBACK_BYTES = 16 * 1024 * 1024
I64_MIN = -(2**63)
I64_MAX = 2**63 - 1
T = TypeVar("T", int, float, bool, str)


class BridgeError(RuntimeError):
    def __init__(self, code: str, message: str, field: str | None = None):
        super().__init__(message)
        self.code, self.field = code, field


class App:
    """Owns one Grafikr host process and its request/event streams."""

    def __init__(self, bundle: Path | None = None, *, sdk_version: str = "1.1.0"):
        try:
            host_path = runtime_host_path(bundle)
        except ValueError as error:
            raise BridgeError("runtime_invalid", str(error)) from error
        self._process = subprocess.Popen(
            [str(host_path)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, bufsize=0, shell=False,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        self._ids = itertools.count(1)
        self._pending: dict[str, Future[Any]] = {}
        self._lock = threading.Lock()
        self._init_callback_dispatch()
        self._closed = False
        self._failure: BaseException | None = None
        self._stderr = threading.Thread(target=self._drain_stderr, daemon=True)
        self._reader = threading.Thread(target=self._read, daemon=True)
        self._callbacks = threading.Thread(target=self._dispatch_events, daemon=True)
        self._stderr.start(); self._reader.start(); self._callbacks.start()
        result = self.request("hello", {"protocol_major": 1, "protocol_minor": 0, "sdk_version": sdk_version})
        if result.get("protocol", {}).get("major") != 1:
            self.close()
            raise BridgeError("version_mismatch", "host does not support protocol 1")

    def _init_callback_dispatch(self) -> None:
        self._handlers: dict[tuple[str, str], list[Callable[[dict[str, Any]], None]]] = {}
        self._error_handler: Callable[[Exception, dict[str, Any]], None] | None = None
        self._callback_queue: queue.Queue[tuple[dict[str, Any], int] | None] = queue.Queue(
            maxsize=MAX_CALLBACK_EVENTS + 1
        )
        self._callback_lock = threading.Lock()
        self._callback_count = 0
        self._callback_bytes = 0
        self._callback_stopped = False

    def _start_request(self, op: str, args: dict[str, Any] | None = None) -> tuple[str, Future[Any]]:
        with self._lock:
            if self._closed or self._failure:
                raise BridgeError("connection_closed", str(self._failure or "application is closed"))
            if len(self._pending) >= MAX_OUTSTANDING_REQUESTS:
                raise BridgeError("queue_full", "outstanding request count quota reached")
            request_id = str(next(self._ids))
            try:
                payload = json.dumps(
                    {"id": request_id, "op": op, "args": args if args is not None else {}},
                    ensure_ascii=False,
                    allow_nan=False,
                    separators=(",", ":"),
                ).encode() + b"\n"
            except (TypeError, ValueError) as error:
                raise BridgeError("invalid_value", "request contains a value that is not valid JSON") from error
            if len(payload) > MAX_FRAME:
                raise BridgeError("frame_too_large", "request exceeds 8 MiB")
            pending: Future[Any] = Future()
            self._pending[request_id] = pending
            try:
                self._process.stdin.write(payload)
                self._process.stdin.flush()
            except (BrokenPipeError, OSError) as error:
                self._pending.pop(request_id, None)
                raise BridgeError("connection_closed", "Grafikr host stopped") from error
        return request_id, pending

    def request_async(self, op: str, args: dict[str, Any] | None = None) -> Future[Any]:
        return self._start_request(op, args)[1]

    def request(self, op: str, args: dict[str, Any] | None = None, *, timeout: float = 30) -> Any:
        request_id, pending = self._start_request(op, args)
        try:
            return pending.result(timeout)
        except TimeoutError as error:
            with self._lock:
                self._pending.pop(request_id, None)
            raise BridgeError("timeout", f"request {request_id} timed out") from error

    def on(self, node: str, event: str, callback: Callable[[dict[str, Any]], None]) -> Subscription:
        key = (str(node), event)
        with self._lock:
            self._handlers.setdefault(key, []).append(callback)
        try:
            result = self.request("event.subscribe", {"node": node, "event": event})
        except BaseException:
            with self._lock:
                callbacks = self._handlers.get(key, [])
                if callback in callbacks:
                    callbacks.remove(callback)
            raise
        return Subscription(self, str(result["subscription"]), key, callback)

    def create_collection_provider(self, node: Node, metadata: CollectionMetadata) -> CollectionProvider:
        return CollectionProvider.create(node, metadata)

    def set_error_handler(self, callback: Callable[[Exception, dict[str, Any]], None] | None) -> None:
        with self._lock:
            self._error_handler = callback

    def close(self, timeout: float = 5) -> None:
        with self._lock:
            if self._closed:
                return
        deadline = time.monotonic() + min(max(timeout, 0.0), 4.9)
        remaining = lambda: max(0.0, min(4.9, deadline - time.monotonic()))
        try:
            self.request("shutdown", timeout=remaining())
        except BridgeError:
            pass
        with self._lock:
            self._closed = True
        try:
            self._process.stdin.close()
        except OSError:
            pass
        try:
            self._process.wait(timeout=remaining())
        except subprocess.TimeoutExpired:
            self._process.terminate()
            try:
                self._process.wait(timeout=remaining())
            except subprocess.TimeoutExpired:
                self._process.kill()
                try:
                    self._process.wait(timeout=0)
                except subprocess.TimeoutExpired:
                    pass
            self._fail_pending(BridgeError("timeout", "Grafikr host did not stop within five seconds"))
        self._stop_callback_dispatch()
        for thread in (getattr(self, "_reader", None), getattr(self, "_stderr", None)):
            if thread is not None and thread is not threading.current_thread():
                thread.join(timeout=remaining())
        for pipe in (getattr(self._process, "stdout", None), getattr(self._process, "stderr", None)):
            close_pipe = getattr(pipe, "close", None)
            if callable(close_pipe):
                try:
                    close_pipe()
                except (OSError, ValueError):
                    pass

    def _read(self) -> None:
        try:
            while True:
                line = self._process.stdout.readline(MAX_FRAME + 1)
                if not line:
                    break
                if len(line) > MAX_FRAME or not line.endswith(b"\n"):
                    raise BridgeError("invalid_frame", "host emitted an oversized or unterminated frame")
                message = json.loads(line)
                if message.get("kind") == "response":
                    with self._lock:
                        future = self._pending.pop(message.get("id"), None)
                    if future is not None and not future.done():
                        if message.get("ok"):
                            future.set_result(message.get("result"))
                        else:
                            error = message.get("error") or {}
                            future.set_exception(BridgeError(error.get("code", "internal_failure"), error.get("message", "request failed"), error.get("field")))
                elif message.get("kind") == "event":
                    self._queue_event(message, len(line))
                else:
                    raise BridgeError("invalid_frame", "host emitted an unknown message kind")
        except BaseException as error:
            self._failure = error
            try:
                self._process.terminate()
            except OSError:
                pass
        finally:
            self._fail_pending(BridgeError("connection_closed", str(self._failure or "Grafikr host exited")))
            self._stop_callback_dispatch()

    def _queue_event(self, event: dict[str, Any], frame_size: int) -> None:
        with self._callback_lock:
            if self._callback_stopped:
                raise BridgeError("stopped", "event dispatcher is stopping")
            if self._callback_count >= MAX_CALLBACK_EVENTS or self._callback_bytes + frame_size > MAX_CALLBACK_BYTES:
                raise BridgeError("queue_full", "callback queue quota reached")
            self._callback_count += 1
            self._callback_bytes += frame_size
            try:
                self._callback_queue.put_nowait((event, frame_size))
            except queue.Full as error:
                self._callback_count -= 1
                self._callback_bytes -= frame_size
                raise BridgeError("queue_full", "callback queue quota reached") from error

    def _stop_callback_dispatch(self) -> None:
        with self._callback_lock:
            if self._callback_stopped:
                return
            self._callback_stopped = True
            self._callback_queue.put_nowait(None)

    def _dispatch_events(self) -> None:
        while True:
            queued = self._callback_queue.get()
            if queued is None:
                return
            event, frame_size = queued
            with self._callback_lock:
                self._callback_count -= 1
                self._callback_bytes -= frame_size
            key = (str(event.get("source")), event.get("event", ""))
            with self._lock:
                callbacks = tuple(self._handlers.get(key, ()))
            for callback in callbacks:
                try:
                    callback(event)
                except Exception as error:
                    with self._lock:
                        handler = self._error_handler
                    if handler is not None:
                        try:
                            handler(error, event)
                        except Exception:
                            continue

    def _drain_stderr(self) -> None:
        for _ in iter(self._process.stderr.readline, b""):
            pass

    def _fail_pending(self, error: BaseException) -> None:
        with self._lock:
            pending, self._pending = self._pending, {}
        for future in pending.values():
            if not future.done():
                future.set_exception(error)

    def __enter__(self) -> App:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()


@dataclass
class Subscription:
    app: App
    handle: str
    key: tuple[str, str]
    callback: Callable[[dict[str, Any]], None]
    _closed: bool = False

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        with self.app._lock:
            callbacks = self.app._handlers.get(self.key, [])
            if self.callback in callbacks:
                callbacks.remove(self.callback)
        self.app.request("event.unsubscribe", {"subscription": self.handle})


@dataclass
class Window:
    app: App
    handle: str
    root: str

    @classmethod
    def create(cls, app: App, title: str, width: float, height: float, theme: str = "dark") -> Window:
        result = app.request("window.create", {"title": title, "width": width, "height": height, "theme": theme})
        return cls(app, str(result["window"]), str(result["root"]))

    def close(self) -> None:
        self.app.request("window.close", {"window": self.handle})


@dataclass
class Node:
    app: App
    handle: str

    @classmethod
    def create(cls, app: App, window: Window, widget: str, **properties: Any) -> Node:
        result = app.request("node.create", {"window": window.handle, "widget": widget, "properties": properties})
        return cls(app, str(result["node"]))

    def attach(self, parent: Node | Window, index: int | None = None) -> None:
        parent_handle = parent.root if isinstance(parent, Window) else parent.handle
        args: dict[str, Any] = {"node": self.handle, "parent": parent_handle}
        if index is not None:
            args["index"] = index
        self.app.request("node.attach", args)

    def update(self, **properties: Any) -> None:
        self.app.request("property.update", {"node": self.handle, "properties": properties})

    def destroy(self) -> None:
        self.app.request("node.destroy", {"node": self.handle})


@dataclass(frozen=True)
class CollectionMetadata:
    row_count: int
    row_height: float
    columns: list[str] | tuple[str, ...]

    def args(self) -> dict[str, Any]:
        if isinstance(self.row_count, bool) or not isinstance(self.row_count, int) or self.row_count < 0:
            raise BridgeError("invalid_value", "collection row_count must be a nonnegative integer")
        if not isinstance(self.row_height, (int, float)) or isinstance(self.row_height, bool) or not math.isfinite(self.row_height) or self.row_height <= 0:
            raise BridgeError("invalid_value", "collection row_height must be finite and positive")
        if not isinstance(self.columns, (list, tuple)) or not all(isinstance(column, str) for column in self.columns):
            raise BridgeError("invalid_value", "collection columns must be strings")
        return {"row_count": self.row_count, "row_height": self.row_height, "columns": list(self.columns)}


@dataclass(frozen=True)
class CollectionRequest:
    revision: str
    request: str
    start: int
    count: int


_COLLECTION_CLOSED = object()


class CollectionProvider:
    """Supplies visible rows requested by a VirtualList or DataGrid."""

    def __init__(self, node: Node):
        self.app = node.app
        self.node = node
        self._requests: queue.Queue[CollectionRequest | object] = queue.Queue(maxsize=1)
        self._lock = threading.Lock()
        self._closed = False
        self._subscription: Subscription | None = None

    @classmethod
    def create(cls, node: Node, metadata: CollectionMetadata) -> CollectionProvider:
        provider = cls(node)
        provider._subscription = provider.app.on(node.handle, "data.request", provider._receive)
        try:
            provider.app.request("collection.create", {"node": node.handle, **metadata.args()})
        except BaseException:
            provider.close()
            raise
        return provider

    def _receive(self, event: dict[str, Any]) -> None:
        payload = event.get("payload")
        if not isinstance(payload, dict):
            raise BridgeError("invalid_frame", "collection request payload is invalid")
        revision, request = payload.get("revision"), payload.get("request")
        start, count = payload.get("start"), payload.get("count")
        if (
            not isinstance(revision, str) or not isinstance(request, str)
            or isinstance(start, bool) or not isinstance(start, int) or start < 0
            or isinstance(count, bool) or not isinstance(count, int) or count < 0
        ):
            raise BridgeError("invalid_frame", "collection request fields are invalid")
        with self._lock:
            if self._closed:
                return
            try:
                self._requests.put_nowait(CollectionRequest(revision, request, start, count))
            except queue.Full as error:
                raise BridgeError("queue_full", "collection provider request queue is full") from error

    def next_request(self, timeout: float | None = None) -> CollectionRequest | None:
        request = self._requests.get(timeout=timeout)
        return None if request is _COLLECTION_CLOSED else request

    def provide(self, request: CollectionRequest, rows: list[list[Any]]) -> None:
        with self._lock:
            if self._closed:
                raise BridgeError("connection_closed", "collection provider is closed")
        self.app.request("data.provide", {
            "node": self.node.handle, "revision": request.revision,
            "request": request.request, "start": request.start, "rows": rows,
        })

    def update(self, metadata: CollectionMetadata) -> None:
        with self._lock:
            if self._closed:
                raise BridgeError("connection_closed", "collection provider is closed")
        self.app.request("collection.update", {"node": self.node.handle, "metadata": metadata.args()})

    def close(self) -> None:
        with self._lock:
            if self._closed:
                return
            self._closed = True
            while not self._requests.empty():
                try:
                    self._requests.get_nowait()
                except queue.Empty:
                    break
            self._requests.put_nowait(_COLLECTION_CLOSED)
        if self._subscription is not None:
            self._subscription.close()


@dataclass
class State(Generic[T]):
    app: App
    handle: str

    @classmethod
    def create(cls, app: App, value: T) -> State[T]:
        result = app.request("state.create", {"value": _encode_scalar(value)})
        return cls(app, str(result["state"]))

    def get(self) -> T:
        return _decode_scalar(self.app.request("state.get", {"state": self.handle}))

    def set(self, value: T) -> None:
        self.app.request("state.set", {"state": self.handle, "value": _encode_scalar(value)})

    def close(self) -> None:
        self.app.request("state.destroy", {"state": self.handle})


def _encode_scalar(value: Any) -> dict[str, Any]:
    if isinstance(value, dict):
        _decode_scalar(value)
        return value
    if isinstance(value, bool):
        return {"type": "bool", "value": value}
    if isinstance(value, int):
        if value < I64_MIN or value > I64_MAX:
            raise BridgeError("invalid_value", "integer state is outside signed 64-bit range")
        return {"type": "i64", "value": str(value)}
    if isinstance(value, float):
        if not math.isfinite(value):
            raise BridgeError("invalid_value", "floating state must be finite")
        return {"type": "f64", "value": value}
    if isinstance(value, str):
        return {"type": "string", "value": value}
    raise BridgeError("invalid_value", "state must be an int, float, bool, string, or tagged scalar")


def _decode_scalar(value: Any) -> Any:
    if not isinstance(value, dict) or set(value) != {"type", "value"}:
        raise BridgeError("invalid_value", "state response is not a tagged scalar")
    tag, raw = value["type"], value["value"]
    if tag == "bool" and isinstance(raw, bool):
        return raw
    if tag == "i64" and isinstance(raw, str):
        try:
            integer = int(raw)
        except ValueError as error:
            raise BridgeError("invalid_value", "state integer is not decimal") from error
        if str(integer) != raw or not I64_MIN <= integer <= I64_MAX:
            raise BridgeError("invalid_value", "state integer is outside signed 64-bit range")
        return integer
    if tag == "f64" and isinstance(raw, (int, float)) and not isinstance(raw, bool):
        number = float(raw)
        if math.isfinite(number):
            return number
    if tag == "string" and isinstance(raw, str):
        return raw
    raise BridgeError("invalid_value", "state tag does not match its value")
