//! Hover tooltip with a small delay. The card is painted by the wrapper widget.

use std::time::{Duration, Instant};

use grafikr_core::{
    DirtyFlags, EventCx, EventStatus, IntoViews, MeasureCx, MountCx, NodeId, PaintCx, Point, Rect,
    Signal, Size, Style, TextCache, UiEvent, View, Widget, observe,
};
use std::rc::Rc;

use crate::into_views;

pub struct Tooltip {
    label: Signal<String>,
    child: Box<dyn IntoViews>,
    delay_ms: Signal<u64>,
}

pub fn tooltip(label: impl Into<String>, child: impl IntoViews + 'static) -> Tooltip {
    Tooltip {
        label: Signal::new(label.into()),
        child: Box::new(child),
        delay_ms: Signal::new(450),
    }
}

impl Tooltip {
    pub fn delay_ms(self, delay: u64) -> Self {
        self.delay_ms.set(delay);
        self
    }

    pub(crate) fn controller(&self) -> TooltipController {
        TooltipController {
            label: self.label.clone(),
            delay_ms: self.delay_ms.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct TooltipController {
    label: Signal<String>,
    delay_ms: Signal<u64>,
}

impl TooltipController {
    pub(crate) fn set_label(&self, value: String) {
        self.label.set(value);
    }
    pub(crate) fn set_delay_ms(&self, value: u64) {
        self.delay_ms.set(value);
    }
}

impl View for Tooltip {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style {
            overlay: true,
            ..Style::default()
        });
        cx.children(id, |cx| self.child.mount_into(cx));
        let runtime = cx.runtime.clone();
        let label = self.label.clone();
        let delay = self.delay_ms.clone();
        let subscriber: Rc<dyn Fn()> =
            Rc::new(move || runtime.invalidate(id, DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY));
        observe(&subscriber, || {
            label.get();
            delay.get();
        });
        cx.set_widget(
            id,
            TooltipWidget {
                label: self.label.clone(),
                delay_ms: self.delay_ms.clone(),
                since: None,
                shown: false,
                cache: TextCache::default(),
                _subscriber: subscriber,
            },
        );
        id
    }
}

struct TooltipWidget {
    label: Signal<String>,
    delay_ms: Signal<u64>,
    since: Option<Instant>,
    shown: bool,
    cache: TextCache,
    _subscriber: Rc<dyn Fn()>,
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
            if since.elapsed() >= Duration::from_millis(self.delay_ms.get()) {
                self.shown = true;
            } else {
                cx.runtime.request_animation();
                return;
            }
        }
        let shape = self.cache.get(
            cx.text,
            &self.label.get(),
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
                    && since.elapsed() >= Duration::from_millis(self.delay_ms.get())
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
        node.set_description(self.label.get());
    }
}

into_views!(Tooltip);
