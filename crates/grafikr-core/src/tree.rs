//! Retained node tree backed by a generational arena and Taffy layout.

use std::cell::Cell;
use std::rc::Rc;

use taffy::prelude::{
    AlignItems, AvailableSpace, Dimension, Display, FlexDirection, GridTemplateComponent,
    JustifyContent, LengthPercentage, LengthPercentageAuto, MaxTrackSizingFunction,
    MinTrackSizingFunction, Rect as TaffyRect, Size as TaffySize, Style as TaffyStyle,
    TrackSizingFunction,
};
use taffy::tree::TaffyTree;

use crate::color::{Point, Rect, Size};
use crate::display::DisplayList;
use crate::event::{EventStatus, UiEvent};
use crate::runtime::DirtyFlags;
use crate::scroll::ScrollState;
use crate::style::{Align, CursorIcon, Direction, Interaction, Style, Theme};
use crate::text::TextEngine;
use crate::ui::{EventCx, MeasureCx, PaintCx};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId {
    index: u32,
    generation: u32,
}

impl NodeId {
    pub const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    pub const fn index(self) -> u32 {
        self.index
    }

    pub fn to_u64(self) -> u64 {
        ((self.generation as u64) << 32) | self.index as u64
    }

    pub fn from_u64(value: u64) -> Self {
        Self {
            index: value as u32,
            generation: (value >> 32) as u32,
        }
    }
}

pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub style: Style,
    pub interaction: Interaction,
    pub rect: Rect,
    pub dirty: DirtyFlags,
    pub widget: Option<Box<dyn Widget>>,
    pub scroll: Option<std::rc::Rc<ScrollState>>,
    /// Dynamic visibility driven by a signal. Hidden nodes are skipped by
    /// layout, paint, hit-testing, focus, and accessibility.
    pub visible: Option<Rc<Cell<bool>>>,
    pub applied_visible: bool,
    pub modal_scope: bool,
    taffy: taffy::NodeId,
}

impl Node {
    pub fn is_focusable(&self) -> bool {
        self.widget
            .as_ref()
            .is_some_and(|widget| widget.focusable())
            && !self.interaction.disabled
    }

    pub fn accepts_text_input(&self) -> bool {
        self.widget
            .as_ref()
            .is_some_and(|widget| widget.accepts_text_input())
    }

    pub fn cursor(&self) -> CursorIcon {
        self.widget
            .as_ref()
            .map(|widget| widget.cursor())
            .unwrap_or_default()
    }
}

/// Behavior of a retained leaf node.
///
/// Containers have no widget: Taffy lays their children out. Custom widgets
/// implement this trait directly; they never touch the renderer.
pub trait Widget {
    fn measure(&mut self, cx: &mut MeasureCx, available: Size) -> Size;
    fn paint(&mut self, cx: &mut PaintCx, rect: Rect);

    fn event(&mut self, _cx: &mut EventCx, _event: &UiEvent) -> EventStatus {
        EventStatus::Ignored
    }

    fn focusable(&self) -> bool {
        false
    }

    fn accepts_text_input(&self) -> bool {
        false
    }

    fn cursor(&self) -> CursorIcon {
        CursorIcon::Default
    }

    fn accessibility(&mut self, _node: &mut accesskit::Node) {}
}

struct Slot {
    generation: u32,
    node: Option<Node>,
}

pub struct Tree {
    slots: Vec<Slot>,
    free: Vec<u32>,
    taffy: TaffyTree<NodeId>,
    root: Option<NodeId>,
    /// Union of every node's pending flags.
    pub dirty: DirtyFlags,
    pub viewport: Size,
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            taffy: TaffyTree::new(),
            root: None,
            dirty: DirtyFlags::ALL,
            viewport: Size::ZERO,
        }
    }

    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    pub fn set_root(&mut self, root: NodeId) {
        self.root = Some(root);
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.slots
            .get(id.index as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.node.as_ref())
    }

    pub fn node(&self, id: NodeId) -> &Node {
        self.get(id).expect("valid NodeId")
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.slots[id.index as usize]
            .node
            .as_mut()
            .expect("valid NodeId")
    }

    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.slots
            .get_mut(id.index as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.node.as_mut())
    }

    pub(crate) fn create_node(&mut self, parent: Option<NodeId>, style: Style) -> NodeId {
        let (index, generation) = if let Some(index) = self.free.pop() {
            (index, self.slots[index as usize].generation)
        } else {
            (self.slots.len() as u32, 1)
        };
        let id = NodeId::new(index, generation);
        let taffy_style = to_taffy(&style, parent.is_none());
        let taffy_id = self
            .taffy
            .new_leaf_with_context(taffy_style, id)
            .expect("taffy node allocation");

        if index as usize == self.slots.len() {
            self.slots.push(Slot {
                generation,
                node: None,
            });
        }
        let node = Node {
            id,
            parent,
            children: Vec::new(),
            style,
            interaction: Interaction::default(),
            rect: Rect::ZERO,
            dirty: DirtyFlags::ALL,
            widget: None,
            scroll: None,
            visible: None,
            applied_visible: true,
            modal_scope: false,
            taffy: taffy_id,
        };
        self.slots[index as usize].node = Some(node);

        if let Some(parent) = parent {
            self.node_mut(parent).children.push(id);
        } else {
            self.root = Some(id);
        }
        self.dirty |= DirtyFlags::ALL;
        id
    }

    pub(crate) fn set_widget(&mut self, id: NodeId, widget: Box<dyn Widget>) {
        self.node_mut(id).widget = Some(widget);
    }

    pub fn set_scroll(&mut self, id: NodeId, state: std::rc::Rc<ScrollState>) {
        self.node_mut(id).scroll = Some(state);
    }

    pub fn set_visibility(&mut self, id: NodeId, visible: Rc<Cell<bool>>) {
        self.node_mut(id).visible = Some(visible);
    }

    pub fn set_modal_scope(&mut self, id: NodeId) {
        self.node_mut(id).modal_scope = true;
    }

    /// Make a child fill its parent (used by `Stack`).
    pub fn set_absolute_fill(&mut self, id: NodeId) {
        let mut style = self.node(id).style.clone();
        style.absolute = true;
        style.inset = crate::color::Insets::all(0.0);
        let taffy_id = self.node(id).taffy;
        let _ = self.taffy.set_style(taffy_id, to_taffy(&style, false));
        self.node_mut(id).style = style;
        self.mark_dirty(id, DirtyFlags::LAYOUT);
    }

    pub fn is_visible(&self, id: NodeId) -> bool {
        let mut current = Some(id);
        while let Some(id) = current {
            let Some(node) = self.get(id) else {
                return false;
            };
            if node.visible.as_ref().is_some_and(|visible| !visible.get()) {
                return false;
            }
            current = node.parent;
        }
        true
    }

    fn sync_visibility(&mut self) {
        let changes: Vec<(NodeId, bool)> = self
            .slots
            .iter()
            .filter_map(|slot| slot.node.as_ref())
            .filter_map(|node| {
                let visible = node.visible.as_ref()?.get();
                (visible != node.applied_visible).then_some((node.id, visible))
            })
            .collect();
        for (id, visible) in changes {
            let taffy_id = self.node(id).taffy;
            if let Ok(style) = self.taffy.style(taffy_id) {
                let mut style = style.clone();
                style.display = if visible {
                    Display::Flex
                } else {
                    Display::None
                };
                let _ = self.taffy.set_style(taffy_id, style);
            }
            self.node_mut(id).applied_visible = visible;
            self.dirty |= DirtyFlags::LAYOUT;
        }
    }

    /// Copy the child list into Taffy. Called once after mounting a container.
    pub(crate) fn sync_children(&mut self, parent: NodeId) {
        let taffy_parent = self.node(parent).taffy;
        let children: Vec<taffy::NodeId> = self
            .node(parent)
            .children
            .iter()
            .map(|child| self.node(*child).taffy)
            .collect();
        self.taffy
            .set_children(taffy_parent, &children)
            .expect("taffy set_children");
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.node(id).children
    }

    /// Number of live nodes in the arena.
    pub fn node_count(&self) -> usize {
        self.slots.iter().filter(|slot| slot.node.is_some()).count()
    }

    /// Allocated arena slots, including reusable free slots.
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    pub fn path_to(&self, id: NodeId) -> Vec<NodeId> {
        let mut path = Vec::new();
        let mut current = Some(id);
        while let Some(node) = current {
            path.push(node);
            current = self.get(node).and_then(|node| node.parent);
        }
        path.reverse();
        path
    }

    /// Mark a node dirty. Style/layout changes escalate to dependent phases.
    pub fn mark_dirty(&mut self, id: NodeId, flags: DirtyFlags) {
        let flags = flags.escalate();
        if let Some(node) = self.slots.get_mut(id.index as usize).and_then(|slot| {
            (slot.generation == id.generation)
                .then_some(slot.node.as_mut())
                .flatten()
        }) {
            node.dirty |= flags;
        }
        self.dirty |= flags;
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = DirtyFlags::NONE;
        for slot in &mut self.slots {
            if let Some(node) = &mut slot.node {
                node.dirty = DirtyFlags::NONE;
            }
        }
    }

    /// Remove a node and its subtree.
    pub fn remove(&mut self, id: NodeId) {
        let children = self.node(id).children.clone();
        for child in children {
            self.remove(child);
        }
        if let Some(parent) = self.node(id).parent {
            self.node_mut(parent).children.retain(|child| *child != id);
        }
        let taffy_id = self.node(id).taffy;
        let _ = self.taffy.remove(taffy_id);
        let slot = &mut self.slots[id.index as usize];
        slot.node = None;
        slot.generation = slot.generation.wrapping_add(1).max(1);
        self.free.push(id.index);
        self.dirty |= DirtyFlags::ALL;
    }

    /// Run Taffy for the whole tree. Taffy caches clean subtrees, so only
    /// dirty branches do work even though the call starts at the root.
    pub fn layout(&mut self, viewport: Size, text: &mut TextEngine, theme: &Theme, scale: f32) {
        let Some(root) = self.root else {
            return;
        };
        self.viewport = viewport;
        self.sync_visibility();

        let stale: Vec<taffy::NodeId> = self
            .slots
            .iter()
            .filter_map(|slot| slot.node.as_ref())
            .filter(|node| node.dirty.contains(DirtyFlags::LAYOUT))
            .map(|node| node.taffy)
            .collect();
        for taffy_id in stale {
            let _ = self.taffy.mark_dirty(taffy_id);
        }

        let Tree { taffy, slots, .. } = self;
        let root_taffy = slots[root.index as usize]
            .node
            .as_ref()
            .expect("root node")
            .taffy;

        let available = TaffySize {
            width: AvailableSpace::Definite(viewport.width),
            height: AvailableSpace::Definite(viewport.height),
        };
        taffy
            .compute_layout_with_measure(
                root_taffy,
                available,
                |input, _taffy_id, context, _style| {
                    let Some(node_id) = context.map(|id| *id) else {
                        return taffy::LayoutOutput::from_outer_size(TaffySize::ZERO);
                    };
                    let Some(node) = slots[node_id.index as usize].node.as_mut() else {
                        return taffy::LayoutOutput::from_outer_size(TaffySize::ZERO);
                    };
                    let Some(widget) = node.widget.as_mut() else {
                        return taffy::LayoutOutput::from_outer_size(TaffySize::ZERO);
                    };

                    let width = input
                        .known_dimensions
                        .width
                        .or_else(|| available_f32(input.available_space.width));
                    let height = input
                        .known_dimensions
                        .height
                        .or_else(|| available_f32(input.available_space.height));
                    let mut cx = MeasureCx { text, theme, scale };
                    let measured = widget.measure(
                        &mut cx,
                        Size::new(
                            width.unwrap_or(f32::INFINITY),
                            height.unwrap_or(f32::INFINITY),
                        ),
                    );
                    let size = TaffySize {
                        width: input.known_dimensions.width.unwrap_or(measured.width),
                        height: input.known_dimensions.height.unwrap_or(measured.height),
                    };
                    taffy::LayoutOutput::from_outer_size(size)
                },
            )
            .expect("taffy layout");

        place(slots, taffy, root, Point::ZERO, Point::ZERO);
        self.clear_dirty();
    }

    /// Deepest widget under `point`, honoring clip rectangles.
    pub fn hit_test(&self, point: Point) -> Option<NodeId> {
        let root = self.root?;
        let viewport = Rect::new(0.0, 0.0, self.viewport.width, self.viewport.height);
        self.hit_test_inner(root, point, viewport)
    }

    fn hit_test_inner(&self, id: NodeId, point: Point, clip: Rect) -> Option<NodeId> {
        let node = self.get(id)?;
        if !node.applied_visible {
            return None;
        }
        let transform = node.style.transform;
        let origin = Point::new(node.rect.x, node.rect.y);
        let own_rect = if transform.is_identity() {
            node.rect
        } else {
            transform.apply_rect(node.rect, origin)
        };
        let local = if transform.is_identity() {
            point
        } else {
            transform.inverse(point, origin)
        };
        let inside = own_rect.contains(point);
        if !inside && !node.style.overflow_hit {
            return None;
        }
        let clip = if node.style.clip {
            clip.intersect(own_rect)
        } else {
            clip
        };
        if !clip.contains(point) {
            return None;
        }
        for child in node
            .children
            .iter()
            .rev()
            .filter(|child| self.get(**child).is_some_and(|node| node.style.overlay))
        {
            if let Some(hit) = self.hit_test_inner(*child, local, clip) {
                return Some(hit);
            }
        }
        for child in node
            .children
            .iter()
            .rev()
            .filter(|child| self.get(**child).is_some_and(|node| !node.style.overlay))
        {
            if let Some(hit) = self.hit_test_inner(*child, local, clip) {
                return Some(hit);
            }
        }
        if inside {
            node.widget.as_ref().map(|_| id)
        } else {
            None
        }
    }

    /// Depth-first order used by Tab and accessibility.
    pub fn focus_order(&self) -> Vec<NodeId> {
        let mut order = Vec::new();
        if let Some(root) = self.root {
            self.collect_focusable(root, &mut order);
        }
        order
    }

    fn collect_focusable(&self, id: NodeId, order: &mut Vec<NodeId>) {
        let Some(node) = self.get(id) else {
            return;
        };
        if !node.applied_visible {
            return;
        }
        if node.is_focusable() {
            order.push(id);
        }
        for child in &node.children {
            self.collect_focusable(*child, order);
        }
    }

    pub fn focus_next(&self, current: Option<NodeId>, backwards: bool) -> Option<NodeId> {
        self.focus_next_in(None, current, backwards)
    }

    pub fn focus_order_within(&self, scope: NodeId) -> Vec<NodeId> {
        let mut order = Vec::new();
        self.collect_focusable(scope, &mut order);
        order
    }

    pub fn focus_next_in(
        &self,
        scope: Option<NodeId>,
        current: Option<NodeId>,
        backwards: bool,
    ) -> Option<NodeId> {
        let order = match scope {
            Some(scope) => self.focus_order_within(scope),
            None => self.focus_order(),
        };
        if order.is_empty() {
            return None;
        }
        match current.and_then(|current| order.iter().position(|id| *id == current)) {
            Some(index) if backwards => Some(order[(index + order.len() - 1) % order.len()]),
            Some(index) => Some(order[(index + 1) % order.len()]),
            None if backwards => order.last().copied(),
            None => order.first().copied(),
        }
    }

    /// Innermost visible modal focus scope, if any.
    pub fn top_visible_modal(&self) -> Option<NodeId> {
        let root = self.root?;
        let mut found = None;
        self.find_modal(root, &mut found);
        found
    }

    fn find_modal(&self, id: NodeId, found: &mut Option<NodeId>) {
        let Some(node) = self.get(id) else {
            return;
        };
        if !node.applied_visible {
            return;
        }
        if node.modal_scope {
            *found = Some(id);
        }
        for child in &node.children {
            self.find_modal(*child, found);
        }
    }

    pub fn is_within(&self, node: NodeId, ancestor: NodeId) -> bool {
        let mut current = Some(node);
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            current = self.get(id).and_then(|node| node.parent);
        }
        false
    }

    pub fn first_focusable_within(&self, scope: NodeId) -> Option<NodeId> {
        self.focus_order_within(scope).first().copied()
    }
}

fn available_f32(space: AvailableSpace) -> Option<f32> {
    match space {
        AvailableSpace::Definite(value) => Some(value),
        AvailableSpace::MinContent | AvailableSpace::MaxContent => None,
    }
}

fn place(slots: &mut [Slot], taffy: &TaffyTree<NodeId>, id: NodeId, origin: Point, extra: Point) {
    let Some(node) = slots[id.index as usize].node.as_mut() else {
        return;
    };
    let layout = *taffy.layout(node.taffy).expect("taffy layout result");
    let rect = Rect::new(
        origin.x + layout.location.x + extra.x,
        origin.y + layout.location.y + extra.y,
        layout.size.width,
        layout.size.height,
    );
    node.rect = rect;
    let scroll = node.scroll.clone();
    let children = node.children.clone();

    let child_extra = if let Some(scroll) = scroll {
        scroll.set_viewport(rect.height);
        if let Some(first) = children.first()
            && let Some(content) = slots[first.index as usize].node.as_ref()
            && let Ok(content_layout) = taffy.layout(content.taffy)
        {
            scroll.set_content(content_layout.size.height);
        }
        Point::new(0.0, -scroll.offset())
    } else {
        Point::ZERO
    };
    let child_origin = Point::new(rect.x, rect.y);
    for child in children {
        place(slots, taffy, child, child_origin, child_extra);
    }
}

fn to_taffy(style: &Style, is_root: bool) -> TaffyStyle {
    let padding = TaffyRect {
        top: LengthPercentage::length(style.padding.top),
        right: LengthPercentage::length(style.padding.right),
        bottom: LengthPercentage::length(style.padding.bottom),
        left: LengthPercentage::length(style.padding.left),
    };
    let direction = match style.direction {
        Direction::Row => FlexDirection::Row,
        Direction::Column => FlexDirection::Column,
    };
    let align = |align: Align| match align {
        Align::Start => JustifyContent::START,
        Align::Center => JustifyContent::CENTER,
        Align::End => JustifyContent::END,
        Align::Stretch => JustifyContent::STRETCH,
    };
    let align_items = match style.align_items {
        Align::Start => AlignItems::START,
        Align::Center => AlignItems::CENTER,
        Align::End => AlignItems::END,
        Align::Stretch => AlignItems::STRETCH,
    };

    TaffyStyle {
        display: if style.grid_columns.is_some() {
            Display::Grid
        } else {
            Display::Flex
        },
        flex_direction: direction,
        grid_template_columns: style
            .grid_columns
            .map(|columns| {
                (0..columns.max(1))
                    .map(|_| {
                        GridTemplateComponent::Single(TrackSizingFunction {
                            min: MinTrackSizingFunction::auto(),
                            max: MaxTrackSizingFunction::fr(1.0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        position: if style.absolute {
            taffy::Position::Absolute
        } else {
            taffy::Position::Relative
        },
        inset: TaffyRect {
            left: style
                .inset
                .left
                .map(LengthPercentageAuto::length)
                .unwrap_or(LengthPercentageAuto::auto()),
            right: style
                .inset
                .right
                .map(LengthPercentageAuto::length)
                .unwrap_or(LengthPercentageAuto::auto()),
            top: style
                .inset
                .top
                .map(LengthPercentageAuto::length)
                .unwrap_or(LengthPercentageAuto::auto()),
            bottom: style
                .inset
                .bottom
                .map(LengthPercentageAuto::length)
                .unwrap_or(LengthPercentageAuto::auto()),
        },
        size: if is_root {
            TaffySize {
                width: Dimension::percent(1.0),
                height: Dimension::percent(1.0),
            }
        } else {
            TaffySize {
                width: style
                    .width
                    .map(Dimension::length)
                    .unwrap_or(Dimension::auto()),
                height: style
                    .height
                    .map(Dimension::length)
                    .unwrap_or(Dimension::auto()),
            }
        },
        min_size: TaffySize {
            width: style
                .min_width
                .map(LengthPercentageAuto::length)
                .unwrap_or(LengthPercentageAuto::auto()),
            height: style
                .min_height
                .map(LengthPercentageAuto::length)
                .unwrap_or(LengthPercentageAuto::auto()),
        },
        padding,
        gap: TaffySize {
            width: LengthPercentage::length(style.gap),
            height: LengthPercentage::length(style.gap),
        },
        flex_grow: style.flex_grow,
        align_items,
        justify_content: align(style.justify_content),
        ..TaffyStyle::default()
    }
}

/// Paint traversal shared by the UI layer and tests.
pub(crate) fn paint_tree(
    tree: &mut Tree,
    list: &mut DisplayList,
    text: &mut TextEngine,
    theme: &Theme,
    scale: f32,
    runtime: &crate::runtime::Runtime,
    dt: f32,
) {
    let Some(root) = tree.root else {
        return;
    };
    let viewport = Rect::new(0.0, 0.0, tree.viewport.width, tree.viewport.height);
    paint_node(tree, list, text, theme, scale, runtime, dt, root, viewport);
}

#[allow(
    clippy::too_many_arguments,
    reason = "painting needs the tree, engine, theme, scale, runtime, clip, and node here"
)]
fn paint_node(
    tree: &mut Tree,
    list: &mut DisplayList,
    text: &mut TextEngine,
    theme: &Theme,
    scale: f32,
    runtime: &crate::runtime::Runtime,
    dt: f32,
    id: NodeId,
    clip: Rect,
) {
    let start = list.items.len();
    let (rect, state, children, overlays, transform) = {
        let Some(node) = tree.get(id) else {
            return;
        };
        if !node.applied_visible {
            return;
        }
        let transform = node.style.transform;
        let own_rect = if transform.is_identity() {
            node.rect
        } else {
            transform.apply_rect(node.rect, Point::new(node.rect.x, node.rect.y))
        };
        let clip = if node.style.clip {
            clip.intersect(own_rect)
        } else {
            clip
        };
        if clip.is_empty() {
            return;
        }
        if let Some(shadow) = node.style.shadow {
            list.push_shadow(
                node.rect,
                node.style.radius,
                shadow.blur,
                shadow.color,
                Some(clip),
            );
        }
        if let Some(background) = node.style.background {
            list.push_bordered_rect(
                node.rect,
                background,
                node.style.radius,
                node.style.border.map(|border| (border.width, border.color)),
                Some(clip),
            );
        } else if let Some(border) = node.style.border {
            list.push_bordered_rect(
                node.rect,
                crate::color::Color::TRANSPARENT,
                node.style.radius,
                Some((border.width, border.color)),
                Some(clip),
            );
        }
        let children = node.children.clone();
        let overlays: Vec<bool> = children
            .iter()
            .map(|child| tree.get(*child).is_some_and(|child| child.style.overlay))
            .collect();
        (node.rect, node.interaction, children, overlays, transform)
    };

    if let Some(widget) = tree.node_mut(id).widget.as_mut() {
        let mut cx = PaintCx {
            display: list,
            text,
            theme,
            scale,
            node: id,
            rect,
            state,
            clip: Some(clip),
            runtime,
            dt,
        };
        widget.paint(&mut cx, rect);
    }

    for (child, overlay) in children.iter().zip(overlays.iter()) {
        if !overlay {
            paint_node(tree, list, text, theme, scale, runtime, dt, *child, clip);
        }
    }
    for (child, overlay) in children.iter().zip(overlays.iter()) {
        if *overlay {
            paint_node(tree, list, text, theme, scale, runtime, dt, *child, clip);
        }
    }
    list.apply_transform(start, Point::new(rect.x, rect.y), transform);
}
