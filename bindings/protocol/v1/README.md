# Grafikr protocol 1.0

Requests are UTF-8 JSON objects terminated by LF. A frame is at most 8 MiB including LF and JSON nesting is limited to 64. Request IDs are 1–64 UTF-8 bytes and must be unique while outstanding. `args` is operation-specific; unknown fields are errors. Opaque handles, event sequences, state `i64` values, and indices whose schema says decimal token are canonical unsigned/signed decimal strings to preserve precision across JavaScript.

The normative operation names and widget inventory are in `schema.json` and `widgets.json`. The runtime validates the complete operation payload before applying a mutation. Responses have `kind: "response"`, `id`, `ok`, and exactly one of `result` or `error`. Events have `kind: "event"`, decimal `sequence`, `event`, decimal `source`, and `payload`.

The schema and runtime enforce resource quotas from the design spec. A successful response means the retained UI model changed; it does not guarantee a frame has already appeared. No request is replayed after transport failure.

After `hello`, the response reports the selected protocol version, every
implemented feature group in `capabilities`, and the active limits for framing,
queues, handles, images, Canvas, stored model data, collections, and TreeView.
Clients must check these values before using optional features or scheduling
work beyond the advertised quotas.

Protocol 1.0 currently advertises `windows`, `nodes`, `properties`, `state`,
`bindings`, `events`, `resources`, `canvas`, `collections`, `tree_view`, and
`transitions`.

TreeView node keys are unique strings within the tree. `selected` is a key, not
a positional index. Replacing `roots` through `property.update` preserves
expanded nodes and selection whose keys still exist, even when sibling order
changes. A `select` event carries `{ "key": "..." }` for TreeView and
`{ "index": "..." }` for index-based selection widgets. `data.request` asks a
client for the visible collection range; its payload carries decimal-string
revision and request tokens plus numeric `start` and `count`.

`transition.start` animates an `f64` state to another `f64` over an integer
`duration_ms` (`u32`). Easing is `linear`, `ease_out_cubic`, or
`ease_in_out_cubic`; setting or destroying the state cancels its active
transition. `window.create` and `window.update` accept an optional
`reduced_motion` boolean; enabled windows complete bound transitions immediately.
