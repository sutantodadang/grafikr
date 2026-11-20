local uv = require("luv")

local MAX_FRAME = 8 * 1024 * 1024
local MAX_MESSAGES = 1024
local MAX_BYTES = 16 * 1024 * 1024
local input, acknowledgements, output = uv.new_pipe(false), uv.new_pipe(false), uv.new_pipe(false)
assert(input:open(3) == 0, "cannot open host output pipe")
assert(acknowledgements:open(0) == 0, "cannot open parent acknowledgement pipe")
assert(output:open(1) == 0, "cannot open parent output pipe")

local exit_code = 0
local finished, source_eof, writing, waiting_ack = false, false, false, false
local frame_buffer, acknowledgement_buffer = "", ""
local frames, frame_head, frame_tail = {}, 1, 0
local outstanding, outstanding_head, outstanding_tail, outstanding_bytes = {}, 1, 0, 0
local message_count = 0

local function close(handle)
  if handle and not handle:is_closing() then handle:close() end
end

local function finish(code)
  if finished then return end
  finished, exit_code = true, code
  input:read_stop()
  acknowledgements:read_stop()
  close(input); close(acknowledgements); close(output)
end

local pump
local function finish_if_drained()
  if source_eof and not writing and frame_head > frame_tail and outstanding_head > outstanding_tail then finish(0) end
end

pump = function()
  if finished or writing or waiting_ack then return end
  if frame_head <= frame_tail then
    local frame = frames[frame_head]
    frames[frame_head] = nil
    frame_head = frame_head + 1
    writing = true
    local request = output:write(frame, function(err)
      writing = false
      if err then finish(70) else waiting_ack = true end
    end)
    if not request then writing = false; finish(70) end
  else
    finish_if_drained()
  end
end

input:read_start(function(err, chunk)
  if finished then return end
  if err then finish(70); return end
  if not chunk then
    source_eof = true
    input:read_stop(); close(input)
    if frame_buffer ~= "" then finish(65) else finish_if_drained() end
    return
  end
  frame_buffer = frame_buffer .. chunk
  while true do
    local lf = frame_buffer:find("\n", 1, true)
    if not lf then
      if #frame_buffer >= MAX_FRAME then finish(65) end
      break
    end
    local size = lf
    if size > MAX_FRAME then finish(65); return end
    local frame = frame_buffer:sub(1, lf)
    frame_buffer = frame_buffer:sub(lf + 1)
    if message_count >= MAX_MESSAGES or outstanding_bytes + size > MAX_BYTES then finish(75); return end
    message_count = message_count + 1
    outstanding_tail = outstanding_tail + 1
    outstanding[outstanding_tail] = size
    outstanding_bytes = outstanding_bytes + size
    frame_tail = frame_tail + 1
    frames[frame_tail] = frame
    pump()
  end
end)

acknowledgements:read_start(function(err, chunk)
  if finished then return end
  if err or not chunk then finish(70); return end
  acknowledgement_buffer = acknowledgement_buffer .. chunk
  while true do
    local lf = acknowledgement_buffer:find("\n", 1, true)
    if not lf then break end
    local acknowledgement = acknowledgement_buffer:sub(1, lf - 1)
    acknowledgement_buffer = acknowledgement_buffer:sub(lf + 1)
    local size = outstanding_head <= outstanding_tail and outstanding[outstanding_head] or nil
    if acknowledgement ~= "1" or not size then finish(65); return end
    outstanding[outstanding_head] = nil
    outstanding_head = outstanding_head + 1
    waiting_ack = false
    message_count = message_count - 1
    outstanding_bytes = outstanding_bytes - size
  end
  if outstanding_head > outstanding_tail then outstanding, outstanding_head, outstanding_tail = {}, 1, 0 end
  if frame_head > frame_tail then frames, frame_head, frame_tail = {}, 1, 0 end
  pump()
  finish_if_drained()
end)

uv.run()
os.exit(exit_code)
