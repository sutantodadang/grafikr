//! Core of Grafikr: reactive state, retained tree, layout, events, text, painting.

mod a11y;
mod animation;
mod color;
mod display;
mod event;
mod image;
mod runtime;
mod scroll;
mod signal;
mod style;
mod text;
mod tree;
mod ui;
mod view;

pub use animation::{Easing, Transition};
pub use color::{Border, Color, EdgeInsets, Insets, Point, Rect, Shadow, Size, Transform};
pub use display::{DisplayItem, DisplayList, GlyphPos, TextRun};
pub use event::{Clipboard, EventStatus, Key, Modifiers, MouseButton, UiEvent};
pub use image::ImageData;
pub use parley::FontData;
pub use runtime::{DirtyFlags, Runtime};
pub use scroll::ScrollState;
pub use signal::{BoundText, Signal, bind, observe};
pub use style::{Align, CursorIcon, Direction, Interaction, Style, Theme};
pub use text::{TextCache, TextEngine, TextShape, push_layout, push_layout_masked, shape_key};
pub use tree::{Node, NodeId, Tree, Widget};
pub use ui::{EventCx, MeasureCx, Outcome, PaintCx, Ui, UiStats};
pub use view::{IntoViews, MountCx, View};
