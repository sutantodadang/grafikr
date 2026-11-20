local uv = require("luv")
local json = require("dkjson")
local has_openssl = pcall(require, "openssl.digest")
if not has_openssl then
  assert(not os.getenv("GRAFIKR_LUA_REQUIRE_OPENSSL"), "luaossl is required for this SDK suite")
  package.preload["openssl.digest"] = function()
    return { new=function()
      local value = ""
      return {
        update=function(_, chunk) value = value .. chunk; return true end,
        final=function()
          local sum = 0
          for index = 1, #value do sum = (sum + value:byte(index) * index) % 256 end
          return string.rep(string.char(sum), 32)
        end,
      }
    end }
  end
end
local openssl_digest = require("openssl.digest")
local grafikr = require("grafikr")

local function read(path)
  local file = assert(io.open(path, "rb"))
  local value = file:read("*a")
  assert(file:close())
  return value
end

local function write(path, value)
  local file = assert(io.open(path, "wb"))
  assert(file:write(value))
  assert(file:close())
end

local function join(a, b)
  return a:gsub("[/\\]+$", "") .. package.config:sub(1, 1) .. b
end

local function mkdir(path)
  assert(uv.fs_mkdir(path, 448))
end

local function remove_tree(path)
  local stat = uv.fs_lstat(path)
  if not stat then return end
  if stat.type ~= "directory" then
    assert(uv.fs_unlink(path))
    return
  end
  local scan = assert(uv.fs_scandir(path))
  while true do
    local name = uv.fs_scandir_next(scan)
    if not name then break end
    remove_tree(join(path, name))
  end
  assert(uv.fs_rmdir(path))
end

local function sha256(value)
  local digest = openssl_digest.new("sha256")
  assert(digest:update(value))
  return (digest:final():gsub(".", function(byte) return string.format("%02x", string.byte(byte)) end))
end

local function fixture(root, relative, content, digest, record_path)
  mkdir(root)
  local file_path = join(root, relative)
  local parent = file_path:match("^(.*)[/\\][^/\\]+$")
  mkdir(parent)
  write(file_path, content)
  local target = assert(os.getenv("GRAFIKR_RUNTIME_DIR"), "runtime bundle is required")
  local metadata = assert(json.decode(read(join(target, "manifest.json"))))
  metadata.engine_version = "9.9.9"
  metadata.build_sha = string.rep("a", 40)
  metadata.build_config_sha256 = string.rep("b", 64)
  metadata.source_dirty = false
  metadata.development_fixture = false
  metadata.files = {{ path=record_path or relative, size=#content, sha256=digest, role="host" }}
  write(join(root, "manifest.json"), json.encode(metadata))
  return metadata
end

local parent = assert(uv.fs_mkdtemp(join(uv.os_tmpdir(), "grafikr-lua-runtime-XXXXXX")))
local ok, message = pcall(function()
  local bundle = join(parent, "bundle")
  local relative = "bin/grafikr-host" .. (package.config:sub(1, 1) == "\\" and ".exe" or "")
  local contents = "Grafikr test host\n"
  fixture(bundle, relative, contents, sha256(contents))
  local bundle_manifest = assert(json.decode(read(join(bundle, "manifest.json"))))

  local cache = join(parent, "cache")
  mkdir(cache)
  local host, err = grafikr.runtime.install(bundle, cache)
  assert(host, err and (err.code .. ": " .. err.message))
  assert(read(host) == contents)
  local again, second_error = grafikr.runtime.install(bundle, cache)
  assert(again == host, second_error and second_error.message)
  local installed_root = join(join(join(join(cache, bundle_manifest.target), "9.9.9"), string.rep("a", 40)), "manifest.json")
  local installed_manifest_path = installed_root
  local installed_manifest = assert(json.decode(read(installed_manifest_path)))
  installed_manifest.source_dirty = true
  write(installed_manifest_path, json.encode(installed_manifest))
  local substituted, cache_error = grafikr.runtime.install(bundle, cache)
  assert(not substituted and cache_error.code == "runtime_cache", "installer accepted a substituted cached manifest")

  local traversal = join(parent, "traversal")
  fixture(traversal, relative, contents, sha256(contents), "../escape")
  local rejected, traversal_error = grafikr.runtime.install(traversal, join(parent, "unsafe-cache"))
  assert(not rejected and traversal_error.code == "runtime_path", "installer accepted traversal path")

  local tampered = join(parent, "tampered")
  fixture(tampered, relative, contents, string.rep("0", 64))
  local tampered_manifest = assert(json.decode(read(join(tampered, "manifest.json"))))
  local corrupt, hash_error = grafikr.runtime.install(tampered, join(parent, "tampered-cache"))
  assert(not corrupt and hash_error.code == "runtime_hash", "installer accepted a wrong file hash")
  local cache = join(parent, "tampered-cache")
  assert(not uv.fs_lstat(join(join(join(join(cache, tampered_manifest.target), "9.9.9"), string.rep("a", 40)), "bin")), "failed install left a partial cache")
end)
remove_tree(parent)
assert(ok, message)
print("Grafikr Lua runtime installer checks passed")
