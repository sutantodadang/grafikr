local grafikr = require("grafikr")
local uv = require("luv")
local host = assert(os.getenv("GRAFIKR_FIXTURE_HOST"), "fixture host required")
local app = grafikr.start(host)
local complete, received = false, 0

local function start_reads(first, second)
  local function finish()
    received = received + 1
    if received == 2 then app:close(function() complete=true end) end
  end
  app:state_get(first, function(reply)
    assert(reply.ok and reply.result.value == "first", "first reply was miscorrelated")
    finish()
  end)
  app:state_get(second, function(reply)
    assert(reply.ok and reply.result.value == "second", "second reply was miscorrelated")
    finish()
  end)
end

app:create_state({ type="string", value="first" }, function(first, first_error)
  assert(first, first_error and first_error.message or "first state create failed")
  app:create_state({ type="string", value="second" }, function(second, second_error)
    assert(second, second_error and second_error.message or "second state create failed")
    start_reads(first, second)
  end)
end)
uv.run()
assert(complete and received == 2, "out-of-order replies did not complete")
print("Grafikr Lua out-of-order response check passed")
