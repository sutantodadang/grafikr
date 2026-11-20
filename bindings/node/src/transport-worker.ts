import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { parentPort, workerData } from "node:worker_threads";

const MAX_FRAME = 8 * 1024 * 1024;
const MAX_PENDING_REQUESTS = 256;
const MAX_PENDING_BYTES = 16 * 1024 * 1024;
const MAX_CALLBACK_EVENTS = 1024;
const MAX_CALLBACK_BYTES = 16 * 1024 * 1024;

type WorkerConfig = { hostPath: string; hostArgs: string[] };
type ParentRequest = { type: "request"; id: string; frame: Uint8Array; timeoutMs: number };
type ParentMessage = ParentRequest | { type: "ack"; ticket: string } | { type: "close"; timeoutMs: number };
type Pending = { timer: NodeJS.Timeout; bytes: number };

const config = workerData as WorkerConfig;
const port = parentPort;
if (!port) throw new Error("Grafikr transport worker requires a parent port");

let child: ChildProcessWithoutNullStreams | undefined;
let started = false;
let failed = false;
let stopping = false;
let shutdownRequested = false;
let buffer = Buffer.alloc(0);
let pendingBytes = 0;
let callbackBytes = 0;
let sequence = 0n;
let nextTicket = 1n;
const pending = new Map<string, Pending>();
const pendingInputs: ParentMessage[] = [];
const outstandingEvents = new Map<string, number>();
let startupRequestBytes = 0;
let startupRequestCount = 0;

function post(message: object): void {
  try {
    port?.postMessage(message);
  } catch {
    stopChild();
  }
}

function stopChild(): void {
  if (child && child.exitCode === null) child.kill();
}

function finishPending(error: { code: string; message: string }): void {
  for (const [id, entry] of pending) {
    clearTimeout(entry.timer);
    post({ type: "response", id, ok: false, error });
  }
  pending.clear();
  pendingBytes = 0;
}

function fail(code: string, message: string): void {
  if (failed) return;
  failed = true;
  finishPending({ code, message });
  post({ type: "failure", code, message });
  stopChild();
}

function receiveFrame(frame: Buffer): void {
  let message: any;
  try {
    const text = new TextDecoder("utf-8", { fatal: true }).decode(frame.subarray(0, frame.length - 1));
    message = JSON.parse(text);
  } catch {
    fail("invalid_frame", "Grafikr host emitted invalid UTF-8 or JSON");
    return;
  }
  if (message?.kind === "response") {
    const entry = pending.get(message.id);
    if (!entry) return;
    clearTimeout(entry.timer);
    pending.delete(message.id);
    pendingBytes -= entry.bytes;
    post({
      type: "response",
      id: message.id,
      ok: Boolean(message.ok),
      result: message.result,
      error: message.error,
    });
    return;
  }
  if (message?.kind === "event" && typeof message.sequence === "string") {
    let current: bigint;
    try {
      if (!/^(0|[1-9][0-9]*)$/.test(message.sequence)) throw new Error("invalid sequence");
      current = BigInt(message.sequence);
    } catch {
      fail("invalid_frame", "Grafikr host emitted an invalid event sequence");
      return;
    }
    if (current <= sequence) {
      fail("invalid_frame", "Grafikr host event sequence did not increase");
      return;
    }
    sequence = current;
    const bytes = frame.length;
    if (outstandingEvents.size >= MAX_CALLBACK_EVENTS || callbackBytes + bytes > MAX_CALLBACK_BYTES) {
      fail("queue_full", "Node callback queue quota reached");
      return;
    }
    const ticket = String(nextTicket++);
    outstandingEvents.set(ticket, bytes);
    callbackBytes += bytes;
    post({ type: "event", ticket, bytes, message });
    return;
  }
  fail("invalid_frame", "Grafikr host emitted an unknown message kind");
}

function consume(chunk: Buffer): void {
  let offset = 0;
  while (offset < chunk.length && !failed) {
    const lf = chunk.indexOf(10, offset);
    if (lf < 0) {
      const remaining = chunk.length - offset;
      if (buffer.length + remaining > MAX_FRAME) {
        fail("frame_too_large", "Grafikr host frame exceeds 8 MiB");
        return;
      }
      buffer = Buffer.concat([buffer, chunk.subarray(offset)]);
      return;
    }
    const end = lf + 1;
    if (buffer.length + end - offset > MAX_FRAME) {
      fail("frame_too_large", "Grafikr host frame exceeds 8 MiB");
      return;
    }
    const frame = buffer.length ? Buffer.concat([buffer, chunk.subarray(offset, end)]) : chunk.subarray(offset, end);
    buffer = Buffer.alloc(0);
    receiveFrame(frame);
    offset = end;
  }
}

function submit(request: ParentRequest): void {
  if (failed || stopping || !child?.stdin.writable) {
    post({ type: "response", id: request.id, ok: false, error: { code: "connection_closed", message: "Grafikr host is closed" } });
    return;
  }
  if (pending.size >= MAX_PENDING_REQUESTS || pendingBytes + request.frame.byteLength > MAX_PENDING_BYTES) {
    post({ type: "response", id: request.id, ok: false, error: { code: "queue_full", message: "Node request queue quota reached" } });
    return;
  }
  let wire: any;
  try {
    wire = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(request.frame));
  } catch {
    post({ type: "response", id: request.id, ok: false, error: { code: "invalid_value", message: "request frame is invalid" } });
    return;
  }
  if (wire?.id !== request.id || typeof wire.op !== "string") {
    post({ type: "response", id: request.id, ok: false, error: { code: "invalid_value", message: "request frame is invalid" } });
    return;
  }
  if (wire.op === "shutdown") shutdownRequested = true;
  const timeoutMs = Math.max(1, Math.min(request.timeoutMs, 3_600_000));
  const timer = setTimeout(() => {
    const entry = pending.get(request.id);
    if (!entry) return;
    pending.delete(request.id);
    pendingBytes -= entry.bytes;
    post({ type: "response", id: request.id, ok: false, error: { code: "timeout", message: `request ${request.id} timed out` } });
  }, timeoutMs);
  pending.set(request.id, { timer, bytes: request.frame.byteLength });
  pendingBytes += request.frame.byteLength;
  child.stdin.write(Buffer.from(request.frame), error => {
    if (!error) return;
    const entry = pending.get(request.id);
    if (!entry) return;
    clearTimeout(entry.timer);
    pending.delete(request.id);
    pendingBytes -= entry.bytes;
    post({ type: "response", id: request.id, ok: false, error: { code: "connection_closed", message: error.message } });
  });
}

function handleParentMessage(message: ParentMessage): void {
  if (message.type === "request") {
    submit(message);
    return;
  }
  if (message.type === "ack") {
    const bytes = outstandingEvents.get(message.ticket);
    if (bytes !== undefined) {
      outstandingEvents.delete(message.ticket);
      callbackBytes -= bytes;
    }
    return;
  }
  if (message.type === "close") {
    stopping = true;
    if (!child || child.exitCode !== null) {
      port?.close();
      return;
    }
    child.stdin.end();
    const timer = setTimeout(() => stopChild(), Math.max(1, Math.min(message.timeoutMs, 5_000) - 50));
    child.once("close", () => clearTimeout(timer));
  }
}

port.on("message", (message: ParentMessage) => {
  if (failed) return;
  if (started) {
    handleParentMessage(message);
    return;
  }
  if (message.type === "request") {
    if (startupRequestCount >= MAX_PENDING_REQUESTS || startupRequestBytes + message.frame.byteLength > MAX_PENDING_BYTES) {
      post({ type: "response", id: message.id, ok: false, error: { code: "queue_full", message: "Node startup request queue quota reached" } });
      return;
    }
    startupRequestCount++;
    startupRequestBytes += message.frame.byteLength;
  }
  pendingInputs.push(message);
});

try {
  child = spawn(config.hostPath, config.hostArgs, {
    shell: false,
    windowsHide: true,
    stdio: ["pipe", "pipe", "pipe"],
  });
  child.stdout.on("data", (chunk: Buffer) => consume(chunk));
  child.stdout.on("end", () => {
    if (buffer.length) fail("invalid_frame", "Grafikr host ended before the LF frame terminator");
  });
  child.stdout.on("error", error => fail("connection_closed", error.message));
  child.stderr.resume();
  child.on("error", error => fail("connection_closed", error.message));
  child.on("close", (code, signal) => {
    if (failed) return;
    if (shutdownRequested && code === 0) {
      finishPending({ code: "connection_closed", message: "Grafikr host exited" });
      post({ type: "closed" });
      port?.close();
      return;
    }
    fail("connection_closed", `Grafikr host exited (${code ?? signal ?? "unknown"})`);
  });
  child.once("spawn", () => {
    started = true;
    for (const message of pendingInputs.splice(0)) {
      if (message.type === "request") {
        startupRequestCount--;
        startupRequestBytes -= message.frame.byteLength;
      }
      handleParentMessage(message);
    }
  });
} catch (error) {
  fail("connection_closed", error instanceof Error ? error.message : String(error));
}
