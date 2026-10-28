//! Dirty tracking and frame requests.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::tree::NodeId;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirtyFlags(u8);

impl DirtyFlags {
    pub const NONE: Self = Self(0);
    pub const STYLE: Self = Self(1 << 0);
    pub const LAYOUT: Self = Self(1 << 1);
    pub const PAINT: Self = Self(1 << 2);
    pub const ACCESSIBILITY: Self = Self(1 << 3);
    pub const ALL: Self = Self(0b1111);

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Style changes require a fresh layout, layout changes require a repaint.
    pub fn escalate(self) -> Self {
        let mut flags = self;
        if flags.contains(Self::STYLE) {
            flags.insert(Self::LAYOUT);
        }
        if flags.contains(Self::LAYOUT) {
            flags.insert(Self::PAINT);
            flags.insert(Self::ACCESSIBILITY);
        }
        flags
    }
}

impl std::ops::BitOr for DirtyFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for DirtyFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.insert(rhs);
    }
}

/// Shared invalidation channel between widgets, signals, and the shell.
#[derive(Clone)]
pub struct Runtime {
    dirty: Rc<RefCell<Vec<(NodeId, DirtyFlags)>>>,
    frame_requested: Rc<Cell<bool>>,
    animating: Rc<Cell<bool>>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            dirty: Rc::new(RefCell::new(Vec::new())),
            frame_requested: Rc::new(Cell::new(false)),
            animating: Rc::new(Cell::new(false)),
        }
    }

    pub fn invalidate(&self, node: NodeId, flags: DirtyFlags) {
        self.dirty.borrow_mut().push((node, flags));
        self.frame_requested.set(true);
    }

    pub fn take_dirty(&self) -> Vec<(NodeId, DirtyFlags)> {
        std::mem::take(&mut *self.dirty.borrow_mut())
    }

    pub fn needs_frame(&self) -> bool {
        self.frame_requested.get()
    }

    pub fn request_frame(&self) {
        self.frame_requested.set(true);
    }

    pub fn clear_frame(&self) {
        self.frame_requested.set(false);
    }

    /// Ask for another frame even if nothing is dirty (spinner, transition).
    pub fn request_animation(&self) {
        self.animating.set(true);
        self.frame_requested.set(true);
    }

    pub fn is_animating(&self) -> bool {
        self.animating.get()
    }

    pub fn take_animation(&self) -> bool {
        self.animating.replace(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_escalate() {
        let flags = DirtyFlags::STYLE.escalate();
        assert!(flags.contains(DirtyFlags::LAYOUT));
        assert!(flags.contains(DirtyFlags::PAINT));
        assert!(flags.contains(DirtyFlags::ACCESSIBILITY));

        let paint = DirtyFlags::PAINT.escalate();
        assert!(!paint.contains(DirtyFlags::LAYOUT));
    }

    #[test]
    fn dirty_queue_is_drained() {
        let runtime = Runtime::new();
        let node = NodeId::new(1, 1);
        runtime.invalidate(node, DirtyFlags::PAINT);
        assert!(runtime.needs_frame());
        assert_eq!(runtime.take_dirty().len(), 1);
        assert!(runtime.take_dirty().is_empty());
        runtime.clear_frame();
        assert!(!runtime.needs_frame());
    }
}
