//! The UI runtime: mounting, input dispatch, layout, paint, and accessibility.

use accesskit::Action;
use std::time::Instant;

use crate::a11y;
use crate::color::{Point, Rect, Size};
use crate::display::DisplayList;
use crate::event::{Clipboard, EventStatus, Key, NoopClipboard, UiEvent};
use crate::runtime::{DirtyFlags, Runtime};
use crate::style::{CursorIcon, Interaction, Theme};
use crate::text::TextEngine;
use crate::tree::{NodeId, Tree, paint_tree};
use crate::view::{MountCx, View};

pub struct MeasureCx<'a> {
    pub text: &'a mut TextEngine,
    pub theme: &'a Theme,
    pub scale: f32,
}

pub struct PaintCx<'a> {
    pub display: &'a mut DisplayList,
    pub text: &'a mut TextEngine,
    pub theme: &'a Theme,
    pub scale: f32,
    pub node: NodeId,
    pub rect: Rect,
    pub state: Interaction,
    pub clip: Option<Rect>,
    pub runtime: &'a Runtime,
    /// Seconds since the previous painted frame, for animations.
    pub dt: f32,
}

impl PaintCx<'_> {
    pub fn push_rect(&mut self, rect: Rect, color: crate::color::Color, radius: f32) {
        self.display.push_rect(rect, color, radius, self.clip);
    }

    pub fn push_bordered_rect(
        &mut self,
        rect: Rect,
        color: crate::color::Color,
        radius: f32,
        border: Option<(f32, crate::color::Color)>,
    ) {
        self.display
            .push_bordered_rect(rect, color, radius, border, self.clip);
    }
}

pub struct EventCx<'a> {
    pub node: NodeId,
    pub rect: Rect,
    pub state: Interaction,
    pub theme: &'a Theme,
    pub scale: f32,
    pub runtime: &'a Runtime,
    pub text: &'a mut TextEngine,
    pub clipboard: &'a mut dyn Clipboard,
}

impl EventCx<'_> {
    pub fn request_paint(&self) {
        self.runtime.invalidate(self.node, DirtyFlags::PAINT);
    }

    pub fn request_layout(&self) {
        self.runtime.invalidate(self.node, DirtyFlags::LAYOUT);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Outcome {
    pub repaint: bool,
    pub cursor: CursorIcon,
    pub ime_allowed: bool,
}

/// Cheap CPU-side memory/navigation counters for diagnostics and benchmarks.
#[derive(Clone, Copy, Debug, Default)]
pub struct UiStats {
    pub nodes: usize,
    pub slots: usize,
    pub display_items: usize,
    pub text_runs: usize,
}

pub struct Ui {
    pub tree: Tree,
    pub runtime: Runtime,
    pub text: TextEngine,
    pub theme: Theme,
    pub display: DisplayList,
    pub focus: Option<NodeId>,
    pub hovered: Option<NodeId>,
    pub capture: Option<NodeId>,
    pub pressed: Option<NodeId>,
    pub pointer: Point,
    pub clipboard: Box<dyn Clipboard>,
    pub ime_allowed: bool,
    pub a11y_dirty: bool,
    pub window_title: String,
    modal_return_focus: Option<NodeId>,
    scale: f32,
    last_frame: Option<Instant>,
}

impl Ui {
    pub fn new(theme: Theme, scale: f32, viewport: Size, title: impl Into<String>) -> Self {
        let mut tree = Tree::new();
        tree.viewport = viewport;
        Self {
            tree,
            runtime: Runtime::new(),
            text: TextEngine::new(),
            theme,
            display: DisplayList::new(),
            focus: None,
            hovered: None,
            capture: None,
            pressed: None,
            pointer: Point::ZERO,
            clipboard: Box::new(NoopClipboard),
            ime_allowed: false,
            a11y_dirty: true,
            window_title: title.into(),
            modal_return_focus: None,
            scale,
            last_frame: None,
        }
    }

    pub fn mount(&mut self, view: &dyn View) {
        if let Some(root) = self.tree.root() {
            self.tree.remove(root);
        }
        self.focus = None;
        self.hovered = None;
        self.capture = None;
        self.pressed = None;
        self.modal_return_focus = None;
        self.display.clear();
        let runtime = self.runtime.clone();
        let parent = None;
        let mut cx = MountCx {
            tree: &mut self.tree,
            text: &mut self.text,
            theme: &self.theme,
            runtime: &runtime,
            parent,
        };
        let root = view.mount(&mut cx);
        self.tree.set_root(root);
        self.tree.dirty = DirtyFlags::ALL;
        self.runtime.request_frame();
    }

    pub fn set_scale(&mut self, scale: f32) {
        if (self.scale - scale).abs() > f32::EPSILON {
            self.scale = scale;
            if let Some(root) = self.tree.root() {
                self.tree.mark_dirty(root, DirtyFlags::ALL);
            }
        }
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn stats(&self) -> UiStats {
        UiStats {
            nodes: self.tree.node_count(),
            slots: self.tree.slot_count(),
            display_items: self.display.items.len(),
            text_runs: self
                .display
                .items
                .iter()
                .filter(|item| matches!(item, crate::DisplayItem::Text(_)))
                .count(),
        }
    }

    pub fn set_viewport(&mut self, viewport: Size) {
        if self.tree.viewport != viewport {
            self.tree.viewport = viewport;
            if let Some(root) = self.tree.root() {
                self.tree.mark_dirty(root, DirtyFlags::LAYOUT);
            }
        }
    }

    /// Apply queued invalidation, then run layout and paint only if needed.
    pub fn prepare_frame(&mut self) {
        self.sync_from_runtime();
        let dt = self
            .last_frame
            .map(|last| last.elapsed().as_secs_f32().min(0.1))
            .unwrap_or(0.0);
        self.last_frame = Some(Instant::now());
        let animating = self.runtime.take_animation();
        let laid_out = self.tree.dirty.contains(DirtyFlags::LAYOUT);
        if laid_out {
            let viewport = self.tree.viewport;
            let Ui {
                tree,
                text,
                theme,
                scale,
                ..
            } = self;
            tree.layout(viewport, text, theme, *scale);
            self.sync_modal_focus();
            self.a11y_dirty = true;
        }
        if laid_out
            || animating
            || self.tree.dirty.contains(DirtyFlags::PAINT)
            || self.display.is_empty()
        {
            let Ui {
                tree,
                display,
                text,
                theme,
                scale,
                runtime,
                ..
            } = self;
            display.clear();
            paint_tree(tree, display, text, theme, *scale, runtime, dt);
        }
        self.tree.clear_dirty();
        self.runtime.clear_frame();
    }

    pub fn handle_event(&mut self, event: UiEvent) -> Outcome {
        match event {
            UiEvent::PointerMoved { pos } => {
                self.pointer = pos;
                let hit = self.update_hover(pos);
                let target = self.capture.or(hit);
                if let Some(target) = target {
                    self.dispatch_bubble(target, &UiEvent::PointerMoved { pos });
                }
            }
            UiEvent::PointerDown { pos, button } => {
                self.pointer = pos;
                let hit = self.update_hover(pos);
                if let Some(hit) = hit {
                    self.capture = Some(hit);
                    self.pressed = Some(hit);
                    if self.tree.node(hit).is_focusable() {
                        self.set_focus(Some(hit));
                    }
                    if let Some(node) = self.tree.get_mut(hit) {
                        node.interaction.pressed = true;
                        self.tree.mark_dirty(hit, DirtyFlags::PAINT);
                    }
                    self.dispatch_bubble(hit, &UiEvent::PointerDown { pos, button });
                } else {
                    self.set_focus(None);
                }
            }
            UiEvent::PointerUp { pos, button } => {
                self.pointer = pos;
                let hit = self.update_hover(pos);
                let target = self.capture.or(hit);
                if let Some(target) = target {
                    if let Some(node) = self.tree.get_mut(target) {
                        node.interaction.pressed = false;
                        self.tree.mark_dirty(target, DirtyFlags::PAINT);
                    }
                    if let Some(pressed) = self.pressed.take()
                        && pressed != target
                        && let Some(node) = self.tree.get_mut(pressed)
                    {
                        node.interaction.pressed = false;
                        self.tree.mark_dirty(pressed, DirtyFlags::PAINT);
                    }
                    self.dispatch_bubble(target, &UiEvent::PointerUp { pos, button });
                }
                self.capture = None;
            }
            UiEvent::PointerLeft => {
                if let Some(hovered) = self.hovered.take() {
                    if let Some(node) = self.tree.get_mut(hovered) {
                        node.interaction.hovered = false;
                        self.tree.mark_dirty(hovered, DirtyFlags::PAINT);
                    }
                    self.dispatch_leave(hovered);
                }
            }
            UiEvent::KeyDown {
                key,
                modifiers,
                repeat,
            } => {
                let focus = self.visible_focus();
                if key == Key::Tab {
                    let scope = self.tree.top_visible_modal();
                    let next = self.tree.focus_next_in(scope, focus, modifiers.shift);
                    self.set_focus(next);
                } else if let Some(focus) = focus {
                    let status = self.dispatch_bubble(
                        focus,
                        &UiEvent::KeyDown {
                            key,
                            modifiers,
                            repeat,
                        },
                    );
                    if !status.is_handled() && key == Key::Escape {
                        self.set_focus(None);
                    }
                }
            }
            UiEvent::KeyUp { key, modifiers } => {
                if let Some(focus) = self.visible_focus() {
                    self.dispatch_bubble(focus, &UiEvent::KeyUp { key, modifiers });
                }
            }
            UiEvent::Text(text) => {
                if let Some(focus) = self.visible_focus() {
                    self.dispatch_bubble(focus, &UiEvent::Text(text));
                }
            }
            UiEvent::ImePreedit { text, cursor } => {
                if let Some(focus) = self.visible_focus() {
                    self.dispatch_bubble(focus, &UiEvent::ImePreedit { text, cursor });
                }
            }
            UiEvent::ImeCommit(text) => {
                if let Some(focus) = self.visible_focus() {
                    self.dispatch_bubble(focus, &UiEvent::ImeCommit(text));
                }
            }
            UiEvent::FocusLost => {
                if let Some(focus) = self.focus {
                    self.dispatch_bubble(focus, &UiEvent::FocusLost);
                }
                self.pressed = None;
                self.capture = None;
                if let Some(hovered) = self.hovered.take() {
                    if let Some(node) = self.tree.get_mut(hovered) {
                        node.interaction.hovered = false;
                        self.tree.mark_dirty(hovered, DirtyFlags::PAINT);
                    }
                    self.dispatch_leave(hovered);
                }
            }
            UiEvent::Wheel { delta } => {
                let target = self
                    .capture
                    .or(self.hovered)
                    .or_else(|| self.tree.hit_test(self.pointer))
                    .or_else(|| self.tree.root());
                if let Some(target) = target {
                    self.dispatch_bubble(target, &UiEvent::Wheel { delta });
                }
            }
            UiEvent::AccessibilityClick => {
                if let Some(focus) = self.focus {
                    self.dispatch_bubble(focus, &UiEvent::AccessibilityClick);
                }
            }
            UiEvent::SetValue(_) => {}
        }

        self.sync_from_runtime();
        self.refresh_ime_allowed();
        let repaint = !self.tree.dirty.is_empty() || self.runtime.needs_frame();
        Outcome {
            repaint,
            cursor: self.cursor_icon(),
            ime_allowed: self.ime_allowed,
        }
    }

    pub fn handle_a11y_action(&mut self, request: &accesskit::ActionRequest) -> bool {
        let node = NodeId::from_u64(request.target_node.0);
        if self.tree.get(node).is_none() {
            return false;
        }
        match &request.action {
            Action::Click => {
                if let Some(node) = self.tree.get(node).filter(|node| node.is_focusable()) {
                    let node = node.id;
                    self.dispatch_bubble(node, &UiEvent::AccessibilityClick);
                }
            }
            Action::Focus => self.set_focus(Some(node)),
            Action::SetValue => {
                let Some(accesskit::ActionData::Value(value)) = &request.data else {
                    return false;
                };
                let event = UiEvent::SetValue(value.to_string());
                self.dispatch_bubble(node, &event);
            }
            _ => return false,
        }
        self.sync_from_runtime();
        self.refresh_ime_allowed();
        true
    }

    pub fn a11y_update(&mut self) -> accesskit::TreeUpdate {
        a11y::tree_update(&mut self.tree, self.focus, &self.window_title)
    }

    pub fn cursor_icon(&self) -> CursorIcon {
        self.capture
            .or(self.hovered)
            .and_then(|node| self.tree.get(node))
            .map(|node| node.cursor())
            .unwrap_or_default()
    }

    pub fn ime_cursor_rect(&self) -> Option<Rect> {
        self.focus
            .and_then(|focus| self.tree.get(focus))
            .map(|node| node.rect)
    }

    /// Current shell-facing state without dispatching an event.
    pub fn current_outcome(&self) -> Outcome {
        Outcome {
            repaint: false,
            cursor: self.cursor_icon(),
            ime_allowed: self.ime_allowed,
        }
    }

    fn update_hover(&mut self, pos: Point) -> Option<NodeId> {
        let hit = self.tree.hit_test(pos);
        if self.hovered != hit {
            if let Some(old) = self.hovered {
                self.dispatch_leave(old);
                if let Some(node) = self.tree.get_mut(old) {
                    node.interaction.hovered = false;
                    self.tree.mark_dirty(old, DirtyFlags::PAINT);
                }
            }
            if let Some(new) = hit
                && let Some(node) = self.tree.get_mut(new)
            {
                node.interaction.hovered = true;
                self.tree.mark_dirty(new, DirtyFlags::PAINT);
            }
            self.hovered = hit;
        }
        hit
    }

    /// Pointer-left is delivered to every ancestor, even if a child handles it.
    fn dispatch_leave(&mut self, node: NodeId) {
        let path = self.tree.path_to(node);
        for node in path.into_iter().rev() {
            let _ = self.dispatch_to(node, &UiEvent::PointerLeft);
        }
    }

    fn sync_from_runtime(&mut self) {
        for (node, flags) in self.runtime.take_dirty() {
            self.tree.mark_dirty(node, flags);
        }
    }

    /// Reflect the focused node into interaction state and IME policy.
    fn refresh_ime_allowed(&mut self) {
        let ime_allowed = self
            .focus
            .and_then(|focus| self.tree.get(focus))
            .is_some_and(|node| node.accepts_text_input());
        if ime_allowed != self.ime_allowed {
            self.ime_allowed = ime_allowed;
            self.a11y_dirty = true;
        }
    }

    /// Drop focus if the node was hidden by a signal.
    fn visible_focus(&mut self) -> Option<NodeId> {
        let focus = self.focus.filter(|id| self.tree.is_visible(*id));
        if focus != self.focus {
            self.set_focus(focus);
        }
        focus
    }

    /// Move focus into a modal scope, and restore it when the modal closes.
    fn sync_modal_focus(&mut self) {
        if let Some(modal) = self.tree.top_visible_modal() {
            let inside = self
                .focus
                .is_some_and(|focus| self.tree.is_within(focus, modal));
            if !inside {
                if self.modal_return_focus.is_none() {
                    self.modal_return_focus = self.focus;
                }
                let target = self.tree.first_focusable_within(modal).or(Some(modal));
                self.set_focus(target);
            }
        } else if let Some(previous) = self.modal_return_focus.take() {
            let target = self.tree.is_visible(previous).then_some(previous);
            self.set_focus(target);
        }
    }

    fn set_focus(&mut self, focus: Option<NodeId>) {
        if self.focus == focus {
            return;
        }
        if let Some(old) = self.focus {
            self.dispatch_bubble(old, &UiEvent::FocusLost);
            if let Some(node) = self.tree.get_mut(old) {
                node.interaction.focused = false;
                self.tree.mark_dirty(old, DirtyFlags::PAINT);
            }
        }
        self.focus = focus;
        if let Some(focus) = focus
            && let Some(node) = self.tree.get_mut(focus)
        {
            node.interaction.focused = true;
            self.tree
                .mark_dirty(focus, DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY);
        }
        self.a11y_dirty = true;
        self.refresh_ime_allowed();
    }

    fn dispatch_bubble(&mut self, target: NodeId, event: &UiEvent) -> EventStatus {
        let path = self.tree.path_to(target);
        for node in path.into_iter().rev() {
            if self.dispatch_to(node, event).is_handled() {
                return EventStatus::Handled;
            }
        }
        EventStatus::Ignored
    }

    fn dispatch_to(&mut self, node: NodeId, event: &UiEvent) -> EventStatus {
        let Ui {
            tree,
            runtime,
            text,
            theme,
            clipboard,
            scale,
            ..
        } = self;
        let Some(target) = tree.get(node) else {
            return EventStatus::Ignored;
        };
        let rect = target.rect;
        let state = target.interaction;
        let Some(widget) = tree.node_mut(node).widget.as_mut() else {
            return EventStatus::Ignored;
        };
        let mut cx = EventCx {
            node,
            rect,
            state,
            theme,
            scale: *scale,
            runtime,
            text,
            clipboard: clipboard.as_mut(),
        };
        widget.event(&mut cx, event)
    }
}
