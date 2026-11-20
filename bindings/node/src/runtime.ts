import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { lstat, readFile } from "node:fs/promises";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { BridgeError } from "./index.js";

const targets: Record<string, string> = {
  "win32-x64": "x86_64-pc-windows-msvc",
  "win32-arm64": "aarch64-pc-windows-msvc",
  "linux-x64": "x86_64-unknown-linux-gnu",
  "linux-arm64": "aarch64-unknown-linux-gnu",
  "darwin-x64": "x86_64-apple-darwin",
  "darwin-arm64": "aarch64-apple-darwin",
};

type Manifest = {
  target: string;
  development_fixture: boolean;
  files: Array<{ path: string; size: number; sha256: string; role: string }>;
};

function target(): string {
  const value = targets[`${process.platform}-${process.arch}`];
  if (!value) throw new BridgeError("runtime_target", `Grafikr runtime is unavailable for ${process.platform}-${process.arch}`);
  return value;
}

async function bundlePath(bundle?: string): Promise<string> {
  if (bundle !== undefined) {
    if (!isAbsolute(bundle)) throw new BridgeError("runtime_path", "runtime bundle path must be absolute");
    return resolve(bundle);
  }
  const packageName = `@grafikr/runtime-${process.platform}-${process.arch}`;
  try {
    const require = createRequire(import.meta.url);
    return join(dirname(require.resolve(`${packageName}/package.json`)), "runtime");
  } catch {
    throw new BridgeError("runtime_missing", `Install the matching optional package ${packageName} or pass { bundle: "/absolute/path/to/runtime-bundle" }`);
  }
}

export async function resolveHost(bundle?: string): Promise<string> {
  const root = await bundlePath(bundle);
  const rootInfo = await lstat(root).catch(() => undefined);
  if (!rootInfo?.isDirectory() || rootInfo.isSymbolicLink()) throw new BridgeError("runtime_path", "runtime bundle directory is missing or unsafe");
  let manifest: Manifest;
  try {
    const manifestPath = join(root, "manifest.json");
    const manifestInfo = await lstat(manifestPath);
    if (!manifestInfo.isFile() || manifestInfo.isSymbolicLink() || manifestInfo.size > 1024 * 1024) throw new Error("unsafe manifest");
    manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  }
  catch { throw new BridgeError("runtime_manifest", "runtime manifest is missing or invalid"); }
  if (manifest.target !== target()) throw new BridgeError("runtime_target", `runtime target ${manifest.target} does not match ${target()}`);
  if (manifest.development_fixture) throw new BridgeError("runtime_fixture", "development fixture runtimes cannot be used by the Node SDK");
  if (!Array.isArray(manifest.files)) throw new BridgeError("runtime_manifest", "runtime manifest file list is invalid");
  let host: string | undefined;
  for (const file of manifest.files) {
    if (typeof file.path !== "string" || file.path.includes("\\") || file.path.startsWith("/") || file.path.split("/").includes("..")) {
      throw new BridgeError("runtime_manifest", "runtime manifest contains an unsafe path");
    }
    const path = resolve(root, ...file.path.split("/"));
    const rel = relative(root, path);
    if (!rel || rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel)) throw new BridgeError("runtime_manifest", "runtime manifest path escapes its bundle");
    let parent = root;
    for (const part of file.path.split("/").slice(0, -1)) {
      parent = join(parent, part);
      const directory = await lstat(parent).catch(() => undefined);
      if (!directory?.isDirectory() || directory.isSymbolicLink()) throw new BridgeError("runtime_integrity", `runtime path is unsafe: ${file.path}`);
    }
    let info;
    try { info = await lstat(path); } catch { throw new BridgeError("runtime_integrity", `runtime file is missing: ${file.path}`); }
    if (!info.isFile() || info.isSymbolicLink() || info.nlink !== 1 || info.size !== file.size) throw new BridgeError("runtime_integrity", `runtime file is unsafe or has changed: ${file.path}`);
    const bytes = await readFile(path);
    if (createHash("sha256").update(bytes).digest("hex") !== file.sha256) throw new BridgeError("runtime_integrity", `runtime file checksum does not match: ${file.path}`);
    if (file.role === "host") host = path;
  }
  if (!host) throw new BridgeError("runtime_manifest", "runtime bundle does not contain a host executable");
  return host;
}
