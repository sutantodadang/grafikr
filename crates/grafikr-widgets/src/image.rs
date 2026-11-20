//! Raw RGBA image view. Stretches to its node rect.

use std::rc::Rc;
use std::sync::Arc;

use grafikr_core::{
    DirtyFlags, ImageData, MeasureCx, MountCx, NodeId, PaintCx, Rect, Signal, Size, Style, View,
    Widget, observe,
};

use crate::into_views;

pub struct Image {
    data: Signal<Arc<ImageData>>,
    width: Option<f32>,
    height: Option<f32>,
    label: Signal<String>,
}

pub fn image(data: Arc<ImageData>) -> Image {
    Image {
        data: Signal::new(data),
        width: None,
        height: None,
        label: Signal::new("Image".to_owned()),
    }
}

impl Image {
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    pub fn label(self, label: impl Into<String>) -> Self {
        self.label.set(label.into());
        self
    }
    pub(crate) fn controller(&self) -> ImageController {
        ImageController {
            data: self.data.clone(),
            label: self.label.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct ImageController {
    data: Signal<Arc<ImageData>>,
    label: Signal<String>,
}
impl ImageController {
    pub(crate) fn set_data(&self, value: Arc<ImageData>) {
        self.data.set(value)
    }
    pub(crate) fn set_label(&self, value: String) {
        self.label.set(value)
    }
}

impl View for Image {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let data = self.data.get();
        let id = cx.add_node(Style {
            width: Some(self.width.unwrap_or(data.width as f32)),
            height: Some(self.height.unwrap_or(data.height as f32)),
            ..Style::default()
        });
        let runtime = cx.runtime.clone();
        let data = self.data.clone();
        let label = self.label.clone();
        let subscriber: Rc<dyn Fn()> = Rc::new(move || {
            runtime.invalidate(
                id,
                DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY,
            )
        });
        observe(&subscriber, || {
            data.get();
            label.get();
        });
        cx.set_widget(
            id,
            ImageWidget {
                data: self.data.clone(),
                label: self.label.clone(),
                _subscriber: subscriber,
            },
        );
        id
    }
}

struct ImageWidget {
    data: Signal<Arc<ImageData>>,
    label: Signal<String>,
    _subscriber: Rc<dyn Fn()>,
}

impl Widget for ImageWidget {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        let data = self.data.get();
        Size::new(data.width as f32, data.height as f32)
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        cx.display.push_image(self.data.get(), rect, cx.clip);
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.get());
    }
}

into_views!(Image);
