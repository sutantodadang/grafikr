//! Text view: static or bound to signals.

use std::rc::Rc;

use grafikr_core::{
    BoundText, Color, DirtyFlags, MeasureCx, MountCx, NodeId, PaintCx, Point, Rect, Size, Style,
    TextCache, Theme, View, Widget,
};

use crate::into_views;

pub enum TextContent {
    Static(String),
    Bound(BoundText),
}

impl From<&str> for TextContent {
    fn from(value: &str) -> Self {
        Self::Static(value.to_owned())
    }
}

impl From<String> for TextContent {
    fn from(value: String) -> Self {
        Self::Static(value)
    }
}

impl From<BoundText> for TextContent {
    fn from(value: BoundText) -> Self {
        Self::Bound(value)
    }
}

enum Source {
    Static(String),
    Bound {
        value: BoundText,
        // Keeps the subscription callback alive while the node lives.
        _subscriber: Rc<dyn Fn()>,
    },
}

pub struct Text {
    content: TextContent,
    font_size: Option<f32>,
    color: Option<Color>,
}

pub fn text(content: impl Into<TextContent>) -> Text {
    Text {
        content: content.into(),
        font_size: None,
        color: None,
    }
}

impl Text {
    pub fn size(mut self, size: f32) -> Self {
        self.font_size = Some(size);
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

impl View for Text {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style::default());
        let source = match &self.content {
            TextContent::Static(value) => Source::Static(value.clone()),
            TextContent::Bound(bound) => {
                let runtime = cx.runtime.clone();
                let subscriber: Rc<dyn Fn()> = Rc::new(move || {
                    runtime.invalidate(id, DirtyFlags::LAYOUT);
                });
                let _ = bound.tracked(&subscriber);
                Source::Bound {
                    value: bound.clone(),
                    _subscriber: subscriber,
                }
            }
        };
        cx.set_widget(
            id,
            TextWidget {
                source,
                font_size: self.font_size,
                color: self.color,
                cache: TextCache::default(),
            },
        );
        id
    }
}

struct TextWidget {
    source: Source,
    font_size: Option<f32>,
    color: Option<Color>,
    cache: TextCache,
}

impl TextWidget {
    fn text(&self) -> String {
        match &self.source {
            Source::Static(value) => value.clone(),
            Source::Bound { value, .. } => value.evaluate(),
        }
    }

    fn style(&self, theme: &Theme) -> (f32, Color) {
        (
            self.font_size.unwrap_or(theme.body_size),
            self.color.unwrap_or(theme.foreground),
        )
    }
}

impl Widget for TextWidget {
    fn measure(&mut self, cx: &mut MeasureCx, available: Size) -> Size {
        let (size, color) = self.style(cx.theme);
        let text = self.text();
        let max_width = available.width.is_finite().then_some(available.width);
        let shape = self.cache.get(cx.text, &text, size, color, max_width);
        Size::new(shape.width, shape.height)
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        let (size, color) = self.style(cx.theme);
        let text = self.text();
        let max_width = Some(rect.width);
        let shape = match self.cache.cached() {
            Some(shape) => shape,
            None => self.cache.get(cx.text, &text, size, color, max_width),
        };
        cx.display
            .push_layout(&shape.layout, Point::new(rect.x, rect.y), cx.clip);
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Label);
        node.set_label(self.text());
    }
}

into_views!(Text);
