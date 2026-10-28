//! Vertical scroll state shared by scroll containers and virtual lists.

use std::cell::Cell;

#[derive(Default)]
pub struct ScrollState {
    offset: Cell<f32>,
    viewport: Cell<f32>,
    content: Cell<f32>,
}

impl ScrollState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn offset(&self) -> f32 {
        self.offset.get()
    }

    pub fn viewport(&self) -> f32 {
        self.viewport.get()
    }

    pub fn content(&self) -> f32 {
        self.content.get()
    }

    pub fn max_offset(&self) -> f32 {
        (self.content.get() - self.viewport.get()).max(0.0)
    }

    pub fn at_end(&self) -> bool {
        self.offset.get() >= self.max_offset() - 0.5
    }

    /// Clamp to `[0, max]` and return whether the value changed.
    pub fn set_offset(&self, offset: f32) -> bool {
        let clamped = offset.clamp(0.0, self.max_offset());
        let changed = (clamped - self.offset.get()).abs() > f32::EPSILON;
        self.offset.set(clamped);
        changed
    }

    /// Positive `delta` scrolls content up (toward the end).
    pub fn scroll_by(&self, delta: f32) -> bool {
        self.set_offset(self.offset.get() + delta)
    }

    pub fn set_viewport(&self, viewport: f32) -> bool {
        self.viewport.set(viewport.max(0.0));
        self.set_offset(self.offset.get())
    }

    pub fn set_content(&self, content: f32) -> bool {
        self.content.set(content.max(0.0));
        self.set_offset(self.offset.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_to_content() {
        let state = ScrollState::new();
        state.set_viewport(100.0);
        state.set_content(250.0);
        assert_eq!(state.max_offset(), 150.0);
        assert!(state.scroll_by(500.0));
        assert_eq!(state.offset(), 150.0);
        assert!(state.at_end());
        assert!(!state.scroll_by(10.0));
        assert!(state.scroll_by(-1000.0));
        assert_eq!(state.offset(), 0.0);
    }

    #[test]
    fn content_shrink_clamps_offset() {
        let state = ScrollState::new();
        state.set_viewport(100.0);
        state.set_content(500.0);
        state.set_offset(300.0);
        state.set_content(120.0);
        assert_eq!(state.offset(), 20.0);
    }
}
