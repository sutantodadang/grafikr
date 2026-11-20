use grafikr_core::{NodeId, Size, Theme, Ui};
use grafikr_shell::{ShellCx, ShellError, ShellWindowId};

use crate::protocol::BridgeError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowKey(pub ShellWindowId);

pub trait WindowBackend {
    fn create_window(
        &mut self,
        title: &str,
        size: Size,
        theme: Theme,
    ) -> Result<(WindowKey, NodeId), BridgeError>;
    fn close_window(&mut self, key: WindowKey) -> Result<(), BridgeError>;
    fn set_title(&mut self, key: WindowKey, title: &str) -> Result<(), BridgeError>;
    fn set_size(&mut self, key: WindowKey, size: Size) -> Result<(), BridgeError>;
    fn set_theme(&mut self, key: WindowKey, theme: Theme) -> Result<(), BridgeError>;
    fn ui_mut(&mut self, key: WindowKey) -> Option<&mut Ui>;
}

pub struct ShellBackend<'cx, 'event> {
    cx: &'cx mut ShellCx<'event>,
}

impl<'cx, 'event> ShellBackend<'cx, 'event> {
    pub fn new(cx: &'cx mut ShellCx<'event>) -> Self {
        Self { cx }
    }
}

impl WindowBackend for ShellBackend<'_, '_> {
    fn create_window(
        &mut self,
        title: &str,
        size: Size,
        theme: Theme,
    ) -> Result<(WindowKey, NodeId), BridgeError> {
        let id = self
            .cx
            .create_window(title, size, theme)
            .map_err(map_shell_error)?;
        let root = self
            .cx
            .ui_mut(id)
            .and_then(|ui| ui.tree.root())
            .ok_or_else(|| BridgeError::new("internal_failure", "created window has no UI root"))?;
        Ok((WindowKey(id), root))
    }
    fn close_window(&mut self, key: WindowKey) -> Result<(), BridgeError> {
        self.cx.close_window(key.0).map_err(map_shell_error)
    }
    fn set_title(&mut self, key: WindowKey, title: &str) -> Result<(), BridgeError> {
        self.cx.set_title(key.0, title).map_err(map_shell_error)
    }
    fn set_size(&mut self, key: WindowKey, size: Size) -> Result<(), BridgeError> {
        self.cx.set_size(key.0, size).map_err(map_shell_error)
    }
    fn set_theme(&mut self, key: WindowKey, theme: Theme) -> Result<(), BridgeError> {
        self.cx.set_theme(key.0, theme).map_err(map_shell_error)
    }
    fn ui_mut(&mut self, key: WindowKey) -> Option<&mut Ui> {
        self.cx.ui_mut(key.0)
    }
}

fn map_shell_error(error: ShellError) -> BridgeError {
    BridgeError::new(
        match error {
            ShellError::UnknownWindow => "invalid_handle",
            ShellError::InvalidSize => "invalid_value",
            ShellError::WindowCreationFailed => "window_creation_failed",
        },
        "controlled shell operation failed",
    )
}
