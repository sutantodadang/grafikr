import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { resolveHost } from "../dist/index.js";

test("verifies bundle files under a non-ASCII absolute path", async () => {
  const parent = await mkdtemp(join(tmpdir(), "grafikr-测试-"));
  const bundle = join(parent, "runtíme bundle");
  try {
    await mkdir(join(bundle, "bin"), { recursive: true });
    const host = Buffer.from("host fixture");
    await writeFile(join(bundle, "bin", "host.exe"), host);
    const target = ({ win32: { x64: "x86_64-pc-windows-msvc", arm64: "aarch64-pc-windows-msvc" }, linux: { x64: "x86_64-unknown-linux-gnu", arm64: "aarch64-unknown-linux-gnu" }, darwin: { x64: "x86_64-apple-darwin", arm64: "aarch64-apple-darwin" } })[process.platform][process.arch];
    await writeFile(join(bundle, "manifest.json"), JSON.stringify({
      target,
      development_fixture: false,
      files: [{ path: "bin/host.exe", size: host.length, sha256: createHash("sha256").update(host).digest("hex"), role: "host" }],
    }));
    assert.equal(await resolveHost(bundle), join(bundle, "bin", "host.exe"));
    await writeFile(join(bundle, "bin", "host.exe"), Buffer.from("modified"));
    await assert.rejects(resolveHost(bundle), error => error.code === "runtime_integrity");
  } finally {
    await rm(parent, { recursive: true, force: true });
  }
});
