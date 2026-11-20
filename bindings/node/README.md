# Grafikr Node.js and TypeScript SDK

Node 24+ desktop client. Call `Client.start({ bundle })` with an absolute path
to a verified runtime bundle, or install the matching `@grafikr/runtime-*`
platform package for your target. Import never downloads or starts native code. Await
`client.ready` to complete protocol negotiation. `request(op, args)` exposes
the protocol surface, and `Window`, `Node`, `State`, and `Subscription` provide
the common object helpers. i64 State values use JavaScript `bigint` and are
encoded as protocol decimal strings. Callback exceptions are isolated from
transport I/O; callbacks run sequentially and may submit nested requests.
`close()` waits for shutdown and terminates only the child started by that
client.

The interactive counter/form example is in `examples/counter-form.mjs`.
TypeScript declarations are generated with `npm run build`.

For `VirtualList` and `DataGrid`, `createCollectionProvider(node, metadata)`
subscribes before collection creation. Call `nextRequest()` to receive a
revision-tagged range, respond with `provide(request, rows)`, use `update()` for
new metadata, and `close()` to unsubscribe and resolve a waiting read with
`null`. The provider allows one queued range and one pending read.
