using System.Text.Json;
using System.Collections.Concurrent;
using System.Reflection;
using System.Diagnostics;
using Grafikr;

if (args.Length == 2 && args[0] == "--sentinel")
{
    await Task.Delay(300);
    await File.WriteAllTextAsync(args[1], "alive");
    return;
}

if (App.MaxFrameBytes != 8 * 1024 * 1024) throw new Exception("frame limit drift");
var collection = new CollectionProvider(null);
using (var collectionEvent = JsonDocument.Parse("{\"payload\":{\"revision\":\"1\",\"request\":\"2\",\"start\":4,\"count\":3}}"))
{
    collection.Accept(collectionEvent.RootElement);
    if ((await collection.NextRequestAsync(TimeSpan.FromSeconds(1)))?.Start != 4) throw new Exception("collection request was lost");
    await collection.DisposeAsync();
    if (await collection.NextRequestAsync(TimeSpan.FromSeconds(1)) is not null) throw new Exception("closed collection provider did not wake its reader");
}
var overflowedCollection = new CollectionProvider(null);
using (var collectionEvent = JsonDocument.Parse("{\"payload\":{\"revision\":\"1\",\"request\":\"2\",\"start\":4,\"count\":3}}"))
{
    overflowedCollection.Accept(collectionEvent.RootElement);
    var overflow = false;
    try { overflowedCollection.Accept(collectionEvent.RootElement); }
    catch (BridgeException error) when (error.Code == "queue_full") { overflow = true; }
    if (!overflow) throw new Exception("collection request queue was not bounded");
}
if ((await overflowedCollection.NextRequestAsync(TimeSpan.FromSeconds(1)))?.Start != 4) throw new Exception("queued collection request was lost");
var collectionFailed = false;
try { await overflowedCollection.NextRequestAsync(TimeSpan.FromSeconds(1)); }
catch (BridgeException error) when (error.Code == "queue_full") { collectionFailed = true; }
if (!collectionFailed) throw new Exception("collection request overflow was hidden");
await overflowedCollection.DisposeAsync();
using var eventJson = JsonDocument.Parse("{\"kind\":\"event\",\"sequence\":\"1\",\"event\":\"change\",\"source\":\"7\",\"payload\":{}}");
if (eventJson.RootElement.GetProperty("source").GetString() != "7") throw new Exception("opaque handle string changed");

var repository = new DirectoryInfo(AppContext.BaseDirectory);
while (repository is not null && !File.Exists(Path.Combine(repository.FullName, "Cargo.toml"))) repository = repository.Parent;
if (repository is null) throw new Exception("repository root not found");
var fixtureHost = Path.Combine(repository.FullName, "target", "debug", OperatingSystem.IsWindows() ? "fixture-host.exe" : "fixture-host");
fixtureHost = Environment.GetEnvironmentVariable("GRAFIKR_FIXTURE_HOST") ?? fixtureHost;
Environment.SetEnvironmentVariable("GRAFIKR_FIXTURE_EARLY_SUBSCRIBE", "1");
await using var app = await App.StartAsync(fixtureHost);
var window = await app.CreateWindowAsync("fixture", 320, 240);
var button = await app.CreateNodeAsync(window, "Button", new { label = "click" });
var observed = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
await using var subscription = await app.SubscribeAsync(button.Handle, "click", message =>
{
    observed.TrySetResult(message.GetProperty("source").GetString()!);
    return ValueTask.CompletedTask;
});
if (await observed.Task.WaitAsync(TimeSpan.FromSeconds(2)) != button.Handle) throw new Exception("early event was not delivered");
if (Environment.GetEnvironmentVariable("GRAFIKR_FIXTURE_WIDGET_INVENTORY") is not null)
    await VerifyWidgetInventory(app, window);
await VerifyProtocolCases(app, fixtureHost);
await window.CloseAsync();
var pendingField = typeof(App).GetField("_pending", BindingFlags.Instance | BindingFlags.NonPublic)!;
var pending = (ConcurrentDictionary<string, TaskCompletionSource<JsonElement>>)pendingField.GetValue(app)!;
for (var i = 0; i < 256; i++) pending.TryAdd($"held-{i}", new TaskCompletionSource<JsonElement>());
var rejectedAtLimit = false;
try { await app.RequestAsync("shutdown", new { }); }
catch (BridgeException error) when (error.Code == "queue_full") { rejectedAtLimit = true; }
finally { pending.Clear(); }
if (!rejectedAtLimit) throw new Exception("257th outstanding request was not rejected");
Console.WriteLine("Grafikr.Client checks passed");

static async Task VerifyWidgetInventory(App app, Window window)
{
    var repository = new DirectoryInfo(AppContext.BaseDirectory);
    while (repository is not null && !File.Exists(Path.Combine(repository.FullName, "Cargo.toml"))) repository = repository.Parent;
    if (repository is null) throw new Exception("repository root not found");
    using var inventory = JsonDocument.Parse(File.ReadAllText(Path.Combine(repository.FullName, "bindings", "protocol", "v1", "widgets.json")));
    var state = await app.CreateStateAsync(new { type = "i64", value = "1" });
    var upload = await app.RequestAsync("image.begin", new { width = 1, height = 1, byte_length = 4 });
    var uploadId = upload.GetProperty("upload").GetString()!;
    await app.RequestAsync("image.chunk", new { upload = uploadId, offset = 0, data = Convert.ToBase64String([0, 0, 0, 255]) });
    var image = (await app.RequestAsync("image.finalize", new { upload = uploadId })).GetProperty("image").GetString()!;

    foreach (var widget in inventory.RootElement.GetProperty("widgets").EnumerateObject())
    {
        var name = widget.Name;
        var properties = new Dictionary<string, object?>();
        foreach (var property in widget.Value.GetProperty("properties").EnumerateObject())
            properties.Add(property.Name, SampleWidgetValue(name, property.Name, property.Value.GetString()!, state.Handle, image));
        Node node;
        try { node = await app.CreateNodeAsync(window, name, properties); }
        catch (BridgeException error) { throw new Exception($"{name} create failed ({error.Code}): {error.Message}", error); }
        await node.AttachAsync(window.Root);
        if (widget.Value.TryGetProperty("events", out var events))
        {
            foreach (var eventName in events.EnumerateArray().Select(value => value.GetString()!))
            {
                var received = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
                await using var subscription = await app.SubscribeAsync(node.Handle, eventName, message =>
                {
                    received.TrySetResult(message.Clone());
                    return ValueTask.CompletedTask;
                });
                var message = await received.Task.WaitAsync(TimeSpan.FromSeconds(2));
                if (message.GetProperty("source").GetString() != node.Handle || message.GetProperty("event").GetString() != eventName)
                    throw new Exception($"{name}@{eventName} returned the wrong event");
            }
        }
        await node.DestroyAsync();
    }
    await app.RequestAsync("image.destroy", new { image });
    await state.DestroyAsync();
}

static async Task VerifyProtocolCases(App app, string fixtureHost)
{
    await VerifyOutOfOrderResponses(fixtureHost);
    var first = await app.CreateStateAsync(new { type = "i64", value = "7" });
    var second = await app.CreateStateAsync(new { type = "i64", value = "8" });
    var reversed = await Task.WhenAll(
        app.RequestAsync("state.get", new { state = first.Handle }),
        app.RequestAsync("state.get", new { state = second.Handle }));
    if (reversed[0].GetProperty("value").GetString() != "7" || reversed[1].GetProperty("value").GetString() != "8")
        throw new Exception("out-of-order responses were correlated to the wrong request");

    var text = "line one\nline two 雪 😀" + new string('x', 102_400);
    var textState = await app.CreateStateAsync(new { type = "string", value = text });
    if ((await textState.GetAsync()).GetProperty("value").GetString() != text)
        throw new Exception("large UTF-8 state frame did not round trip");
    var boundary = await app.CreateStateAsync(new { type = "i64", value = "-9223372036854775808" });
    if ((await boundary.GetAsync()).GetProperty("value").GetString() != "-9223372036854775808")
        throw new Exception("i64 minimum did not round trip");
    await boundary.SetAsync(new { type = "i64", value = "9223372036854775807" });
    if ((await boundary.GetAsync()).GetProperty("value").GetString() != "9223372036854775807")
        throw new Exception("i64 maximum did not round trip");

    var nonFiniteRejected = false;
    try { await app.RequestAsync("state.create", new { value = new { type = "f64", value = double.NaN } }); }
    catch (BridgeException error) when (error.Code == "invalid_value") { nonFiniteRejected = true; }
    if (!nonFiniteRejected) throw new Exception("non-finite JSON value did not return invalid_value");

    await VerifyPendingByteLimit(fixtureHost);
    await VerifyPendingEof(fixtureHost);
    await VerifyUnsubscribeRace(fixtureHost);
    await VerifyBlockedCallbackQueue(fixtureHost);
    await VerifyUnrelatedProcessSurvivesClose(fixtureHost);
}

static async Task VerifyOutOfOrderResponses(string fixtureHost)
{
    await using var app = await StartFixtureWithEnvironment(fixtureHost, ("GRAFIKR_FIXTURE_REVERSE_RESPONSES", "1"));
    var first = await app.CreateStateAsync(new { type = "i64", value = "7" });
    var second = await app.CreateStateAsync(new { type = "i64", value = "8" });
    var reversed = await Task.WhenAll(
        app.RequestAsync("state.get", new { state = first.Handle }),
        app.RequestAsync("state.get", new { state = second.Handle }));
    if (reversed[0].GetProperty("value").GetString() != "7" || reversed[1].GetProperty("value").GetString() != "8")
        throw new Exception("out-of-order responses were correlated to the wrong request");
}

static async Task VerifyPendingByteLimit(string fixtureHost)
{
    await using var app = await StartFixtureWithEnvironment(fixtureHost, ("GRAFIKR_FIXTURE_HOLD_ALL_RESPONSES", "1"));
    var state = await app.CreateStateAsync(new { type = "string", value = "seed" });
    var padding = new string('x', 100_000);
    var accepted = Enumerable.Range(0, 167)
        .Select(_ => state.SetAsync(new { type = "string", value = padding }))
        .ToArray();
    var limited = false;
    try { await state.SetAsync(new { type = "string", value = padding }); }
    catch (BridgeException error) when (error.Code == "queue_full") { limited = true; }
    if (!limited) throw new Exception("16 MiB pending request byte quota was not enforced");
    await app.DisposeAsync();
    var rejected = await Task.WhenAll(accepted.Select(CaptureBridgeCode));
    if (rejected.Any(code => code != "connection_closed")) throw new Exception("accepted pending requests were not released on close");
}

static async Task VerifyPendingEof(string fixtureHost)
{
    await using var app = await StartFixtureWithEnvironment(fixtureHost,
        ("GRAFIKR_FIXTURE_HOLD_RESPONSES", "1"), ("GRAFIKR_FIXTURE_EXIT_AFTER_HELLO_MS", "500"));
    var pending = Enumerable.Range(0, 256)
        .Select(_ => app.RequestAsync("state.get", new { state = "0" }))
        .ToArray();
    var rejected = await Task.WhenAll(pending.Select(CaptureBridgeCode).ToArray());
    if (rejected.Any(code => code != "connection_closed")) throw new Exception("EOF did not reject all 256 pending requests");
}

static async Task VerifyUnsubscribeRace(string fixtureHost)
{
    await using var app = await StartFixtureWithEnvironment(fixtureHost,
        ("GRAFIKR_FIXTURE_EARLY_SUBSCRIBE", "1"),
        ("GRAFIKR_FIXTURE_UNSUBSCRIBE_EVENT", "1"),
        ("GRAFIKR_FIXTURE_DELAY_UNSUBSCRIBE_RESPONSE_MS", "200"));
    var window = await app.CreateWindowAsync("unsubscribe", 320, 240);
    var button = await app.CreateNodeAsync(window, "Button", new { label = "click" });
    await button.AttachAsync(window.Root);
    var count = 0;
    var firstEvent = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
    var subscription = await app.SubscribeAsync(button.Handle, "click", _ =>
    {
        Interlocked.Increment(ref count);
        firstEvent.TrySetResult();
        return ValueTask.CompletedTask;
    });
    await firstEvent.Task.WaitAsync(TimeSpan.FromSeconds(2));
    await subscription.DisposeAsync();
    await Task.Delay(30);
    if (count != 1) throw new Exception("event dispatched after unsubscribe began");
}

static async Task VerifyBlockedCallbackQueue(string fixtureHost)
{
    await using var app = await App.StartAsync(fixtureHost);
    var window = await app.CreateWindowAsync("blocked callbacks", 320, 240);
    var button = await app.CreateNodeAsync(window, "Button", new { label = "click" });
    await button.AttachAsync(window.Root);
    var callbackEntered = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
    var releaseCallback = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
    await using var subscription = await app.SubscribeAsync(button.Handle, "click", async _ =>
    {
        callbackEntered.TrySetResult();
        await releaseCallback.Task;
    });
    await callbackEntered.Task.WaitAsync(TimeSpan.FromSeconds(2));
    var handleFrame = typeof(App).GetMethod("HandleFrame", BindingFlags.Instance | BindingFlags.NonPublic)!;
    byte[] Frame(int sequence)
    {
        var json = JsonSerializer.SerializeToUtf8Bytes(new { kind = "event", sequence = sequence.ToString(), @event = "click", source = button.Handle, payload = new { } });
        return [.. json, (byte)'\n'];
    }
    for (var sequence = 1; sequence <= 1024; sequence++) handleFrame.Invoke(app, [Frame(sequence)]);
    var overflow = false;
    try { handleFrame.Invoke(app, [Frame(1025)]); }
    catch (TargetInvocationException error) when (error.InnerException is BridgeException bridge && bridge.Code == "output_overflow") { overflow = true; }
    finally { releaseCallback.TrySetResult(); }
    if (!overflow) throw new Exception("blocked callback queue did not fail at its event limit");
}

static async Task VerifyUnrelatedProcessSurvivesClose(string fixtureHost)
{
    var marker = Path.Combine(Path.GetTempPath(), $"grafikr-sentinel-{Guid.NewGuid():N}");
    var executable = Environment.ProcessPath ?? throw new Exception("test executable path is unavailable");
    var start = new ProcessStartInfo(executable) { UseShellExecute = false, CreateNoWindow = true };
    if (Path.GetFileNameWithoutExtension(executable).Equals("dotnet", StringComparison.OrdinalIgnoreCase))
        start.ArgumentList.Add(Assembly.GetEntryAssembly()!.Location);
    start.ArgumentList.Add("--sentinel");
    start.ArgumentList.Add(marker);
    using var sentinel = Process.Start(start) ?? throw new Exception("sentinel process did not start");
    await using (var app = await App.StartAsync(fixtureHost)) await app.DisposeAsync();
    await sentinel.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(3));
    try
    {
        if (await File.ReadAllTextAsync(marker) != "alive") throw new Exception("closing Grafikr affected an unrelated process");
    }
    finally { File.Delete(marker); }
}

static async Task<App> StartFixtureWithEnvironment(string host, params (string Name, string Value)[] variables)
{
    var previous = variables.ToDictionary(variable => variable.Name, variable => Environment.GetEnvironmentVariable(variable.Name));
    foreach (var variable in variables) Environment.SetEnvironmentVariable(variable.Name, variable.Value);
    try { return await App.StartAsync(host); }
    finally { foreach (var (name, value) in previous) Environment.SetEnvironmentVariable(name, value); }
}

static async Task<string?> CaptureBridgeCode(Task<JsonElement> request)
{
    try { await request; return null; }
    catch (BridgeException error) { return error.Code; }
}

static object? SampleWidgetValue(string widget, string property, string type, string state, string image)
{
    if (widget == "Radio" && property == "group") return state;
    if (widget == "TreeView" && property == "selected") return "root";
    if (widget == "Slider") return property switch { "min" => 0.0, "max" => 10.0, "width" => 120.0, _ => 1.0 };
    if (widget == "Spinner" && property == "size") return 16.0;
    if (property == "row_count") return 1;
    if (property == "row_height") return 24.0;
    return type switch
    {
        "number" or "f32" => 1.0,
        "u16" => 1,
        "u64" => 0,
        "string" => "sample",
        "color" => "#336699",
        "transform" => new { x = 0.0, y = 0.0, scale = 1.0 },
        "boolean" => false,
        "primary|secondary|icon" => "primary",
        "state" => state,
        "GridColumn[]" => new[] { new { title = "Column", width = 120.0 } },
        "string[]" => new[] { "One", "Two" },
        "TreeNode[]" => new[] { new { key = "root", label = "Root", children = Array.Empty<object>() } },
        "CanvasCommand[]" => new[] { new { kind = "rect", rect = new { x = 0.0, y = 0.0, width = 1.0, height = 1.0 }, color = "#336699", radius = 0.0 } },
        "ImageHandle" => image,
        _ => throw new Exception($"No .NET conformance value for {widget}.{property}: {type}"),
    };
}
