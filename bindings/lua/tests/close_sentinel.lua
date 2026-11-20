local grafikr = require("grafikr")
local uv = require("luv")
local host = assert(os.getenv("GRAFIKR_FIXTURE_HOST"), "fixture host required")
local windows = package.config:sub(1, 1) == "\\"
local sentinel_exe = windows and "ping.exe" or "sleep"
local sentinel_args = windows and { "-t", "127.0.0.1" } or { "30" }
local sentinel_exited, sentinel = false, nil
sentinel = uv.spawn(sentinel_exe, { args=sentinel_args, stdio={ 0, 1, 2 } }, function()
  sentinel_exited = true
end)
assert(sentinel, "failed to launch sentinel process")
local app = grafikr.start(host, function(client)
  client:close(function() client.closed_test=true end)
end)
while not app.closed_test do uv.run("once") end
assert(app.closed_test, "Grafikr app did not finish close")
assert(not sentinel_exited, "closing Grafikr killed the unrelated sentinel")
local killed, kill_error = sentinel:kill("sigterm")
assert(killed, tostring(kill_error or "failed to stop test sentinel"))
sentinel:close()
print("Grafikr Lua close sentinel check passed")
