import { Worker } from "node:worker_threads";
import { isAbsolute } from "node:path";
import { resolveHost } from "./runtime.js";

export const MAX_FRAME = 8 * 1024 * 1024;
const MAX_PENDING_REQUESTS = 256;
const MAX_PENDING_BYTES = 16 * 1024 * 1024;
export class BridgeError extends Error {
  constructor(readonly code: string, message: string, readonly field?: string) { super(message); }
}
export type EventMessage = { kind: "event"; sequence: string; event: string; source: string; payload: any };
export type Handle<Kind extends string> = string & { readonly __grafikrHandle: Kind };
type TransportMessage =
  | { type: "response"; id: string; ok: boolean; result?: any; error?: { code: string; message: string; field?: string } }
  | { type: "event"; ticket: string; bytes: number; message: EventMessage }
  | { type: "failure"; code: string; message: string }
  | { type: "closed" };
type Listener = (message: EventMessage) => void | Promise<void>;
type CallbackErrorHandler = (error: unknown, message: EventMessage) => void;

export class App {
  private readonly worker: Worker;
  private readonly pending = new Map<string, { resolve(value: any): void; reject(error: Error): void; timer: NodeJS.Timeout; bytes: number }>();
  private readonly listeners = new Map<string, Set<Listener>>();
  private readonly subscriptions = new Map<string, { key: string; listener: Listener }>();
  private readonly eventQueue: Array<{ ticket: string; message: EventMessage }> = [];
  private nextId = 1n;
  private pendingBytes = 0;
  private closed = false;
  private handlingEvents = false;
  private callbackErrorHandler: CallbackErrorHandler | undefined;
  private closing: Promise<void> | undefined;
  readonly ready: Promise<void>;

  constructor(hostPath: string, sdkVersion = "1.1.0", hostArgs: string[] = []) {
    if (!isAbsolute(hostPath)) throw new BridgeError("runtime_path", "Grafikr host path must be absolute");
    this.worker = new Worker(new URL("./transport-worker.js", import.meta.url), {
      workerData: { hostPath, hostArgs },
    });
    this.worker.on("message", (message: TransportMessage) => this.consume(message));
    this.worker.on("error", error => this.fail(error));
    this.worker.on("exit", code => {
      if (!this.closed && !this.closing) this.fail(new BridgeError("connection_closed", `Grafikr transport worker exited (${code})`));
    });
    this.ready = this.request("hello", { protocol_major: 1, protocol_minor: 0, sdk_version: sdkVersion }).then(result => {
      if (result?.protocol?.major !== 1) throw new BridgeError("version_mismatch", "Grafikr host requires protocol 1");
    });
  }

  request<T = any>(op: string, args: Record<string, unknown> = {}, timeoutMs = 30_000): Promise<T> {
    if (this.closed) return Promise.reject(new BridgeError("connection_closed", "Grafikr host is closed"));
    const id = String(this.nextId++);
    let frame: Buffer;
    try {
      frame = Buffer.from(JSON.stringify({ id, op, args }, (_key, value) => {
        if (typeof value === "number" && !Number.isFinite(value)) throw new BridgeError("invalid_value", "request contains a non-finite number");
        return value;
      }) + "\n", "utf8");
    } catch (error) {
      return Promise.reject(error instanceof BridgeError ? error : new BridgeError("invalid_value", "request arguments are not JSON serializable"));
    }
    if (frame.length > MAX_FRAME) return Promise.reject(new BridgeError("frame_too_large", "request exceeds 8 MiB"));
    if (this.pending.size >= MAX_PENDING_REQUESTS || this.pendingBytes + frame.length > MAX_PENDING_BYTES) {
      return Promise.reject(new BridgeError("queue_full", "Node request queue quota reached"));
    }
    return new Promise<T>((resolve, reject) => {
      const bytes = frame.length;
      const deadline = Math.max(1, Math.min(timeoutMs, 3_600_000));
      const timer = setTimeout(() => {
        const entry = this.pending.get(id);
        if (!entry) return;
        this.pending.delete(id);
        this.pendingBytes -= entry.bytes;
        reject(new BridgeError("timeout", `request ${id} timed out`));
      }, deadline);
      this.pending.set(id, { resolve, reject, timer, bytes });
      this.pendingBytes += bytes;
      try { this.worker.postMessage({ type: "request", id, frame, timeoutMs: deadline }); }
      catch (error) {
        clearTimeout(timer);
        this.pending.delete(id);
        this.pendingBytes -= bytes;
        reject(new BridgeError("connection_closed", error instanceof Error ? error.message : String(error)));
      }
    });
  }

  onError(handler: CallbackErrorHandler): void { this.callbackErrorHandler = handler; }

  async on(source: string, event: string, callback: Listener): Promise<Subscription> {
    const key = `${source}\0${event}`;
    const listeners = this.listeners.get(key) ?? new Set<Listener>();
    listeners.add(callback);
    this.listeners.set(key, listeners);
    try {
      const { subscription } = await this.request<{ subscription: string }>("event.subscribe", { node: source, event });
      const handle = subscription as Handle<"subscription">;
      this.subscriptions.set(handle, { key, listener: callback });
      return new Subscription(this, handle, key, callback);
    } catch (error) {
      listeners.delete(callback);
      if (!listeners.size) this.listeners.delete(key);
      throw error;
    }
  }

  async createCollectionProvider(node: Node, metadata: CollectionMetadata): Promise<CollectionProvider> {
    return CollectionProvider.create(node, metadata);
  }

  async close(timeoutMs = 5_000): Promise<void> {
    if (this.closing) return this.closing;
    this.closing = (async () => {
      if (!this.closed) {
        try { await this.request("shutdown", {}, Math.min(timeoutMs, 5_000)); } catch { /* process may already be gone */ }
        this.closed = true;
        this.rejectPending(new BridgeError("connection_closed", "Grafikr app was closed"));
        this.worker.postMessage({ type: "close", timeoutMs });
        let timer: NodeJS.Timeout | undefined;
        await Promise.race([
          new Promise<void>(resolve => this.worker.once("exit", () => resolve())),
          new Promise<void>(resolve => { timer = setTimeout(resolve, timeoutMs); }),
        ]);
        if (timer) clearTimeout(timer);
      }
      this.closed = true;
      this.rejectPending(new BridgeError("connection_closed", "Grafikr app was closed"));
      if (this.worker.threadId !== -1) await this.worker.terminate();
    })();
    return this.closing;
  }

  async _unsubscribe(subscription: Handle<"subscription">, key: string, listener: Listener): Promise<void> {
    const listeners = this.listeners.get(key);
    listeners?.delete(listener);
    if (listeners?.size === 0) this.listeners.delete(key);
    this.subscriptions.delete(subscription);
    await this.request("event.unsubscribe", { subscription });
  }

  private consume(message: TransportMessage): void {
    if (message.type === "response") {
      const entry = this.pending.get(message.id);
      if (!entry) return;
      clearTimeout(entry.timer);
      this.pending.delete(message.id);
      this.pendingBytes -= entry.bytes;
      if (message.ok) entry.resolve(message.result);
      else entry.reject(new BridgeError(message.error?.code ?? "internal_failure", message.error?.message ?? "request failed", message.error?.field));
    } else if (message.type === "event") {
      this.eventQueue.push({ ticket: message.ticket, message: message.message });
      void this.pumpEvents();
    } else if (message.type === "failure") {
      this.fail(new BridgeError(message.code, message.message));
    } else if (message.type === "closed") {
      this.closed = true;
    }
  }

  private async pumpEvents(): Promise<void> {
    if (this.handlingEvents) return;
    this.handlingEvents = true;
    try {
      while (this.eventQueue.length) {
        const { ticket, message } = this.eventQueue.shift()!;
        for (const listener of this.listeners.get(`${message.source}\0${message.event}`) ?? []) {
          try { await listener(message); }
          catch (error) { try { this.callbackErrorHandler?.(error, message); } catch { /* isolate the error hook */ } }
        }
        this.worker.postMessage({ type: "ack", ticket });
      }
    } finally {
      this.handlingEvents = false;
    }
  }

  private fail(error: Error): void {
    if (this.closed && this.pending.size === 0) return;
    this.closed = true;
    this.rejectPending(error);
    if (this.worker.threadId !== -1) void this.worker.terminate();
  }

  private rejectPending(error: Error): void {
    for (const [id, entry] of this.pending) { clearTimeout(entry.timer); entry.reject(error); this.pending.delete(id); }
    this.pendingBytes = 0;
  }
}

export class Client extends App {
  private constructor(hostPath: string) { super(hostPath); }
  static async start(options: { bundle?: string } = {}): Promise<Client> {
    return new Client(await resolveHost(options.bundle));
  }
}
export { resolveHost } from "./runtime.js";

export class Subscription {
  private active = true;
  constructor(private readonly app: App, readonly handle: Handle<"subscription">, private readonly key: string, private readonly listener: Listener) {}
  async close(): Promise<void> { if (this.active) { this.active = false; await this.app._unsubscribe(this.handle, this.key, this.listener); } }
}

export class Window {
  private constructor(readonly app: App, readonly handle: Handle<"window">, readonly root: Handle<"node">) {}
  static async create(app: App, title: string, width: number, height: number, theme = "dark"): Promise<Window> {
    const result = await app.request<{ window: string; root: string }>("window.create", { title, width, height, theme });
    return new Window(app, result.window as Handle<"window">, result.root as Handle<"node">);
  }
  async close(): Promise<void> { await this.app.request("window.close", { window: this.handle }); }
}

export class Node {
  constructor(readonly app: App, readonly handle: Handle<"node">) {}
  static async create(app: App, window: Window, widget: string, properties: Record<string, unknown> = {}): Promise<Node> {
    const result = await app.request<{ node: string }>("node.create", { window: window.handle, widget, properties });
    return new Node(app, result.node as Handle<"node">);
  }
  async attach(parent: Node | Window, index?: number): Promise<void> {
    const args: Record<string, unknown> = { node: this.handle, parent: parent instanceof Window ? parent.root : parent.handle };
    if (index !== undefined) args.index = index;
    await this.app.request("node.attach", args);
  }
  async update(properties: Record<string, unknown>): Promise<void> { await this.app.request("property.update", { node: this.handle, properties }); }
  async destroy(): Promise<void> { await this.app.request("node.destroy", { node: this.handle }); }
}

export type CollectionMetadata = Readonly<{ row_count: number; row_height: number; columns: readonly string[] }>;
export type CollectionRequest = Readonly<{ revision: string; request: string; start: number; count: number }>;

export class CollectionProvider {
  private subscription: Subscription | undefined;
  private readonly requests: CollectionRequest[] = [];
  private readonly waiters: Array<(request: CollectionRequest | null) => void> = [];
  private closed = false;

  private constructor(private readonly node: Node) {}

  static async create(node: Node, metadata: CollectionMetadata): Promise<CollectionProvider> {
    const args = validateCollectionMetadata(metadata);
    const provider = new CollectionProvider(node);
    try {
      provider.subscription = await node.app.on(node.handle, "data.request", event => provider.enqueue(event));
      await node.app.request("collection.create", { node: node.handle, ...args });
      return provider;
    } catch (error) {
      await provider.close();
      throw error;
    }
  }

  nextRequest(): Promise<CollectionRequest | null> {
    const request = this.requests.shift();
    if (request) return Promise.resolve(request);
    if (this.closed) return Promise.resolve(null);
    if (this.waiters.length >= 1) return Promise.reject(new BridgeError("queue_full", "only one collection read may wait"));
    return new Promise(resolve => this.waiters.push(resolve));
  }

  async provide(request: CollectionRequest, rows: readonly (readonly unknown[])[]): Promise<void> {
    if (this.closed) throw new BridgeError("connection_closed", "collection provider is closed");
    await this.node.app.request("data.provide", {
      node: this.node.handle, revision: request.revision, request: request.request,
      start: request.start, rows,
    });
  }

  async update(metadata: CollectionMetadata): Promise<void> {
    if (this.closed) throw new BridgeError("connection_closed", "collection provider is closed");
    await this.node.app.request("collection.update", { node: this.node.handle, metadata: validateCollectionMetadata(metadata) });
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.requests.length = 0;
    for (const resolve of this.waiters.splice(0)) resolve(null);
    await this.subscription?.close();
  }

  private enqueue(event: EventMessage): void {
    const payload = event.payload;
    const request = payload && typeof payload === "object" ? payload as Partial<CollectionRequest> : {};
    if (
      typeof request.revision !== "string" || typeof request.request !== "string"
      || !Number.isSafeInteger(request.start) || request.start! < 0
      || !Number.isSafeInteger(request.count) || request.count! < 0
    ) throw new BridgeError("invalid_frame", "host emitted an invalid collection request");
    if (this.closed) return;
    const value: CollectionRequest = {
      revision: request.revision, request: request.request,
      start: request.start!, count: request.count!,
    };
    const waiter = this.waiters.shift();
    if (waiter) waiter(value);
    else if (this.requests.length >= 1) throw new BridgeError("queue_full", "collection provider request queue is full");
    else this.requests.push(value);
  }
}

function validateCollectionMetadata(metadata: CollectionMetadata): CollectionMetadata {
  if (!Number.isSafeInteger(metadata.row_count) || metadata.row_count < 0) {
    throw new BridgeError("invalid_value", "collection row_count must be a nonnegative safe integer");
  }
  if (!Number.isFinite(metadata.row_height) || metadata.row_height <= 0) {
    throw new BridgeError("invalid_value", "collection row_height must be finite and positive");
  }
  if (!Array.isArray(metadata.columns) || !metadata.columns.every(column => typeof column === "string")) {
    throw new BridgeError("invalid_value", "collection columns must be strings");
  }
  return { row_count: metadata.row_count, row_height: metadata.row_height, columns: [...metadata.columns] };
}

type StateKind = "bool" | "i64" | "f64" | "string";
type StateResult<K extends StateKind> = K extends "bool" ? boolean : K extends "i64" ? bigint : K extends "f64" ? number : string;

export class State<K extends StateKind = StateKind> {
  private constructor(readonly app: App, readonly handle: Handle<"state">) {}
  static create(app: App, value: { type: "bool"; value: boolean }): Promise<State<"bool">>;
  static create(app: App, value: { type: "i64"; value: bigint | number | string }): Promise<State<"i64">>;
  static create(app: App, value: { type: "f64"; value: number }): Promise<State<"f64">>;
  static create(app: App, value: { type: "string"; value: string }): Promise<State<"string">>;
  static async create<K extends StateKind>(app: App, value: StateValue<K>): Promise<State<K>> {
    const result = await app.request<{ state: string }>("state.create", { value: encodeStateValue(value) });
    return new State<K>(app, result.state as Handle<"state">);
  }
  async get(): Promise<StateResult<K>> {
    const result = await this.app.request<StateValue>("state.get", { state: this.handle });
    const value = result.value;
    if (result.type === "i64") return BigInt(value as string) as StateResult<K>;
    return value as StateResult<K>;
  }
  async set(value: StateValue<K>): Promise<void> { await this.app.request("state.set", { state: this.handle, value: encodeStateValue(value) }); }
  async close(): Promise<void> { await this.app.request("state.destroy", { state: this.handle }); }
}

export type StateValue<K extends StateKind = StateKind> = K extends "bool"
  ? { type: "bool"; value: boolean }
  : K extends "i64"
    ? { type: "i64"; value: bigint | number | string }
    : K extends "f64"
      ? { type: "f64"; value: number }
      : K extends "string"
        ? { type: "string"; value: string }
        : never;

function encodeStateValue(value: StateValue): Omit<StateValue, "value"> & { value: boolean | string | number } {
  if (value.type === "i64") {
    let number: bigint;
    try {
      if (typeof value.value === "number" && !Number.isSafeInteger(value.value)) throw new Error("unsafe integer");
      if (typeof value.value === "string" && !/^-?[0-9]+$/.test(value.value)) throw new Error("invalid decimal integer");
      number = BigInt(value.value);
    } catch {
      throw new BridgeError("invalid_value", "i64 state value must be an integer");
    }
    if (number < -(1n << 63n) || number > (1n << 63n) - 1n) throw new BridgeError("invalid_value", "i64 state value is outside signed 64-bit range");
    return { type: "i64", value: number.toString() };
  }
  if (value.type === "f64" && !Number.isFinite(value.value)) throw new BridgeError("invalid_value", "f64 state value must be finite");
  if (value.type === "bool" && typeof value.value !== "boolean") throw new BridgeError("invalid_value", "bool state value must be boolean");
  if (value.type === "string" && typeof value.value !== "string") throw new BridgeError("invalid_value", "string state value must be a string");
  if (!["bool", "f64", "string"].includes(value.type)) throw new BridgeError("invalid_value", "state value has an unknown type");
  return value;
}
