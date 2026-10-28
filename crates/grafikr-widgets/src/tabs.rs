//! Tabs with signal-driven panes. Only the active pane participates in layout.

use std::cell::Cell;
use std::rc::Rc;

use grafikr_core::{
    Align, Color, DirtyFlags, EdgeInsets, EventCx, EventStatus, IntoViews, Key, MeasureCx, MountCx,
    NodeId, PaintCx, Point, Rect, Runtime, Signal, Size, Style, TextCache, UiEvent, View, Widget,
    observe,
};

use crate::into_views;

const HEADER_HEIGHT: f32 = 40.0;

pub struct Tabs {
    labels: Vec<String>,
    active: Signal<usize>,
    panes: Box<dyn IntoViews>,
    width: Option<f32>,
    height: Option<f32>,
}

/// `labels` should contain one entry per pane; extra panes get a fallback label.
pub fn tabs(
    labels: impl IntoIterator<Item = impl Into<String>>,
    active: Signal<usize>,
    panes: impl IntoViews + 'static,
) -> Tabs {
    Tabs {
        labels: labels.into_iter().map(Into::into).collect(),
        active,
        panes: Box::new(panes),
        width: None,
        height: None,
    }
}

impl Tabs {
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }
}

impl View for Tabs {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let root = cx.add_node(Style {
            width: self.width,
            height: self.height,
            padding: EdgeInsets {
                top: HEADER_HEIGHT,
                ..EdgeInsets::ZERO
            },
            ..Style::default()
        });

        let mut pane_ids = Vec::new();
        cx.children(root, |cx| {
            let panes = cx.add_node(Style {
                direction: grafikr_core::Direction::Column,
                flex_grow: 1.0,
                align_items: Align::Stretch,
                ..Style::default()
            });
            cx.children(panes, |cx| self.panes.mount_into(cx));
            pane_ids = cx.tree.children(panes).to_vec();
        });

        let active = self.active.get();
        let visibility: Vec<Rc<Cell<bool>>> = pane_ids
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let visible = Rc::new(Cell::new(index == active));
                cx.tree.set_visibility(*id, visible.clone());
                visible
            })
            .collect();

        let runtime: Runtime = cx.runtime.clone();
        let signal = self.active.clone();
        let cells = visibility.clone();
        let subscriber: Rc<dyn Fn()> = Rc::new(move || {
            let active = signal.get();
            for (index, cell) in cells.iter().enumerate() {
                cell.set(index == active);
            }
            runtime.invalidate(root, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
        });
        let _ = observe(&subscriber, || self.active.get());

        let labels = (0..pane_ids.len())
            .map(|index| {
                self.labels
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| format!("Tab {}", index + 1))
            })
            .collect();
        cx.set_widget(
            root,
            TabsWidget {
                labels,
                active: self.active.clone(),
                caches: Vec::new(),
                hover: None,
                _subscriber: subscriber,
            },
        );
        root
    }
}

struct TabsWidget {
    labels: Vec<String>,
    active: Signal<usize>,
    caches: Vec<TextCache>,
    hover: Option<usize>,
    _subscriber: Rc<dyn Fn()>,
}

impl TabsWidget {
    fn count(&self) -> usize {
        self.labels.len().max(1)
    }

    fn index_at(&self, rect: Rect, pos: Point) -> Option<usize> {
        if pos.y < rect.y || pos.y >= rect.y + HEADER_HEIGHT || self.labels.is_empty() {
            return None;
        }
        let width = rect.width / self.count() as f32;
        if width <= 0.0 {
            return None;
        }
        let index = ((pos.x - rect.x) / width).floor() as isize;
        (index >= 0 && (index as usize) < self.labels.len()).then_some(index as usize)
    }

    fn select(&self, index: usize) {
        if self.active.get() != index {
            self.active.set(index);
        }
    }
}

impl Widget for TabsWidget {
    fn measure(&mut self, cx: &mut MeasureCx, available: Size) -> Size {
        Size::new(
            available.width.min(600.0),
            cx.theme.control_height + HEADER_HEIGHT,
        )
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        if self.labels.is_empty() {
            return;
        }
        if self.caches.len() != self.labels.len() {
            self.caches = (0..self.labels.len())
                .map(|_| TextCache::default())
                .collect();
        }
        let header = Rect::new(rect.x, rect.y, rect.width, HEADER_HEIGHT);
        cx.push_rect(header, cx.theme.surface, 0.0);
        let width = rect.width / self.labels.len() as f32;
        let active = self.active.get().min(self.labels.len() - 1);

        for (index, label) in self.labels.iter().enumerate() {
            let cell = Rect::new(rect.x + index as f32 * width, rect.y, width, HEADER_HEIGHT);
            if self.hover == Some(index) && index != active {
                cx.push_rect(
                    Rect::new(cell.x, cell.y, cell.width, cell.height - 2.0),
                    cx.theme.muted.with_alpha(18),
                    0.0,
                );
            }
            let color = if index == active {
                cx.theme.foreground
            } else {
                cx.theme.muted
            };
            let cache = &mut self.caches[index];
            let shape = cache.get(
                cx.text,
                label,
                cx.theme.body_size,
                color,
                Some((width - 12.0).max(1.0)),
            );
            let origin = Point::new(
                cell.x + (cell.width - shape.width) * 0.5,
                cell.y + (HEADER_HEIGHT - shape.height) * 0.5,
            );
            cx.display.push_layout(&shape.layout, origin, cx.clip);
            if index == active {
                cx.push_rect(
                    Rect::new(
                        cell.x + 8.0,
                        cell.y + HEADER_HEIGHT - 2.0,
                        (cell.width - 16.0).max(0.0),
                        2.0,
                    ),
                    cx.theme.accent,
                    1.0,
                );
                if cx.state.focused {
                    cx.push_bordered_rect(
                        Rect::new(
                            cell.x + 4.0,
                            cell.y + 4.0,
                            cell.width - 8.0,
                            HEADER_HEIGHT - 8.0,
                        ),
                        Color::TRANSPARENT,
                        4.0,
                        Some((1.0, cx.theme.accent.with_alpha(150))),
                    );
                }
            }
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::PointerMoved { pos } => {
                let hover = self.index_at(cx.rect, *pos);
                if hover != self.hover {
                    self.hover = hover;
                    cx.request_paint();
                }
                EventStatus::Ignored
            }
            UiEvent::PointerDown { pos, .. } => {
                if let Some(index) = self.index_at(cx.rect, *pos) {
                    self.select(index);
                    EventStatus::Handled
                } else {
                    EventStatus::Ignored
                }
            }
            UiEvent::PointerLeft => {
                if self.hover.take().is_some() {
                    cx.request_paint();
                }
                EventStatus::Ignored
            }
            UiEvent::KeyDown { key, .. } if cx.state.focused => {
                let current = self.active.get().min(self.labels.len().saturating_sub(1));
                let next = match key {
                    Key::Left => current.saturating_sub(1),
                    Key::Right => (current + 1).min(self.labels.len().saturating_sub(1)),
                    Key::Home => 0,
                    Key::End => self.labels.len().saturating_sub(1),
                    _ => return EventStatus::Ignored,
                };
                self.select(next);
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn cursor(&self) -> grafikr_core::CursorIcon {
        grafikr_core::CursorIcon::Pointer
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::TabList);
        node.set_label("Tabs");
        node.add_action(accesskit::Action::Focus);
    }
}

into_views!(Tabs);
