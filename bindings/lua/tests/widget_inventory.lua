local grafikr = require("grafikr")
local uv = require("luv")
local json = require("dkjson")

local array = { __jsontype = "array" }
local function values(kind, property, type_name, state, image)
  if kind == "Radio" and property == "group" then return state end
  if kind == "TreeView" and property == "selected" then return "root" end
  if kind == "Slider" then
    if property == "min" then return 0 end
    if property == "max" then return 10 end
    if property == "width" then return 120 end
    if property == "step" then return 1 end
  end
  if kind == "Spinner" and property == "size" then return 16 end
  if (kind == "VirtualList" or kind == "DataGrid") and property == "row_count" then return 1 end
  if (kind == "VirtualList" or kind == "DataGrid") and property == "row_height" then return 24 end
  if type_name == "string" then return "sample" end
  if type_name == "boolean" then return false end
  if type_name == "u16" then return 1 end
  if type_name == "u64" then return 0 end
  if type_name == "number" or type_name == "f32" then return 1 end
  if type_name == "color" then return "#336699" end
  if type_name == "transform" then return { x=0, y=0, scale=1 } end
  if type_name == "primary|secondary|icon" then return "primary" end
  if type_name == "state" then return state end
  if type_name == "ImageHandle" then return image end
  if type_name == "string[]" then return setmetatable({ "One", "Two" }, array) end
  if type_name == "GridColumn[]" then return setmetatable({ { title="Column", width=120 } }, array) end
  if type_name == "TreeNode[]" then return setmetatable({ { key="root", label="Root", children=setmetatable({}, array) } }, array) end
  if type_name == "CanvasCommand[]" then
    return setmetatable({ { kind="rect", rect={ x=0, y=0, width=1, height=1 }, color="#336699", radius=0 } }, array)
  end
  error("No Lua conformance value for " .. kind .. "." .. property .. ": " .. type_name)
end

local inventory_file = assert(io.open("../../bindings/protocol/v1/widgets.json"))
local inventory = assert(json.decode(inventory_file:read("*a"))).widgets
inventory_file:close()
local host = assert(os.getenv("GRAFIKR_FIXTURE_HOST"), "fixture host required")
local app = grafikr.start(host)
local index, kinds, current_kind = 0, {}, "setup"
for kind in pairs(inventory) do kinds[#kinds + 1] = kind end
table.sort(kinds)

local function fail(err) error(current_kind .. ": " .. tostring(err and (err.code .. ": " .. err.message) or "widget inventory failed")) end
local function finish(image)
  app:request("image.destroy", { image=image }, function(reply)
    if not reply.ok then fail(reply.error) end
    app:close()
  end)
end

local function next_widget(window, state, image)
  index = index + 1
  local kind = kinds[index]
  if not kind then return finish(image) end
  current_kind = kind
  local spec, properties = inventory[kind], {}
  for name, type_name in pairs(inventory[kind].properties) do
    properties[name] = values(kind, name, type_name, state, image)
  end
  app:create_node(window, kind, properties, function(node, create_error)
    if not node then return fail(create_error) end
    app:attach(node, window.root, function(attached)
      if not attached.ok then return fail(attached.error) end
      local events, event_index = spec.events or {}, 0
      local function destroy_node()
        app:request("node.destroy", { node=node.handle }, function(reply)
          if not reply.ok then return fail(reply.error) end
          next_widget(window, state, image)
        end)
      end
      local function next_event()
        event_index = event_index + 1
        local event = events[event_index]
        if not event then return destroy_node() end
        local seen, subscribed, subscription, closed = false, false, nil, false
        local function complete()
          if seen and subscribed and not closed then
            closed = true
            subscription:close(function(err)
              if err then return fail(err) end
              next_event()
            end)
          end
        end
        app:on(node.handle, event, function(message)
          assert(message.source == node.handle and message.event == event, kind .. "@" .. event .. " mismatch")
          seen = true
          complete()
        end, function(value, err)
          if not value then return fail(err) end
          subscription, subscribed = value, true
          complete()
        end)
      end
      next_event()
    end)
  end)
end

app:create_window("widget inventory", 640, 480, function(window, err)
  if not window then return fail(err) end
  app:create_state({ type="i64", value="1" }, function(state, state_error)
    if not state then return fail(state_error) end
    app:request("image.begin", { width=1, height=1, byte_length=4 }, function(begin)
      if not begin.ok then return fail(begin.error) end
      local upload = begin.result.upload
      app:request("image.chunk", { upload=upload, offset=0, data="AAAA/w==" }, function(chunk)
        if not chunk.ok then return fail(chunk.error) end
        app:request("image.finalize", { upload=upload }, function(finalized)
          if not finalized.ok then return fail(finalized.error) end
          next_widget(window, state.handle, finalized.result.image)
        end)
      end)
    end)
  end)
end)
uv.run()
print("Grafikr Lua widget inventory passed")
