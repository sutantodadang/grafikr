//! Raw RGBA image view. Stretches to its node rect.

use std::sync::Arc;

use grafikr_core::{
    ImageData, MeasureCx, MountCx, NodeId, PaintCx, Rect, Size, Style, View, Widget,
};

use crate::into_views;

pub struct Image {
    data: Arc<ImageData>,
    width: Option<f32>,
    height: Option<f32>,
    label: String,
}

pub fn image(data: Arc<ImageData>) -> Image {
    Image {
        data,
        width: None,
        height: None,
        label: "Image".to_owned(),
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

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
}

impl View for Image {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style {
            width: Some(self.width.unwrap_or(self.data.width as f32)),
            height: Some(self.height.unwrap_or(self.data.height as f32)),
            ..Style::default()
        });
        cx.set_widget(
            id,
            ImageWidget {
                data: self.data.clone(),
                label: self.label.clone(),
            },
        );
        id
    }
}

struct ImageWidget {
    data: Arc<ImageData>,
    label: String,
}

impl Widget for ImageWidget {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::new(self.data.width as f32, self.data.height as f32)
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        cx.display.push_image(self.data.clone(), rect, cx.clip);
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
    }
}

into_views!(Image);
