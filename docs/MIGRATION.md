# Migrating to 1.0

Release 1.0.0 freezes the public API of the `grafikr` crate. Since 0.1.0 was
never published, the following changes are the relevant upgrade surface.

## Version

All workspace crates are now `1.0.0`. Use an explicit version with the path
dependency:

```toml
grafikr = "1.0"
```

## Style

`Style` gained `shadow` and `transform` fields. Always close struct literals with
`..Style::default()`:

```rust
Style {
    background: Some(color),
    radius: 8.0,
    ..Style::default()
}
```

`Style::transform` takes a `Transform`:

```rust
use grafikr::Transform;

style.transform = Transform::translate(12.0, 0.0);
// or
style.transform = Transform::scale(1.05);
```

Transform supports translation and uniform scale only. Layout is unchanged;
hit-testing applies the inverse automatically. Rotation and offscreen layers are
not available yet.

## Containers

`column`, `row`, `grid`, and `stack` gained a `.transform(Transform)` builder.
Existing calls are unchanged.

## Animation

New `Transition` and `Easing` APIs in the `grafikr` crate:

```rust
let fade = Transition::new(0.0).duration(0.18).easing(Easing::EaseOutCubic);
fade.set_target(1.0);
let active = fade.tick(dt);
```

`PaintCx` now provides `dt: f32` (seconds since the previous frame). Animated
widgets call `cx.runtime.request_animation()` while `tick` returns `true`. The
built-in button already uses a transition for hover; `Theme::reduced_motion`
disables animation with `jump_to`.

## Events

`UiEvent` gained `SetValue(String)` for the AccessKit `Action::SetValue` action.
Custom widgets with exhaustive matches must add a wildcard or handle this variant.

`TextField` now advertises `Role::PasswordInput` and `Action::SetValue`, and uses
`Role::MultilineTextInput` for multiline fields.

## Renderer

- `DisplayList` gained `DisplayItem::Shadow` and `apply_transform`.
- `Renderer::stats()` returns `RendererStats`.
- The golden image test lives in `crates/grafikr-render/tests/golden.rs`.

There are no vertex-format or shader changes that affect API consumers.

## Shell

- Surface `Lost`/`Outdated` is reconfigured automatically.
- Device loss recreates the instance, adapter, device, surface, and renderer for
  every window. Applications do not need to handle this.
- `Ui::mount` now clears the previous tree before mounting again.

## Testing

- `cargo test --workspace` runs unit, integration, and golden tests.
- `GRAFIKR_SKIP_GOLDEN=1` skips pixel comparison.
- `GRAFIKR_UPDATE_GOLDEN=1` rewrites the golden references.
