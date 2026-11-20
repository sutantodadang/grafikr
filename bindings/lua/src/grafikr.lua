local uv = require("luv")
local json = require("dkjson")
local sigpipe

if uv.constants and uv.constants.SIGPIPE then
  sigpipe = uv.new_signal()
  sigpipe:start("sigpipe", function() end)
  sigpipe:unref()
end

local M = { MAX_FRAME = 8 * 1024 * 1024, MAX_PENDING_BYTES = 16 * 1024 * 1024, MAX_OUTSTANDING_REQUESTS = 256 }
local App = {}; App.__index = App
local Subscription = {}; Subscription.__index = Subscription
local CollectionProvider = {}; CollectionProvider.__index = CollectionProvider

local function close(handle)
  if handle and type(handle.close) == "function" and (type(handle.is_closing) ~= "function" or not handle:is_closing()) then handle:close() end
end
local function has_nonfinite(value, seen, depth)
  if type(value) == "number" then return value ~= value or value == math.huge or value == -math.huge end
  if type(value) ~= "table" then return false end
  if depth >= 64 or seen[value] then return true end
  seen[value] = true
  for key, item in pairs(value) do
    if type(key) == "number" and (key ~= key or key == math.huge or key == -math.huge) then return true end
    if has_nonfinite(item, seen, depth + 1) then return true end
  end
  seen[value] = nil
  return false
end

function M.start(host, on_ready)
  assert(type(host) == "string" and host:match("^[/\\]") or host:match("^%a:[/\\]"), "host path must be absolute")
  local self = setmetatable({ next_id = 0, pending = {}, pending_count = 0, pending_bytes = 0, handlers = {}, subscriptions = {}, buffer = "", closed = false, closing = false, host_exited = false, reader_exited = false, relay_eof = false }, App)
  self.stdin, self.host_stdout, self.stderr = uv.new_pipe(false), uv.new_pipe(false), uv.new_pipe(false)
  self.reader_stdin, self.stdout, self.reader_stderr = uv.new_pipe(false), uv.new_pipe(false), uv.new_pipe(false)
  local reader_script = os.getenv("GRAFIKR_LUA_READER") or package.searchpath("grafikr_reader", package.path)
  assert(reader_script, "Grafikr reader helper is missing from the Lua package")
  if not reader_script:match("^[/\\]") and not reader_script:match("^%a:[/\\]") then
    reader_script = uv.cwd() .. package.config:sub(1, 1) .. reader_script
  end
  local lua = (arg and arg[-1]) or os.getenv("LUA") or "lua"
  self.process, self.pid = uv.spawn(host, { args = {}, stdio = { self.stdin, self.host_stdout, self.stderr } }, function(code, signal)
    self:_host_exit(code, signal)
  end)
  assert(self.process, "failed to start Grafikr host")
  self.reader, self.reader_pid = uv.spawn(lua, { args = { reader_script }, stdio = { self.reader_stdin, self.stdout, self.reader_stderr, self.host_stdout }, hide = true }, function(code, signal)
    self:_reader_exit(code, signal)
  end)
  if not self.reader then
    pcall(function() self.process:kill("sigterm") end)
    close(self.stdin); close(self.host_stdout); close(self.stderr); close(self.process)
    close(self.reader_stdin); close(self.stdout); close(self.reader_stderr)
    error("failed to start Grafikr frame reader")
  end
  close(self.host_stdout)
  self.stdout:read_start(function(err, chunk)
    if err then self:_fail(err); return end
    if chunk then self:_consume(chunk) else self:_relay_eof() end
  end)
  self.stderr:read_start(function(_, chunk) if not chunk then self.stderr:read_stop() end end)
  self.reader_stderr:read_start(function(_, chunk) if not chunk then self.reader_stderr:read_stop() end end)
    self:request("hello", { protocol_major=1, protocol_minor=0, sdk_version="1.1.0" }, function(reply)
    if not reply.ok then self:_fail(reply.error); return end
    if reply.result.protocol.major ~= 1 then self:_fail({ code="version_mismatch", message="Protocol 1 required" }); return end
    if on_ready then on_ready(self) end
  end)
  return self
end

function App:request(op, args, callback)
  if self.closed then return nil, { code="connection_closed", message="Grafikr app is closed" } end
  if has_nonfinite(args, {}, 0) then
    local error = { code="invalid_value", message="Request contains a non-finite number, cycle, or excessive JSON depth" }
    if callback then callback({ ok=false, error=error }) end
    return nil, error
  end
  if self.pending_count >= M.MAX_OUTSTANDING_REQUESTS then
    local error = { code="queue_full", message="Outstanding request count quota reached" }
    if callback then callback({ ok=false, error=error }) end
    return nil, error
  end
  self.next_id = self.next_id + 1
  local id = tostring(self.next_id)
  local encoded, payload = pcall(json.encode, { id=id, op=op, args=args or {} })
  if not encoded then
    local error = { code="invalid_value", message="Request cannot be encoded as JSON" }
    if callback then callback({ ok=false, error=error }) end
    return nil, error
  end
  local frame = payload .. "\n"
  if #frame > M.MAX_FRAME then
    local error = { code="frame_too_large", message="Request exceeds the 8 MiB frame limit" }
    if callback then callback({ ok=false, error=error }) end
    return nil, error
  end
  if self.pending_bytes + #frame > M.MAX_PENDING_BYTES then
    local error = { code="queue_full", message="Outstanding request byte quota reached" }
    if callback then callback({ ok=false, error=error }) end
    return nil, error
  end
  self.pending[id] = { callback = callback, bytes = #frame }
  self.pending_count = self.pending_count + 1
  self.pending_bytes = self.pending_bytes + #frame
  self.stdin:write(frame, function(err) if err then self.stdin_error = err end end)
  return id
end

function App:on(node, event, callback, on_subscribed)
  local key = tostring(node) .. "\0" .. event
  local pending = "pending:" .. tostring(self.next_id + 1)
  self.handlers[key] = self.handlers[key] or {}; self.handlers[key][pending] = callback
  local request_id, request_error = self:request("event.subscribe", { node=node, event=event }, function(reply)
    if not reply.ok then
      self.handlers[key][pending] = nil
      if on_subscribed then on_subscribed(nil, reply.error) elseif type(callback) == "table" and callback.onerror then callback.onerror(reply.error) end
      return
    end
    local id = reply.result.subscription
    self.subscriptions[id] = { key=key, callback=callback }
    self.handlers[key][id] = self.handlers[key][pending]; self.handlers[key][pending] = nil
    if on_subscribed then on_subscribed(setmetatable({ app=self, id=id, key=key, closed=false }, Subscription), nil) end
  end)
  return request_id, request_error
end

function App:unsubscribe(id, callback)
  local sub = self.subscriptions[id]
  if sub then self.handlers[sub.key][id] = nil; self.subscriptions[id] = nil end
  self:request("event.unsubscribe", { subscription=id }, function(reply)
    if not reply.ok then if callback then callback(reply.error) end; return end
    if callback then callback(nil) end
  end)
end

function App:on_error(callback)
  assert(type(callback) == "function", "error callback is required")
  self.error_callback = callback
end

function Subscription:close(callback)
  if self.closed then if callback then callback(nil) end; return end
  self.closed = true
  self.app:unsubscribe(self.id, callback)
end

function App:create_window(title, width, height, callback)
  self:request("window.create", { title=title, width=width, height=height }, function(r) if callback then callback(r.ok and { app=self, handle=r.result.window, root=r.result.root } or nil, r.error) end end)
end
function App:create_node(window, widget, properties, callback)
  self:request("node.create", { window=window.handle, widget=widget, properties=properties or {} }, function(r) if callback then callback(r.ok and { app=self, handle=r.result.node } or nil, r.error) end end)
end
function App:create_state(value, callback)
  self:request("state.create", { value=value }, function(r) if callback then callback(r.ok and { app=self, handle=r.result.state } or nil, r.error) end end)
end
local function validate_collection_metadata(metadata)
  if type(metadata) ~= "table" or type(metadata.row_count) ~= "number" or metadata.row_count < 0 or metadata.row_count > 4294967296 or metadata.row_count % 1 ~= 0 or
      type(metadata.row_height) ~= "number" or metadata.row_height ~= metadata.row_height or metadata.row_height == math.huge or metadata.row_height == -math.huge or metadata.row_height <= 0 or
      type(metadata.columns) ~= "table" or #metadata.columns < 1 or #metadata.columns > 256 then
    return nil, { code="invalid_value", message="Collection metadata is invalid" }
  end
  for _, column in ipairs(metadata.columns) do if type(column) ~= "string" then return nil, { code="invalid_value", message="Collection columns must be strings" } end end
  return true
end

function App:create_collection_provider(node, metadata, callback)
  if type(node) ~= "table" or type(node.handle) ~= "string" then return nil, { code="invalid_handle", message="Collection node is invalid" } end
  local valid, validation_error = validate_collection_metadata(metadata)
  if not valid then return nil, validation_error end
  local provider = setmetatable({ app=self, node=node, requests={}, closed=false, disposed=false }, CollectionProvider)
  local _, subscribe_request_error = self:on(node.handle, "data.request", function(event) provider:_receive(event) end, function(subscription, subscribe_error)
    if not subscription then provider.closed=true; provider.disposed=true; if callback then callback(nil, subscribe_error) end; return end
    provider.subscription = subscription
    local args = { node=node.handle, row_count=metadata.row_count, row_height=metadata.row_height, columns=metadata.columns }
    self:request("collection.create", args, function(reply)
      if not reply.ok then provider:close(); if callback then callback(nil, reply.error) end; return end
      if callback then callback(provider, nil) end
    end)
  end)
  if subscribe_request_error then provider.closed=true; provider.disposed=true; return nil, subscribe_request_error end
  return provider
end

function CollectionProvider:_receive(event)
  if self.closed then return end
  local payload = event and event.payload
  local request = type(payload) == "table" and payload or nil
  if not request or type(request.revision) ~= "string" or type(request.request) ~= "string" or
      type(request.start) ~= "number" or request.start < 0 or request.start > 4294967295 or request.start % 1 ~= 0 or
      type(request.count) ~= "number" or request.count < 0 or request.count > 512 or request.count % 1 ~= 0 then
    self.failure = { code="invalid_frame", message="Collection request fields are invalid" }
    if self.waiter then local waiter=self.waiter; self.waiter=nil; waiter.timer:stop(); close(waiter.timer); waiter.callback(nil,self.failure) end
    return
  end
  if self.waiter then
    local waiter=self.waiter; self.waiter=nil; waiter.timer:stop(); close(waiter.timer); waiter.callback(request,nil)
  elseif #self.requests < 1 then
    self.requests[1]=request
  else
    self.failure = { code="queue_full", message="Collection provider request queue is full" }
  end
end

function CollectionProvider:next_request(timeout_ms, callback)
  if type(timeout_ms) == "function" then callback, timeout_ms = timeout_ms, 30000 end
  timeout_ms = timeout_ms or 30000
  assert(type(callback) == "function", "next_request callback is required")
  if self.requests[1] then local request=table.remove(self.requests,1); callback(request,nil); return end
  if self.failure then local error=self.failure; self.failure=nil; callback(nil,error); return end
  if self.closed then callback(nil,nil); return end
  if self.waiter then callback(nil,{ code="invalid_lifecycle", message="Only one collection reader is allowed" }); return end
  if type(timeout_ms) ~= "number" or timeout_ms < 0 or timeout_ms % 1 ~= 0 then callback(nil,{ code="invalid_value", message="Collection timeout must be nonnegative milliseconds" }); return end
  local waiter={ timer=uv.new_timer(), callback=callback }; self.waiter=waiter
  waiter.timer:start(timeout_ms,0,function()
    if self.waiter == waiter then self.waiter=nil; close(waiter.timer); callback(nil,nil) end
  end)
end

function CollectionProvider:provide(request, rows, callback)
  if self.disposed then local error={ code="connection_closed", message="Collection provider is closed" }; if callback then callback(nil,error) end; return nil,error end
  return self.app:request("data.provide",{ node=self.node.handle, revision=request.revision, request=request.request, start=request.start, rows=rows },callback)
end

function CollectionProvider:update(metadata, callback)
  if self.disposed then local error={ code="connection_closed", message="Collection provider is closed" }; if callback then callback(nil,error) end; return nil,error end
  local valid, validation_error=validate_collection_metadata(metadata)
  if not valid then if callback then callback(nil,validation_error) end; return nil,validation_error end
  return self.app:request("collection.update",{ node=self.node.handle, metadata=metadata },callback)
end

function CollectionProvider:close(callback)
  if self.disposed then if callback then callback(nil) end; return end
  self.disposed=true; self.closed=true; self.requests={}; self.failure=nil
  if self.waiter then local waiter=self.waiter; self.waiter=nil; waiter.timer:stop(); close(waiter.timer); waiter.callback(nil,nil) end
  if self.subscription then self.subscription:close(callback) elseif callback then callback(nil) end
end

function App:state_get(state, callback) self:request("state.get", { state=state.handle }, callback) end
function App:state_set(state, value, callback) self:request("state.set", { state=state.handle, value=value }, callback) end
function App:state_destroy(state, callback) self:request("state.destroy", { state=state.handle }, callback) end
function App:update_node(node, properties, callback) self:request("property.update", { node=node.handle, properties=properties }, callback) end
function App:attach(node, parent, callback) self:request("node.attach", { node=node.handle, parent=parent }, callback) end

function App:close(callback)
  if self.finalized then
    if callback then callback() end
    return
  end
  if callback then self.close_callbacks = self.close_callbacks or {}; self.close_callbacks[#self.close_callbacks + 1] = callback end
  if self.closing then return end
  if self.closed then self.closing = true; self:_terminate_children(); self:_maybe_finalize(); return end
  self.closing = true
  local timer = uv.new_timer()
  self.close_timer = timer
  timer:start(5000, 0, function()
    self:_fail({ code="timeout", message="Grafikr close exceeded five seconds" })
  end)
  self:request("shutdown", {}, function(reply)
    if not reply.ok then self:_fail(reply.error); return end
    self.stdin:shutdown(function() close(self.stdin) end)
  end)
end

function App:_consume(chunk)
  self.buffer = self.buffer .. chunk
  if #self.buffer > M.MAX_FRAME and not self.buffer:find("\n", 1, true) then self:_fail({ code="frame_too_large", message="host frame exceeds 8 MiB" }); return end
  while true do
    local lf = self.buffer:find("\n", 1, true); if not lf then return end
    if lf > M.MAX_FRAME then self:_fail({ code="frame_too_large", message="host frame exceeds 8 MiB" }); return end
    local line = self.buffer:sub(1, lf - 1); self.buffer = self.buffer:sub(lf + 1)
    local message, _, err = json.decode(line); if err then self:_fail({ code="invalid_frame", message=err }); return end
    if message.kind == "response" then
      local pending = self.pending[message.id]; self.pending[message.id] = nil
      if pending then self.pending_count = self.pending_count - 1; self.pending_bytes = self.pending_bytes - pending.bytes end
      if pending and pending.callback then
        local ok, callback_error = pcall(pending.callback, message)
        if not ok and self.error_callback then pcall(self.error_callback, callback_error, message) end
      end
    elseif message.kind == "event" then
      local key = tostring(message.source) .. "\0" .. message.event
      if not self.closed then
        for _, callback in pairs(self.handlers[key] or {}) do
          local ok, callback_error = pcall(callback, message)
          if not ok and self.error_callback then pcall(self.error_callback, callback_error, message) end
        end
      end
    else self:_fail({ code="invalid_frame", message="unknown message kind" }); return end
    if not self.closed and self.reader_stdin and not self.reader_stdin:is_closing() then
      self.reader_stdin:write("1\n")
    end
  end
end

function App:_fail(error)
  if self.closed then return end; self.closed = true
  self.failure = error
  for _, pending in pairs(self.pending) do if pending.callback then pcall(pending.callback, { ok=false, error=error }) end end
  self.pending = {}; self.pending_count = 0; self.pending_bytes = 0
  self:_terminate_children()
  self.relay_eof = true
  self:_maybe_finalize()
end

function App:_terminate_children()
  for _, process in ipairs({ self.process, self.reader }) do
    if process and not process:is_closing() then pcall(function() process:kill("sigterm") end) end
  end
  for _, pipe in ipairs({ self.stdin, self.host_stdout, self.stderr, self.reader_stdin, self.stdout, self.reader_stderr }) do
    if pipe then pcall(function() pipe:read_stop() end); close(pipe) end
  end
end

function App:_host_exit(code, signal)
  self.host_exited = true
  close(self.process); close(self.stdin); close(self.host_stdout); close(self.stderr)
  if self.closing and not self.closed then
    self.closed = true
    for _, pending in pairs(self.pending) do if pending.callback then pcall(pending.callback, { ok=false, error={ code="connection_closed", message="host exited" } }) end end
    self.pending = {}; self.pending_count = 0; self.pending_bytes = 0
  elseif not self.closing then
    self.host_exit_error = { code="connection_closed", message="host exited with code " .. tostring(code) }
  end
  self:_maybe_finalize(code, signal)
end

function App:_reader_exit(code, signal)
  self.reader_exit_code = code
  self.reader_exited = true
  close(self.reader); close(self.reader_stdin); close(self.reader_stderr)
  if not self.closing and not self.closed then
    local error = code == 75 and { code="queue_full", message="Callback reader queue quota reached" } or { code="connection_closed", message="Grafikr reader exited with code " .. tostring(code) }
    self:_fail(error)
  end
  self:_maybe_finalize(code, signal)
end

function App:_relay_eof()
  self.relay_eof = true
  pcall(function() self.stdout:read_stop() end); close(self.stdout)
  self:_maybe_finalize()
end

function App:_maybe_finalize(code, signal)
  if self.finalized or not (self.host_exited and self.reader_exited and self.relay_eof) then return end
  self.finalized = true
  if self.close_timer then self.close_timer:stop(); close(self.close_timer); self.close_timer = nil end
  if self.exit_callback then
    local callback = self.exit_callback; self.exit_callback = nil
    pcall(callback, code, signal)
  end
  if self.close_callbacks then
    local callbacks = self.close_callbacks; self.close_callbacks = nil
    for _, callback in ipairs(callbacks) do pcall(callback, code, signal) end
  end
end

M.App = App
M.runtime = require("grafikr_runtime")
return M
