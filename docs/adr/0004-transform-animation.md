# ADR 0004 — Visual transforms and animation

**Status:** accepted (1.0)

## Context

Widgets need to move or scale a subtree without changing layout, and to animate
values (hover, fade) without global threads or timers.

## Decision

- `Style::transform` stores translation + uniform scale. Taffy layout is
  unchanged.
- Paint applies the transform to every `DisplayItem` produced by the node (rect,
  glyph, image, shadow) after the subtree finishes drawing. Nested transforms
  compose automatically.
- Hit-testing maps points with the inverse transform per node.
- Clips remain screen-space; a transformed node's own clip is transformed with
  it.
- `Transition` is an `f32` value backed by `Signal`. Widgets call `tick(dt)`
  during paint and `runtime.request_animation()` while active.
- `PaintCx::dt` is the time since the previous frame, capped at 0.1 seconds.
- `Theme::reduced_motion` uses `jump_to` instead of animating.

## Consequences

- Rotation, non-uniform scale, and offscreen layers are not supported yet.
- Text scaling rasterizes glyphs at the new size; the atlas cache can grow during
  a scale animation.
- Clips from transformed ancestors are not remapped; clips stay screen-space.
  This is correct for common cases, but nested clips with transforms need review
  if layers are added.
- Animation runs on the UI thread and stops when idle.
