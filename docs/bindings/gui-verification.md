# Installed GUI verification

GUI evidence is collected from a clean installed consumer using one of the
three packaged examples: counter/form, dashboard, or multi-window. The required
cells and scenario names are in `bindings/runtime/gui-cases.json`.

Each GUI record identifies the SDK, scenario, display cell, target, backend,
source SHA, tested artifact hash, and OS input method. Accepted input methods
are native OS automation (`windows_sendinput`, `xdotool`, or `osascript`) and
supervised manual testing (`manual_supervised`). Direct `UiEvent` injection is
headless engine evidence only. Attach a screenshot and the consumer's state
report; keep GPU and accessibility reports in separate hardware evidence.

The counter/form scenario checks mouse activation, Unicode keyboard input,
state updates, Tab navigation, and IME composition. Dashboard checks scrolling,
visible data requests, selection, and canvas/image updates. Multi-window checks
modal focus, Escape, one-window close, and final shutdown. The C and C++ installed
scenario harnesses pass those input flows under local Xvfb/X11, but use a
temporary mixed Linux bundle and do not populate the required desktop display
cells. No scenario has passed on a native desktop runner yet.
