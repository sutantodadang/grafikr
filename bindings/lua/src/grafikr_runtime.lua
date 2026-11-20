local uv = require("luv")
local json = require("dkjson")

local M = {}
local SEPARATOR = package.config:sub(1, 1)
local MAX_MANIFEST_BYTES = 1024 * 1024
local READ_CHUNK_BYTES = 1024 * 1024

local function abort(code, message)
  error({ code=code, message=tostring(message) }, 0)
end

local function join(left, right)
  return left:gsub("[/\\]+$", "") .. SEPARATOR .. right
end

local function absolute(path)
  if type(path) ~= "string" or path == "" then return false end
  if path:sub(1, 1) == "/" or path:sub(1, 1) == "\\" then return true end
  return path:match("^%a:[/\\]") ~= nil
end

local function stat(path, code)
  local value, err = uv.fs_lstat(path)
  if not value then abort(code, err or ("path does not exist: " .. path)) end
  return value
end

local function require_directory(path, code)
  local value = stat(path, code)
  if value.type ~= "directory" then abort(code, "path is not a regular directory: " .. path) end
  return value
end

local function mkdir_p(path)
  if not absolute(path) then abort("runtime_path", "cache path must be absolute") end
  local normalized = path:gsub("\\", "/")
  if normalized:sub(1, 2) == "//" then abort("runtime_path", "UNC cache paths are not supported") end
  local current, rest
  if normalized:match("^%a:/") then
    current, rest = normalized:sub(1, 3), normalized:sub(4)
  elseif normalized:sub(1, 1) == "/" then
    current, rest = "/", normalized:sub(2)
  else
    abort("runtime_path", "cache path must be absolute")
  end
  for part in rest:gmatch("[^/]+") do
    if part == "." or part == ".." or part:find(":", 1, true) then
      abort("runtime_path", "cache path contains an unsafe component")
    end
    current = join(current, part)
    local existing = uv.fs_lstat(current)
    if existing then
      if existing.type ~= "directory" then abort("runtime_path", "cache path contains a non-directory component") end
    else
      local created, err = uv.fs_mkdir(current, 448)
      if not created then
        existing = uv.fs_lstat(current)
        if not existing or existing.type ~= "directory" then abort("runtime_cache", err or "could not create cache directory") end
      end
    end
  end
  return current
end

local function target_for_host()
  local info = uv.os_uname()
  local system = tostring(info.sysname or ""):lower()
  local machine = tostring(info.machine or ""):lower()
  local platform
  if system:find("windows", 1, true) or system:find("mingw", 1, true) then
    platform = "windows"
  elseif system == "linux" then
    platform = "linux"
  elseif system == "darwin" then
    platform = "apple"
  end
  local arch
  if machine == "x86_64" or machine == "amd64" or machine == "x64" then
    arch = "x86_64"
  elseif machine == "aarch64" or machine == "arm64" then
    arch = "aarch64"
  end
  if platform == "windows" and arch then return arch .. "-pc-windows-msvc" end
  if platform == "linux" and arch then return arch .. "-unknown-linux-gnu" end
  if platform == "apple" and arch then return arch .. "-apple-darwin" end
  abort("runtime_target", "Grafikr has no runtime for this platform and architecture")
end

local function safe_relative(value)
  if type(value) ~= "string" or value == "" or #value > 240
    or value:sub(1, 1) == "/" or value:sub(1, 1) == "\\"
    or value:find("\\", 1, true) or value:find("//", 1, true)
    or value:match("^%a:") or value:find(":", 1, true)
    or value:find("%c") then
    abort("runtime_path", "runtime manifest contains an unsafe relative path: " .. tostring(value))
  end
  for component in value:gmatch("[^/]+") do
    if component == "" or component == "." or component == ".." then
      abort("runtime_path", "runtime manifest contains an unsafe relative path: " .. value)
    end
  end
  if value:sub(-1) == "/" then abort("runtime_path", "runtime manifest contains a directory path") end
  return value:gsub("/", SEPARATOR)
end

local function hex(raw)
  return (raw:gsub(".", function(byte) return string.format("%02x", string.byte(byte)) end))
end

local function hash_file(path, expected_size, code)
  local digest = require("openssl.digest")
  local fd, err = uv.fs_open(path, "r", 0)
  if not fd then abort(code, err or ("could not open runtime file: " .. path)) end
  local ok, result, opened = pcall(function()
    local info, stat_error = uv.fs_fstat(fd)
    if not info then abort(code, stat_error or "could not inspect runtime file") end
    if info.type ~= "file" or (info.nlink and info.nlink > 1) then
      abort(code, "runtime file is not a single-link regular file: " .. path)
    end
    if expected_size ~= nil and info.size ~= expected_size then
      abort(code, "runtime file size does not match manifest: " .. path)
    end
    local hasher = digest.new("sha256")
    local offset = 0
    while offset < info.size do
      local chunk, read_error = uv.fs_read(fd, math.min(READ_CHUNK_BYTES, info.size - offset), offset)
      if not chunk or #chunk == 0 then abort(code, read_error or "runtime file ended before its declared size") end
      assert(hasher:update(chunk))
      offset = offset + #chunk
    end
    return hex(hasher:final()), info
  end)
  uv.fs_close(fd)
  if not ok then error(result, 0) end
  return result, opened
end

local function read_manifest(root)
  local root_info = stat(root, "runtime_manifest")
  if root_info.type ~= "directory" then abort("runtime_manifest", "runtime bundle root is not a directory") end
  local manifest_path = join(root, "manifest.json")
  local info = stat(manifest_path, "runtime_manifest")
  if info.type ~= "file" or (info.nlink and info.nlink > 1) or info.size > MAX_MANIFEST_BYTES then
    abort("runtime_manifest", "runtime manifest is unsafe or too large")
  end
  local fd = assert(uv.fs_open(manifest_path, "r", 0))
  local opened = uv.fs_fstat(fd)
  if not opened or opened.type ~= "file" or (opened.nlink and opened.nlink > 1) or opened.size ~= info.size then
    uv.fs_close(fd)
    abort("runtime_manifest", "runtime manifest changed while opening")
  end
  local contents, err = uv.fs_read(fd, info.size, 0)
  uv.fs_close(fd)
  if not contents or err then abort("runtime_manifest", err or "could not read runtime manifest") end
  local decoded, manifest, _, parse_error = pcall(json.decode, contents)
  if not decoded or type(manifest) ~= "table" then
    abort("runtime_manifest", parse_error or manifest or "runtime manifest JSON is invalid")
  end
  return manifest, contents
end

local function validate_bundle(root, expected_target)
  local manifest, manifest_data = read_manifest(root)
  if manifest.format_version ~= 1 or type(manifest.engine_version) ~= "string"
    or not manifest.engine_version:match("^%d+%.%d+%.%d+$")
    or type(manifest.build_sha) ~= "string" or #manifest.build_sha ~= 40
    or not manifest.build_sha:match("^[0-9a-f]+$")
    or type(manifest.build_config_sha256) ~= "string" or #manifest.build_config_sha256 ~= 64
    or not manifest.build_config_sha256:match("^[0-9a-f]+$")
    or manifest.target ~= expected_target or manifest.development_fixture ~= false
    or type(manifest.protocol) ~= "table" or manifest.protocol.major ~= 1
    or type(manifest.abi) ~= "table" or manifest.abi.major ~= 1
    or type(manifest.files) ~= "table" then
    abort("runtime_manifest", "runtime target or release metadata is invalid")
  end

  local count, host, seen = 0, nil, {}
  for key in pairs(manifest.files) do
    if type(key) ~= "number" or key < 1 or key % 1 ~= 0 then abort("runtime_manifest", "runtime files must be an array") end
    count = count + 1
  end
  if count == 0 or count > 256 then abort("runtime_manifest", "runtime file inventory is invalid") end
  for index = 1, count do
    local item = manifest.files[index]
    if type(item) ~= "table" or type(item.role) ~= "string"
      or type(item.size) ~= "number" or item.size < 0 or item.size % 1 ~= 0
      or type(item.sha256) ~= "string" or #item.sha256 ~= 64
      or not item.sha256:match("^[0-9a-f]+$") then
      abort("runtime_manifest", "runtime file record is invalid")
    end
    local relative = safe_relative(item.path)
    if seen[relative] or relative == "manifest.json" then abort("runtime_path", "runtime manifest contains a duplicate or reserved path") end
    seen[relative] = true
    if item.role == "host" then
      if host then abort("runtime_manifest", "runtime manifest contains multiple host executables") end
      host = relative
    end
    local current = root
    local parts = {}
    for part in item.path:gmatch("[^/]+") do parts[#parts + 1] = part end
    for i, part in ipairs(parts) do
      current = join(current, part)
      local file_info = stat(current, "runtime_file")
      if i < #parts then
        if file_info.type ~= "directory" then abort("runtime_file", "runtime path has a non-directory parent") end
      else
        if file_info.type ~= "file" or (file_info.nlink and file_info.nlink > 1) then
          abort("runtime_file", "runtime file is not a regular single-link file: " .. item.path)
        end
        local actual, opened = hash_file(current, item.size, "runtime_file")
        if opened.size ~= file_info.size or actual ~= item.sha256 then
          abort("runtime_hash", "runtime checksum does not match: " .. item.path)
        end
      end
    end
  end
  if count ~= #manifest.files or not host then abort("runtime_manifest", "runtime manifest has no host executable") end
  return { manifest=manifest, manifest_data=manifest_data, host=host }
end

local function remove_tree(path)
  local info = uv.fs_lstat(path)
  if not info then return end
  if info.type ~= "directory" then
    uv.fs_unlink(path)
    return
  end
  local scan = uv.fs_scandir(path)
  if not scan then return end
  while true do
    local name = uv.fs_scandir_next(scan)
    if not name then break end
    remove_tree(join(path, name))
  end
  uv.fs_rmdir(path)
end

local function make_parent(root, relative)
  local parent = relative:match("^(.*)[/\\][^/\\]+$")
  if not parent then return end
  local current = root
  for component in parent:gmatch("[^/\\]+") do
    current = join(current, component)
    local info = uv.fs_lstat(current)
    if info then
      if info.type ~= "directory" then abort("runtime_cache", "staging path contains a non-directory") end
    else
      local made, err = uv.fs_mkdir(current, 448)
      if not made then abort("runtime_cache", err or "could not create staging directory") end
    end
  end
end

local function write_file(path, contents)
  local fd, err = uv.fs_open(path, "w", 384)
  if not fd then abort("runtime_cache", err or "could not create staged manifest") end
  local written, write_error = uv.fs_write(fd, contents, 0)
  uv.fs_close(fd)
  if not written or written ~= #contents then abort("runtime_cache", write_error or "could not write staged manifest") end
end

local function install(bundle_dir, cache_dir)
  if not absolute(bundle_dir) or not absolute(cache_dir) then abort("runtime_path", "bundle and cache paths must be absolute") end
  local target = target_for_host()
  local source = validate_bundle(bundle_dir, target)
  local cache = mkdir_p(cache_dir)
  local final_parent = mkdir_p(join(join(cache, target), source.manifest.engine_version))
  local final = join(final_parent, source.manifest.build_sha)
  local existing = uv.fs_lstat(final)
  if existing then
    if existing.type ~= "directory" then abort("runtime_cache", "existing runtime cache path is unsafe") end
    local installed = validate_bundle(final, target)
    if installed.manifest.build_sha ~= source.manifest.build_sha or installed.manifest_data ~= source.manifest_data then
      abort("runtime_cache", "cached runtime manifest does not match source bundle")
    end
    return join(final, installed.host)
  end

  local stage = join(final_parent, ".grafikr-runtime-" .. tostring(uv.os_getpid()) .. "-" .. tostring(uv.hrtime()))
  local made, mkdir_error = uv.fs_mkdir(stage, 448)
  if not made then abort("runtime_cache", mkdir_error or "could not create runtime staging directory") end
  local ok, result = pcall(function()
    for index = 1, #source.manifest.files do
      local item = source.manifest.files[index]
      local relative = safe_relative(item.path)
      make_parent(stage, relative)
      local source_path, destination = join(bundle_dir, relative), join(stage, relative)
      local copied, copy_error = uv.fs_copyfile(source_path, destination, { excl=true })
      if not copied then abort("runtime_copy", copy_error or "could not copy runtime file") end
      local permissions = stat(source_path, "runtime_file").mode
      if permissions and SEPARATOR ~= "\\" then
        local chmod_ok, chmod_error = uv.fs_chmod(destination, permissions & 511)
        if not chmod_ok then abort("runtime_permissions", chmod_error or "could not preserve runtime permissions") end
      end
    end
    write_file(join(stage, "manifest.json"), source.manifest_data)
    local staged = validate_bundle(stage, target)
    if staged.manifest.build_sha ~= source.manifest.build_sha then abort("runtime_manifest", "staged runtime build does not match source bundle") end
    local renamed, rename_error = uv.fs_rename(stage, final)
    if not renamed then
      local race = uv.fs_lstat(final)
      if race and race.type == "directory" then
        local winner = validate_bundle(final, target)
        if winner.manifest.build_sha == source.manifest.build_sha and winner.manifest_data == source.manifest_data then
          return join(final, winner.host)
        end
      end
      abort("runtime_cache", rename_error or "could not atomically install runtime bundle")
    end
    return join(final, staged.host)
  end)
  if not ok then
    remove_tree(stage)
    if type(result) == "table" then error(result, 0) end
    abort("runtime_install", result)
  end
  if uv.fs_lstat(stage) then remove_tree(stage) end
  return result
end

function M.install(bundle_dir, cache_dir)
  local ok, result = pcall(install, bundle_dir, cache_dir)
  if ok then return result end
  if type(result) == "table" then return nil, result end
  return nil, { code="runtime_install", message=tostring(result) }
end

return M
