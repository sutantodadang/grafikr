//! Text view: static or bound to signals.

use std::cell::Cell;
use std::rc::Rc;

use grafikr_core::{
    BoundText, Color, DirtyFlags, MeasureCx, MountCx, NodeId, PaintCx, Point, Rect, Signal, Size,
    Style, TextCache, Theme, View, Widget, bind, observe,
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
    font_size: Rc<Cell<Option<f32>>>,
    color: Rc<Cell<Option<Color>>>,
}

pub fn text(content: impl Into<TextContent>) -> Text {
    Text {
        content: content.into(),
        font_size: Rc::new(Cell::new(None)),
        color: Rc::new(Cell::new(None)),
    }
}

impl Text {
    pub fn size(self, size: f32) -> Self {
        self.font_size.set(Some(size));
        self
    }

    pub fn color(self, color: Color) -> Self {
        self.color.set(Some(color));
        self
    }
}

#[derive(Clone)]
pub(crate) struct TextController {
    text: Signal<String>,
    font_size: Rc<Cell<Option<f32>>>,
    color: Rc<Cell<Option<Color>>>,
}

impl TextController {
    pub(crate) fn set_text(&self, value: String) {
        self.text.set(value);
    }
    pub(crate) fn set_size(&self, value: f32) {
        self.font_size.set(Some(value));
    }
    pub(crate) fn set_color(&self, value: Color) {
        self.color.set(Some(value));
    }
}

pub(crate) fn dynamic_text(initial: String) -> (Text, TextController) {
    let signal = Signal::new(initial);
    let source = signal.clone();
    let view = text(bind(move || source.get()));
    let controller = TextController {
        text: signal,
        font_size: view.font_size.clone(),
        color: view.color.clone(),
    };
    (view, controller)
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
                observe(&subscriber, || {
                    self.font_size.get();
                    self.color.get();
                });
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
                font_size: self.font_size.clone(),
                color: self.color.clone(),
                cache: TextCache::default(),
            },
        );
        id
    }
}

struct TextWidget {
    source: Source,
    font_size: Rc<Cell<Option<f32>>>,
    color: Rc<Cell<Option<Color>>>,
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
            self.font_size.get().unwrap_or(theme.body_size),
            self.color.get().unwrap_or(theme.foreground),
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
