//! Indeterminate spinner. Uses the runtime animation clock.

use std::time::Instant;

use grafikr_core::{MeasureCx, MountCx, NodeId, PaintCx, Point, Rect, Size, Style, View, Widget};

use crate::into_views;

const DOTS: usize = 10;

pub struct Spinner {
    size: f32,
    label: String,
}

pub fn spinner() -> Spinner {
    Spinner {
        size: 28.0,
        label: "Loading".to_owned(),
    }
}

impl Spinner {
    pub fn size(mut self, size: f32) -> Self {
        self.size = size.max(12.0);
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
}

impl View for Spinner {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style {
            width: Some(self.size),
            height: Some(self.size),
            ..Style::default()
        });
        cx.set_widget(
            id,
            SpinnerWidget {
                start: Instant::now(),
                label: self.label.clone(),
            },
        );
        id
    }
}

struct SpinnerWidget {
    start: Instant,
    label: String,
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
        node.set_label(self.label.clone());
    }
}

into_views!(Spinner);
