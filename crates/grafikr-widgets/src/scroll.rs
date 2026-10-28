//! Scroll container and virtualized list.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use grafikr_core::{
    Align, Color, EventCx, EventStatus, IntoViews, Key, MeasureCx, MountCx, NodeId, PaintCx, Point,
    Rect, ScrollState, Size, Style, UiEvent, View, Widget,
};
use parley::Layout;

use crate::into_views;

const SCROLLBAR_WIDTH: f32 = 6.0;
const SCROLLBAR_GUTTER: f32 = 12.0;

pub(crate) fn scrollbar_track(rect: Rect) -> Rect {
    Rect::new(
        rect.right() - SCROLLBAR_WIDTH - 3.0,
        rect.y + 4.0,
        SCROLLBAR_WIDTH,
        (rect.height - 8.0).max(0.0),
    )
}

pub(crate) fn scrollbar_thumb(rect: Rect, state: &ScrollState) -> Option<Rect> {
    let max = state.max_offset();
    if max <= 0.0 || state.content() <= 0.0 {
        return None;
    }
    let track = scrollbar_track(rect);
    if track.height <= 0.0 {
        return None;
    }
    let visible = (state.viewport() / state.content()).clamp(0.0, 1.0);
    let thumb_height = (track.height * visible).max(24.0).min(track.height);
    let travel = track.height - thumb_height;
    let y = track.y + (state.offset() / max) * travel;
    Some(Rect::new(track.x, y, track.width, thumb_height))
}

pub(crate) fn scrollbar_offset(rect: Rect, state: &ScrollState, pointer_y: f32) -> f32 {
    if state.content() <= 0.0 {
        return 0.0;
    }
    let track = scrollbar_track(rect);
    let visible = (state.viewport() / state.content()).clamp(0.0, 1.0);
    let thumb_height = (track.height * visible).max(24.0).min(track.height);
    let travel = track.height - thumb_height;
    if travel <= 0.0 {
        return 0.0;
    }
    ((pointer_y - track.y - thumb_height * 0.5) / travel).clamp(0.0, 1.0) * state.max_offset()
}

pub(crate) fn paint_scrollbar(cx: &mut PaintCx, rect: Rect, state: &ScrollState) {
    if let Some(thumb) = scrollbar_thumb(rect, state) {
        let track = scrollbar_track(rect);
        cx.display.push_rect(
            track,
            cx.theme.muted.with_alpha(30),
            SCROLLBAR_WIDTH * 0.5,
            cx.clip,
        );
        cx.display.push_rect(
            thumb,
            cx.theme.muted.with_alpha(140),
            SCROLLBAR_WIDTH * 0.5,
            cx.clip,
        );
    }
}

pub(crate) fn hit_scrollbar(rect: Rect, pos: Point) -> bool {
    pos.x >= rect.right() - SCROLLBAR_GUTTER
}

// ---------------------------------------------------------------- ScrollView

pub struct ScrollView {
    children: Box<dyn IntoViews>,
    height: Option<f32>,
    width: Option<f32>,
    grow: f32,
}

pub fn scroll_view(children: impl IntoViews + 'static) -> ScrollView {
    ScrollView {
        children: Box::new(children),
        height: None,
        width: None,
        grow: 0.0,
    }
}

impl ScrollView {
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Fill remaining vertical space when the parent gives one. Implies no fixed height.
    pub fn grow(mut self) -> Self {
        self.grow = 1.0;
        self
    }
}

impl View for ScrollView {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let height = self
            .height
            .or(if self.grow > 0.0 { None } else { Some(240.0) });
        let outer = cx.add_node(Style {
            clip: true,
            width: self.width,
            height,
            flex_grow: self.grow,
            align_items: Align::Stretch,
            padding: grafikr_core::EdgeInsets {
                right: SCROLLBAR_GUTTER,
                ..grafikr_core::EdgeInsets::ZERO
            },
            ..Style::default()
        });
        cx.children(outer, |cx| {
            let content = cx.add_node(Style {
                direction: grafikr_core::Direction::Column,
                ..Style::default()
            });
            cx.children(content, |cx| self.children.mount_into(cx));
        });

        let state = Rc::new(ScrollState::new());
        cx.tree.set_scroll(outer, state.clone());
        cx.set_widget(
            outer,
            ScrollWidget {
                state,
                dragging: false,
            },
        );
        outer
    }
}

struct ScrollWidget {
    state: Rc<ScrollState>,
    dragging: bool,
}

impl ScrollWidget {
    fn page(&self) -> f32 {
        (self.state.viewport() * 0.85).max(40.0)
    }

    fn mark_scrolled(&self, cx: &mut EventCx) {
        // Children rects move, so a layout pass is required (Taffy is cached).
        cx.request_layout();
    }
}

impl Widget for ScrollWidget {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::ZERO
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        paint_scrollbar(cx, rect, &self.state);
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::Wheel { delta } => {
                if self.state.scroll_by(delta.y) {
                    self.mark_scrolled(cx);
                }
                EventStatus::Handled
            }
            UiEvent::PointerDown { pos, .. } if hit_scrollbar(cx.rect, *pos) => {
                self.dragging = true;
                if self
                    .state
                    .set_offset(scrollbar_offset(cx.rect, &self.state, pos.y))
                {
                    self.mark_scrolled(cx);
                }
                EventStatus::Handled
            }
            UiEvent::PointerMoved { pos } if self.dragging => {
                if self
                    .state
                    .set_offset(scrollbar_offset(cx.rect, &self.state, pos.y))
                {
                    self.mark_scrolled(cx);
                }
                EventStatus::Handled
            }
            UiEvent::PointerUp { .. } => {
                self.dragging = false;
                EventStatus::Handled
            }
            UiEvent::KeyDown { key, .. } if cx.state.focused => {
                let delta = match key {
                    Key::Up => -40.0,
                    Key::Down => 40.0,
                    Key::PageUp => -self.page(),
                    Key::PageDown => self.page(),
                    Key::Home => f32::NEG_INFINITY,
                    Key::End => f32::INFINITY,
                    _ => return EventStatus::Ignored,
                };
                let target = if delta.is_infinite() {
                    if delta.is_sign_negative() {
                        0.0
                    } else {
                        self.state.max_offset()
                    }
                } else {
                    self.state.offset() + delta
                };
                if self.state.set_offset(target) {
                    self.mark_scrolled(cx);
                }
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::ScrollView);
        node.add_action(accesskit::Action::ScrollDown);
        node.add_action(accesskit::Action::ScrollUp);
    }
}

// --------------------------------------------------------------- VirtualList

type LabelFn = Rc<dyn Fn(usize) -> String>;
type SelectFn = Rc<dyn Fn(usize)>;

pub struct VirtualList {
    rows: usize,
    row_height: f32,
    height: f32,
    width: Option<f32>,
    label: LabelFn,
    on_select: Option<SelectFn>,
    selected: Option<usize>,
}

pub fn virtual_list(
    rows: usize,
    row_height: f32,
    label: impl Fn(usize) -> String + 'static,
) -> VirtualList {
    VirtualList {
        rows,
        row_height: row_height.max(1.0),
        height: 240.0,
        width: None,
        label: Rc::new(label),
        on_select: None,
        selected: None,
    }
}

impl VirtualList {
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn on_select(mut self, handler: impl Fn(usize) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected = Some(index);
        self
    }
}

impl View for VirtualList {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let state = Rc::new(ScrollState::new());
        state.set_content(self.rows as f32 * self.row_height);
        let node = cx.add_node(Style {
            clip: true,
            width: self.width,
            height: Some(self.height),
            ..Style::default()
        });
        cx.tree.set_scroll(node, state.clone());
        cx.set_widget(
            node,
            VirtualListWidget {
                rows: self.rows,
                row_height: self.row_height,
                label: self.label.clone(),
                on_select: self.on_select.clone(),
                selected: self.selected,
                state,
                cache: HashMap::new(),
                cache_width: 0.0,
                dragging: false,
            },
        );
        node
    }
}

struct VirtualListWidget {
    rows: usize,
    row_height: f32,
    label: LabelFn,
    on_select: Option<SelectFn>,
    selected: Option<usize>,
    state: Rc<ScrollState>,
    cache: HashMap<usize, Arc<Layout<Color>>>,
    cache_width: f32,
    dragging: bool,
}

impl VirtualListWidget {
    fn content_width(&self, rect: Rect) -> f32 {
        (rect.width - SCROLLBAR_GUTTER - 16.0).max(1.0)
    }

    fn visible_range(&self, viewport: f32) -> (usize, usize) {
        if self.rows == 0 {
            return (0, 0);
        }
        let first = (self.state.offset() / self.row_height).floor() as usize;
        let visible = (viewport / self.row_height).ceil() as usize + 1;
        (first.min(self.rows - 1), (first + visible).min(self.rows))
    }

    fn select(&mut self, index: usize, cx: &mut EventCx) {
        if self.rows == 0 {
            return;
        }
        let index = index.min(self.rows - 1);
        if self.selected != Some(index) {
            self.selected = Some(index);
            if let Some(handler) = &self.on_select {
                handler(index);
            }
            cx.request_paint();
        }
    }

    fn scroll_to(&mut self, index: usize, viewport: f32, cx: &mut EventCx) {
        let top = index as f32 * self.row_height;
        let bottom = top + self.row_height;
        let offset = self.state.offset();
        if top < offset {
            self.state.set_offset(top);
            cx.request_paint();
        } else if bottom > offset + viewport {
            self.state.set_offset(bottom - viewport);
            cx.request_paint();
        }
    }

    fn row_at(&self, rect: Rect, pos: Point) -> Option<usize> {
        if self.rows == 0 || pos.y < rect.y || pos.y >= rect.bottom() {
            return None;
        }
        let row = ((pos.y - rect.y + self.state.offset()) / self.row_height).floor() as usize;
        (row < self.rows).then_some(row)
    }
}

impl Widget for VirtualListWidget {
    fn measure(&mut self, _cx: &mut MeasureCx, available: Size) -> Size {
        Size::new(available.width.min(320.0), 240.0)
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        let content = Rect::new(
            rect.x,
            rect.y,
            (rect.width - SCROLLBAR_GUTTER).max(1.0),
            rect.height,
        );
        let clip = cx
            .clip
            .map(|clip| clip.intersect(content))
            .unwrap_or(content);
        let (first, end) = self.visible_range(rect.height);
        let row_width = content.width;
        let width = self.content_width(rect);
        if (width - self.cache_width).abs() > 0.5 {
            self.cache.clear();
            self.cache_width = width;
        }
        if self.cache.len() > 256 {
            self.cache.clear();
        }

        for index in first..end {
            let y = rect.y + index as f32 * self.row_height - self.state.offset();
            let row = Rect::new(content.x, y, row_width, self.row_height);
            let background = if Some(index) == self.selected {
                cx.theme.accent.with_alpha(48)
            } else if index % 2 == 0 {
                cx.theme.surface
            } else {
                cx.theme.background
            };
            cx.display.push_rect(row, background, 0.0, Some(clip));

            let label = (self.label)(index);
            let layout = self.cache.entry(index).or_insert_with(|| {
                cx.text
                    .shape(&label, cx.theme.body_size, cx.theme.foreground, Some(width))
            });
            let origin = Point::new(
                row.x + 8.0,
                row.y + (self.row_height - layout.height()).max(0.0) * 0.5,
            );
            cx.display.push_layout(layout, origin, Some(clip));
        }

        paint_scrollbar(cx, rect, &self.state);
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::Wheel { delta } => {
                if self.state.scroll_by(delta.y) {
                    cx.request_paint();
                }
                EventStatus::Handled
            }
            UiEvent::PointerDown { pos, .. } if hit_scrollbar(cx.rect, *pos) => {
                self.dragging = true;
                if self
                    .state
                    .set_offset(scrollbar_offset(cx.rect, &self.state, pos.y))
                {
                    cx.request_paint();
                }
                EventStatus::Handled
            }
            UiEvent::PointerDown { pos, .. } => {
                if let Some(row) = self.row_at(cx.rect, *pos) {
                    self.select(row, cx);
                }
                EventStatus::Handled
            }
            UiEvent::PointerMoved { pos } if self.dragging => {
                if self
                    .state
                    .set_offset(scrollbar_offset(cx.rect, &self.state, pos.y))
                {
                    cx.request_paint();
                }
                EventStatus::Handled
            }
            UiEvent::PointerUp { .. } => {
                self.dragging = false;
                EventStatus::Handled
            }
            UiEvent::KeyDown { key, .. } if cx.state.focused => {
                let visible = (cx.rect.height / self.row_height).floor().max(1.0) as usize;
                match key {
                    Key::Up => {
                        let next = self.selected.map(|i| i.saturating_sub(1)).unwrap_or(0);
                        self.select(next, cx);
                        self.scroll_to(next, cx.rect.height, cx);
                    }
                    Key::Down => {
                        let next = self
                            .selected
                            .map(|i| (i + 1).min(self.rows.saturating_sub(1)))
                            .unwrap_or(0);
                        self.select(next, cx);
                        self.scroll_to(next, cx.rect.height, cx);
                    }
                    Key::PageUp => {
                        let next = self.selected.unwrap_or(0).saturating_sub(visible);
                        self.select(next, cx);
                        self.scroll_to(next, cx.rect.height, cx);
                    }
                    Key::PageDown => {
                        let next =
                            (self.selected.unwrap_or(0) + visible).min(self.rows.saturating_sub(1));
                        self.select(next, cx);
                        self.scroll_to(next, cx.rect.height, cx);
                    }
                    Key::Home => {
                        self.select(0, cx);
                        self.state.set_offset(0.0);
                        cx.request_paint();
                    }
                    Key::End => {
                        let last = self.rows.saturating_sub(1);
                        self.select(last, cx);
                        self.state.set_offset(self.state.max_offset());
                        cx.request_paint();
                    }
                    _ => return EventStatus::Ignored,
                }
                EventStatus::Handled
            }
            UiEvent::AccessibilityClick => {
                if let Some(selected) = self.selected {
                    self.select(selected, cx);
                }
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
        node.set_role(accesskit::Role::List);
        node.set_label(format!("Daftar {} item", self.rows));
        if let Some(selected) = self.selected {
            node.set_description(format!("Item {} terpilih", selected + 1));
        }
        node.add_action(accesskit::Action::Focus);
    }
}

into_views!(ScrollView, VirtualList);
