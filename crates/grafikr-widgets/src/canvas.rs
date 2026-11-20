//! Custom paint surface. Draw with the same display list the built-in widgets use.

use std::cell::Cell;
use std::rc::Rc;

use grafikr_core::{
    DirtyFlags, MeasureCx, MountCx, NodeId, PaintCx, Rect, Signal, Size, Style, View, Widget,
    observe,
};

use crate::into_views;

pub type CanvasPainter = Rc<dyn for<'a> Fn(&mut PaintCx<'a>, Rect)>;

pub struct Canvas {
    width: Signal<f32>,
    height: Signal<f32>,
    label: Signal<String>,
    painter: CanvasPainter,
    resized: Rc<dyn Fn(NodeId, f32, f32)>,
}

/// A fixed-size custom-painted area. The closure runs on every paint.
pub fn canvas(
    width: f32,
    height: f32,
    painter: impl for<'a> Fn(&mut PaintCx<'a>, Rect) + 'static,
) -> Canvas {
    Canvas {
        width: Signal::new(width),
        height: Signal::new(height),
        label: Signal::new("Canvas".to_owned()),
        painter: Rc::new(painter),
        resized: Rc::new(|_, _, _| {}),
    }
}

impl Canvas {
    pub fn label(self, label: impl Into<String>) -> Self {
        self.label.set(label.into());
        self
    }

    pub(crate) fn controlled(
        width: f32,
        height: f32,
        label: String,
        painter: CanvasPainter,
        resized: impl Fn(NodeId, f32, f32) + 'static,
    ) -> (Self, CanvasController) {
        let width = Signal::new(width);
        let height = Signal::new(height);
        let label = Signal::new(label);
        (
            Self {
                width: width.clone(),
                height: height.clone(),
                label: label.clone(),
                painter,
                resized: Rc::new(resized),
            },
            CanvasController {
                label,
                width,
                height,
            },
        )
    }
}

#[derive(Clone)]
pub(crate) struct CanvasController {
    label: Signal<String>,
    width: Signal<f32>,
    height: Signal<f32>,
}
impl CanvasController {
    pub(crate) fn set_label(&self, label: String) {
        self.label.set(label)
    }
    pub(crate) fn set_size(&self, width: f32, height: f32) {
        self.width.set(width);
        self.height.set(height);
    }
}

impl View for Canvas {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style {
            width: Some(self.width.get()),
            height: Some(self.height.get()),
            ..Style::default()
        });
        let runtime = cx.runtime.clone();
        let label = self.label.clone();
        let subscriber: Rc<dyn Fn()> =
            Rc::new(move || runtime.invalidate(id, DirtyFlags::ACCESSIBILITY));
        observe(&subscriber, || label.get());
        let runtime = cx.runtime.clone();
        let width = self.width.clone();
        let height = self.height.clone();
        let size_subscriber: Rc<dyn Fn()> =
            Rc::new(move || runtime.invalidate(id, DirtyFlags::LAYOUT | DirtyFlags::PAINT));
        observe(&size_subscriber, || (width.get(), height.get()));
        cx.set_widget(
            id,
            CanvasWidget {
                painter: self.painter.clone(),
                label: self.label.clone(),
                node: id,
                width: self.width.clone(),
                height: self.height.clone(),
                resized: self.resized.clone(),
                last_size: Cell::new(None),
                _subscriber: subscriber,
                _size_subscriber: size_subscriber,
            },
        );
        id
    }
}

struct CanvasWidget {
    painter: CanvasPainter,
    label: Signal<String>,
    node: NodeId,
    width: Signal<f32>,
    height: Signal<f32>,
    resized: Rc<dyn Fn(NodeId, f32, f32)>,
    last_size: Cell<Option<Size>>,
    _subscriber: Rc<dyn Fn()>,
    _size_subscriber: Rc<dyn Fn()>,
}

impl Widget for CanvasWidget {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::new(self.width.get(), self.height.get())
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        let size = Size::new(rect.width, rect.height);
        if self
            .last_size
            .replace(Some(size))
            .is_some_and(|last| last != size)
        {
            (self.resized)(self.node, size.width, size.height);
        }
        (self.painter)(cx, rect);
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.get());
    }
}

into_views!(Canvas);
