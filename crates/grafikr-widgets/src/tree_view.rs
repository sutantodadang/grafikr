//! Virtualized tree view with expand/collapse.

use std::cell::{Cell, RefCell};
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
    pub key: Option<String>,
    pub label: String,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            key: None,
            label: label.into(),
            children: Vec::new(),
        }
    }

    pub fn with_key(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: Some(key.into()),
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
    key: String,
    label: String,
    depth: usize,
    parent: Option<usize>,
    children: Vec<usize>,
}

type SelectFn = Rc<dyn Fn(usize)>;
type SelectKeyFn = Rc<dyn Fn(String)>;

pub struct TreeView {
    roots: Rc<RefCell<Vec<TreeNode>>>,
    revision: Rc<Cell<u64>>,
    height: f32,
    width: Option<f32>,
    on_select: Option<SelectFn>,
    on_select_key: Option<SelectKeyFn>,
    selected: Rc<Cell<Option<usize>>>,
    selected_key_override: Rc<RefCell<Option<Option<String>>>>,
}

#[derive(Clone)]
pub(crate) struct TreeViewController {
    roots: Rc<RefCell<Vec<TreeNode>>>,
    revision: Rc<Cell<u64>>,
    selected_key_override: Rc<RefCell<Option<Option<String>>>>,
}
impl TreeViewController {
    pub(crate) fn set_roots(&self, roots: Vec<TreeNode>) -> Result<usize, ()> {
        let count = validate_roots(&roots)?;
        *self.roots.borrow_mut() = roots;
        self.revision.set(self.revision.get().saturating_add(1));
        Ok(count)
    }
    pub(crate) fn contains_key(&self, key: &str) -> bool {
        self.roots
            .borrow()
            .iter()
            .any(|root| contains_key(root, key))
    }
    pub(crate) fn roots_contain_key(roots: &[TreeNode], key: &str) -> bool {
        roots.iter().any(|root| contains_key(root, key))
    }
    pub(crate) fn set_selected_key(&self, key: Option<String>) {
        *self.selected_key_override.borrow_mut() = Some(key);
    }
}

pub fn tree_view(roots: impl IntoIterator<Item = TreeNode>) -> TreeView {
    TreeView {
        roots: Rc::new(RefCell::new(roots.into_iter().collect())),
        revision: Rc::new(Cell::new(0)),
        height: 320.0,
        width: None,
        on_select: None,
        on_select_key: None,
        selected: Rc::new(Cell::new(None)),
        selected_key_override: Rc::new(RefCell::new(None)),
    }
}

impl TreeView {
    pub(crate) fn controller(&self) -> TreeViewController {
        TreeViewController {
            roots: self.roots.clone(),
            revision: self.revision.clone(),
            selected_key_override: self.selected_key_override.clone(),
        }
    }
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn selected(self, id: usize) -> Self {
        self.selected.set(Some(id));
        self
    }

    pub fn on_select(mut self, handler: impl Fn(usize) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    pub(crate) fn on_select_key(mut self, handler: impl Fn(String) + 'static) -> Self {
        self.on_select_key = Some(Rc::new(handler));
        self
    }
}

impl View for TreeView {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let (nodes, roots) = flatten(&self.roots.borrow());
        let expanded = std::collections::HashSet::new();
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
                selected: self.selected.clone(),
                on_select: self.on_select.clone(),
                on_select_key: self.on_select_key.clone(),
                roots_data: self.roots.clone(),
                revision: self.revision.clone(),
                applied_revision: self.revision.get(),
                selected_key_override: self.selected_key_override.clone(),
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
            key: node.key.clone().unwrap_or_else(|| id.to_string()),
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

fn validate_roots(roots: &[TreeNode]) -> Result<usize, ()> {
    fn walk(
        node: &TreeNode,
        depth: usize,
        count: &mut usize,
        keys: &mut std::collections::HashSet<String>,
    ) -> Result<(), ()> {
        if depth > 64 || *count >= 50_000 {
            return Err(());
        }
        let key = node.key.clone().unwrap_or_else(|| count.to_string());
        if !keys.insert(key) {
            return Err(());
        }
        *count += 1;
        for child in &node.children {
            walk(child, depth + 1, count, keys)?;
        }
        Ok(())
    }
    let mut count = 0;
    let mut keys = std::collections::HashSet::new();
    for root in roots {
        walk(root, 1, &mut count, &mut keys)?;
    }
    Ok(count)
}

fn contains_key(node: &TreeNode, key: &str) -> bool {
    node.key.as_deref() == Some(key) || node.children.iter().any(|child| contains_key(child, key))
}

fn visible_nodes(
    nodes: &[FlatNode],
    roots: &[usize],
    expanded: &std::collections::HashSet<String>,
) -> Vec<usize> {
    fn walk(
        id: usize,
        nodes: &[FlatNode],
        expanded: &std::collections::HashSet<String>,
        out: &mut Vec<usize>,
    ) {
        out.push(id);
        if expanded.contains(&nodes[id].key) {
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
    expanded: std::collections::HashSet<String>,
    visible: Vec<usize>,
    state: ScrollState,
    selected: Rc<Cell<Option<usize>>>,
    on_select: Option<SelectFn>,
    on_select_key: Option<SelectKeyFn>,
    roots_data: Rc<RefCell<Vec<TreeNode>>>,
    revision: Rc<Cell<u64>>,
    applied_revision: u64,
    selected_key_override: Rc<RefCell<Option<Option<String>>>>,
    caches: HashMap<usize, TextCache>,
    dragging: bool,
    width: f32,
}

impl TreeWidget {
    fn sync_roots(&mut self) {
        let revision = self.revision.get();
        let requested = self.selected_key_override.borrow_mut().take();
        if revision == self.applied_revision {
            if let Some(key) = requested {
                self.selected
                    .set(key.and_then(|key| self.nodes.iter().position(|node| node.key == key)));
            }
            return;
        }
        let previous_key = self
            .selected
            .get()
            .and_then(|index| self.nodes.get(index).map(|node| node.key.clone()));
        let tree = self.roots_data.borrow().clone();
        let (nodes, roots) = flatten(&tree);
        let keys: std::collections::HashSet<_> =
            nodes.iter().map(|node| node.key.clone()).collect();
        self.expanded.retain(|key| keys.contains(key));
        self.visible = visible_nodes(&nodes, &roots, &self.expanded);
        let selected_key = requested.unwrap_or(previous_key);
        self.selected
            .set(selected_key.and_then(|key| nodes.iter().position(|node| node.key == key)));
        self.state
            .set_content(self.visible.len() as f32 * ROW_HEIGHT);
        self.nodes = nodes;
        self.roots = roots;
        self.caches.clear();
        self.applied_revision = revision;
    }

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
        if !self.expanded.remove(&self.nodes[id].key) {
            self.expanded.insert(self.nodes[id].key.clone());
        }
        self.rebuild_visible(cx);
    }

    fn select(&mut self, id: usize, cx: &mut EventCx) {
        if self.selected.get() != Some(id) {
            self.selected.set(Some(id));
            if let Some(handler) = &self.on_select {
                handler(id);
            }
            if let Some(handler) = &self.on_select_key {
                handler(self.nodes[id].key.clone());
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
            .get()
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
        self.sync_roots();
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
            if self.selected.get() == Some(id) {
                cx.display
                    .push_rect(row_rect, cx.theme.accent.with_alpha(55), 0.0, cx.clip);
            }
            let x = rect.x + 8.0 + node.depth as f32 * INDENT;
            if !node.children.is_empty() {
                let marker = if self.expanded.contains(&node.key) {
                    "-"
                } else {
                    "+"
                };
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
        self.sync_roots();
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
                        if let Some(id) = self.selected.get() {
                            if !self.nodes[id].children.is_empty()
                                && !self.expanded.contains(&self.nodes[id].key)
                            {
                                self.toggle(id, cx);
                            } else if let Some(child) = self.nodes[id].children.first() {
                                self.select(*child, cx);
                            }
                        }
                    }
                    Key::Left => {
                        if let Some(id) = self.selected.get() {
                            if self.expanded.contains(&self.nodes[id].key) {
                                self.toggle(id, cx);
                            } else if let Some(parent) = self.nodes[id].parent {
                                self.select(parent, cx);
                            }
                        }
                    }
                    Key::Enter | Key::Space => {
                        if let Some(id) = self.selected.get() {
                            self.toggle(id, cx);
                        }
                    }
                    _ => return EventStatus::Ignored,
                }
                if let Some(id) = self.selected.get()
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
