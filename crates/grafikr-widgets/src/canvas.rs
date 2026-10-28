//! Custom paint surface. Draw with the same display list the built-in widgets use.

use std::rc::Rc;

use grafikr_core::{MeasureCx, MountCx, NodeId, PaintCx, Rect, Size, Style, View, Widget};

use crate::into_views;

type Painter = Rc<dyn for<'a> Fn(&mut PaintCx<'a>, Rect)>;

pub struct Canvas {
    width: f32,
    height: f32,
    label: String,
    painter: Painter,
}

/// A fixed-size custom-painted area. The closure runs on every paint.
pub fn canvas(
    width: f32,
    height: f32,
    painter: impl for<'a> Fn(&mut PaintCx<'a>, Rect) + 'static,
) -> Canvas {
    Canvas {
        width,
        height,
        label: "Canvas".to_owned(),
        painter: Rc::new(painter),
    }
}

impl Canvas {
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
}

impl View for Canvas {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style {
            width: Some(self.width),
            height: Some(self.height),
            ..Style::default()
        });
        cx.set_widget(
            id,
            CanvasWidget {
                painter: self.painter.clone(),
                label: self.label.clone(),
                width: self.width,
                height: self.height,
            },
        );
        id
    }
}

struct CanvasWidget {
    painter: Painter,
    label: String,
    width: f32,
    height: f32,
}

impl Widget for CanvasWidget {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::new(self.width, self.height)
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        (self.painter)(cx, rect);
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
    }
}

into_views!(Canvas);
