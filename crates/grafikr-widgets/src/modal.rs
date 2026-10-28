//! Modal/dialog overlay. Compose with any content; visibility is signal-driven.

use std::cell::Cell;
use std::rc::Rc;

use grafikr_core::{
    Align, Color, DirtyFlags, EdgeInsets, EventCx, EventStatus, IntoViews, Key, MeasureCx, MountCx,
    NodeId, PaintCx, Rect, Runtime, Signal, Size, Style, UiEvent, View, Widget, observe,
};

use crate::into_views;

pub struct Modal {
    open: Signal<bool>,
    child: Box<dyn IntoViews>,
    width: f32,
}

/// A full-window dim layer with a centered card. Mount it last in a container.
pub fn modal(open: Signal<bool>, child: impl IntoViews + 'static) -> Modal {
    Modal {
        open,
        child: Box::new(child),
        width: 360.0,
    }
}

impl Modal {
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
}

impl View for Modal {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let visible = Rc::new(Cell::new(self.open.get()));
        let overlay = cx.add_node(Style {
            absolute: true,
            inset: grafikr_core::Insets::all(0.0),
            background: Some(Color::BLACK.with_alpha(150)),
            align_items: Align::Center,
            justify_content: Align::Center,
            overlay: true,
            ..Style::default()
        });
        cx.children(overlay, |cx| {
            let card = cx.add_node(Style {
                width: Some(self.width),
                background: Some(cx.theme.surface),
                border: Some(grafikr_core::Border {
                    width: 1.0,
                    color: cx.theme.muted.with_alpha(60),
                }),
                shadow: Some(grafikr_core::Shadow {
                    blur: 24.0,
                    color: Color::BLACK.with_alpha(120),
                }),
                radius: 12.0,
                padding: EdgeInsets::all(20.0),
                gap: 12.0,
                ..Style::default()
            });
            cx.children(card, |cx| self.child.mount_into(cx));
        });
        cx.tree.set_visibility(overlay, visible.clone());
        cx.tree.set_modal_scope(overlay);

        let runtime: Runtime = cx.runtime.clone();
        let open = self.open.clone();
        let visibility = visible.clone();
        let subscriber: Rc<dyn Fn()> = Rc::new(move || {
            visibility.set(open.get());
            runtime.invalidate(overlay, DirtyFlags::LAYOUT);
        });
        let _ = observe(&subscriber, || self.open.get());

        cx.set_widget(
            overlay,
            ModalWidget {
                open: self.open.clone(),
                _subscriber: subscriber,
            },
        );
        overlay
    }
}

struct ModalWidget {
    open: Signal<bool>,
    _subscriber: Rc<dyn Fn()>,
}

impl Widget for ModalWidget {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::ZERO
    }

    fn paint(&mut self, _cx: &mut PaintCx, _rect: Rect) {}

    fn event(&mut self, _cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::KeyDown {
                key: Key::Escape, ..
            } if self.open.get() => {
                self.open.set(false);
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn focusable(&self) -> bool {
        false
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Dialog);
        node.set_label("Dialog");
    }
}

into_views!(Modal);
