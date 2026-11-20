# Grafikr .NET SDK

Targets .NET 10 and uses only the BCL. Packaged apps call
`App.StartAsync(new RuntimeOptions())` to validate and launch the matching RID
host from the NuGet package. An explicit absolute host path remains available
for development. The SDK offers async request, window, node, state, and event
subscription APIs. `DisposeAsync` requests shutdown and waits five seconds
before terminating only its own host process. The counter/form example is in
`examples/CounterForm`; `scripts.verify_dotnet_package` packs the runtime and
checks it from a clean consumer.

`App.CreateCollectionProviderAsync(node, metadata)` subscribes to visible-range
requests before publishing metadata. Read requests with `NextRequestAsync`,
answer them with `ProvideAsync`, and refresh metadata with `UpdateAsync`.
Disposing the provider unsubscribes and wakes a waiting reader.
