package.preload["luv"] = function() return {} end
package.preload["dkjson"] = function() return { encode=function(value) return string.rep("x", #(value.args.payload or "")) end } end
local grafikr = require("grafikr")

local writes = 0
local app = setmetatable({
  next_id=0, pending={}, pending_count=0, handlers={}, subscriptions={},
  buffer="", closed=false, pending_bytes=0,
  stdin={ write=function() writes=writes+1 end },
}, grafikr.App)

for _, value in ipairs({ 0/0, math.huge, -math.huge }) do
  local callback_error
  local id, err = app:request("state.create", { value={ type="number", value=value } }, function(reply)
    callback_error = reply.error
  end)
  assert(id == nil and err.code == "invalid_value")
  assert(callback_error.code == "invalid_value")
end
assert(writes == 0 and app.pending_count == 0, "invalid number reached the host")
assert(app:request("state.create", { value={ type="number", value=1.5 } }) == "1")
assert(writes == 1 and app.pending_count == 1)

grafikr.MAX_PENDING_BYTES = 12
local byte_writes = 0
local bytes = setmetatable({
  next_id=0, pending={}, pending_count=0, pending_bytes=0, handlers={},
  subscriptions={}, buffer="", closed=false,
  stdin={ write=function() byte_writes=byte_writes+1 end },
}, grafikr.App)
assert(bytes:request("test", { payload="12345" }) == "1")
assert(bytes:request("test", { payload="12345" }) == "2")
local overflow_id, overflow_error = bytes:request("test", { payload="12345" })
assert(overflow_id == nil and overflow_error.code == "queue_full")
assert(byte_writes == 2 and bytes.pending_bytes == 12 and bytes.pending_count == 2)
bytes:_fail({ code="connection_closed", message="test" })
assert(bytes.pending_bytes == 0 and bytes.pending_count == 0)
print("Grafikr Lua request validation checks passed")
