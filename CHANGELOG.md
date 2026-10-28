# Changelog

## 1.0.0 — 2026-10-28

First stable API release, tested on Windows/D3D12. Linux, macOS, and other GPU
vendors have not been verified on real hardware.

### Core

- Retained tree with generational `NodeId`, dynamic visibility, overlay, focus
  scope, and `modal_scope`.
- `Signal<T>`, `bind`, dependency tracking, dirty flags, and a frame scheduler.
- Taffy flex/grid layout, `Stack`, and visual translate/scale transforms with
  inverse hit-testing.
- Parley text: Unicode, BiDi, font fallback, multiline, selection, clipboard,
  IME, and password masking.
- `Transition`/`Easing` for frame-scheduler-driven value animation.
- `Ui::mount` clears the previous tree; `Ui::stats()` supports memory
  diagnostics.
- AccessKit tree with `Click`, `Focus`, and `SetValue`.

### Rendering

- wgpu 30: SDF quads, rounded rects, borders, glyph atlas, images, clipping,
  layered shadows, batching, and bounded caches.
- Headless golden-image tests with cross-driver tolerance.
- Surface recovery and device/renderer recreation after device loss.

### Widgets

- Text, button (primary/secondary/icon, hover transition), TextField
  (single/multiline/password), checkbox, radio, switch, slider, progress, and
  spinner.
- ScrollView, 50k VirtualList, Canvas, Grid, Stack, Modal, Tabs, Popup/menu,
  ComboBox, Tooltip, 50k DataGrid, and TreeView.
- Multi-window with per-window GPU, UI, and AccessKit state.

### Tooling

- Workspace 1.0.0, AGPL-3.0-only license, CI fmt/clippy/test/benchmark.
- Tutorial, migration guide, ADRs, status document, and micro-benchmarks.

### Known missing

- Wayland/X11/macOS and the real GPU vendor matrix.
- Rotation, offscreen layers, damage tracking, and fully isolated subtree
  layout.
- Virtual rows are not yet per-row AccessKit widgets.
- Rich clipboard (HTML/image) and drag-and-drop.
- Crate publication to crates.io.
