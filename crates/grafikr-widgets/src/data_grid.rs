//! Virtualized data grid: fixed columns, visible rows only.

use std::collections::HashMap;
use std::rc::Rc;

use grafikr_core::{
    EventCx, EventStatus, Key, MeasureCx, MountCx, NodeId, PaintCx, Point, Rect, ScrollState, Size,
    Style, TextCache, UiEvent, View, Widget,
};

use crate::into_views;
use crate::scroll::{hit_scrollbar, paint_scrollbar, scrollbar_offset};

const HEADER_HEIGHT: f32 = 32.0;
const PADDING_X: f32 = 8.0;

#[derive(Clone)]
pub struct GridColumn {
    pub title: String,
    pub width: f32,
}

impl GridColumn {
    pub fn new(title: impl Into<String>, width: f32) -> Self {
        Self {
            title: title.into(),
            width: width.max(24.0),
        }
    }
}

type RowFn = Rc<dyn Fn(usize) -> Vec<String>>;
type SelectFn = Rc<dyn Fn(usize)>;

pub struct DataGrid {
    columns: Vec<GridColumn>,
    rows: usize,
    row_height: f32,
    height: f32,
    width: Option<f32>,
    row: RowFn,
    on_select: Option<SelectFn>,
    selected: Option<usize>,
    label: String,
}

pub fn data_grid(
    columns: impl IntoIterator<Item = GridColumn>,
    rows: usize,
    row_height: f32,
    row: impl Fn(usize) -> Vec<String> + 'static,
) -> DataGrid {
    DataGrid {
        columns: columns.into_iter().collect(),
        rows,
        row_height: row_height.max(18.0),
        height: 320.0,
        width: None,
        row: Rc::new(row),
        on_select: None,
        selected: None,
        label: "Data grid".to_owned(),
    }
}

impl DataGrid {
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected = Some(index);
        self
    }

    pub fn on_select(mut self, handler: impl Fn(usize) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
}

impl View for DataGrid {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let natural = self.columns.iter().map(|column| column.width).sum::<f32>() + 12.0;
        let width = self.width.unwrap_or(natural);
        let node = cx.add_node(Style {
            width: Some(width),
            height: Some(self.height),
            clip: true,
            ..Style::default()
        });
        let state = ScrollState::new();
        state.set_content(self.rows as f32 * self.row_height);
        cx.set_widget(
            node,
            DataGridWidget {
                columns: self.columns.clone(),
                rows: self.rows,
                row_height: self.row_height,
                row: self.row.clone(),
                on_select: self.on_select.clone(),
                selected: self.selected,
                state,
                header_caches: Vec::new(),
                cell_caches: HashMap::new(),
                cache_width: 0.0,
                dragging: false,
                label: self.label.clone(),
            },
        );
        node
    }
}

struct DataGridWidget {
    columns: Vec<GridColumn>,
    rows: usize,
    row_height: f32,
    row: RowFn,
    on_select: Option<SelectFn>,
    selected: Option<usize>,
    state: ScrollState,
    header_caches: Vec<TextCache>,
    cell_caches: HashMap<u64, TextCache>,
    cache_width: f32,
    dragging: bool,
    label: String,
}

impl DataGridWidget {
    fn content_rect(&self, rect: Rect) -> Rect {
        Rect::new(
            rect.x,
            rect.y + HEADER_HEIGHT,
            rect.width,
            (rect.height - HEADER_HEIGHT).max(1.0),
        )
    }

    fn total_width(&self) -> f32 {
        self.columns.iter().map(|column| column.width).sum()
    }

    fn visible_range(&self, viewport: f32) -> (usize, usize) {
        if self.rows == 0 {
            return (0, 0);
        }
        let first = (self.state.offset() / self.row_height).floor() as usize;
        let count = (viewport / self.row_height).ceil() as usize + 1;
        (first.min(self.rows - 1), (first + count).min(self.rows))
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
        let content = self.content_rect(rect);
        if !content.contains(pos) {
            return None;
        }
        let row = ((pos.y - content.y + self.state.offset()) / self.row_height).floor() as usize;
        (row < self.rows).then_some(row)
    }
}

impl Widget for DataGridWidget {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::new(self.total_width() + 12.0, 320.0)
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        let content = self.content_rect(rect);
        self.state.set_viewport(content.height);
        self.state.set_content(self.rows as f32 * self.row_height);
        if self.header_caches.len() != self.columns.len() {
            self.header_caches = self.columns.iter().map(|_| TextCache::default()).collect();
        }
        if (self.cache_width - rect.width).abs() > 0.5 {
            self.cell_caches.clear();
            self.cache_width = rect.width;
        }
        if self.cell_caches.len() > 1024 {
            self.cell_caches.clear();
        }

        let header_rect = Rect::new(rect.x, rect.y, rect.width, HEADER_HEIGHT);
        let header_clip = cx
            .clip
            .map(|clip| clip.intersect(header_rect))
            .unwrap_or(header_rect);
        cx.push_rect(header_rect, cx.theme.surface, 0.0);
        let mut x = rect.x;
        for (index, column) in self.columns.iter().enumerate() {
            let cache = &mut self.header_caches[index];
            let shape = cache.get(
                cx.text,
                &column.title,
                cx.theme.caption_size,
                cx.theme.muted,
                Some((column.width - PADDING_X * 2.0).max(1.0)),
            );
            cx.display.push_layout(
                &shape.layout,
                Point::new(x + PADDING_X, rect.y + (HEADER_HEIGHT - shape.height) * 0.5),
                Some(header_clip),
            );
            x += column.width;
        }
        cx.push_rect(
            Rect::new(rect.x, rect.y + HEADER_HEIGHT - 1.0, rect.width, 1.0),
            cx.theme.muted.with_alpha(70),
            0.0,
        );

        let (first, end) = self.visible_range(content.height);
        let offset = self.state.offset();
        for index in first..end {
            let row_y = content.y + index as f32 * self.row_height - offset;
            let row_rect = Rect::new(rect.x, row_y, rect.width, self.row_height);
            let row_clip = cx
                .clip
                .map(|clip| clip.intersect(row_rect))
                .unwrap_or(row_rect);
            let background = if Some(index) == self.selected {
                cx.theme.accent.with_alpha(55)
            } else if index % 2 == 0 {
                cx.theme.surface.with_alpha(120)
            } else {
                cx.theme.background
            };
            cx.display.push_rect(row_rect, background, 0.0, cx.clip);

            let values = (self.row)(index);
            let mut cell_x = rect.x;
            for (column_index, column) in self.columns.iter().enumerate() {
                if let Some(value) = values.get(column_index) {
                    let key = ((index as u64) << 32) | column_index as u64;
                    let cache = self.cell_caches.entry(key).or_default();
                    let shape = cache.get(
                        cx.text,
                        value,
                        cx.theme.body_size,
                        cx.theme.foreground,
                        Some((column.width - PADDING_X * 2.0).max(1.0)),
                    );
                    cx.display.push_layout(
                        &shape.layout,
                        Point::new(
                            cell_x + PADDING_X,
                            row_y + (self.row_height - shape.height) * 0.5,
                        ),
                        Some(row_clip),
                    );
                }
                cell_x += column.width;
            }
        }

        paint_scrollbar(cx, content, &self.state);
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        let content = self.content_rect(cx.rect);
        match event {
            UiEvent::Wheel { delta } => {
                if self.state.scroll_by(delta.y) {
                    cx.request_paint();
                }
                EventStatus::Handled
            }
            UiEvent::PointerDown { pos, .. } if hit_scrollbar(content, *pos) => {
                self.dragging = true;
                if self
                    .state
                    .set_offset(scrollbar_offset(content, &self.state, pos.y))
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
                    .set_offset(scrollbar_offset(content, &self.state, pos.y))
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
                let page = (content.height / self.row_height).floor().max(1.0) as usize;
                let next = match key {
                    Key::Up => self.selected.map(|i| i.saturating_sub(1)).unwrap_or(0),
                    Key::Down => self
                        .selected
                        .map(|i| (i + 1).min(self.rows.saturating_sub(1)))
                        .unwrap_or(0),
                    Key::PageUp => self.selected.unwrap_or(0).saturating_sub(page),
                    Key::PageDown => {
                        (self.selected.unwrap_or(0) + page).min(self.rows.saturating_sub(1))
                    }
                    Key::Home => 0,
                    Key::End => self.rows.saturating_sub(1),
                    _ => return EventStatus::Ignored,
                };
                self.select(next, cx);
                self.scroll_to(next, content.height, cx);
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
        node.set_role(accesskit::Role::Grid);
        node.set_label(self.label.clone());
        node.add_action(accesskit::Action::Focus);
    }
}

into_views!(DataGrid);
