# ADR 0002 — Signals with implicit dependency tracking

**Status:** accepted (M0/M2)

## Context

Target API:

```rust
let count = Signal::new(0);
text(bind(move || format!("Value: {}", count.get())))
```

Developers must not register subscriptions manually, and there must be no
virtual DOM diff.

## Decision

- `Signal<T>` stores its value in `Rc<RefCell<T>>`, a version, and a subscriber
  list of `Weak<dyn Fn()>`.
- During mount, `BoundText` is evaluated inside a thread-local observer. Every
  `Signal::get()` during that evaluation registers the observer.
- When the value changes, subscribers run. A widget subscriber marks its node
  with `DirtyFlags::LAYOUT`; the shell requests a redraw.
- Paint-time evaluation has no observer, so it does not add subscriptions.
- Subscribers are `Weak`, so a released node/widget cannot make a signal leak
  memory.
- Signals are not `Send`: all UI state lives on the UI thread. Background tasks
  send messages through a queue to the UI thread later.

## Consequences

- Dependencies are dynamic: an `if` branch inside a closure subscribes only to
  the signals actually read during mount. A condition change must be triggered
  through a signal that was already read; this pattern fits text binding, not
  dynamic tree structure.
- Writing a signal inside a `get()` evaluation causes re-entrancy. The API avoids
  this in practice because `get` only reads.
- There is no batching/transaction yet. Several sequential `set` calls trigger
  several invalidations; the event loop still coalesces redraws.
- Computed values are not implemented yet; they can be added as derived signals
  with a version cache.
