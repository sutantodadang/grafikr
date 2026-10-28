//! Push button with mouse, keyboard, focus, and accessibility activation.

use std::rc::Rc;

use grafikr_core::{
    EventCx, EventStatus, Interaction, MeasureCx, MountCx, NodeId, PaintCx, Point, Rect, Size,
    Style, TextCache, Transition, UiEvent, View, Widget,
};

use crate::into_views;

/// Passed to click handlers.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClickEvent;

type Handler = Rc<dyn Fn(ClickEvent)>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    #[default]
    Primary,
    Secondary,
}

pub struct Button {
    label: String,
    on_click: Option<Handler>,
    variant: ButtonVariant,
    icon: bool,
}

pub fn button(label: impl Into<String>) -> Button {
    Button {
        label: label.into(),
        on_click: None,
        variant: ButtonVariant::Primary,
        icon: false,
    }
}

impl Button {
    pub fn on_click(mut self, handler: impl Fn(ClickEvent) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn secondary(mut self) -> Self {
        self.variant = ButtonVariant::Secondary;
        self
    }

    /// Compact square button for an icon glyph or one short label.
    pub fn icon(mut self) -> Self {
        self.icon = true;
        self
    }
}

impl View for Button {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let theme = cx.theme.clone();
        let id = cx.add_node(Style::default());
        cx.set_widget(
            id,
            ButtonWidget {
                label: self.label.clone(),
                on_click: self.on_click.clone(),
                cache: TextCache::default(),
                pressed: false,
                font_size: theme.body_size,
                variant: self.variant,
                icon: self.icon,
                hover: Transition::new(0.0),
            },
        );
        id
    }
}

struct ButtonWidget {
    label: String,
    on_click: Option<Handler>,
    cache: TextCache,
    pressed: bool,
    font_size: f32,
    variant: ButtonVariant,
    icon: bool,
    hover: Transition,
}

impl ButtonWidget {
    fn activate(&self) {
        if let Some(handler) = &self.on_click {
            handler(ClickEvent);
        }
    }
}

impl Widget for ButtonWidget {
    fn measure(&mut self, cx: &mut MeasureCx, _available: Size) -> Size {
        let shape = self.cache.get(
            cx.text,
            &self.label,
            self.font_size,
            cx.theme.foreground,
            None,
        );
        Size::new(
            if self.icon {
                (shape.width + 16.0).max(cx.theme.control_height)
            } else {
                (shape.width + 32.0).max(80.0)
            },
            (shape.height + 14.0).max(cx.theme.control_height),
        )
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        let radius = cx.theme.control_radius();
        let hovered = if cx.state.hovered { 1.0 } else { 0.0 };
        self.hover.set_target(hovered);
        if cx.theme.reduced_motion {
            self.hover.jump_to(hovered);
        }
        if self.hover.tick(cx.dt) {
            cx.runtime.request_animation();
        }
        let t = self.hover.value();
        let (fill, text_color) = match self.variant {
            ButtonVariant::Primary => {
                let idle = Interaction {
                    hovered: false,
                    ..cx.state
                };
                let hover = Interaction {
                    hovered: true,
                    ..cx.state
                };
                (
                    cx.theme
                        .accent_control(idle)
                        .mix(cx.theme.accent_control(hover), t),
                    cx.theme.accent_foreground(cx.state),
                )
            }
            ButtonVariant::Secondary => {
                let fill = if cx.state.pressed {
                    cx.theme.muted.with_alpha(55)
                } else {
                    cx.theme.surface.mix(cx.theme.muted.with_alpha(30), t)
                };
                (fill, cx.theme.foreground)
            }
        };
        match self.variant {
            ButtonVariant::Primary => cx.push_rect(rect, fill, radius),
            ButtonVariant::Secondary => cx.push_bordered_rect(
                rect,
                fill,
                radius,
                Some((1.0, cx.theme.muted.with_alpha(80))),
            ),
        }
        if cx.state.focused {
            let ring = match self.variant {
                ButtonVariant::Primary => grafikr_core::Color::WHITE.with_alpha(170),
                ButtonVariant::Secondary => cx.theme.accent,
            };
            cx.push_bordered_rect(
                rect,
                grafikr_core::Color::TRANSPARENT,
                radius,
                Some((2.0, ring)),
            );
        }

        let shape = self
            .cache
            .get(cx.text, &self.label, self.font_size, text_color, None);
        let origin = Point::new(
            rect.x + (rect.width - shape.width) * 0.5,
            rect.y + (rect.height - shape.height) * 0.5,
        );
        cx.display.push_layout(&shape.layout, origin, cx.clip);
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::PointerDown { .. } => {
                self.pressed = true;
                EventStatus::Handled
            }
            UiEvent::PointerUp { pos, .. } => {
                let clicked = self.pressed && cx.rect.contains(*pos);
                self.pressed = false;
                if clicked {
                    self.activate();
                }
                EventStatus::Handled
            }
            UiEvent::PointerLeft => {
                self.pressed = false;
                EventStatus::Handled
            }
            UiEvent::KeyDown { key, .. }
                if cx.state.focused
                    && matches!(key, grafikr_core::Key::Space | grafikr_core::Key::Enter) =>
            {
                self.pressed = true;
                cx.request_paint();
                EventStatus::Handled
            }
            UiEvent::KeyUp { key, .. }
                if cx.state.focused
                    && matches!(key, grafikr_core::Key::Space | grafikr_core::Key::Enter) =>
            {
                if self.pressed {
                    self.pressed = false;
                    self.activate();
                }
                EventStatus::Handled
            }
            UiEvent::AccessibilityClick => {
                self.activate();
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn cursor(&self) -> grafikr_core::CursorIcon {
        if self.on_click.is_some() {
            grafikr_core::CursorIcon::Pointer
        } else {
            grafikr_core::CursorIcon::Default
        }
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Button);
        node.set_label(self.label.clone());
        node.add_action(accesskit::Action::Click);
    }
}

into_views!(Button);
