# ADR 0001 — Retained widget tree instead of immediate mode

**Status:** accepted (M0/M2)

## Context

Immediate mode is easy to build, but a large form/dashboard application would
rebuild the tree, layout, hit-testing, and accessibility every frame. Focus,
selection, and internal widget state are also hard to preserve.

## Decision

- Keep a tree of nodes alive for the lifetime of the window.
- `NodeId` is stable: arena index + generation, so a removed node cannot be
  reused accidentally.
- Each node stores style, layout rect, interaction state, and an optional
  `Box<dyn Widget>`.
- Containers have no widget; Taffy lays out their children.
- Dirty flags are separate: STYLE, LAYOUT, PAINT, ACCESSIBILITY. Style triggers
  layout; layout triggers paint + accessibility; PAINT alone does not trigger
  layout.
- Signal updates mark specific nodes; the tree is not diffed or rebuilt.

## Consequences

- Node lifetimes and subscription cleanup need care (subscribers use `Weak`, so
  dead callbacks are dropped automatically).
- Layout still starts at the root. Taffy caches per node so only dirty branches
  are computed, but root traversal remains O(number of nodes). Subtree-only
  layout is follow-up work, not an architecture change.
- The display list is rebuilt on paint; this is simple and correct. Damage
  tracking/partial present is added later if profiling shows a need.

## Rejected alternatives

- Immediate mode: fragile state/focus/accessibility and repeated per-frame work.
- Virtual DOM + diff: extra allocation and complexity without benefit because
  signal dependencies already mark changed nodes.
