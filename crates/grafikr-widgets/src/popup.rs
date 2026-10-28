//! Popup/menu layer: transparent full-parent layer with a positioned card.

use std::cell::Cell;
use std::rc::Rc;

use grafikr_core::{
    Align, DirtyFlags, EdgeInsets, EventCx, EventStatus, Insets, IntoViews, Key, MeasureCx,
    MountCx, NodeId, PaintCx, Rect, Runtime, Signal, Size, Style, UiEvent, View, Widget, observe,
};

use crate::into_views;

pub struct Popup {
    open: Signal<bool>,
    x: f32,
    y: f32,
    width: Option<f32>,
    child: Box<dyn IntoViews>,
}

/// Coordinates are relative to the popup's parent (normally the root view).
/// Mount it last so it paints above content.
pub fn popup(open: Signal<bool>, x: f32, y: f32, child: impl IntoViews + 'static) -> Popup {
    Popup {
        open,
        x,
        y,
        width: None,
        child: Box::new(child),
    }
}

impl Popup {
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
}

impl View for Popup {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let visible = Rc::new(Cell::new(self.open.get()));
        let layer = cx.add_node(Style {
            absolute: true,
            inset: Insets::all(0.0),
            overlay: true,
            ..Style::default()
        });
        cx.children(layer, |cx| {
            let card = cx.add_node(Style {
                absolute: true,
                inset: Insets {
                    left: Some(self.x),
                    top: Some(self.y),
                    right: None,
                    bottom: None,
                },
                width: self.width,
                background: Some(cx.theme.surface),
                border: Some(grafikr_core::Border {
                    width: 1.0,
                    color: cx.theme.muted.with_alpha(80),
                }),
                shadow: Some(grafikr_core::Shadow {
                    blur: 18.0,
                    color: grafikr_core::Color::BLACK.with_alpha(110),
                }),
                radius: 8.0,
                padding: EdgeInsets::symmetric(6.0, 6.0),
                gap: 2.0,
                align_items: Align::Stretch,
                ..Style::default()
            });
            cx.set_widget(card, CardWidget);
            cx.children(card, |cx| self.child.mount_into(cx));
        });
        cx.tree.set_visibility(layer, visible.clone());
        cx.tree.set_modal_scope(layer);

        let runtime: Runtime = cx.runtime.clone();
        let open = self.open.clone();
        let visibility = visible.clone();
        let subscriber: Rc<dyn Fn()> = Rc::new(move || {
            visibility.set(open.get());
            runtime.invalidate(layer, DirtyFlags::LAYOUT);
        });
        let _ = observe(&subscriber, || self.open.get());

        cx.set_widget(
            layer,
            PopupLayer {
                open: self.open.clone(),
                _subscriber: subscriber,
            },
        );
        layer
    }
}

struct PopupLayer {
    open: Signal<bool>,
    _subscriber: Rc<dyn Fn()>,
}

impl Widget for PopupLayer {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::ZERO
    }

    fn paint(&mut self, _cx: &mut PaintCx, _rect: Rect) {}

    fn event(&mut self, _cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::PointerDown { .. } | UiEvent::AccessibilityClick => {
                self.open.set(false);
                EventStatus::Handled
            }
            UiEvent::KeyDown {
                key: Key::Escape, ..
            } => {
                self.open.set(false);
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Menu);
        node.set_label("Menu");
    }
}

/// Swallows clicks inside the card so the layer closes only on outside clicks.
struct CardWidget;

impl Widget for CardWidget {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::ZERO
    }

    fn paint(&mut self, _cx: &mut PaintCx, _rect: Rect) {}

    fn event(&mut self, _cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::PointerDown { .. } | UiEvent::PointerUp { .. } => EventStatus::Handled,
            _ => EventStatus::Ignored,
        }
    }
}

into_views!(Popup);
