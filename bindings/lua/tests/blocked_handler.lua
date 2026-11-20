local grafikr = require("grafikr")
local uv = require("luv")

local executable = package.config:sub(1, 1) == "\\" and "fixture-host.exe" or "fixture-host"
local host = os.getenv("GRAFIKR_FIXTURE_HOST") or ("../../target/debug/" .. executable)

local function verify_overflow(widget, properties, event, blocked_ms)
  local blocked, terminal_code, timed_out, app = false, nil, false, nil
  grafikr.start(host, function(client)
    app = client
    client:create_window("blocked callback", 320, 240, function(window, err)
      assert(window, err and err.message or "window creation failed")
      client:create_node(window, widget, properties, function(node, node_error)
        assert(node, node_error and node_error.message or "node creation failed")
        client:attach(node, window.root, function(attached)
          assert(attached.ok, attached.error and attached.error.message or "attach failed")
          client:on(node.handle, event, function()
            if blocked then return end
            blocked = true
            client:request("state.get", { state="0" }, function(reply)
              terminal_code = reply.error and reply.error.code
            end)
            uv.sleep(blocked_ms)
          end)
        end)
      end)
    end)
  end)

  local deadline = uv.new_timer()
  deadline:start(12000, 0, function()
    timed_out = true
    if app then app:close(function() end) end
    deadline:close()
  end)
  local monitor = uv.new_timer()
  monitor:start(10, 10, function()
    if app and app.closed then
      if not deadline:is_closing() then deadline:stop(); deadline:close() end
      monitor:stop(); monitor:close()
    end
  end)
  uv.run()
  assert(blocked, "fixture did not run the blocked handler")
  assert(not timed_out, "reader did not terminate the session after callback queue overflow")
  assert(app.closed, "blocked callback overflow did not fail the session")
  assert(terminal_code == "queue_full", "request got " .. tostring(terminal_code) .. "; app failure=" .. tostring(app.failure and app.failure.code) .. "; reader exit=" .. tostring(app.reader_exit_code))
end

verify_overflow("VirtualList", { row_count=1, row_height=24, height=120 }, "select", 6000)
verify_overflow("Canvas", { commands={}, label="bounded", width=120, height=80 }, "resize", 2500)
print("Lua blocked-handler count and byte quota checks passed")
