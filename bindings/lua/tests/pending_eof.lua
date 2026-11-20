local grafikr = require("grafikr")
local uv = require("luv")
local host = assert(os.getenv("GRAFIKR_FIXTURE_HOST"), "fixture host required")
local app = grafikr.start(host, function(client)
  local closed, queue_full = 0, 0
  for _ = 1, grafikr.MAX_OUTSTANDING_REQUESTS do
    local id, err = client:request("state.get", { state="0" }, function(reply)
      if reply.error and reply.error.code == "connection_closed" then closed = closed + 1 end
    end)
    assert(id and not err, "request below cap was rejected")
  end
  local id, err = client:request("state.get", { state="0" }, function(reply)
    if reply.error and reply.error.code == "queue_full" then queue_full = queue_full + 1 end
  end)
  assert(id == nil and err.code == "queue_full", "257th request was not rejected")
  client.exit_callback = function()
    assert(closed == grafikr.MAX_OUTSTANDING_REQUESTS, "EOF did not reject every pending request")
    assert(queue_full == 1 and client.pending_count == 0)
    client.closed_test = true
  end
end)
uv.run()
assert(app.closed_test, "fixture host did not exit with pending requests")
print("Grafikr Lua pending EOF check passed")
