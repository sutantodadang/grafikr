import test from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { App, BridgeError, Node, State, Window } from "../dist/index.js";
import { fileURLToPath } from "node:url";

test("correlates responses from a real child and closes its own process", async () => {
  const fakeHost = fileURLToPath(new URL("./fake-host.mjs", import.meta.url));
  const app = new App(process.execPath, "test", [fakeHost]);
  await app.ready;
  assert.deepEqual(await app.request("state.get", { state: "2" }), {});
  await app.close();
});

test("correlates out-of-order responses by request id", async () => {
  const fakeHost = fileURLToPath(new URL("./fake-host.mjs", import.meta.url));
  const app = new App(process.execPath, "test", [fakeHost]);
  try {
    await app.ready;
    const first = app.request("reverse", { value: "first" });
    const second = app.request("reverse", { value: "second" });
    assert.deepEqual(await Promise.all([first, second]), [
      { value: "first" },
      { value: "second" },
    ]);
  } finally {
    await app.close();
  }
});

test("round-trips split UTF-8 and escaped newlines in a 100 KiB frame", async () => {
  const fakeHost = fileURLToPath(new URL("./fake-host.mjs", import.meta.url));
  const app = new App(process.execPath, "test", [fakeHost]);
  const text = "line one\nline two 雪 😀" + "x".repeat(102_400);
  try {
    await app.ready;
    assert.deepEqual(await app.request("echo", { text }), { text });
  } finally {
    await app.close();
  }
});

test("creates every shared widget and receives its declared events", {
  skip: !process.env.GRAFIKR_FIXTURE_WIDGET_INVENTORY,
}, async () => {
  const inventory = JSON.parse(readFileSync("../protocol/v1/widgets.json", "utf8")).widgets;
  const app = new App(process.env.GRAFIKR_FIXTURE_HOST, "contract");
  try {
    await app.ready;
    const window = await Window.create(app, "widget inventory", 640, 480);
    const state = await State.create(app, { type: "i64", value: 1n });
    const boundary = await State.create(app, { type: "i64", value: "-9223372036854775808" });
    assert.equal(await boundary.get(), -(1n << 63n));
    await boundary.set({ type: "i64", value: "9223372036854775807" });
    assert.equal(await boundary.get(), (1n << 63n) - 1n);
    await boundary.close();
    const { upload } = await app.request("image.begin", { width: 1, height: 1, byte_length: 4 });
    await app.request("image.chunk", { upload, offset: 0, data: Buffer.from([0, 0, 0, 255]).toString("base64") });
    const { image } = await app.request("image.finalize", { upload });

    for (const [widget, spec] of Object.entries(inventory)) {
      const properties = Object.fromEntries(Object.entries(spec.properties).map(([name, type]) => [
        name, sampleWidgetValue(widget, name, type, state.handle, image),
      ]));
      const node = await Node.create(app, window, widget, properties);
      await node.attach(window);
      for (const event of spec.events ?? []) {
        let receive;
        const delivered = new Promise(resolve => { receive = resolve; });
        const subscription = await app.on(node.handle, event, receive);
        let timer;
        const message = await Promise.race([delivered, new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error(`${widget}@${event} was not delivered`)), 2_000);
        })]).finally(() => clearTimeout(timer));
        assert.equal(message.source, node.handle, `${widget}@${event}`);
        assert.equal(message.event, event, `${widget}@${event}`);
        await subscription.close();
      }
      await node.destroy();
    }

    await app.request("image.destroy", { image });
    await state.close();
    await window.close();
  } finally {
    await app.close();
  }
});

test("requires an absolute host path", () => {
  assert.throws(() => new App("relative-host"), error => error.code === "runtime_path");
});

test("delivers an early subscription event and supports an async nested request", async () => {
  const fakeHost = fileURLToPath(new URL("./fake-host.mjs", import.meta.url));
  const app = new App(process.execPath, "test", [fakeHost]);
  try {
    await app.ready;
    let nestedResult;
    const subscription = await app.on("node-1", "click", async () => {
      const nested = app.request("state.set", { state: "state-1", value: { type: "i64", value: "1" } });
      const until = Date.now() + 100;
      while (Date.now() < until) { /* simulate a synchronous application callback */ }
      nestedResult = await nested;
    });
    await new Promise(resolve => setTimeout(resolve, 20));
    assert.deepEqual(nestedResult, {});
    await subscription.close();
  } finally {
    await app.close();
  }
});

test("suppresses an event emitted after unsubscribe begins", {
  skip: !process.env.GRAFIKR_FIXTURE_HOST || !process.env.GRAFIKR_FIXTURE_UNSUBSCRIBE_EVENT,
}, async () => {
  const app = new App(process.env.GRAFIKR_FIXTURE_HOST, "contract");
  let received = 0;
  try {
    await app.ready;
    const window = await Window.create(app, "unsubscribe fixture", 320, 240);
    const button = await Node.create(app, window, "Button", { label: "click" });
    await button.attach(window);
    const subscription = await app.on(button.handle, "click", () => received++);
    await subscription.close();
    await new Promise(resolve => setTimeout(resolve, 20));
    assert.equal(received, 1);
    await window.close();
  } finally {
    await app.close();
  }
});

test("collection provider subscribes before creation and supplies revisioned rows", async () => {
  const fakeHost = fileURLToPath(new URL("./fake-host.mjs", import.meta.url));
  const app = new App(process.execPath, "test", [fakeHost]);
  try {
    await app.ready;
    const node = new Node(app, "node-1");
    const provider = await app.createCollectionProvider(node, {
      row_count: 20, row_height: 24, columns: ["label"],
    });
    const request = await provider.nextRequest();
    assert.deepEqual(request, { revision: "1", request: "request-1", start: 3, count: 2 });
    await provider.provide(request, [["row-3"], ["row-4"]]);
    await provider.update({ row_count: 2, row_height: 30, columns: ["label"] });
    const waiting = provider.nextRequest();
    await assert.rejects(provider.nextRequest(), error => error.code === "queue_full");
    await provider.close();
    assert.equal(await waiting, null);
    assert.equal(await provider.nextRequest(), null);
  } finally {
    await app.close();
  }
});

test("reports callback exceptions without stopping response processing", async () => {
  const fakeHost = fileURLToPath(new URL("./fake-host.mjs", import.meta.url));
  const app = new App(process.execPath, "test", [fakeHost]);
  try {
    await app.ready;
    let callbackErrors = 0;
    app.onError(() => callbackErrors++);
    const subscription = await app.on("node-1", "click", () => { throw new Error("fixture callback error"); });
    await new Promise(resolve => setTimeout(resolve, 20));
    assert.equal(callbackErrors, 1);
    assert.deepEqual(await app.request("state.get", { state: "1" }), {});
    await subscription.close();
  } finally {
    await app.close();
  }
});

test("rejects values JSON cannot represent as a bridge error", async () => {
  const fakeHost = fileURLToPath(new URL("./fake-host.mjs", import.meta.url));
  const app = new App(process.execPath, "test", [fakeHost]);
  try {
    await app.ready;
    await assert.rejects(app.request("state.set", { value: 1n }), BridgeError);
    await assert.rejects(State.create(app, { type: "i64", value: 1n << 63n }), error => error.code === "invalid_value");
    await assert.rejects(State.create(app, { type: "i64", value: "" }), error => error.code === "invalid_value");
    await assert.rejects(app.request("property.update", { properties: { value: Number.NaN } }), error => error.code === "invalid_value");
  } finally {
    await app.close();
  }
});

test("rejects pending work when the host crashes", async () => {
  const fakeHost = fileURLToPath(new URL("./fake-host.mjs", import.meta.url));
  const app = new App(process.execPath, "test", [fakeHost]);
  try {
    await app.ready;
    await assert.rejects(app.request("crash"), error => error.code === "connection_closed");
  } finally {
    await app.close();
  }
});

test("rejects 256 pending requests exactly once on EOF", async () => {
  const fakeHost = fileURLToPath(new URL("./fake-host.mjs", import.meta.url));
  const previous = process.env.GRAFIKR_FAKE_HOST_EOF_AFTER_HELLO;
  process.env.GRAFIKR_FAKE_HOST_EOF_AFTER_HELLO = "1";
  const app = new App(process.execPath, "test", [fakeHost]);
  try {
    await app.ready;
    const pending = Array.from({ length: 256 }, () => app.request("hold").catch(error => error));
    const rejected = await Promise.all(pending);
    assert.equal(rejected.length, 256);
    assert.ok(rejected.every(error => error.code === "connection_closed"));
  } finally {
    await app.close();
    if (previous === undefined) delete process.env.GRAFIKR_FAKE_HOST_EOF_AFTER_HELLO;
    else process.env.GRAFIKR_FAKE_HOST_EOF_AFTER_HELLO = previous;
  }
});

test("bounds outstanding MessagePort requests and rejects them on close", async () => {
  const fakeHost = fileURLToPath(new URL("./fake-host.mjs", import.meta.url));
  const app = new App(process.execPath, "test", [fakeHost]);
  const sentinel = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" });
  try {
    await app.ready;
    const pending = Array.from({ length: 256 }, () => app.request("hold").catch(error => error));
    await assert.rejects(app.request("overflow"), error => error.code === "queue_full");
    const started = Date.now();
    await app.close(250);
    assert.ok(Date.now() - started < 1_000, "close exceeded its shorter shutdown deadline");
    const closed = await Promise.all(pending);
    assert.equal(closed.length, 256);
    assert.ok(closed.every(error => error.code === "connection_closed"));
    assert.equal(sentinel.exitCode, null);
  } finally {
    await app.close();
    if (sentinel.exitCode === null) {
      const exited = once(sentinel, "exit");
      sentinel.kill();
      await exited;
    }
  }
});

test("bounds queued request bytes at 16 MiB", async () => {
  const fakeHost = fileURLToPath(new URL("./fake-host.mjs", import.meta.url));
  const app = new App(process.execPath, "test", [fakeHost]);
  const padding = "x".repeat(100_000);
  try {
    await app.ready;
    const pending = Array.from({ length: 167 }, () => app.request("hold", { padding }).catch(error => error));
    await assert.rejects(app.request("hold", { padding }), error => error.code === "queue_full");
    await app.close(250);
    const closed = await Promise.all(pending);
    assert.ok(closed.every(error => error.code === "connection_closed"));
  } finally {
    await app.close();
  }
});

test("ignores a response that arrives after close begins", async () => {
  const fakeHost = fileURLToPath(new URL("./fake-host.mjs", import.meta.url));
  const app = new App(process.execPath, "test", [fakeHost]);
  await app.ready;
  const pending = app.request("late").then(() => null, error => error);
  await new Promise(resolve => setTimeout(resolve, 20));
  await app.close();
  const error = await pending;
  assert.equal(error.code, "connection_closed");
});

test("terminates a blocked callback queue at its event credit limit", async () => {
  const fakeHost = fileURLToPath(new URL("./fake-host.mjs", import.meta.url));
  const app = new App(process.execPath, "test", [fakeHost]);
  let timer;
  try {
    await app.ready;
    const subscription = app.on("node-1", "flood", async () => new Promise(() => {}));
    const deadline = new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("event credit limit was not enforced")), 5_000); });
    await assert.rejects(Promise.race([subscription, deadline]), error => error.code === "queue_full");
  } finally {
    if (timer) clearTimeout(timer);
    await app.close();
  }
});

function sampleWidgetValue(widget, name, type, state, image) {
  if (widget === "Radio" && name === "group") return state;
  if (widget === "TreeView" && name === "selected") return "root";
  if (widget === "Slider") return ({ min: 0, max: 10, width: 120 })[name] ?? 1;
  if (widget === "Spinner" && name === "size") return 16;
  if (name === "row_count") return 1;
  if (name === "row_height") return 24;
  return ({
    number: 1,
    f32: 1,
    u16: 1,
    u64: 0,
    string: "sample",
    color: "#336699",
    transform: { x: 0, y: 0, scale: 1 },
    boolean: false,
    "primary|secondary|icon": "primary",
    state,
    "GridColumn[]": [{ title: "Column", width: 120 }],
    "string[]": ["One", "Two"],
    "TreeNode[]": [{ key: "root", label: "Root", children: [] }],
    "CanvasCommand[]": [{ kind: "rect", rect: { x: 0, y: 0, width: 1, height: 1 }, color: "#336699", radius: 0 }],
    ImageHandle: image,
  })[type];
}


