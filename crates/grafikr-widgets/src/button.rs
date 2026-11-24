//! Push button with mouse, keyboard, focus, and accessibility activation.

use std::rc::Rc;

use grafikr_core::{
    DirtyFlags, EventCx, EventStatus, Interaction, MeasureCx, MountCx, NodeId, PaintCx, Point,
    Rect, Signal, Size, Style, TextCache, Transition, UiEvent, View, Widget, observe,
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
    label: Signal<String>,
    on_click: Option<Handler>,
    variant: Signal<ButtonVariant>,
    icon: Signal<bool>,
}

pub fn button(label: impl Into<String>) -> Button {
    Button {
        label: Signal::new(label.into()),
        on_click: None,
        variant: Signal::new(ButtonVariant::Primary),
        icon: Signal::new(false),
    }
}

impl Button {
    pub fn on_click(mut self, handler: impl Fn(ClickEvent) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn secondary(self) -> Self {
        self.variant.set(ButtonVariant::Secondary);
        self
    }

    /// Compact square button for an icon glyph or one short label.
    pub fn icon(self) -> Self {
        self.icon.set(true);
        self
    }

    pub(crate) fn controller(&self) -> ButtonController {
        ButtonController {
            label: self.label.clone(),
            variant: self.variant.clone(),
            icon: self.icon.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct ButtonController {
    label: Signal<String>,
    variant: Signal<ButtonVariant>,
    icon: Signal<bool>,
}

impl ButtonController {
    pub(crate) fn set_label(&self, value: String) {
        self.label.set(value);
    }
    pub(crate) fn set_variant(&self, value: ButtonVariant) {
        self.variant.set(value);
    }
    pub(crate) fn set_icon(&self, value: bool) {
        self.icon.set(value);
    }
}

impl View for Button {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let theme = cx.theme.clone();
        let id = cx.add_node(Style::default());
        let runtime = cx.runtime.clone();
        let subscriber: Rc<dyn Fn()> = Rc::new(move || runtime.invalidate(id, DirtyFlags::LAYOUT));
        observe(&subscriber, || {
            self.label.get();
            self.variant.get();
            self.icon.get();
        });
        cx.set_widget(
            id,
            ButtonWidget {
                label: self.label.clone(),
                on_click: self.on_click.clone(),
                cache: TextCache::default(),
                pressed: false,
                font_size: theme.body_size,
                variant: self.variant.clone(),
                icon: self.icon.clone(),
                _subscriber: subscriber,
                hover: Transition::new(0.0),
            },
        );
        id
    }
}

struct ButtonWidget {
    label: Signal<String>,
    on_click: Option<Handler>,
    cache: TextCache,
    pressed: bool,
    font_size: f32,
    variant: Signal<ButtonVariant>,
    icon: Signal<bool>,
    hover: Transition,
    _subscriber: Rc<dyn Fn()>,
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
            &self.label.get(),
            self.font_size,
            cx.theme.foreground,
            None,
        );
        Size::new(
            if self.icon.get() {
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
        let variant = self.variant.get();
        let label = self.label.get();
        let (fill, text_color) = match variant {
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
        match variant {
            ButtonVariant::Primary => cx.push_rect(rect, fill, radius),
            ButtonVariant::Secondary => cx.push_bordered_rect(
                rect,
                fill,
                radius,
                Some((1.0, cx.theme.muted.with_alpha(80))),
            ),
        }
        if cx.state.focused {
            let ring = match variant {
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
            .get(cx.text, &label, self.font_size, text_color, None);
        let origin = Point::new(
            rect.x + (rect.width - shape.width) * 0.5,
            rect.y + (rect.height - shape.height) * 0.5,
        );
        cx.display.push_layout(&shape.layout, origin, cx.clip);
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        if cx.state.disabled {
            return EventStatus::Ignored;
        }
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
        node.set_label(self.label.get());
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
    }
}

into_views!(Button);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessibility_button_exposes_focus_and_click_actions() {
        let mut widget = ButtonWidget {
            label: Signal::new("Increment".to_owned()),
            on_click: None,
            cache: TextCache::default(),
            pressed: false,
            font_size: 16.0,
            variant: Signal::new(ButtonVariant::Primary),
            icon: Signal::new(false),
            _subscriber: Rc::new(|| {}),
            hover: Transition::new(0.0),
        };
        let mut node = accesskit::Node::new(accesskit::Role::Button);

        Widget::accessibility(&mut widget, &mut node);

        let description = format!("{node:?}");
        assert!(
            description.contains("Focus"),
            "missing accessible focus action: {description}"
        );
        assert!(
            description.contains("Click"),
            "missing accessible click action: {description}"
        );
    }
}
