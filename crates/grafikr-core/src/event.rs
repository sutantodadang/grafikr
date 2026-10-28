//! Platform-neutral input events.

use crate::color::Point;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Tab,
    Enter,
    Space,
    Escape,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Character(char),
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiEvent {
    PointerMoved {
        pos: Point,
    },
    PointerDown {
        pos: Point,
        button: MouseButton,
    },
    PointerUp {
        pos: Point,
        button: MouseButton,
    },
    PointerLeft,
    Wheel {
        delta: Point,
    },
    KeyDown {
        key: Key,
        modifiers: Modifiers,
        repeat: bool,
    },
    KeyUp {
        key: Key,
        modifiers: Modifiers,
    },
    Text(String),
    ImePreedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    ImeCommit(String),
    FocusLost,
    /// Synthetic activation from an accessibility client.
    AccessibilityClick,
    /// Full replacement value requested by an accessibility client.
    SetValue(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EventStatus {
    #[default]
    Ignored,
    Handled,
}

impl EventStatus {
    pub fn is_handled(self) -> bool {
        matches!(self, Self::Handled)
    }
}

/// Platform clipboard bridge. The shell provides the real implementation.
pub trait Clipboard {
    fn get_text(&mut self) -> Option<String>;
    fn set_text(&mut self, text: String);
}

/// Used by tests and headless runs.
#[derive(Default)]
pub struct NoopClipboard;

impl Clipboard for NoopClipboard {
    fn get_text(&mut self) -> Option<String> {
        None
    }

    fn set_text(&mut self, _text: String) {}
}
