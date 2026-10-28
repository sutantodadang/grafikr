//! ComboBox with a dropdown list.

use std::cell::Cell;
use std::rc::Rc;

use grafikr_core::{
    Align, DirtyFlags, EdgeInsets, EventCx, EventStatus, Insets, Key, MeasureCx, MountCx, NodeId,
    PaintCx, Point, Rect, Signal, Size, Style, TextCache, UiEvent, View, Widget, observe,
};

use crate::into_views;

const ROW_HEIGHT: f32 = 28.0;

pub struct ComboBox {
    options: Vec<String>,
    selected: Signal<usize>,
    width: f32,
}

pub fn combo_box(
    options: impl IntoIterator<Item = impl Into<String>>,
    selected: Signal<usize>,
) -> ComboBox {
    ComboBox {
        options: options.into_iter().map(Into::into).collect(),
        selected,
        width: 220.0,
    }
}

impl ComboBox {
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
}

impl View for ComboBox {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let open = Signal::new(false);
        let visible = Rc::new(Cell::new(false));
        let control = cx.add_node(Style {
            width: Some(self.width),
            height: Some(cx.theme.control_height),
            overflow_hit: true,
            overlay: true,
            ..Style::default()
        });

        let runtime = cx.runtime.clone();
        let open_signal = open.clone();
        let visibility = visible.clone();
        let open_subscriber: Rc<dyn Fn()> = Rc::new(move || {
            visibility.set(open_signal.get());
            runtime.invalidate(control, DirtyFlags::LAYOUT);
        });
        let _ = observe(&open_subscriber, || open.get());

        let selected_runtime = cx.runtime.clone();
        let selected_subscriber: Rc<dyn Fn()> = Rc::new(move || {
            selected_runtime.invalidate(control, DirtyFlags::PAINT);
        });
        let _ = observe(&selected_subscriber, || self.selected.get());

        let mut backdrop = None;
        let mut card = None;
        cx.children(control, |cx| {
            let layer = cx.add_node(Style {
                absolute: true,
                inset: Insets {
                    top: Some(-4000.0),
                    left: Some(-4000.0),
                    right: Some(-4000.0),
                    bottom: Some(-4000.0),
                },
                ..Style::default()
            });
            cx.set_widget(layer, DropdownBackdrop { open: open.clone() });
            backdrop = Some(layer);

            let dropdown = cx.add_node(Style {
                absolute: true,
                inset: Insets {
                    left: Some(0.0),
                    top: Some(cx.theme.control_height + 4.0),
                    right: None,
                    bottom: None,
                },
                width: Some(self.width),
                background: Some(cx.theme.surface),
                border: Some(grafikr_core::Border {
                    width: 1.0,
                    color: cx.theme.muted.with_alpha(90),
                }),
                shadow: Some(grafikr_core::Shadow {
                    blur: 18.0,
                    color: grafikr_core::Color::BLACK.with_alpha(110),
                }),
                radius: 8.0,
                padding: EdgeInsets::all(4.0),
                align_items: Align::Stretch,
                ..Style::default()
            });
            card = Some(dropdown);

            cx.children(dropdown, |cx| {
                let list = cx.add_node(Style {
                    height: Some(self.options.len() as f32 * ROW_HEIGHT),
                    ..Style::default()
                });
                let list_runtime = cx.runtime.clone();
                let list_selected = self.selected.clone();
                let list_subscriber: Rc<dyn Fn()> = Rc::new(move || {
                    list_runtime.invalidate(list, DirtyFlags::PAINT);
                });
                let _ = observe(&list_subscriber, || self.selected.get());
                cx.set_widget(
                    list,
                    OptionList {
                        options: self.options.clone(),
                        selected: list_selected,
                        open: open.clone(),
                        highlight: self
                            .selected
                            .get()
                            .min(self.options.len().saturating_sub(1)),
                        caches: Vec::new(),
                        _subscriber: list_subscriber,
                    },
                );
            });
        });

        let card = card.expect("dropdown");
        let backdrop = backdrop.expect("backdrop");
        cx.tree.set_visibility(card, visible.clone());
        cx.tree.set_visibility(backdrop, visible.clone());
        cx.tree.set_modal_scope(card);

        cx.set_widget(
            control,
            ComboBoxWidget {
                options: self.options.clone(),
                selected: self.selected.clone(),
                open,
                cache: TextCache::default(),
                _open_subscriber: open_subscriber,
                _selected_subscriber: selected_subscriber,
            },
        );

        control
    }
}

struct ComboBoxWidget {
    options: Vec<String>,
    selected: Signal<usize>,
    open: Signal<bool>,
    cache: TextCache,
    _open_subscriber: Rc<dyn Fn()>,
    _selected_subscriber: Rc<dyn Fn()>,
}

impl Widget for ComboBoxWidget {
    fn measure(&mut self, cx: &mut MeasureCx, _available: Size) -> Size {
        Size::new(220.0, cx.theme.control_height)
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        let radius = cx.theme.control_radius();
        cx.push_bordered_rect(
            rect,
            cx.theme.field_background(cx.state),
            radius,
            Some((cx.theme.border_width, cx.theme.field_border(cx.state))),
        );
        let index = self
            .selected
            .get()
            .min(self.options.len().saturating_sub(1));
        let label = self
            .options
            .get(index)
            .cloned()
            .unwrap_or_else(|| "(kosong)".to_owned());
        let shape = self.cache.get(
            cx.text,
            &label,
            cx.theme.body_size,
            cx.theme.foreground,
            Some((rect.width - 32.0).max(1.0)),
        );
        cx.display.push_layout(
            &shape.layout,
            Point::new(rect.x + 10.0, rect.y + (rect.height - shape.height) * 0.5),
            cx.clip,
        );
        let arrow = self
            .cache
            .get(cx.text, "v", cx.theme.caption_size, cx.theme.muted, None);
        cx.display.push_layout(
            &arrow.layout,
            Point::new(
                rect.right() - 10.0 - arrow.width,
                rect.y + (rect.height - arrow.height) * 0.5,
            ),
            cx.clip,
        );
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::PointerDown { .. } => {
                self.open.set(!self.open.get());
                EventStatus::Handled
            }
            UiEvent::AccessibilityClick => {
                self.open.set(!self.open.get());
                EventStatus::Handled
            }
            UiEvent::KeyDown { key, .. } if cx.state.focused && !self.open.get() => {
                if matches!(key, Key::Space | Key::Enter | Key::Down) {
                    self.open.set(true);
                    return EventStatus::Handled;
                }
                EventStatus::Ignored
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
        node.set_role(accesskit::Role::ComboBox);
        node.set_label("ComboBox");
        node.add_action(accesskit::Action::Focus);
        node.add_action(accesskit::Action::Click);
    }
}

struct DropdownBackdrop {
    open: Signal<bool>,
}

impl Widget for DropdownBackdrop {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::ZERO
    }

    fn paint(&mut self, _cx: &mut PaintCx, _rect: Rect) {}

    fn event(&mut self, _cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::PointerDown { .. } => {
                self.open.set(false);
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }
}

struct OptionList {
    options: Vec<String>,
    selected: Signal<usize>,
    open: Signal<bool>,
    highlight: usize,
    caches: Vec<TextCache>,
    _subscriber: Rc<dyn Fn()>,
}

impl OptionList {
    fn select(&mut self, index: usize) {
        if self.options.is_empty() {
            return;
        }
        self.selected.set(index.min(self.options.len() - 1));
        self.open.set(false);
    }
}

impl Widget for OptionList {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::new(200.0, self.options.len() as f32 * ROW_HEIGHT)
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        if self.caches.len() != self.options.len() {
            self.caches = (0..self.options.len())
                .map(|_| TextCache::default())
                .collect();
        }
        let selected = self.selected.get();
        for (index, option) in self.options.iter().enumerate() {
            let row = Rect::new(
                rect.x,
                rect.y + index as f32 * ROW_HEIGHT,
                rect.width,
                ROW_HEIGHT,
            );
            if index == selected {
                cx.display
                    .push_rect(row, cx.theme.accent.with_alpha(55), 4.0, cx.clip);
            } else if index == self.highlight {
                cx.display
                    .push_rect(row, cx.theme.muted.with_alpha(24), 4.0, cx.clip);
            }
            let color = if index == selected {
                cx.theme.foreground
            } else {
                cx.theme.foreground.with_alpha(220)
            };
            let shape = self.caches[index].get(
                cx.text,
                option,
                cx.theme.body_size,
                color,
                Some((rect.width - 16.0).max(1.0)),
            );
            cx.display.push_layout(
                &shape.layout,
                Point::new(row.x + 8.0, row.y + (ROW_HEIGHT - shape.height) * 0.5),
                cx.clip,
            );
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::PointerMoved { pos } => {
                let index = ((pos.y - cx.rect.y) / ROW_HEIGHT).floor();
                if index >= 0.0 && (index as usize) < self.options.len() {
                    self.highlight = index as usize;
                    cx.request_paint();
                }
                EventStatus::Handled
            }
            UiEvent::PointerDown { pos, .. } => {
                let index = ((pos.y - cx.rect.y) / ROW_HEIGHT).floor();
                if index >= 0.0 && (index as usize) < self.options.len() {
                    self.select(index as usize);
                }
                EventStatus::Handled
            }
            UiEvent::KeyDown { key, .. } if cx.state.focused => {
                match key {
                    Key::Up => self.highlight = self.highlight.saturating_sub(1),
                    Key::Down => {
                        self.highlight =
                            (self.highlight + 1).min(self.options.len().saturating_sub(1))
                    }
                    Key::Home => self.highlight = 0,
                    Key::End => self.highlight = self.options.len().saturating_sub(1),
                    Key::Enter | Key::Space => self.select(self.highlight),
                    Key::Escape => self.open.set(false),
                    _ => return EventStatus::Ignored,
                }
                cx.request_paint();
                EventStatus::Handled
            }
            UiEvent::FocusLost => {
                self.open.set(false);
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
        node.set_role(accesskit::Role::ListBox);
        node.set_label("Pilihan");
    }
}

into_views!(ComboBox);
