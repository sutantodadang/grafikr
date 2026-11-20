using Grafikr;

await using var app = await App.StartAsync(new RuntimeOptions());
var window = await app.CreateWindowAsync("Counter and form", 480, 320);
var label = await app.CreateNodeAsync(window, "Text", new { text = "Count: 0" });
await label.AttachAsync(window.Root);
var button = await app.CreateNodeAsync(window, "Button", new { label = "Add" });
await button.AttachAsync(window.Root);
var count = await app.CreateStateAsync(new { type = "i64", value = "0" });
await app.SubscribeAsync(button.Handle, "click", async _ =>
{
    var value = long.Parse((await count.GetAsync()).GetProperty("value").GetString()!);
    if (value == long.MaxValue) return;
    value++;
    await count.SetAsync(new { type = "i64", value = value.ToString(System.Globalization.CultureInfo.InvariantCulture) });
    await label.UpdateAsync(new { text = $"Count: {value}" });
});
var textField = await app.CreateNodeAsync(window, "TextField", new { text = "Grafikr 雪", width = 320 });
await textField.AttachAsync(window.Root);
var preview = await app.CreateNodeAsync(window, "Text", new { text = "Grafikr 雪" });
await preview.AttachAsync(window.Root);
await app.SubscribeAsync(textField.Handle, "change", async message =>
{
    if (message.GetProperty("payload").TryGetProperty("text", out var value) && value.ValueKind == System.Text.Json.JsonValueKind.String)
        await preview.UpdateAsync(new { text = value.GetString() });
});
await Task.Delay(Timeout.InfiniteTimeSpan);
