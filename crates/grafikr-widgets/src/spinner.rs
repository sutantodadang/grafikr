//! Indeterminate spinner. Uses the runtime animation clock.

use std::rc::Rc;
use std::time::Instant;

use grafikr_core::{
    DirtyFlags, MeasureCx, MountCx, NodeId, PaintCx, Point, Rect, Signal, Size, Style, View,
    Widget, observe,
};

use crate::into_views;

const DOTS: usize = 10;

pub struct Spinner {
    size: Signal<f32>,
    label: Signal<String>,
}

pub fn spinner() -> Spinner {
    Spinner {
        size: Signal::new(28.0),
        label: Signal::new("Loading".to_owned()),
    }
}

impl Spinner {
    pub fn size(self, size: f32) -> Self {
        self.size.set(size.max(12.0));
        self
    }

    pub fn label(self, label: impl Into<String>) -> Self {
        self.label.set(label.into());
        self
    }

    pub(crate) fn controller(&self) -> SpinnerController {
        SpinnerController {
            size: self.size.clone(),
            label: self.label.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct SpinnerController {
    size: Signal<f32>,
    label: Signal<String>,
}
impl SpinnerController {
    pub(crate) fn set_size(&self, value: f32) {
        self.size.set(value);
    }
    pub(crate) fn set_label(&self, value: String) {
        self.label.set(value);
    }
}

impl View for Spinner {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style {
            width: Some(self.size.get()),
            height: Some(self.size.get()),
            ..Style::default()
        });
        let runtime = cx.runtime.clone();
        let subscriber: Rc<dyn Fn()> = Rc::new(move || {
            runtime.invalidate(
                id,
                DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY,
            )
        });
        observe(&subscriber, || {
            self.size.get();
            self.label.get();
        });
        cx.set_widget(
            id,
            SpinnerWidget {
                start: Instant::now(),
                label: self.label.clone(),
                _subscriber: subscriber,
            },
        );
        id
    }
}

struct SpinnerWidget {
    start: Instant,
    label: Signal<String>,
    _subscriber: Rc<dyn Fn()>,
}

impl Widget for SpinnerWidget {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::new(28.0, 28.0)
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        cx.runtime.request_animation();
        let radius = (rect.width.min(rect.height) * 0.5 - 4.0).max(2.0);
        let center = Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
        let phase = (self.start.elapsed().as_secs_f32() * 1.4).fract();
        let dot = (radius * 0.28).clamp(1.5, 4.0);
        for index in 0..DOTS {
            let angle =
                index as f32 / DOTS as f32 * std::f32::consts::TAU + phase * std::f32::consts::TAU;
            let strength = 1.0 - (index as f32 / DOTS as f32);
            let alpha = (60.0 + 195.0 * strength) as u8;
            let x = center.x + angle.cos() * radius;
            let y = center.y + angle.sin() * radius;
            cx.push_rect(
                Rect::new(x - dot, y - dot, dot * 2.0, dot * 2.0),
                cx.theme.accent.with_alpha(alpha),
                dot,
            );
        }
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::ProgressIndicator);
        node.set_label(self.label.get());
    }
}

into_views!(Spinner);
