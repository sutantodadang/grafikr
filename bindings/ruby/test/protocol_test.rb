require "minitest/autorun"
require_relative "../lib/grafikr"
require "timeout"
require "fileutils"
require "tmpdir"
require "stringio"
require "base64"

class ProtocolTest < Minitest::Test
  def test_creates_every_shared_widget_and_receives_declared_events
    previous_early = ENV["GRAFIKR_FIXTURE_EARLY_SUBSCRIBE"]
    skip "requires isolated widget inventory fixture mode" unless ENV["GRAFIKR_FIXTURE_WIDGET_INVENTORY"]
    host = ENV.fetch("GRAFIKR_FIXTURE_HOST")
    inventory = JSON.parse(File.read(File.expand_path("../../protocol/v1/widgets.json", __dir__))).fetch("widgets")
    ENV["GRAFIKR_FIXTURE_EARLY_SUBSCRIBE"] = "1"
    app = Grafikr::App.new(host)
    window = app.create_window("widget inventory", 640, 480)
    state = app.create_state(type: "i64", value: "1")
    upload = app.request("image.begin", width: 1, height: 1, byte_length: 4).fetch("upload")
    app.request("image.chunk", upload: upload, offset: 0, data: Base64.strict_encode64([0, 0, 0, 255].pack("C*")))
    image = app.request("image.finalize", upload: upload).fetch("image")

    inventory.each do |widget, spec|
      properties = spec.fetch("properties").to_h do |name, type|
        [name, sample_widget_value(widget, name, type, state.handle, image)]
      end
      node = app.create_node(window, widget, properties)
      node.attach(window.root)
      spec.fetch("events", []).each do |event_name|
        received = Queue.new
        subscription = app.on(node.handle, event_name) { |event| received << event }
        event = Timeout.timeout(2) { received.pop }
        assert_equal node.handle, event.fetch("source"), "#{widget}@#{event_name}"
        assert_equal event_name, event.fetch("event"), "#{widget}@#{event_name}"
        subscription.close
      end
      node.destroy
    end

    app.request("image.destroy", image: image)
    state.close
    window.close
  ensure
    app&.close
    previous_early.nil? ? ENV.delete("GRAFIKR_FIXTURE_EARLY_SUBSCRIBE") : ENV["GRAFIKR_FIXTURE_EARLY_SUBSCRIBE"] = previous_early
  end

  def test_frame_limit
    assert_equal 8 * 1024 * 1024, Grafikr::MAX_FRAME
  end

  def test_stderr_is_drained_in_bounded_chunks
    stream = StringIO.new("diagnostic\n" * 100_000)
    app = Grafikr::App.allocate
    app.instance_variable_set(:@stderr, stream)

    app.send(:drain_stderr)

    assert_equal stream.string.bytesize, stream.pos
  end

  def test_process_path_is_an_explicit_argv_element
    host = File.join(Dir.tmpdir, "Grafikr host 雪.exe")
    received = nil
    spawn = ->(*args) { received = args; raise "spawn intercepted" }

    Open3.stub(:popen3, spawn) do
      assert_raises(RuntimeError) { Grafikr::App.new(host) }
    end

    assert_equal [[host, host]], received
  end

  def test_host_starts_from_a_path_with_spaces_and_unicode
    executable = Gem.win_platform? ? "fixture-host.exe" : "fixture-host"
    source = ENV["GRAFIKR_FIXTURE_HOST"] || File.expand_path("../../../target/debug/#{executable}", __dir__)
    skip "fixture host is unavailable" unless File.file?(source)
    app = nil

    Dir.mktmpdir("Grafikr host 雪") do |directory|
      host = File.join(directory, "grafikr host 雪.exe")
      FileUtils.cp(source, host)
      app = Grafikr::App.new(host)
      assert_instance_of Grafikr::App, app
    ensure
      app&.close
    end
  end

  def test_accepts_an_absolute_windows_host_path_with_backslashes
    skip "Windows path semantics required" unless Gem.win_platform?
    host = ENV.fetch("GRAFIKR_FIXTURE_HOST")
    app = Grafikr::App.new(host)

    assert_instance_of Grafikr::App, app
  ensure
    app&.close
  end

  def test_outstanding_request_count_is_limited
    app = Grafikr::App.allocate
    app.instance_variable_set(:@lock, Mutex.new)
    app.instance_variable_set(:@pending, (1..256).to_h { |id| [id.to_s, Queue.new] })
    app.instance_variable_set(:@closed, false)

    error = assert_raises(Grafikr::Error) { app.request("state.get") }

    assert_equal "queue_full", error.code
  end

  def test_pending_request_bytes_are_limited_before_writing
    app = request_app
    app.instance_variable_set(:@pending_bytes, 16 * 1024 * 1024)

    error = assert_raises(Grafikr::Error) { app.request("state.get", { state: "1" }, timeout: 0.001) }

    assert_equal "queue_full", error.code
    assert_empty app.instance_variable_get(:@pending)
    assert_empty app.instance_variable_get(:@stdin).string
  end

  def test_non_finite_values_are_rejected_as_invalid_value_before_writing
    app = request_app

    error = assert_raises(Grafikr::Error) do
      app.request("state.create", { value: { type: "f64", value: Float::NAN } }, timeout: 0.001)
    end

    assert_equal "invalid_value", error.code
    assert_empty app.instance_variable_get(:@pending)
    assert_empty app.instance_variable_get(:@stdin).string
  end

  def test_event_queue_is_bounded_and_overflow_fails_the_session
    app = callback_app
    pending = Queue.new
    app.instance_variable_set(:@pending, { "request-1" => pending })
    assert_instance_of SizedQueue, app.instance_variable_get(:@events)

    1024.times { |index| raise "request queue overflowed early" unless app.send(:enqueue_event, { "event" => index }, 1) }
    refute app.send(:enqueue_event, { "event" => "overflow" }, 1)
    assert app.instance_variable_get(:@closed)
    assert_equal "output_overflow", app.instance_variable_get(:@failure).code
    assert_equal "output_overflow", pending.pop.dig("error", "code")
  end

  def test_blocked_callback_cannot_grow_the_reader_queue_without_bound
    app = callback_app
    entered = Queue.new
    release = Queue.new
    calls = 0
    app.instance_variable_get(:@handlers)[["7", "click"]]["sub-1"] = lambda do |_event|
      calls += 1
      entered << true
      release.pop if calls == 1
    end
    dispatch = Thread.new { app.send(:dispatch_loop) }
    app.instance_variable_set(:@event_bytes, 1)
    app.instance_variable_get(:@events) << [{ "source" => "7", "event" => "click" }, 1]
    entered.pop

    1024.times do |index|
      assert app.send(:enqueue_event, { "source" => "8", "event" => "click", "index" => index }, 1)
    end
    refute app.send(:enqueue_event, { "source" => "8", "event" => "click" }, 1)
    assert_equal "output_overflow", app.instance_variable_get(:@failure).code
    release << true
    app.instance_variable_get(:@events) << nil
    dispatch.join(2) || flunk("callback dispatcher did not stop")
  end

  def test_event_queue_byte_limit_fails_the_session
    app = callback_app
    app.instance_variable_set(:@event_bytes, 16 * 1024 * 1024)

    refute app.send(:enqueue_event, { "event" => "overflow" }, 1)
    assert app.instance_variable_get(:@closed)
    assert_equal "output_overflow", app.instance_variable_get(:@failure).code
  end

  def test_callback_errors_reach_the_error_handler_without_stopping_dispatch
    app = callback_app
    errors = Queue.new
    called = Queue.new
    app.on_error { |error, event| errors << [error.message, event["event"]] }
    app.instance_variable_get(:@handlers)[["7", "click"]]["first"] = ->(_event) { raise "callback failed" }
    app.instance_variable_get(:@handlers)[["7", "click"]]["second"] = ->(_event) { called << true }
    app.instance_variable_get(:@events) << [{ "source" => "7", "event" => "click" }, 1]
    app.instance_variable_get(:@events) << nil
    app.instance_variable_set(:@event_bytes, 1)

    app.send(:dispatch_loop)

    assert_equal ["callback failed", "click"], errors.pop
    assert called.pop
  end

  def test_unsubscribe_removes_the_local_handler_before_the_request
    app = callback_app
    key = ["7", "click"]
    app.instance_variable_get(:@handlers)[key]["sub-1"] = ->(_event) {}
    handler_present_during_request = nil
    app.define_singleton_method(:request) do |operation, **_args|
      raise "unexpected request" unless operation == "event.unsubscribe"
      handler_present_during_request = @lock.synchronize { @handlers[key].key?("sub-1") }
      {}
    end

    app.unsubscribe("sub-1", key)

    refute handler_present_during_request
    assert_empty app.instance_variable_get(:@handlers)[key]
  end

  def test_invalid_frame_releases_its_pending_request_slot
    app = Grafikr::App.allocate
    app.instance_variable_set(:@lock, Mutex.new)
    app.instance_variable_set(:@pending, (1..255).to_h { |id| [id.to_s, Queue.new] })
    app.instance_variable_set(:@next_id, 255)
    app.instance_variable_set(:@closed, false)

    error = assert_raises(Grafikr::Error) { app.request("state.set", value: "x" * Grafikr::MAX_FRAME) }

    assert_equal "frame_too_large", error.code
    assert_equal 255, app.instance_variable_get(:@pending).length
  end

  def test_subscription_handler_is_registered_before_request
    app = Grafikr::App.allocate
    app.instance_variable_set(:@lock, Mutex.new)
    app.instance_variable_set(:@handlers, Hash.new { |handlers, key| handlers[key] = {} })
    app.define_singleton_method(:request) do |operation, **_args|
      raise "unexpected operation" unless operation == "event.subscribe"
      callback = @lock.synchronize { @handlers[["7", "click"]].values.first }
      callback.call("event" => "click")
      { "subscription" => "1" }
    end

    received = []
    app.on("7", "click") { |event| received << event }

    assert_equal [{ "event" => "click" }], received
  end

  def test_failed_subscription_removes_provisional_handler
    app = Grafikr::App.allocate
    app.instance_variable_set(:@lock, Mutex.new)
    app.instance_variable_set(:@handlers, Hash.new { |handlers, key| handlers[key] = {} })
    app.define_singleton_method(:request) { |*| raise Grafikr::Error.new("failure", "subscribe failed") }

    assert_raises(Grafikr::Error) { app.on("7", "click") {} }
    assert_empty app.instance_variable_get(:@handlers)[["7", "click"]]
  end

  def test_collection_provider_subscribes_before_create_and_serves_ranges
    app = Grafikr::App.allocate
    subscribed = false
    calls = []
    app.define_singleton_method(:on) do |node, event, &callback|
      raise "unexpected subscription" unless [node, event] == ["7", "data.request"]
      calls << ["event.subscribe", {}]
      subscribed = true
      callback.call("payload" => { "revision" => "2", "request" => "9", "start" => 4, "count" => 2 })
      subscription = Object.new
      subscription.define_singleton_method(:close) {}
      subscription
    end
    app.define_singleton_method(:request) do |operation, **args|
      calls << [operation, args]
      raise "create ran before subscription" if operation == "collection.create" && !subscribed
      {}
    end
    node = Struct.new(:app, :handle).new(app, "7")

    provider = app.create_collection_provider(node, row_count: 20, row_height: 24, columns: ["name"])
    request = provider.next_request(timeout: 1)
    provider.provide(request, [["a"], ["b"]])
    provider.update(row_count: 2, row_height: 24, columns: ["name"])

    assert_equal ["event.subscribe", "collection.create", "data.provide", "collection.update"], calls.map(&:first)
    assert_equal({ node: "7", revision: "2", request: "9", start: 4, rows: [["a"], ["b"]] }, calls[2][1])
    provider.close
    assert_nil provider.next_request(timeout: 1)
  end

  def test_fixture_event_arrives_before_subscribe_response
    executable = Gem.win_platform? ? "fixture-host.exe" : "fixture-host"
    host = ENV["GRAFIKR_FIXTURE_HOST"] || File.expand_path("../../../target/debug/#{executable}", __dir__)
    app = Grafikr::App.new(host)
    window = app.create_window("fixture", 320, 240)
    button = app.create_node(window, "Button", label: "click")
    button.attach(window.root)
    received = Queue.new
    subscription = app.on(button.handle, "click") { |event| received << event }

    event = Timeout.timeout(2) { received.pop }
    assert_equal button.handle, event["source"]
    assert_equal "click", event["event"]
  ensure
    subscription&.close
    app&.close
  end

  def test_fixture_correlates_concurrent_out_of_order_responses
    with_fixture_env("GRAFIKR_FIXTURE_REVERSE_RESPONSES" => "1") do
      app = Grafikr::App.new(ENV.fetch("GRAFIKR_FIXTURE_HOST"))
      first = app.create_state(type: "i64", value: "7")
      second = app.create_state(type: "i64", value: "8")
      results = [first, second].map do |state|
        Thread.new { app.request("state.get", state: state.handle).fetch("value") }
      end.map(&:value)
      assert_equal ["7", "8"], results
    ensure
      app&.close
    end
  end

  def test_fixture_allows_nested_request_from_event_callback
    with_fixture_env("GRAFIKR_FIXTURE_EARLY_SUBSCRIBE" => "1") do
      app = Grafikr::App.new(ENV.fetch("GRAFIKR_FIXTURE_HOST"))
      state = app.create_state(type: "i64", value: "7")
      window = app.create_window("nested callback", 320, 240)
      button = app.create_node(window, "Button", label: "click")
      button.attach(window.root)
      result = Queue.new
      subscription = app.on(button.handle, "click") do |_event|
        result << app.request("state.get", state: state.handle).fetch("value")
      end
      assert_equal "7", Timeout.timeout(2) { result.pop }
      subscription.close
    ensure
      app&.close
    end
  end

  def test_large_utf8_frame_and_i64_boundaries_round_trip
    app = Grafikr::App.new(ENV.fetch("GRAFIKR_FIXTURE_HOST"))
    text = "line one\nline two 雪 😀" + ("x" * 102_400)
    string_state = app.create_state(type: "string", value: text)
    result = app.request("state.get", state: string_state.handle)
    assert_equal text, result.fetch("value")
    assert_equal "string", result.fetch("type")

    integer = app.create_state(type: "i64", value: "-9223372036854775808")
    assert_equal "-9223372036854775808", app.request("state.get", state: integer.handle).fetch("value")
    app.request("state.set", state: integer.handle, value: { type: "i64", value: "9223372036854775807" })
    assert_equal "9223372036854775807", app.request("state.get", state: integer.handle).fetch("value")
  ensure
    app&.close
  end

  def test_eof_rejects_256_pending_requests
    with_fixture_env("GRAFIKR_FIXTURE_HOLD_RESPONSES" => "1", "GRAFIKR_FIXTURE_EXIT_AFTER_HELLO_MS" => "500") do
      app = Grafikr::App.new(ENV.fetch("GRAFIKR_FIXTURE_HOST"))
      errors = Queue.new
      threads = Array.new(256) do
        Thread.new do
          app.request("state.get", state: "0")
        rescue Grafikr::Error => error
          errors << error
        end
      end
      threads.each { |thread| thread.join(3) || flunk("request remained pending after EOF") }
      rejected = Array.new(256) { errors.pop }
      assert rejected.all? { |error| error.code == "connection_closed" }
    ensure
      app&.close
    end
  end

  def test_close_does_not_terminate_an_unrelated_process
    marker = File.join(Dir.tmpdir, "grafikr-sentinel-#{Process.pid}-#{rand(1_000_000)}")
    sentinel = Process.spawn(RbConfig.ruby, "-e", 'sleep 0.3; File.write(ARGV.fetch(0), "alive")', marker)
    app = Grafikr::App.new(ENV.fetch("GRAFIKR_FIXTURE_HOST"))
    app.close
    Process.wait(sentinel)
    assert_equal "alive", File.read(marker)
  ensure
    app&.close
    Process.kill("KILL", sentinel) rescue nil
    FileUtils.rm_f(marker)
  end

  def test_fixture_emits_a_collection_range_when_enabled
    executable = Gem.win_platform? ? "fixture-host.exe" : "fixture-host"
    host = ENV["GRAFIKR_FIXTURE_HOST"] || File.expand_path("../../../target/debug/#{executable}", __dir__)
    app = Grafikr::App.new(host)
    window = app.create_window("fixture", 320, 240)
    list = app.create_node(window, "VirtualList", row_count: 1, row_height: 24, height: 120)
    list.attach(window.root)
    provider = app.create_collection_provider(list, row_count: 1, row_height: 24, columns: ["label"])
    request = provider.next_request(timeout: 2)
    refute_nil request
    assert_equal ["1", 0, 1], [request.revision, request.start, request.count]
    provider.provide(request, [["row"]])
  ensure
    provider&.close
    app&.close
  end

  private

  def with_fixture_env(values)
    previous = values.to_h { |key, _value| [key, ENV[key]] }
    values.each { |key, value| ENV[key] = value }
    yield
  ensure
    previous&.each { |key, value| value.nil? ? ENV.delete(key) : ENV[key] = value }
  end

  def request_app
    app = Grafikr::App.allocate
    app.instance_variable_set(:@lock, Mutex.new)
    app.instance_variable_set(:@write_lock, Mutex.new)
    app.instance_variable_set(:@pending, {})
    app.instance_variable_set(:@next_id, 0)
    app.instance_variable_set(:@pending_bytes, 0)
    app.instance_variable_set(:@pending_sizes, {})
    app.instance_variable_set(:@stdin, StringIO.new)
    app.instance_variable_set(:@closed, false)
    app
  end

  def sample_widget_value(widget, name, type, state, image)
    return state if widget == "Radio" && name == "group"
    return "root" if widget == "TreeView" && name == "selected"
    return { "min" => 0.0, "max" => 10.0, "width" => 120.0 }.fetch(name, 1.0) if widget == "Slider"
    return 16.0 if widget == "Spinner" && name == "size"
    return 1 if name == "row_count"
    return 24.0 if name == "row_height"

    {
      "number" => 1.0, "f32" => 1.0, "u16" => 1, "u64" => 0, "string" => "sample",
      "color" => "#336699", "transform" => { "x" => 0.0, "y" => 0.0, "scale" => 1.0 },
      "boolean" => false, "primary|secondary|icon" => "primary", "state" => state,
      "GridColumn[]" => [{ "title" => "Column", "width" => 120.0 }], "string[]" => ["One", "Two"],
      "TreeNode[]" => [{ "key" => "root", "label" => "Root", "children" => [] }],
      "CanvasCommand[]" => [{ "kind" => "rect", "rect" => { "x" => 0.0, "y" => 0.0, "width" => 1.0, "height" => 1.0 }, "color" => "#336699", "radius" => 0.0 }],
      "ImageHandle" => image
    }.fetch(type)
  end

  def callback_app
    app = Grafikr::App.allocate
    app.instance_variable_set(:@lock, Mutex.new)
    app.instance_variable_set(:@events, SizedQueue.new(1024))
    app.instance_variable_set(:@event_bytes, 0)
    app.instance_variable_set(:@handlers, Hash.new { |handlers, key| handlers[key] = {} })
    app.instance_variable_set(:@pending, {})
    app.instance_variable_set(:@closed, false)
    app
  end
end
