//! Hover tooltip with a small delay. The card is painted by the wrapper widget.

use std::time::{Duration, Instant};

use grafikr_core::{
    EventCx, EventStatus, IntoViews, MeasureCx, MountCx, NodeId, PaintCx, Point, Rect, Size, Style,
    TextCache, UiEvent, View, Widget,
};

use crate::into_views;

pub struct Tooltip {
    label: String,
    child: Box<dyn IntoViews>,
    delay: Duration,
}

pub fn tooltip(label: impl Into<String>, child: impl IntoViews + 'static) -> Tooltip {
    Tooltip {
        label: label.into(),
        child: Box::new(child),
        delay: Duration::from_millis(450),
    }
}

impl Tooltip {
    pub fn delay_ms(mut self, delay: u64) -> Self {
        self.delay = Duration::from_millis(delay);
        self
    }
}

impl View for Tooltip {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style {
            overlay: true,
            ..Style::default()
        });
        cx.children(id, |cx| self.child.mount_into(cx));
        cx.set_widget(
            id,
            TooltipWidget {
                label: self.label.clone(),
                delay: self.delay,
                since: None,
                shown: false,
                cache: TextCache::default(),
            },
        );
        id
    }
}

struct TooltipWidget {
    label: String,
    delay: Duration,
    since: Option<Instant>,
    shown: bool,
    cache: TextCache,
}

impl TooltipWidget {
    fn clear(&mut self) {
        self.since = None;
        self.shown = false;
    }
}

impl Widget for TooltipWidget {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::ZERO
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        if !self.shown {
            let Some(since) = self.since else {
                return;
            };
            if since.elapsed() >= self.delay {
                self.shown = true;
            } else {
                cx.runtime.request_animation();
                return;
            }
        }
        let shape = self.cache.get(
            cx.text,
            &self.label,
            cx.theme.caption_size,
            cx.theme.foreground,
            None,
        );
        let card = Rect::new(
            rect.x,
            rect.bottom() + 6.0,
            shape.width + 16.0,
            shape.height + 8.0,
        );
        cx.display.push_shadow(
            card,
            6.0,
            12.0,
            cx.theme.background.with_alpha(140),
            cx.clip,
        );
        cx.display.push_bordered_rect(
            card,
            cx.theme.surface,
            6.0,
            Some((1.0, cx.theme.muted.with_alpha(80))),
            cx.clip,
        );
        cx.display.push_layout(
            &shape.layout,
            Point::new(card.x + 8.0, card.y + 4.0),
            cx.clip,
        );
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::PointerMoved { .. } => {
                if self.since.is_none() && !self.shown {
                    self.since = Some(Instant::now());
                    cx.runtime.request_animation();
                }
                if let Some(since) = self.since
                    && !self.shown
                    && since.elapsed() >= self.delay
                {
                    self.shown = true;
                    cx.request_paint();
                } else if !self.shown {
                    cx.runtime.request_animation();
                }
                EventStatus::Ignored
            }
            UiEvent::PointerLeft => {
                self.clear();
                cx.request_paint();
                EventStatus::Ignored
            }
            _ => EventStatus::Ignored,
        }
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_description(self.label.clone());
    }
}

into_views!(Tooltip);
