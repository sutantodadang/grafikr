# Grafikr — Plan and Roadmap

**Status:** Initial proposal  
**Date:** October 27, 2026

## 1. Vision

**Grafikr** is a native desktop UI framework for Rust that combines:

- Native GPU rendering like Flutter, **without a WebView**.
- Customizable widgets like Qt.
- A declarative API that is easy to use.
- High performance with updates limited to what changed.
- Windows, Linux, and macOS support.
- Accessibility, Unicode, IME, and keyboard navigation as core features.

Product position:

> Build Rust desktop applications with a simple API, a consistent look, and
> full control over rendering.

Grafikr uses native OS windows and events, but draws its own widgets through the
GPU. Grafikr is not a WinUI, Cocoa, or GTK wrapper.

## 2. Goals and Initial Constraints

### 2.1 Primary goals

1. Easy to use for CRUD apps, dashboards, internal tools, and productivity apps.
2. Low resource usage when idle.
3. Consistent styling and theming across operating systems.
4. Custom widgets without modifying the engine.
5. Text input, scrolling, focus, and accessibility work correctly.
6. Developers do not need to understand the GPU to build an app.

### 2.2 Not MVP goals

- Web, Android, and iOS.
- Full CSS compatibility.
- A visual designer like Qt Designer.
- Hot reload.
- A Word-class rich-text editor.
- A browser engine.
- Built-in video player and charting.
- Every Qt control at once.

These features can be built after the core is stable.

## 3. Architecture Decisions

| Area | Choice | Reason |
| --- | --- | --- |
| Window and OS events | `winit` | Cross-platform, native event loop |
| GPU rendering | `wgpu` | Vulkan, Metal, D3D12, and OpenGL through one API |
| UI model | **Retained widget tree + reactive state** | No need to rebuild the whole UI |
| Layout | `taffy` | Block, Flexbox, and Grid without writing a layout engine |
| Text layout | `parley` | Unicode, shaping, font fallback, BiDi, and line breaking |
| Accessibility | `accesskit` | Integration with each OS accessibility API |
| Complex vector graphics | Evaluate `vello` after the base renderer is stable | Avoid a core dependency before it is proven necessary |
| Styling | Rust typed style + theme tokens | Type-safe and no CSS parser |
| UI coordinates | Logical pixels/DIP | Supports DPI scaling |
| Async | Tokio is not required | The framework stays light; developers choose their runtime |

### 3.1 Why not pure immediate mode?

Immediate mode is easy to build, but large applications tend to run build,
layout, or hit-test work repeatedly. The retained model is a better fit for:

- Large forms.
- Widgets with internal state.
- Focus and accessibility.
- Custom components.
- Updating part of a subtree.

### 3.2 Why not a virtual DOM?

Virtual DOM diffing adds allocation and complexity. Grafikr uses:

- Nodes with stable IDs.
- Reactive dependency tracking.
- Dirty flags for layout, paint, and accessibility.
- Direct updates to the affected node.

## 4. System Architecture

```text
Application
    │
    ▼
Declarative Views / Built-in Widgets / Custom Widgets
    │
    ▼
Reactive Runtime
├── Signals and computed values
├── Subscriptions
├── Commands and background tasks
└── Dirty-node tracking
    │
    ▼
UI Core
├── Retained widget tree
├── Styling and theme resolution
├── Focus and pointer capture
├── Event capture/bubble
├── Animation scheduler
└── Accessibility tree
    │
    ├───────────────┐
    ▼               ▼
Layout Engine      Platform Shell
└── Taffy          ├── winit event loop
└── Custom layout  ├── Window and monitor
                   ├── Keyboard/mouse
                   ├── IME and clipboard
                   └── Drag-and-drop
    │
    ▼
Display List
├── Rect and rounded rect
├── Text runs
├── Images
├── Borders and shadows
├── Clips and transforms
└── Layers
    │
    ▼
GPU Renderer — wgpu
├── Batch management
├── Glyph/image atlas
├── Texture cache
├── Scissor and clipping
└── Surface presentation
```

### 4.1 Update flow

```text
OS event
  → dispatch to widget
  → state changes
  → mark node dirty
  → style if needed
  → layout only the affected subtree
  → rebuild changed display-list items
  → update accessibility tree
  → render one frame
  → sleep again
```

Dirty flags are separate:

```text
STYLE | LAYOUT | PAINT | ACCESSIBILITY
```

A color change must not trigger layout. A pointer move must not rebuild the
widget tree.

## 5. Developer API Design

The following usage targets illustrate the API. They are **not final API**:

```rust
fn counter() -> impl View {
    let count = Signal::new(0);

    column((
        text(bind(move || format!("Value: {}", count.get()))),
        row((
            button("Less")
                .on_click(move |_| count.update(|value| *value -= 1)),
            button("More")
                .on_click(move |_| count.update(|value| *value += 1)),
        ))
        .gap(8),
    ))
    .padding(16)
    .gap(12)
}

fn main() {
    grafikr::App::new(counter)
        .theme(Theme::dark())
        .run();
}
```

API principles:

- Composition over inheritance.
- Macros are not required for simple applications.
- `Signal<T>` is used for local state.
- Computed values are evaluated only when their dependencies change.
- Event handlers never deal with the renderer.
- Resources such as fonts and images use handles, not duplicated data.
- Runtime errors are handled without killing the whole application when
  possible.

### 5.1 Four levels of customization

1. **Compose:** combine existing widgets.
2. **Style:** change colors, padding, typography, and hover/active states.
3. **Custom paint:** draw your own visuals while still using Grafikr layout and
   events.
4. **Full widget:** implement measure, layout, event, paint, focus, and
   accessibility.

Advanced widgets use a trait with optional hooks. Developers are not forced to
implement every method.

## 6. Styling and Components

### 6.1 Theme tokens

```text
colors:
  background, surface, foreground, muted, accent, danger

typography:
  font_family, body, caption, heading, monospace

metrics:
  radius, border_width, control_height, spacing

motion:
  duration, easing, reduced_motion
```

### 6.2 State styling

Every interactive widget supports:

```text
default | hovered | pressed | focused | disabled | selected | invalid
```

Styles are resolved to final values before layout/paint. Avoid parsing strings
on every frame.

### 6.3 MVP components

- Text and icons.
- Button, icon button, and toggle.
- Checkbox, radio, switch, and slider.
- TextField, multiline input, and password input.
- Stack, row, column, flex, and container.
- ScrollView and ScrollBar.
- Virtual list.
- Tooltip, popup, menu, and modal/dialog.
- Progress and spinner.
- Window and basic tabs.

### 6.4 After MVP

- Grid layout.
- ComboBox and DatePicker.
- TreeView and virtual DataGrid.
- Splitter and docking.
- Rich text.
- SVG.
- Notification/toast.
- Context menu and advanced native drag-and-drop.

DataGrid must be virtual from the start. Creating 50,000 widgets for 50,000 rows
is not acceptable.

## 7. Performance and Memory Strategy

### 7.1 Rendering

- Render only when something is invalidated or an animation is active.
- No continuous 60 FPS loop while idle.
- Batch quads, glyphs, and images by pipeline/texture.
- Glyph and image atlases have memory limits.
- Reuse vertex/index buffers and display-list allocations.
- Cache text layout by content, font, size, and constraint.
- Add damage tracking gradually; partial presentation is not assumed to work the
  same on every platform.

### 7.2 UI core

- Stable `NodeId` backed by an arena/slotmap.
- Layout only dirty subtrees.
- Avoid new `String`, `Vec`, and clones on hot paths.
- Use inline storage for small child/property counts.
- No `Arc<Mutex<...>>` per widget.
- Background tasks send results to the UI through an event queue; the widget
  tree stays on the UI thread.

### 7.3 Memory

- Glyph, image, and text caches have limits and eviction.
- Resources are shared through handles.
- The renderer does not copy the whole widget tree.
- Repeated mount/unmount tests detect leaks.
- Keep CPU-side RSS metrics separate from estimated GPU memory.

### 7.4 Initial targets to validate in the spike

- Idle: no layout/render and CPU near zero.
- One label changes: the whole application is not rebuilt.
- 50,000-row list: only visible rows have widgets.
- Hot render loop: no routine per-frame allocations.
- Reference dashboard: p95 frame time below 16.7 ms on reference hardware.
- No memory growth after window open/close and repeated mount/unmount cycles.

Absolute memory numbers are not promised before a baseline is measured because
GPU drivers and the OS strongly affect results.

## 8. Workspace Structure

```text
grafikr/
├── crates/
│   ├── grafikr/           # Public facade and re-exports
│   ├── grafikr-core/      # Tree, state, event, style, layout
│   ├── grafikr-shell/     # winit, window, IME, clipboard
│   ├── grafikr-render/    # Display list, wgpu, atlas
│   └── grafikr-widgets/   # Built-in widgets
├── examples/
│   ├── counter/
│   ├── form/
│   ├── dashboard/
│   └── custom_widget/
├── benchmarks/
└── docs/
```

`grafikr-macros`, devtools, and the designer are added only once their value is
proven. The main public API stays in the `grafikr` crate.

## 9. Roadmap

The estimates below are for a small team of **2–3 engineers**. For solo
development, a realistic target for 1.0 is roughly **18–30 months**.

| Phase | Duration | Outcome and exit criteria |
| --- | ---: | --- |
| **M0 — Technical validation** | 2–3 weeks | winit + wgpu + Parley spike; window, DPI, resize, text, input; baseline benchmark; ADRs for retained tree and signals |
| **M1 — Rendering foundation** | 4–6 weeks | Rect, rounded rect, border, image, glyph atlas, clipping; resize and surface recovery; demo runs on two different GPUs |
| **M2 — UI core** | 6–8 weeks | Retained tree, NodeId, Signal, dirty flags, Taffy, event bubbling/capture, focus, theme; small changes do not trigger a full rebuild |
| **M3 — Input and accessibility** | 6–8 weeks | Unicode input, selection, clipboard, IME, keyboard navigation, pointer capture, AccessKit; TextField is usable for real |
| **M4 — MVP widgets** | 6–8 weeks | Button, checkbox, radio, slider, scroll, virtual list, popup, dialog; custom-widget API; form and dashboard examples |
| **M5 — Alpha and optimization** | 8–10 weeks | Multi-window, animation scheduler, cache limits, batching, memory profiling; Windows stable; Linux/macOS testing begins on real hardware |
| **M6 — Cross-platform beta** | 8–12 weeks | Windows/Linux/macOS, Wayland/X11, Grid, ComboBox, basic Tree/DataGrid; golden tests and custom-widget docs |
| **M7 — Toward 1.0** | 8–12 weeks | API freeze, SemVer, migration guide, complete accessibility, crash recovery, CI benchmark, packaging, and tutorial |
| **Post-1.0** | Ongoing | DevTools, inspector, hot reload, advanced SVG/vector, docking, visual designer, and mobile if needed |

### 9.1 MVP definition

MVP does not mean a full Qt competitor. MVP must be able to build the following
apps without internal workarounds:

- Login and settings forms.
- A dashboard with a large list.
- Simple multi-window.
- Dark/light theme.
- Keyboard-only navigation.
- Basic screen reader support.
- Custom charts/canvas through custom paint.

## 10. Testing and Quality Gates

### 10.1 Test types

- Unit tests: signals, dirty propagation, layout, focus, and style resolution.
- Integration tests: event → state → layout → paint.
- Golden image tests: widgets across themes and DPI.
- Accessibility snapshots: role, name, state, focus, and action.
- Text tests: Unicode, emoji, Arabic/BiDi, combining characters, and IME.
- Memory tests: cache bounds and repeated mount/unmount.
- GPU tests: device loss, rapid resize, minimize/restore, and monitor moves.
- Real-hardware matrix: Intel/AMD/NVIDIA, Apple Silicon, and Wayland/X11.

### 10.2 Required benchmarks

1. Empty application startup.
2. 1,000 static widgets.
3. Update one text in the middle of a large tree.
4. Scroll a 50,000-item list.
5. Continuous window resize.
6. Text shaping and cache miss.
7. Open/close a dialog 100 times.
8. Memory after 10,000 update cycles.

Benchmarks must compare Grafikr against its previous baseline. Comparisons with
Qt/Flutter are supplementary because configuration and drivers differ.

## 11. Main Risks

| Risk | Mitigation |
| --- | --- |
| Text and IME are much harder than drawing widgets | Include Parley, IME, selection, and clipboard from M0–M3 |
| Accessibility comes too late and forces an architecture change | Stable NodeId and AccessKit are part of the core |
| Performance is good on one GPU but poor on another | Test different vendors from M1 |
| The reactive API is hard to use because of Rust lifetimes | Prototype several APIs during M0 before freezing |
| Too many features delay MVP | Focus on forms, scroll, virtual lists, dialogs, and custom widgets |
| Graphics dependencies change quickly | Pin versions, isolate them in `grafikr-shell` and `grafikr-render` |
| Caches grow memory without control | Every cache has a capacity and eviction stats |

## 12. Initial Execution Order

The first step is not building 30 widgets. Build this **vertical slice**:

1. One window with correct DPI and resize.
2. Unicode text and rounded rectangles through the GPU.
3. A button with mouse/keyboard/focus.
4. A TextField with selection, clipboard, and IME.
5. One `Signal` that updates only the related text.
6. Accessibility for the button and TextField.
7. Idle, update, and memory benchmarks.

If this vertical slice is strong, the other widgets can follow the same pattern.

## 13. Key Decision

Do not write windowing, text shaping, and a layout engine from scratch. Grafikr's
value should come from its reactive API, light integration, widget quality, and
ease of customization.
