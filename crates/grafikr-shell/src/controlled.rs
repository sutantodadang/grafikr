use std::sync::Arc;

use grafikr_core::{Size, Theme, Ui};

use winit::event_loop::ActiveEventLoop;

pub type WakeHandle = Arc<dyn Fn() + Send + Sync>;

pub trait AppDriver: 'static {
    fn ready(&mut self, wake: WakeHandle);
    fn commands(&mut self, cx: &mut ShellCx<'_>);
    fn closed(&mut self, id: ShellWindowId);
    fn stopping(&mut self);

    fn is_animating(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShellWindowId(pub(crate) u64);

impl ShellWindowId {
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellError {
    WindowCreationFailed,
    UnknownWindow,
    InvalidSize,
}

pub struct ShellCx<'a> {
    pub(crate) inner: &'a mut crate::Shell,
    pub(crate) event_loop: &'a ActiveEventLoop,
}

impl ShellCx<'_> {
    pub fn create_window(
        &mut self,
        title: &str,
        size: Size,
        theme: Theme,
    ) -> Result<ShellWindowId, ShellError> {
        self.inner
            .create_controlled_window(self.event_loop, title, size, theme)
    }
    pub fn ui_mut(&mut self, id: ShellWindowId) -> Option<&mut Ui> {
        self.inner.controlled_ui_mut(id)
    }
    pub fn set_title(&mut self, id: ShellWindowId, title: &str) -> Result<(), ShellError> {
        self.inner.set_controlled_title(id, title)
    }
    pub fn set_size(&mut self, id: ShellWindowId, size: Size) -> Result<(), ShellError> {
        self.inner.set_controlled_size(id, size)
    }
    pub fn set_theme(&mut self, id: ShellWindowId, theme: Theme) -> Result<(), ShellError> {
        self.inner.set_controlled_theme(id, theme)
    }
    pub fn close_window(&mut self, id: ShellWindowId) -> Result<(), ShellError> {
        self.inner.close_controlled_window(id)
    }
    pub fn stop(&mut self) {
        self.inner.stop_controlled();
    }
}
