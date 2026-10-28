# Grafikr Tutorial

A short guide from an empty application to a custom widget. All coordinates use
DIP (logical pixels); the renderer multiplies them by the window scale.

## 1. Your first application

```rust
use grafikr::{App, Theme, column, text};

fn main() {
    App::new(|| column((text("Hello Grafikr"),)).padding(24.0))
        .theme(Theme::dark())
        .run()
        .unwrap();
}
```

`App::new` accepts a factory that returns any `View`. Every window gets its own
view tree, GPU context, and AccessKit tree.

## 2. Reactive state

```rust
use grafikr::{Signal, View, bind, button, column, row, text};

fn counter() -> impl View {
    let count = Signal::new(0);
    let read = count.clone();
    let less = count.clone();
    let more = count.clone();

    column((
        text(bind(move || format!("Value: {}", read.get()))).size(28.0),
        row((
            button("Less").on_click(move |_| less.update(|n| *n -= 1)),
            button("More").on_click(move |_| more.update(|n| *n += 1)),
        ))
        .gap(8.0),
    ))
    .padding(24.0)
    .gap(16.0)
}
```

`Signal::get` inside `bind` records dependencies. Only nodes that read the signal
are marked dirty.

## 3. Layout

- `column`, `row`, `grid`, and `stack` are the base containers.
- `.padding`, `.gap`, `.width`, and `.height` drive Taffy layout.
- `Stack` layers children and makes them fill the parent.
- `.transform(Transform::translate(x, y))` and `Transform::scale(s)` move or
  scale a subtree visually without changing layout; hit-testing uses the inverse
  transform.

```rust
use grafikr::{Transform, stack};

let badge = stack((text("99+"),))
    .width(64.0)
    .height(32.0)
    .transform(Transform::translate(12.0, -8.0));
```

## 4. Forms

```rust
use grafikr::{Signal, column, text, text_field};

let name = Signal::new(String::new());
let sink = name.clone();
let password = text_field("").password().width(320.0);

column((
    text("Name"),
    text_field("").width(320.0).on_change(move |value| sink.set(value)),
    text("Password"),
    password,
))
.padding(24.0)
.gap(8.0)
```

TextField supports selection, clipboard, IME, multiline input, password masking,
and AccessKit `SetValue`.

## 5. Large lists

```rust
use grafikr::virtual_list;

virtual_list(50_000, 24.0, |index| format!("Row {index}"))
    .width(640.0)
    .height(400.0)
```

Rows are shaped and painted only while visible. `DataGrid` and `TreeView` use the
same virtual pattern.

## 6. Overlays, dialogs, and tabs

```rust
use grafikr::{Signal, button, modal, text};

let open = Signal::new(true);
let close = open.clone();
modal(open, (
    text("Delete file?"),
    button("Close").on_click(move |_| close.set(false)),
))
```

Modal installs a focus trap and restores the previous focus when it closes.
`popup`, `combo_box`, and `tooltip` use overlay layers and paint above normal
content.

## 7. Animation

`Transition` is an `f32` value that moves toward a target. Widgets tick it during
paint and request frames while it is active.

```rust
use grafikr::{Easing, Transition};

let fade = Transition::new(0.0).duration(0.18).easing(Easing::EaseOutCubic);

// inside paint:
fade.set_target(if hovered { 1.0 } else { 0.0 });
if fade.tick(cx.dt) {
    cx.runtime.request_animation();
}
let t = fade.value();
```

The built-in button already uses this pattern for hover. Respect
`theme.reduced_motion` with `fade.jump_to(target)` when animation is not wanted.

## 8. Custom widgets

Implement `Widget`:

- `measure(&mut self, cx, available) -> Size`
- `paint(&mut self, cx, rect)`
- optional `event`, `focusable`, `cursor`, `accepts_text_input`, and
  `accessibility`.

Then create a `View` that calls `cx.add_node(Style)` and `cx.set_widget(id, ...)`.
Add `impl IntoViews for MyView` so it can be used as a child container.

Widgets can append items to the `DisplayList` through `PaintCx::push_rect`,
`push_bordered_rect`, `display.push_layout`, `push_image`, and `push_shadow`. The
renderer never sees the widget tree.

## 9. Tests and benchmarks

```bash
cargo test --workspace
cargo run -p grafikr-bench --release
```

The renderer golden test lives in `crates/grafikr-render/tests/golden.rs`:

```bash
cargo test -p grafikr-render --test golden
# regenerate references after an intentional visual change
GRAFIKR_UPDATE_GOLDEN=1 cargo test -p grafikr-render --test golden
```

The test is skipped when no GPU adapter is available. Set
`GRAFIKR_SKIP_GOLDEN=1` to skip pixel comparison.

`Ui::stats()` reports node, arena-slot, display-item, and text-run counts.
`Renderer::stats()` reports glyph atlas, image cache, vertex, and batch counts
from the last frame.

## 10. Accessibility

Every node with a widget can populate AccessKit in `Widget::accessibility`.
The `Click`, `Focus`, and `SetValue` actions from a screen reader are forwarded
to the widget. Use `modal_scope` for dialogs and give every control a clear label.
