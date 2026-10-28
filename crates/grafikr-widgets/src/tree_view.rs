//! Virtualized tree view with expand/collapse.

use std::collections::HashMap;
use std::rc::Rc;

use grafikr_core::{
    EventCx, EventStatus, Key, MeasureCx, MountCx, NodeId, PaintCx, Point, Rect, ScrollState, Size,
    Style, TextCache, UiEvent, View, Widget,
};

use crate::into_views;
use crate::scroll::{hit_scrollbar, paint_scrollbar, scrollbar_offset};

const ROW_HEIGHT: f32 = 26.0;
const INDENT: f32 = 16.0;

#[derive(Clone)]
pub struct TreeNode {
    pub label: String,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            children: Vec::new(),
        }
    }

    pub fn child(mut self, child: TreeNode) -> Self {
        self.children.push(child);
        self
    }
}

#[derive(Clone)]
struct FlatNode {
    label: String,
    depth: usize,
    parent: Option<usize>,
    children: Vec<usize>,
}

type SelectFn = Rc<dyn Fn(usize)>;

pub struct TreeView {
    roots: Vec<TreeNode>,
    height: f32,
    width: Option<f32>,
    on_select: Option<SelectFn>,
    selected: Option<usize>,
}

pub fn tree_view(roots: impl IntoIterator<Item = TreeNode>) -> TreeView {
    TreeView {
        roots: roots.into_iter().collect(),
        height: 320.0,
        width: None,
        on_select: None,
        selected: None,
    }
}

impl TreeView {
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn selected(mut self, id: usize) -> Self {
        self.selected = Some(id);
        self
    }

    pub fn on_select(mut self, handler: impl Fn(usize) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl View for TreeView {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let (nodes, roots) = flatten(&self.roots);
        let expanded = vec![false; nodes.len()];
        let visible = visible_nodes(&nodes, &roots, &expanded);
        let state = ScrollState::new();
        state.set_content(visible.len() as f32 * ROW_HEIGHT);
        let node = cx.add_node(Style {
            width: self.width,
            height: Some(self.height),
            clip: true,
            ..Style::default()
        });
        cx.set_widget(
            node,
            TreeWidget {
                nodes,
                roots,
                expanded,
                visible,
                state,
                selected: self.selected,
                on_select: self.on_select.clone(),
                caches: HashMap::new(),
                dragging: false,
                width: self.width.unwrap_or(280.0),
            },
        );
        node
    }
}

fn flatten(roots: &[TreeNode]) -> (Vec<FlatNode>, Vec<usize>) {
    fn walk(
        node: &TreeNode,
        depth: usize,
        parent: Option<usize>,
        out: &mut Vec<FlatNode>,
    ) -> usize {
        let id = out.len();
        out.push(FlatNode {
            label: node.label.clone(),
            depth,
            parent,
            children: Vec::new(),
        });
        for child in &node.children {
            let child_id = walk(child, depth + 1, Some(id), out);
            out[id].children.push(child_id);
        }
        id
    }
    let mut nodes = Vec::new();
    let roots = roots
        .iter()
        .map(|root| walk(root, 0, None, &mut nodes))
        .collect();
    (nodes, roots)
}

fn visible_nodes(nodes: &[FlatNode], roots: &[usize], expanded: &[bool]) -> Vec<usize> {
    fn walk(id: usize, nodes: &[FlatNode], expanded: &[bool], out: &mut Vec<usize>) {
        out.push(id);
        if expanded[id] {
            for child in &nodes[id].children {
                walk(*child, nodes, expanded, out);
            }
        }
    }
    let mut out = Vec::new();
    for root in roots {
        walk(*root, nodes, expanded, &mut out);
    }
    out
}

struct TreeWidget {
    nodes: Vec<FlatNode>,
    roots: Vec<usize>,
    expanded: Vec<bool>,
    visible: Vec<usize>,
    state: ScrollState,
    selected: Option<usize>,
    on_select: Option<SelectFn>,
    caches: HashMap<usize, TextCache>,
    dragging: bool,
    width: f32,
}

impl TreeWidget {
    fn rebuild_visible(&mut self, cx: &mut EventCx) {
        self.visible = visible_nodes(&self.nodes, &self.roots, &self.expanded);
        self.state
            .set_content(self.visible.len() as f32 * ROW_HEIGHT);
        cx.request_paint();
    }

    fn toggle(&mut self, id: usize, cx: &mut EventCx) {
        if self.nodes[id].children.is_empty() {
            return;
        }
        self.expanded[id] = !self.expanded[id];
        self.rebuild_visible(cx);
    }

    fn select(&mut self, id: usize, cx: &mut EventCx) {
        if self.selected != Some(id) {
            self.selected = Some(id);
            if let Some(handler) = &self.on_select {
                handler(id);
            }
            cx.request_paint();
        }
    }

    fn row_at(&self, rect: Rect, pos: Point) -> Option<usize> {
        if pos.y < rect.y || pos.y >= rect.bottom() {
            return None;
        }
        let row = ((pos.y - rect.y + self.state.offset()) / ROW_HEIGHT).floor() as usize;
        (row < self.visible.len()).then_some(row)
    }

    fn scroll_to(&mut self, row: usize, viewport: f32, cx: &mut EventCx) {
        let top = row as f32 * ROW_HEIGHT;
        let bottom = top + ROW_HEIGHT;
        let offset = self.state.offset();
        if top < offset {
            self.state.set_offset(top);
            cx.request_paint();
        } else if bottom > offset + viewport {
            self.state.set_offset(bottom - viewport);
            cx.request_paint();
        }
    }

    fn visible_row(&self) -> usize {
        self.selected
            .and_then(|id| self.visible.iter().position(|row| *row == id))
            .unwrap_or(0)
    }

    fn move_selection(&mut self, row: usize, cx: &mut EventCx) {
        if self.visible.is_empty() {
            return;
        }
        let row = row.min(self.visible.len() - 1);
        let id = self.visible[row];
        self.select(id, cx);
    }
}

impl Widget for TreeWidget {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::new(self.width, 320.0)
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        self.state.set_viewport(rect.height);
        self.state
            .set_content(self.visible.len() as f32 * ROW_HEIGHT);
        let offset = self.state.offset();
        let first = (offset / ROW_HEIGHT).floor() as usize;
        let count = (rect.height / ROW_HEIGHT).ceil() as usize + 1;
        let end = (first + count).min(self.visible.len());

        for row in first..end {
            let id = self.visible[row];
            let node = &self.nodes[id];
            let y = rect.y + row as f32 * ROW_HEIGHT - offset;
            let row_rect = Rect::new(rect.x, y, rect.width, ROW_HEIGHT);
            if self.selected == Some(id) {
                cx.display
                    .push_rect(row_rect, cx.theme.accent.with_alpha(55), 0.0, cx.clip);
            }
            let x = rect.x + 8.0 + node.depth as f32 * INDENT;
            if !node.children.is_empty() {
                let marker = if self.expanded[id] { "-" } else { "+" };
                let shape = self.caches.entry(id).or_default().get(
                    cx.text,
                    marker,
                    cx.theme.body_size,
                    cx.theme.muted,
                    None,
                );
                cx.display.push_layout(
                    &shape.layout,
                    Point::new(x, y + (ROW_HEIGHT - shape.height) * 0.5),
                    cx.clip,
                );
            }
            let shape = self.caches.entry(id).or_default().get(
                cx.text,
                &node.label,
                cx.theme.body_size,
                cx.theme.foreground,
                Some((rect.right() - x - 28.0).max(1.0)),
            );
            cx.display.push_layout(
                &shape.layout,
                Point::new(x + INDENT, y + (ROW_HEIGHT - shape.height) * 0.5),
                cx.clip,
            );
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
                    let id = self.visible[row];
                    let node = &self.nodes[id];
                    let marker_x = cx.rect.x + 8.0 + node.depth as f32 * INDENT;
                    if !node.children.is_empty()
                        && pos.x >= marker_x - 4.0
                        && pos.x <= marker_x + INDENT
                    {
                        self.toggle(id, cx);
                    } else {
                        self.select(id, cx);
                    }
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
                let row = self.visible_row();
                match key {
                    Key::Up => self.move_selection(row.saturating_sub(1), cx),
                    Key::Down => {
                        self.move_selection((row + 1).min(self.visible.len().saturating_sub(1)), cx)
                    }
                    Key::Home => self.move_selection(0, cx),
                    Key::End => self.move_selection(self.visible.len().saturating_sub(1), cx),
                    Key::Right => {
                        if let Some(id) = self.selected {
                            if !self.nodes[id].children.is_empty() && !self.expanded[id] {
                                self.toggle(id, cx);
                            } else if let Some(child) = self.nodes[id].children.first() {
                                self.select(*child, cx);
                            }
                        }
                    }
                    Key::Left => {
                        if let Some(id) = self.selected {
                            if self.expanded[id] {
                                self.toggle(id, cx);
                            } else if let Some(parent) = self.nodes[id].parent {
                                self.select(parent, cx);
                            }
                        }
                    }
                    Key::Enter | Key::Space => {
                        if let Some(id) = self.selected {
                            self.toggle(id, cx);
                        }
                    }
                    _ => return EventStatus::Ignored,
                }
                if let Some(id) = self.selected
                    && let Some(row) = self.visible.iter().position(|row| *row == id)
                {
                    self.scroll_to(row, cx.rect.height, cx);
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
        node.set_role(accesskit::Role::Tree);
        node.set_label("Tree");
    }
}

into_views!(TreeView);
