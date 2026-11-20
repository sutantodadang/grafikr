using System.Collections.Concurrent;
using System.Diagnostics;
using System.Text;
using System.Text.Json;
using System.Threading.Channels;

namespace Grafikr;

public sealed class BridgeException(string code, string message, string? field = null) : Exception(message)
{
    public string Code { get; } = code;
    public string? Field { get; } = field;
}

public sealed class App : IAsyncDisposable
{
    public const int MaxFrameBytes = 8 * 1024 * 1024;
    private const int MaxPendingRequests = 256;
    private const int MaxQueueBytes = 16 * 1024 * 1024;
    private sealed record QueuedEvent(JsonElement Message, int Bytes);
    private readonly Process _process;
    private readonly ConcurrentDictionary<string, TaskCompletionSource<JsonElement>> _pending = new();
    private readonly Dictionary<string, int> _pendingSizes = new();
    private readonly object _pendingGate = new();
    private int _pendingBytes;
    private readonly ConcurrentDictionary<string, ConcurrentDictionary<string, Func<JsonElement, ValueTask>>> _handlers = new();
    private readonly Channel<QueuedEvent> _events = Channel.CreateBounded<QueuedEvent>(1024);
    private readonly object _eventGate = new();
    private int _eventBytes;
    private readonly SemaphoreSlim _write = new(1, 1);
    private readonly Task _reader;
    private readonly Task _eventLoop;
    private long _nextId;
    private volatile bool _closed;
    private int _disposed;

    private App(string host)
    {
        _process = new Process { StartInfo = new ProcessStartInfo(Path.GetFullPath(host)) { UseShellExecute = false, RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true } };
        if (!_process.Start()) throw new BridgeException("connection_closed", "Could not start Grafikr host");
        _ = _process.StandardError.ReadToEndAsync();
        _reader = ReadLoopAsync();
        _eventLoop = DispatchEventsAsync();
    }

    public static async Task<App> StartAsync(string host, CancellationToken cancellationToken = default)
    {
        var app = new App(host);
        try
        {
            var result = await app.RequestAsync("hello", new { protocol_major = 1, protocol_minor = 0, sdk_version = "1.1.0" }, cancellationToken).ConfigureAwait(false);
            if (result.GetProperty("protocol").GetProperty("major").GetInt32() != 1) throw new BridgeException("version_mismatch", "Host protocol major must be 1");
            return app;
        }
        catch { await app.DisposeAsync().ConfigureAwait(false); throw; }
    }

    public static Task<App> StartAsync(RuntimeOptions options, CancellationToken cancellationToken = default)
        => StartAsync(options.HostPath ?? Runtime.ResolveHost(options.Bundle), cancellationToken);

    public async Task<JsonElement> RequestAsync(string op, object args, CancellationToken cancellationToken = default)
    {
        var id = Interlocked.Increment(ref _nextId).ToString(System.Globalization.CultureInfo.InvariantCulture);
        var completion = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
        lock (_pendingGate)
        {
            if (_closed) throw new BridgeException("connection_closed", "App is closed");
            if (_pending.Count >= MaxPendingRequests) throw new BridgeException("queue_full", "Request count quota reached");
            if (!_pending.TryAdd(id, completion)) throw new BridgeException("internal_failure", "Request id collision");
        }
        byte[] frame;
        try { frame = JsonSerializer.SerializeToUtf8Bytes(new { id, op, args }); }
        catch (Exception error) when (error is ArgumentException or NotSupportedException or JsonException)
        {
            RemovePending(id);
            throw new BridgeException("invalid_value", "Request arguments are not valid JSON");
        }
        if (frame.Length + 1 > MaxFrameBytes) { RemovePending(id); throw new BridgeException("frame_too_large", "Request exceeds 8 MiB"); }
        lock (_pendingGate)
        {
            if (!_pending.ContainsKey(id)) throw new BridgeException("connection_closed", "App is closed");
            if (_pendingBytes + frame.Length + 1 > MaxQueueBytes)
            {
                _pending.TryRemove(id, out _);
                throw new BridgeException("queue_full", "Request byte quota reached");
            }
            _pendingSizes[id] = frame.Length + 1;
            _pendingBytes += frame.Length + 1;
        }
        var acquired = false;
        try
        {
            await _write.WaitAsync(cancellationToken).ConfigureAwait(false);
            acquired = true;
            await _process.StandardInput.BaseStream.WriteAsync(frame, cancellationToken).ConfigureAwait(false);
            await _process.StandardInput.BaseStream.WriteAsync("\n"u8.ToArray(), cancellationToken).ConfigureAwait(false);
            await _process.StandardInput.BaseStream.FlushAsync(cancellationToken).ConfigureAwait(false);
        }
        catch (Exception error) when (error is IOException or ObjectDisposedException)
        {
            RemovePending(id);
            throw new BridgeException("connection_closed", "Could not write request to Grafikr host");
        }
        catch { RemovePending(id); throw; }
        finally { if (acquired) _write.Release(); }
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(TimeSpan.FromSeconds(30));
        try { return await completion.Task.WaitAsync(timeout.Token).ConfigureAwait(false); }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested) { RemovePending(id); throw new BridgeException("timeout", $"Request {id} timed out"); }
        catch (OperationCanceledException) { RemovePending(id); throw; }
    }

    public async Task<Subscription> SubscribeAsync(string node, string eventName, Func<JsonElement, ValueTask> callback, CancellationToken cancellationToken = default)
    {
        var key = EventKey(node, eventName);
        var listenerId = Guid.NewGuid().ToString("N");
        var listeners = _handlers.GetOrAdd(key, static _ => new());
        listeners[listenerId] = callback;
        try
        {
            var response = await RequestAsync("event.subscribe", new { node, @event = eventName }, cancellationToken).ConfigureAwait(false);
            var id = response.GetProperty("subscription").GetString()!;
            return new Subscription(this, id, key, listenerId);
        }
        catch
        {
            RemoveHandler(key, listenerId);
            throw;
        }
    }

    public async Task<Window> CreateWindowAsync(string title, double width, double height, string theme = "dark", CancellationToken cancellationToken = default)
    {
        var result = await RequestAsync("window.create", new { title, width, height, theme }, cancellationToken).ConfigureAwait(false);
        return new Window(this, result.GetProperty("window").GetString()!, result.GetProperty("root").GetString()!);
    }

    public async Task<Node> CreateNodeAsync(Window window, string widget, object properties, CancellationToken cancellationToken = default)
    {
        var result = await RequestAsync("node.create", new { window = window.Handle, widget, properties }, cancellationToken).ConfigureAwait(false);
        return new Node(this, result.GetProperty("node").GetString()!);
    }

    public async Task<State> CreateStateAsync(object value, CancellationToken cancellationToken = default)
    {
        var result = await RequestAsync("state.create", new { value }, cancellationToken).ConfigureAwait(false);
        return new State(this, result.GetProperty("state").GetString()!);
    }

    public Task<CollectionProvider> CreateCollectionProviderAsync(Node node, CollectionMetadata metadata, CancellationToken cancellationToken = default)
        => CollectionProvider.CreateAsync(node, metadata, cancellationToken);

    internal async Task UnsubscribeAsync(string id, string key, string listenerId)
    {
        RemoveHandler(key, listenerId);
        var _response = await RequestAsync("event.unsubscribe", new { subscription = id }).ConfigureAwait(false);
    }

    public async ValueTask DisposeAsync()
    {
        if (Interlocked.Exchange(ref _disposed, 1) != 0) return;
        if (!_closed && !_process.HasExited) try { var _response = await RequestAsync("shutdown", new { }, CancellationToken.None).WaitAsync(TimeSpan.FromSeconds(5)).ConfigureAwait(false); } catch { }
        _closed = true;
        _events.Writer.TryComplete();
        try { await _process.StandardInput.DisposeAsync().ConfigureAwait(false); } catch { }
        try { await _process.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5)).ConfigureAwait(false); }
        catch { try { _process.Kill(entireProcessTree: true); } catch { } }
        try { await Task.WhenAll(_reader, _eventLoop).WaitAsync(TimeSpan.FromSeconds(1)).ConfigureAwait(false); } catch { }
        FailPending(new BridgeException("connection_closed", "App closed"));
        _process.Dispose();
    }

    private async Task ReadLoopAsync()
    {
        var stream = _process.StandardOutput.BaseStream;
        var chunk = new byte[8192]; var frame = new List<byte>(4096);
        try
        {
            while (true)
            {
                var count = await stream.ReadAsync(chunk).ConfigureAwait(false);
                if (count == 0) { if (frame.Count != 0) throw new BridgeException("invalid_frame", "EOF before LF"); break; }
                for (var i = 0; i < count; i++)
                {
                    frame.Add(chunk[i]);
                    if (frame.Count > MaxFrameBytes) throw new BridgeException("frame_too_large", "Host frame exceeds 8 MiB");
                    if (chunk[i] == (byte)'\n') { HandleFrame(frame.ToArray()); frame.Clear(); }
                }
            }
        }
        catch (Exception error) { FailPending(error); }
        finally { _closed = true; FailPending(new BridgeException("connection_closed", "Host output closed")); _events.Writer.TryComplete(); }
    }

    private void HandleFrame(byte[] frame)
    {
        using var message = JsonDocument.Parse(frame.AsMemory(0, frame.Length - 1));
        var root = message.RootElement;
        var kind = root.GetProperty("kind").GetString();
        if (kind == "response")
        {
            var id = root.GetProperty("id").GetString()!;
            if (!TryRemovePending(id, out var completion)) return;
            if (root.GetProperty("ok").GetBoolean()) completion.TrySetResult(root.GetProperty("result").Clone());
            else { var error = root.GetProperty("error"); completion.TrySetException(new BridgeException(error.GetProperty("code").GetString()!, error.GetProperty("message").GetString()!, error.TryGetProperty("field", out var field) ? field.GetString() : null)); }
        }
        else if (kind == "event")
        {
            lock (_eventGate)
            {
                if (_eventBytes + frame.Length > MaxQueueBytes) throw new BridgeException("output_overflow", "Event byte quota reached");
                _eventBytes += frame.Length;
            }
            if (!_events.Writer.TryWrite(new QueuedEvent(root.Clone(), frame.Length)))
            {
                lock (_eventGate) _eventBytes -= frame.Length;
                throw new BridgeException("output_overflow", "Event count quota reached");
            }
        }
        else throw new BridgeException("invalid_frame", "Unknown message kind");
    }

    private async Task DispatchEventsAsync()
    {
        await foreach (var queued in _events.Reader.ReadAllAsync().ConfigureAwait(false))
        {
            try
            {
                var ev = queued.Message;
                var key = EventKey(ev.GetProperty("source").GetString()!, ev.GetProperty("event").GetString()!);
                if (!_handlers.TryGetValue(key, out var handlers)) continue;
                foreach (var handler in handlers.Values.ToArray()) try { await handler(ev).ConfigureAwait(false); } catch { }
            }
            finally { lock (_eventGate) _eventBytes -= queued.Bytes; }
        }
    }

    private static string EventKey(string node, string eventName) => $"{node}\0{eventName}";

    private void RemoveHandler(string key, string listenerId)
    {
        if (!_handlers.TryGetValue(key, out var handlers)) return;
        handlers.TryRemove(listenerId, out _);
        if (handlers.IsEmpty) _handlers.TryRemove(new KeyValuePair<string, ConcurrentDictionary<string, Func<JsonElement, ValueTask>>>(key, handlers));
    }

    private void FailPending(Exception error)
    {
        lock (_pendingGate)
        {
            foreach (var pair in _pending.ToArray())
                if (_pending.TryRemove(pair.Key, out var completion)) completion.TrySetException(new BridgeException("connection_closed", error.Message));
            _pendingSizes.Clear();
            _pendingBytes = 0;
        }
    }

    private bool RemovePending(string id) => TryRemovePending(id, out _);

    private bool TryRemovePending(string id, out TaskCompletionSource<JsonElement> completion)
    {
        lock (_pendingGate)
        {
            if (!_pending.TryRemove(id, out completion!)) return false;
            if (_pendingSizes.Remove(id, out var bytes)) _pendingBytes -= bytes;
            return true;
        }
    }
}

public sealed class Subscription(App app, string handle, string key, string listenerId) : IAsyncDisposable { public ValueTask DisposeAsync() => new(app.UnsubscribeAsync(handle, key, listenerId)); }
public sealed class Window(App app, string handle, string root) { public App App { get; } = app; public string Handle { get; } = handle; public string Root { get; } = root; public Task CloseAsync(CancellationToken ct = default) => App.RequestAsync("window.close", new { window = Handle }, ct); }
public sealed class Node(App app, string handle) { public App App { get; } = app; public string Handle { get; } = handle; public Task AttachAsync(string parent, CancellationToken ct = default) => App.RequestAsync("node.attach", new { node = Handle, parent }, ct); public Task UpdateAsync(object properties, CancellationToken ct = default) => App.RequestAsync("property.update", new { node = Handle, properties }, ct); public Task DestroyAsync(CancellationToken ct = default) => App.RequestAsync("node.destroy", new { node = Handle }, ct); }
public sealed class State(App app, string handle) { public App App { get; } = app; public string Handle { get; } = handle; public Task<JsonElement> GetAsync(CancellationToken ct = default) => App.RequestAsync("state.get", new { state = Handle }, ct); public Task<JsonElement> SetAsync(object value, CancellationToken ct = default) => App.RequestAsync("state.set", new { state = Handle, value }, ct); public Task<JsonElement> DestroyAsync(CancellationToken ct = default) => App.RequestAsync("state.destroy", new { state = Handle }, ct); }

public sealed record CollectionMetadata(long RowCount, double RowHeight, IReadOnlyList<string> Columns)
{
    internal object ToWire()
    {
        if (RowCount < 0 || !double.IsFinite(RowHeight) || RowHeight <= 0 || Columns is null || Columns.Any(column => column is null))
            throw new BridgeException("invalid_value", "Collection metadata is invalid");
        return new { row_count = RowCount, row_height = RowHeight, columns = Columns };
    }
}

public sealed record CollectionRequest(string Revision, string Request, long Start, int Count)
{
    internal static CollectionRequest Parse(JsonElement message)
    {
        if (!message.TryGetProperty("payload", out var payload) || payload.ValueKind != JsonValueKind.Object ||
            !payload.TryGetProperty("revision", out var revision) || revision.ValueKind != JsonValueKind.String ||
            !payload.TryGetProperty("request", out var request) || request.ValueKind != JsonValueKind.String ||
            !payload.TryGetProperty("start", out var start) || !start.TryGetInt64(out var startValue) || startValue < 0 ||
            !payload.TryGetProperty("count", out var count) || !count.TryGetInt32(out var countValue) || countValue is < 0 or > 512)
            throw new BridgeException("invalid_frame", "Collection request fields are invalid");
        return new CollectionRequest(revision.GetString()!, request.GetString()!, startValue, countValue);
    }
}

public sealed class CollectionProvider : IAsyncDisposable
{
    private readonly Node? _node;
    private readonly Channel<CollectionRequest> _requests = Channel.CreateBounded<CollectionRequest>(new BoundedChannelOptions(1) { SingleReader = true, SingleWriter = true, FullMode = BoundedChannelFullMode.Wait });
    private Subscription? _subscription;
    private int _closed;
    private int _disposed;

    internal CollectionProvider(Node? node) => _node = node;

    internal static async Task<CollectionProvider> CreateAsync(Node node, CollectionMetadata metadata, CancellationToken cancellationToken)
    {
        _ = metadata.ToWire();
        var provider = new CollectionProvider(node);
        try
        {
            provider._subscription = await node.App.SubscribeAsync(node.Handle, "data.request", message =>
            {
                provider.Accept(message);
                return ValueTask.CompletedTask;
            }, cancellationToken).ConfigureAwait(false);
            await node.App.RequestAsync("collection.create", new { node = node.Handle, row_count = metadata.RowCount, row_height = metadata.RowHeight, columns = metadata.Columns }, cancellationToken).ConfigureAwait(false);
            return provider;
        }
        catch
        {
            await provider.DisposeAsync().ConfigureAwait(false);
            throw;
        }
    }

    internal void Accept(JsonElement message)
    {
        if (Volatile.Read(ref _closed) != 0) return;
        if (!_requests.Writer.TryWrite(CollectionRequest.Parse(message)))
        {
            var error = new BridgeException("queue_full", "Collection provider request queue is full");
            Volatile.Write(ref _closed, 1);
            _requests.Writer.TryComplete(error);
            throw error;
        }
    }

    public async Task<CollectionRequest?> NextRequestAsync(TimeSpan timeout, CancellationToken cancellationToken = default)
    {
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        if (timeout != Timeout.InfiniteTimeSpan) deadline.CancelAfter(timeout);
        try { return await _requests.Reader.ReadAsync(deadline.Token).ConfigureAwait(false); }
        catch (ChannelClosedException error) when (error.InnerException is BridgeException bridgeError) { throw bridgeError; }
        catch (ChannelClosedException) { return null; }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested) { return null; }
    }

    public Task<JsonElement> ProvideAsync(CollectionRequest request, object rows, CancellationToken cancellationToken = default)
        => App.RequestAsync("data.provide", new { node = RequireNode().Handle, revision = request.Revision, request = request.Request, start = request.Start, rows }, cancellationToken);

    public Task<JsonElement> UpdateAsync(CollectionMetadata metadata, CancellationToken cancellationToken = default)
        => App.RequestAsync("collection.update", new { node = RequireNode().Handle, metadata = metadata.ToWire() }, cancellationToken);

    private App App => RequireNode().App;
    private Node RequireNode() => _node ?? throw new BridgeException("invalid_handle", "Collection provider has no node");

    public async ValueTask DisposeAsync()
    {
        if (Interlocked.Exchange(ref _disposed, 1) != 0) return;
        Volatile.Write(ref _closed, 1);
        while (_requests.Reader.TryRead(out _)) { }
        _requests.Writer.TryComplete();
        if (_subscription is not null) await _subscription.DisposeAsync().ConfigureAwait(false);
    }
}
