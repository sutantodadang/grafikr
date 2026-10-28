# Implementation Status

Last updated: October 28, 2026. Version 1.0.0. Tested on Windows 11 with a GPU
through D3D12. Wayland/X11/macOS and other GPU vendors have not been verified on
real hardware.

## Phase summary

| Phase | Status | Notes |
| --- | --- | --- |
| M0 — Technical validation | done | winit + wgpu + Parley; DPI, resize, input; initial benchmarks |
| M1 — Rendering foundation | done* | rect, rounded, border, glyph atlas, clipping, RGBA image, shadow, transform, surface/device recovery; offscreen layers still missing |
| M2 — UI core | done* | retained tree, NodeId, Signal, dirty flags, Taffy, bubble/capture, focus, theme, visibility, overlay, transform; Taffy still starts at the root but caches clean subtrees |
| M3 — Input & accessibility | done* | Unicode/BiDi, selection, clipboard, IME, Tab, multiline, password, AccessKit Click/Focus/SetValue; undo/redo still missing |
| M4 — MVP widgets | done | button/icon, text, field (multiline/password), checkbox, radio, switch, slider, progress, spinner, scroll, virtual list, canvas, modal, tabs, popup, grid, Stack, ComboBox, TreeView, tooltip |
| M5 — Alpha/optimization | done* | multi-window, animation scheduler + Transition, glyph/image caches, shadow, batching, benchmarks, `UiStats`/`RendererStats`; RSS profiling still missing |
| M6 — Cross-platform beta | partial | grid, ComboBox, 50k DataGrid, TreeView, and golden images exist; Wayland/X11/macOS and the GPU matrix are untested |
| M7 — Toward 1.0 | partial | 1.0 API, SemVer, license, packaging metadata, tutorial, migration guide, CI benchmark, device-loss recovery; crates.io publication and hardware verification still missing |

`*` means the core feature is complete, with the gaps recorded below.

## Vertical slice §12

1. Native window + DPI/resize — yes.
2. Unicode text + GPU rounded rectangles — yes.
3. Button mouse/keyboard/focus — yes.
4. TextField selection/clipboard/IME — yes.
5. One Signal updates only the related text — yes.
6. Accessibility for button and TextField — yes (Click/Focus/SetValue).
7. Idle/update/memory benchmarks — idle and update yes; CPU-side memory through
   `UiStats`, RSS/GPU not yet.

## MVP definition §9.1

| Requirement | Status |
| --- | --- |
| Login/settings form | `form` and `settings` examples |
| Dashboard with a large list | `dashboard`, 50,000 virtual rows |
| Simple multi-window | `multi-window`, two independent windows |
| Dark/light theme | tokens available; the app chooses at startup |
| Keyboard-only navigation | Tab, Enter/Space, arrow keys, PageUp/PageDown/Home/End |
| Basic screen reader support | AccessKit tree + Click/Focus/SetValue |
| Custom chart/canvas | `canvas` + bar chart example |

## Tests and quality gates

- `cargo test --workspace` — unit, integration, golden image.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo fmt --all -- --check`.
- GitHub Actions CI: fmt/clippy, tests on Windows/Linux/macOS, benchmark.
- Golden images: `crates/grafikr-render/tests/golden/basic.png` (dark, 1x) and
  `scale15.png` (light, 1.5x).
- Micro-benchmarks: signal, shaping, layout of 1,000 nodes, update one label,
  remount 1,000 nodes, scroll 50,000 rows.

## Known gaps

- Subtree layout is not fully isolated; Taffy traverses the root but does not
  recompute clean subtrees.
- No rotation, offscreen layers, or partial damage tracking.
- The scheduler handles values/loops only; multi-property keyframes are missing.
- Virtual lists draw rows with custom paint instead of per-row widgets;
  per-row AccessKit support is limited.
- Only Windows/D3D12 has been tested; Wayland/X11/macOS have not.
- Golden images have not run across a GPU vendor matrix.
- Rich clipboard (HTML/image) and drag-and-drop are missing.
- The crates have not been published to crates.io.

## Next steps after 1.0

1. Verify Wayland/X11/macOS and Intel/AMD/NVIDIA/Apple GPUs.
2. Isolated subtree layout + damage tracking + partial redraw.
3. Offscreen layers, rotation, and multi-property keyframes.
4. Per-row widgets for virtual list/DataGrid with AccessKit.
5. Rich clipboard, drag-and-drop, undo/redo.
6. RSS/GPU profiling and cross-vendor golden images in CI.
