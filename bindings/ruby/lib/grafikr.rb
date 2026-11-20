require "open3"
require "json"
require "pathname"
require "timeout"
require_relative "grafikr/runtime"

module Grafikr
  SDK_VERSION = "1.1.0"
  MAX_FRAME = 8 * 1024 * 1024
  MAX_OUTSTANDING_REQUESTS = 256
  MAX_PENDING_BYTES = 16 * 1024 * 1024
  class Error < StandardError
    attr_reader :code, :field
    def initialize(code, message, field = nil); super(message); @code, @field = code, field; end
  end

  class App
    MAX_CALLBACK_QUEUE_MESSAGES = 1024
    MAX_CALLBACK_QUEUE_BYTES = 16 * 1024 * 1024

    def self.start(bundle: nil, host: nil)
      new(host || Runtime.host_path(bundle))
    end

    def self.open(bundle: nil, host: nil)
      app = start(bundle: bundle, host: host)
      return app unless block_given?
      yield app
    ensure
      app&.close if block_given?
    end

    def initialize(host = nil)
      host ||= Runtime.host_path
      raise ArgumentError, "host path must be absolute" unless Pathname.new(host).absolute?
      @stdin, @stdout, @stderr, @wait = Open3.popen3([host, host])
      @lock = Mutex.new; @write_lock = Mutex.new; @next_id = 0; @pending = {}; @pending_sizes = {}; @pending_bytes = 0; @handlers = Hash.new { |h,k| h[k] = {} }; @events = SizedQueue.new(MAX_CALLBACK_QUEUE_MESSAGES); @event_bytes = 0; @closed = false; @disposed = false; @failure = nil
      @stderr_thread = Thread.new { drain_stderr }
      @reader = Thread.new { read_loop }
      @callbacks = Thread.new { dispatch_loop }
      result = request("hello", protocol_major: 1, protocol_minor: 0, sdk_version: SDK_VERSION)
      raise Error.new("version_mismatch", "Protocol 1 required") unless result.dig("protocol", "major") == 1
    end

    def request(op, args = nil, timeout: 30, **keyword_args)
      args ||= keyword_args
      id, waiter = @lock.synchronize do
        raise @failure || Error.new("connection_closed", "App is closed") if @closed
        raise Error.new("queue_full", "Outstanding request count quota reached") if @pending.length >= MAX_OUTSTANDING_REQUESTS
        @next_id += 1; value = @next_id.to_s; queue = Queue.new; @pending[value] = queue; [value, queue]
      end
      frame = JSON.generate(id: id, op: op, args: args) + "\n"
      raise Error.new("frame_too_large", "Request exceeds 8 MiB") if frame.bytesize > MAX_FRAME
      @lock.synchronize do
        if @pending_bytes + frame.bytesize > MAX_PENDING_BYTES
          @pending.delete(id)
          raise Error.new("queue_full", "Outstanding request byte quota reached")
        end
        @pending_sizes[id] = frame.bytesize
        @pending_bytes += frame.bytesize
      end
      @write_lock.synchronize { @stdin.write(frame); @stdin.flush }
      reply = Timeout.timeout(timeout) { waiter.pop }
      raise Error.new(reply.dig("error", "code") || "internal_failure", reply.dig("error", "message") || "Request failed", reply.dig("error", "field")) unless reply["ok"]
      reply["result"]
    rescue Timeout::Error
      remove_pending(id)
      raise Error.new("timeout", "Request timed out")
    rescue JSON::GeneratorError => error
      remove_pending(id) if id
      raise Error.new("invalid_value", "Request arguments are not valid JSON: #{error.message}")
    rescue StandardError
      remove_pending(id) if id
      raise
    end

    def on(node, event, &callback)
      key = [node.to_s, event]
      pending = Object.new
      @lock.synchronize { @handlers[key][pending] = callback }
      begin
        id = request("event.subscribe", node: node, event: event).fetch("subscription")
        @lock.synchronize { @handlers[key][id] = @handlers[key].delete(pending) }
      rescue StandardError
        @lock.synchronize { @handlers[key].delete(pending) }
        raise
      end
      Subscription.new(self, id, key)
    end
    def on_error(&handler)
      raise ArgumentError, "error handler block is required" unless handler
      @lock.synchronize { @error_handler = handler }
    end
    def unsubscribe(id, key)
      @lock.synchronize do
        @handlers[key].delete(id)
        @handlers.delete(key) if @handlers[key].empty?
      end
      request("event.unsubscribe", subscription: id)
    end
    def create_window(title, width, height)
      value = request("window.create", title: title, width: width, height: height)
      Window.new(self, value.fetch("window"), value.fetch("root"))
    end
    def create_node(window, widget, properties = {})
      Node.new(self, request("node.create", window: window.handle, widget: widget, properties: properties).fetch("node"))
    end
    def create_state(value)
      State.new(self, request("state.create", value: value).fetch("state"))
    end
    def create_collection_provider(node, row_count:, row_height:, columns:)
      CollectionProvider.create(node, row_count: row_count, row_height: row_height, columns: columns)
    end

    def close
      return if @disposed
      @disposed = true
      request("shutdown",{},timeout:5) rescue nil unless @closed
      @stdin.close rescue nil
      unless @wait.join(5)
        Process.kill("TERM", @wait.pid) rescue nil
        @wait.join(1) || Process.kill("KILL", @wait.pid) rescue nil
      end
      @lock.synchronize { @closed = true }
      @stdout.close rescue nil
      @reader.join(1)
      @events.clear
      @lock.synchronize { @event_bytes = 0 }
      @events.push(nil, true)
      @callbacks.join(1) || @callbacks.kill
      @stderr_thread.join(1)
    end

    private
    def drain_stderr
      loop { @stderr.readpartial(8192) }
    rescue EOFError, IOError
      nil
    end
    def read_loop
      buffer = +"".b
      loop do
        chunk = @stdout.readpartial(8192); buffer << chunk
        raise Error.new("frame_too_large", "Host frame exceeds 8 MiB") if buffer.bytesize > MAX_FRAME && !buffer.include?("\n")
        while (lf = buffer.index("\n"))
          raise Error.new("frame_too_large", "Host frame exceeds 8 MiB") if lf + 1 > MAX_FRAME
          frame = buffer.slice!(0, lf + 1)
          message = JSON.parse(frame)
          if message["kind"] == "response"
            waiter = @lock.synchronize do
              id = message["id"]
              @pending_bytes -= @pending_sizes.delete(id).to_i
              @pending.delete(id)
            end
            waiter << message if waiter
          elsif message["kind"] == "event"
            return unless enqueue_event(message, frame.bytesize)
          else
            raise Error.new("invalid_frame", "Unknown message kind")
          end
        end
      end
    rescue EOFError, IOError => e
      fail_pending(Error.new("connection_closed", e.message))
    rescue StandardError => e
      fail_pending(e)
    end
    def dispatch_loop
      while ((queued = @events.pop))
        event, size = queued
        @lock.synchronize { @event_bytes -= size }
        key = [event["source"], event["event"]]
        callbacks = @lock.synchronize { @handlers[key].values.dup }
        callbacks.each do |callback|
          begin
            callback.call(event)
          rescue StandardError => error
            handler = @lock.synchronize { @error_handler }
            handler.call(error, event) rescue nil if handler
          end
        end
      end
    end
    def enqueue_event(event, frame_bytes)
      status = @lock.synchronize do
        if @closed
          :closed
        elsif @event_bytes + frame_bytes > MAX_CALLBACK_QUEUE_BYTES
          :overflow
        else
          @event_bytes += frame_bytes
          :queued
        end
      end
      return false if status == :closed
      if status == :overflow
        fail_pending(Error.new("output_overflow", "event byte quota exceeded"))
        return false
      end
      @events.push([event, frame_bytes], true)
      true
    rescue ThreadError
      @lock.synchronize { @event_bytes -= frame_bytes }
      fail_pending(Error.new("output_overflow", "event queue is full"))
      false
    end
    def fail_pending(error)
      pending = @lock.synchronize do
        next [] if @closed
        @closed = true; @failure = error; values = @pending.values; @pending = {}; @pending_sizes = {}; @pending_bytes = 0; values
      end
      code = error.is_a?(Error) ? error.code : "connection_closed"
      pending.each { |queue| queue << { "ok" => false, "error" => { "code" => code, "message" => error.message } } }
    end
    def remove_pending(id)
      @lock.synchronize do
        @pending.delete(id)
        @pending_bytes = (@pending_bytes || 0) - (@pending_sizes&.delete(id) || 0)
      end
    end
  end

  class Subscription
    def initialize(app, id, key); @app, @id, @key, @closed = app, id, key, false; end
    def close; return if @closed; @closed = true; @app.unsubscribe(@id, @key); end
  end
  CollectionRequest = Struct.new(:revision, :request, :start, :count, keyword_init: true)
  class CollectionProvider
    CLOSED = Object.new.freeze

    def self.create(node, row_count:, row_height:, columns:)
      provider = new(node)
      provider.instance_variable_set(:@subscription, node.app.on(node.handle, "data.request") { |event| provider.receive(event) })
      node.app.request("collection.create", node: node.handle, row_count: row_count, row_height: row_height, columns: columns)
      provider
    rescue StandardError
      provider&.close
      raise
    end

    def initialize(node)
      @node, @app, @requests = node, node.app, SizedQueue.new(1)
      @lock, @closed = Mutex.new, false
    end

    def next_request(timeout: nil)
      value = @requests.pop(timeout: timeout)
      value.equal?(CLOSED) ? nil : value
    end

    def provide(request, rows)
      @lock.synchronize { raise Error.new("connection_closed", "collection provider is closed") if @closed }
      @app.request("data.provide", node: @node.handle, revision: request.revision, request: request.request, start: request.start, rows: rows)
    end

    def update(row_count:, row_height:, columns:)
      @lock.synchronize { raise Error.new("connection_closed", "collection provider is closed") if @closed }
      @app.request("collection.update", node: @node.handle, metadata: { row_count: row_count, row_height: row_height, columns: columns })
    end

    def close
      subscription = @lock.synchronize do
        return if @closed
        @closed = true
        @requests.clear
        @subscription
      end
      begin
        subscription&.close
      ensure
        @requests.push(CLOSED)
      end
    end

    def receive(event)
      payload = event["payload"]
      unless payload.is_a?(Hash) && payload["revision"].is_a?(String) && payload["request"].is_a?(String) &&
          payload["start"].is_a?(Integer) && payload["start"] >= 0 && payload["count"].is_a?(Integer) && payload["count"] >= 0
        raise Error.new("invalid_frame", "collection request fields are invalid")
      end
      @lock.synchronize do
        return if @closed
        raise Error.new("queue_full", "collection provider request queue is full") unless @requests.push(CollectionRequest.new(**payload.slice("revision", "request", "start", "count").transform_keys(&:to_sym)), true)
      end
    rescue ThreadError
      raise Error.new("queue_full", "collection provider request queue is full")
    end
  end

  class Window
    attr_reader :handle, :root
    def initialize(app, handle, root); @app, @handle, @root = app, handle, root; end
    def close; @app.request("window.close", window: @handle); end
  end
  class Node
    attr_reader :app, :handle
    def initialize(app, handle); @app, @handle = app, handle; end
    def attach(parent); @app.request("node.attach", node: @handle, parent: parent); end
    def update(properties); @app.request("property.update", node: @handle, properties: properties); end
    def destroy; @app.request("node.destroy", node: @handle); end
  end
  class State
    attr_reader :handle
    def initialize(app,handle); @app,@handle=app,handle; end
    def get; @app.request("state.get",state:@handle); end
    def set(value); @app.request("state.set",state:@handle,value:value); end
    def close; @app.request("state.destroy",state:@handle); end
  end

  Client = App
end
