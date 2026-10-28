//! Theme tokens and resolved node style.

use crate::color::{Border, Color, EdgeInsets, Insets, Transform};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    Row,
    #[default]
    Column,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Interaction {
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
    pub disabled: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorIcon {
    #[default]
    Default,
    Pointer,
    Text,
}

/// Layout and paint style of one retained node.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub direction: Direction,
    pub gap: f32,
    pub padding: EdgeInsets,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub flex_grow: f32,
    pub align_items: Align,
    pub justify_content: Align,
    pub background: Option<Color>,
    pub border: Option<Border>,
    pub shadow: Option<crate::color::Shadow>,
    pub radius: f32,
    pub clip: bool,
    /// When set, this node lays its children out in a CSS grid with this many
    /// equal `1fr` columns.
    pub grid_columns: Option<u16>,
    /// Out of normal flow, positioned by `inset` relative to the parent.
    pub absolute: bool,
    pub inset: Insets,
    /// Allow hit-testing descendants that paint outside this node's rect.
    pub overflow_hit: bool,
    /// Paint and hit-test this node after its normal-flow siblings.
    pub overlay: bool,
    /// Visual translation/scale for this node and its descendants. Layout is
    /// unaffected; hit-testing applies the inverse.
    pub transform: Transform,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            direction: Direction::Column,
            gap: 0.0,
            padding: EdgeInsets::ZERO,
            width: None,
            height: None,
            min_width: None,
            min_height: None,
            flex_grow: 0.0,
            align_items: Align::Start,
            justify_content: Align::Start,
            background: None,
            border: None,
            shadow: None,
            radius: 0.0,
            clip: false,
            grid_columns: None,
            absolute: false,
            inset: crate::color::Insets::NONE,
            overflow_hit: false,
            overlay: false,
            transform: Transform::IDENTITY,
        }
    }
}

impl Style {
    pub fn row() -> Self {
        Self {
            direction: Direction::Row,
            align_items: Align::Center,
            ..Self::default()
        }
    }
}

/// Typed design tokens. Widgets derive every color they paint from here.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub background: Color,
    pub surface: Color,
    pub foreground: Color,
    pub muted: Color,
    pub accent: Color,
    pub danger: Color,
    pub radius: f32,
    pub border_width: f32,
    pub control_height: f32,
    pub spacing: f32,
    pub body_size: f32,
    pub caption_size: f32,
    pub heading_size: f32,
    pub reduced_motion: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            background: Color::hex(0x121417),
            surface: Color::hex(0x1d2025),
            foreground: Color::hex(0xe8eaed),
            muted: Color::hex(0x9aa0a6),
            accent: Color::hex(0x4c8dff),
            danger: Color::hex(0xe5484d),
            radius: 8.0,
            border_width: 1.0,
            control_height: 34.0,
            spacing: 8.0,
            body_size: 14.0,
            caption_size: 12.0,
            heading_size: 20.0,
            reduced_motion: false,
        }
    }

    pub fn light() -> Self {
        Self {
            background: Color::hex(0xf4f5f7),
            surface: Color::hex(0xffffff),
            foreground: Color::hex(0x1a1c1f),
            muted: Color::hex(0x6b7280),
            accent: Color::hex(0x2563eb),
            danger: Color::hex(0xdc2626),
            radius: 8.0,
            border_width: 1.0,
            control_height: 34.0,
            spacing: 8.0,
            body_size: 14.0,
            caption_size: 12.0,
            heading_size: 20.0,
            reduced_motion: false,
        }
    }

    /// Background of an accent-filled control.
    pub fn accent_control(&self, state: Interaction) -> Color {
        if state.disabled {
            self.surface.mix(self.background, 0.4)
        } else if state.pressed {
            self.accent.mix(Color::BLACK, 0.2)
        } else if state.hovered {
            self.accent.mix(Color::WHITE, 0.1)
        } else {
            self.accent
        }
    }

    pub fn accent_foreground(&self, state: Interaction) -> Color {
        if state.disabled {
            self.muted
        } else {
            Color::WHITE
        }
    }

    pub fn field_background(&self, state: Interaction) -> Color {
        if state.disabled {
            self.background.mix(self.surface, 0.5)
        } else {
            self.surface
        }
    }

    pub fn field_border(&self, state: Interaction) -> Color {
        if state.focused {
            self.accent
        } else if state.hovered {
            self.muted
        } else {
            self.muted.mix(self.surface, 0.55)
        }
    }

    pub fn control_radius(&self) -> f32 {
        self.radius.min(self.control_height * 0.5)
    }
}
