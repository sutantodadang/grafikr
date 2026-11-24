"""Install the indexed Lua source rock into a fresh LuaRocks tree and exercise it."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import tempfile
from pathlib import Path

from scripts.build_runtime import ROOT, _tool
from scripts.package_bindings import _package_path

LUA_INSTALLED_SCENARIOS = r'''
local uv = require("luv")
local sdk_path = assert(package.searchpath("grafikr", package.path))
local installed_root = assert(os.getenv("GRAFIKR_EXPECTED_LUA_ROOT")):gsub("\\", "/")
assert(sdk_path:gsub("\\", "/"):find(installed_root, 1, true), "Lua module did not load from the installed rock: " .. sdk_path .. " != " .. installed_root)
local grafikr = require("grafikr")
local function await(register)
  local done, value, err = false, nil, nil
  register(function(result, failure) value, err, done = result, failure, true end)
  while not done do uv.run("once") end
  if err then error(type(err) == "table" and (err.message or err.code or tostring(err)) or tostring(err)) end
  if type(value) == "table" and value.ok == false then error(value.error.message or value.error.code) end
  return value
end
local function request(app, op, args)
  local response = await(function(cb) app:request(op, args or {}, cb) end)
  if not response.ok then error(response.error.message or response.error.code) end
  return response.result
end
local function create_window(app, title, width, height)
  return await(function(cb) app:create_window(title, width, height, cb) end)
end
local function create_node(app, window, widget, properties)
  return await(function(cb) app:create_node(window, widget, properties, cb) end)
end
local function create_state(app, value)
  return await(function(cb) app:create_state(value, cb) end)
end
local function start(host)
  local app, ready = nil, false
  app = grafikr.start(host, function() ready = true end)
  while not ready do uv.run("once") end
  return app
end
local function close_app(app)
  local done = false
  app:close(function() done = true end)
  while not done do uv.run("once") end
end
local function base64(data)
  local alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
  local out = {}
  for i = 1, #data, 3 do
    local a, b, c = data:byte(i, i + 2)
    b, c = b or 0, c or 0
    local n = a * 65536 + b * 256 + c
    local x, y, z, w = math.floor(n / 262144) % 64, math.floor(n / 4096) % 64, math.floor(n / 64) % 64, n % 64
    out[#out + 1] = alphabet:sub(x + 1, x + 1) .. alphabet:sub(y + 1, y + 1)
    out[#out + 1] = i + 1 <= #data and alphabet:sub(z + 1, z + 1) or "="
    out[#out + 1] = i + 2 <= #data and alphabet:sub(w + 1, w + 1) or "="
  end
  return table.concat(out)
end

local host = assert(os.getenv("GRAFIKR_FIXTURE_HOST"))
do
  local app = start(host)
  local window = create_window(app, "Installed counter", 480, 320)
  local count = create_state(app, { type="i64", value="0" })
  local button = create_node(app, window, "Button", { label="Increment" })
  local label = create_node(app, window, "Text", { text="Count: 0" })
  local field = create_node(app, window, "TextField", { text="Grafikr 雪", width=240 })
  await(function(cb) app:attach(button, window.root, cb) end)
  await(function(cb) app:attach(label, window.root, cb) end)
  await(function(cb) app:attach(field, window.root, cb) end)
  local handled = false
  await(function(cb)
    app:on(button.handle, "click", function()
      app:state_get(count, function(got)
        local next_value = tonumber(got.result.value) + 1
        app:state_set(count, { type="i64", value=tostring(next_value) }, function(set)
          if not set.ok then error(set.error.message) end
          app:update_node(label, { text="Count: " .. next_value }, function(updated)
            if not updated.ok then error(updated.error.message) end
            handled = true
          end)
        end)
      end)
    end, cb)
  end)
  local timer = uv.new_timer(); timer:start(5000, 0, function() timer:stop(); timer:close(); error("counter click timed out") end)
  while not handled do uv.run("once") end
  timer:stop(); timer:close()
  local state = request(app, "state.get", { state=count.handle })
  assert(state.value == "1", "counter state did not update")
  await(function(cb) app:update_node(field, { text="Input 雪" }, cb) end)
  local subs = app.subscriptions
  for id in pairs(subs) do await(function(cb) app:unsubscribe(id, cb) end) end
  request(app, "state.destroy", { state=count.handle })
  request(app, "window.close", { window=window.handle })
  close_app(app)
end

do
  local app = start(host)
  local window = create_window(app, "Installed dashboard", 640, 480)
  local rows = create_node(app, window, "VirtualList", { row_count=50000, row_height=24, width=600, height=360 })
  await(function(cb) app:attach(rows, window.root, cb) end)
  local provider = await(function(cb) app:create_collection_provider(rows, { row_count=50000, row_height=24, columns={"label"} }, cb) end)
  local range = await(function(cb) provider:next_request(5000, cb) end)
  assert(range and range.start == 0 and range.count == 32, "collection range missing")
  local data = {}; for i = 1, range.count do data[i] = { "Row " .. (i - 1) } end
  await(function(cb) provider:provide(range, data, cb) end)
  local canvas = create_node(app, window, "Canvas", { commands={}, label="Dashboard canvas", width=320, height=120 })
  await(function(cb) app:attach(canvas, window.root, cb) end)
  request(app, "canvas.update", { node=canvas.handle, commands={{ kind="rect", rect={x=0,y=0,width=32,height=24}, color="#336699", radius=0 }} })
  local upload = request(app, "image.begin", { width=1, height=1, byte_length=4 }).upload
  request(app, "image.chunk", { upload=upload, offset=0, data=base64(string.char(0,0,0,255)) })
  local image = request(app, "image.finalize", { upload=upload }).image
  local image_node = create_node(app, window, "Image", { image=image, width=1, height=1, label="Pixel" })
  await(function(cb) app:attach(image_node, window.root, cb) end)
  request(app, "image.destroy", { image=image })
  request(app, "node.destroy", { node=image_node.handle })
  request(app, "node.destroy", { node=canvas.handle })
  await(function(cb) provider:close(cb) end)
  request(app, "node.destroy", { node=rows.handle })
  request(app, "window.close", { window=window.handle })
  close_app(app)
end

do
  local app = start(host)
  local first = create_window(app, "First", 320, 240)
  local second = create_window(app, "Second", 320, 240)
  local modal = create_node(app, second, "Modal", { open=true, width=240 })
  await(function(cb) app:attach(modal, second.root, cb) end)
  local message = create_node(app, second, "Text", { text="Confirm" })
  await(function(cb) app:attach(message, modal.handle, cb) end)
  local state = create_state(app, { type="i64", value="1" })
  request(app, "window.close", { window=first.handle })
  request(app, "state.set", { state=state.handle, value={ type="i64", value="2" } })
  assert(request(app, "state.get", { state=state.handle }).value == "2", "remaining window lost session state")
  request(app, "node.destroy", { node=message.handle }); request(app, "node.destroy", { node=modal.handle })
  request(app, "state.destroy", { state=state.handle }); request(app, "window.close", { window=second.handle })
  close_app(app)
end
print("installed Lua counter/form, dashboard, and multi-window passed")
'''


def _lua_native_extension(os_name: str) -> str:
    return ".dll" if os_name == "nt" else ".so"


def _indexed_artifact(package_root: Path) -> Path:
    index_path = package_root / "package-index.json"
    index = json.loads(index_path.read_text(encoding="utf-8"))
    if index.get("format") != "grafikr-bindings-package-index-v1":
        raise ValueError("unsupported local package index format")
    matches = [
        row for row in index.get("packages", [])
        if row.get("sdk") == "lua" and row.get("target") == "source"
    ]
    if len(matches) != 1:
        raise ValueError("package index must contain exactly one Lua source rock")
    artifact = package_root.joinpath(*_package_path(matches[0].get("path")).parts)
    if not artifact.is_file() or artifact.is_symlink():
        raise ValueError(f"Lua source rock is missing or unsafe: {artifact}")
    if hashlib.sha256(artifact.read_bytes()).hexdigest() != matches[0].get("sha256"):
        raise ValueError("Lua source rock checksum does not match its package index")
    return artifact


def verify(
    package_root: Path,
    fixture_host: Path | None = None,
    lua_executable: Path | None = None,
    modules_root: Path | None = None,
) -> None:
    fixture_host = fixture_host or Path(os.environ.get("GRAFIKR_FIXTURE_HOST", ""))
    lua_executable = lua_executable or Path(os.environ.get("GRAFIKR_LUA54", ""))
    modules_root = modules_root or Path(os.environ.get("GRAFIKR_LUA54_MODULES", ""))
    if not lua_executable.is_absolute() and not lua_executable.is_file():
        resolved_lua = shutil.which(str(lua_executable))
        if resolved_lua:
            lua_executable = Path(resolved_lua)
    package_root, fixture_host, lua_executable, modules_root = map(
        Path, (package_root.resolve(), fixture_host.resolve(), lua_executable.resolve(), modules_root.resolve())
    )
    for path, label in ((fixture_host, "fixture host"), (lua_executable, "Lua executable"), (modules_root, "Lua modules tree")):
        if not path.exists() or (label != "Lua modules tree" and not path.is_file()):
            raise ValueError(f"{label} is missing: {path}")
    rock = _indexed_artifact(package_root)
    (ROOT / "target").mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="grafikr-lua-consumer-", dir=ROOT / "target") as temporary:
        root = Path(temporary)
        install = root / "rocks"
        sibling_rocks = lua_executable.with_name("luarocks.exe")
        rocks = sibling_rocks if sibling_rocks.is_file() else Path(shutil.which("luarocks") or "luarocks")
        _tool(str(rocks), "--lua-version=5.4", "install", "--tree", str(install), str(rock), "--deps-mode=none")
        install_share = install / "share" / "lua" / "5.4"
        install_lib = install / "lib" / "lua" / "5.4"
        if not (install_share / "grafikr.lua").is_file():
            raise RuntimeError(f"LuaRocks did not install the SDK into the isolated tree: {install}")
        dep_share = modules_root / "share" / "lua" / "5.4"
        dep_lib = modules_root / "lib" / "lua" / "5.4"
        native_extension = _lua_native_extension(os.name)
        environment = os.environ.copy()
        environment.update({
            "LUA": str(lua_executable),
            "GRAFIKR_FIXTURE_HOST": str(fixture_host),
            "GRAFIKR_EXPECTED_LUA_ROOT": str(install_share),
            "GRAFIKR_FIXTURE_EARLY_SUBSCRIBE": "1",
            "GRAFIKR_FIXTURE_COLLECTION_REQUEST": "1",
            "LUA_PATH": ";".join((str(install_share / "?.lua"), str(install_share / "?/init.lua"), str(dep_share / "?.lua"), str(dep_share / "?/init.lua"), ";;")),
            "LUA_CPATH": ";".join((str(install_lib / f"?{native_extension}"), str(dep_lib / f"?{native_extension}"), ";;")),
            "PATH": os.pathsep.join((str(lua_executable.parent), str(dep_lib), environment.get("PATH", ""))),
        })
        scenario = root / "installed_scenarios.lua"
        scenario.write_text(LUA_INSTALLED_SCENARIOS, encoding="utf-8")
        previous = {key: os.environ.get(key) for key in environment}
        os.environ.update(environment)
        try:
            _tool(str(lua_executable), str(scenario), cwd=root)
        finally:
            for key, value in previous.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value
    print("Lua source rock installed into a clean tree and passed the fixture-backed installed scenarios")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-root", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        verify(args.package_root)
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        parser.exit(1, f"Lua package verification failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
