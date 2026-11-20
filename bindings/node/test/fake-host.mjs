import readline from "node:readline";
const input = readline.createInterface({ input: process.stdin });
const reversed = [];
for await (const line of input) {
  const request = JSON.parse(line);
  if (request.op === "crash") process.exit(17);
  if (request.op === "hold") continue;
  if (request.op === "reverse") {
    reversed.push(request);
    if (reversed.length === 2) {
      for (const pending of reversed.reverse()) {
        process.stdout.write(JSON.stringify({ kind: "response", id: pending.id, ok: true, result: pending.args }) + "\n");
      }
      reversed.length = 0;
    }
    continue;
  }
  if (request.op === "late") {
    setTimeout(() => process.stdout.write(JSON.stringify({ kind: "response", id: request.id, ok: true, result: {} }) + "\n"), 500);
    continue;
  }
  if (request.op === "echo") {
    process.stdout.write(JSON.stringify({ kind: "response", id: request.id, ok: true, result: request.args }) + "\n");
    continue;
  }
  if (request.op === "event.subscribe") {
    const count = request.args.event === "flood" ? 1025 : 1;
    for (let sequence = 1; sequence <= count; sequence++) {
      const payload = request.args.event === "data.request"
        ? { revision: "1", request: "request-1", start: 3, count: 2 }
        : {};
      process.stdout.write(JSON.stringify({ kind: "event", sequence: String(sequence), event: request.args.event, source: request.args.node, payload }) + "\n");
    }
  }
  const result = request.op === "hello" ? { protocol: { major: 1 } } : {};
  process.stdout.write(JSON.stringify({ kind: "response", id: request.id, ok: true, result }) + "\n");
  if (request.op === "hello" && process.env.GRAFIKR_FAKE_HOST_EOF_AFTER_HELLO) {
    setTimeout(() => process.exit(17), 200);
  }
}
