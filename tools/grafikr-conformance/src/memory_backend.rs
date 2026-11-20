use std::collections::HashMap;

use grafikr_bridge::backend::{WindowBackend, WindowKey};
use grafikr_bridge::protocol::BridgeError;
use grafikr_core::{MountCx, NodeId, Size, Style, Theme, Ui, View};
use grafikr_shell::ShellWindowId;

struct Root;

impl View for Root {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        cx.add_node(Style::default())
    }
}

pub struct MemoryBackend {
    next: u64,
    windows: HashMap<WindowKey, Ui>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self {
            next: 1,
            windows: HashMap::new(),
        }
    }

    pub fn paint(&mut self, key: WindowKey) -> Result<Vec<grafikr_core::DisplayItem>, BridgeError> {
        let ui = self.ui_mut(key).ok_or_else(unknown_window)?;
        ui.prepare_frame();
        Ok(ui.display.items.clone())
    }

    pub fn window_keys(&self) -> impl Iterator<Item = WindowKey> + '_ {
        self.windows.keys().copied()
    }
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowBackend for MemoryBackend {
    fn create_window(
        &mut self,
        title: &str,
        size: Size,
        theme: Theme,
    ) -> Result<(WindowKey, NodeId), BridgeError> {
        let mut ui = Ui::new(theme, 1.0, size, title);
        ui.mount(&Root);
        let root = ui
            .tree
            .root()
            .ok_or_else(|| BridgeError::new("internal_failure", "memory window has no root"))?;
        let key = WindowKey(ShellWindowId::from_raw(self.next));
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| BridgeError::new("quota_exceeded", "memory window IDs exhausted"))?;
        self.windows.insert(key, ui);
        Ok((key, root))
    }

    fn close_window(&mut self, key: WindowKey) -> Result<(), BridgeError> {
        self.windows
            .remove(&key)
            .map(|_| ())
            .ok_or_else(unknown_window)
    }

    fn set_title(&mut self, key: WindowKey, _title: &str) -> Result<(), BridgeError> {
        self.windows
            .get(&key)
            .map(|_| ())
            .ok_or_else(unknown_window)
    }

    fn set_size(&mut self, key: WindowKey, _size: Size) -> Result<(), BridgeError> {
        self.windows
            .get(&key)
            .map(|_| ())
            .ok_or_else(unknown_window)
    }

    fn set_theme(&mut self, key: WindowKey, theme: Theme) -> Result<(), BridgeError> {
        self.windows
            .get_mut(&key)
            .map(|ui| ui.theme = theme)
            .ok_or_else(unknown_window)
    }

    fn ui_mut(&mut self, key: WindowKey) -> Option<&mut Ui> {
        self.windows.get_mut(&key)
    }
}

fn unknown_window() -> BridgeError {
    BridgeError::new("invalid_handle", "unknown memory window")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use grafikr_bridge::dispatch::Dispatcher;
    use grafikr_bridge::protocol::{Operation, Outbound, Request};
    use grafikr_bridge::session::Session;
    use grafikr_core::{DisplayItem, MouseButton, Point, UiEvent};
    use serde_json::{Value, json};

    use super::*;

    fn execute(
        dispatcher: &mut Dispatcher,
        backend: &mut MemoryBackend,
        id: &str,
        op: Operation,
        args: Value,
    ) -> Value {
        match dispatcher.execute(
            Request {
                id: id.to_owned(),
                op,
                args,
            },
            backend,
        ) {
            Outbound::Response {
                ok: true,
                result: Some(result),
                ..
            } => result,
            response => panic!("request {id} failed: {response:?}"),
        }
    }

    fn text_glyph_count(items: &[DisplayItem]) -> usize {
        items
            .iter()
            .map(|item| match item {
                DisplayItem::Text(run) => run.glyphs.len(),
                _ => 0,
            })
            .sum()
    }

    #[test]
    fn state_updates_the_cpu_tree_and_ui_click_publishes_subscribed_event() {
        let (control, seed) = Session::channel();
        drop(seed);
        let mut dispatcher = Dispatcher::new();
        dispatcher.set_publisher(control.clone());
        let mut backend = MemoryBackend::new();
        let window = execute(
            &mut dispatcher,
            &mut backend,
            "1",
            Operation::WindowCreate,
            json!({"title":"parity","width":320,"height":200}),
        );
        let window_id = window["window"].as_str().unwrap();
        let root = window["root"].as_str().unwrap();
        let text = execute(
            &mut dispatcher,
            &mut backend,
            "2",
            Operation::NodeCreate,
            json!({"window":window_id,"widget":"Text","properties":{"text":"Before"}}),
        )["node"]
            .as_str()
            .unwrap()
            .to_owned();
        let button = execute(
            &mut dispatcher,
            &mut backend,
            "3",
            Operation::NodeCreate,
            json!({"window":window_id,"widget":"Button","properties":{"label":"Click"}}),
        )["node"]
            .as_str()
            .unwrap()
            .to_owned();
        execute(
            &mut dispatcher,
            &mut backend,
            "4",
            Operation::NodeAttach,
            json!({"node":text,"parent":root}),
        );
        execute(
            &mut dispatcher,
            &mut backend,
            "5",
            Operation::NodeAttach,
            json!({"node":button,"parent":root}),
        );
        let state = execute(
            &mut dispatcher,
            &mut backend,
            "6",
            Operation::StateCreate,
            json!({"value":{"type":"string","value":"Before"}}),
        )["state"]
            .as_str()
            .unwrap()
            .to_owned();
        execute(
            &mut dispatcher,
            &mut backend,
            "7",
            Operation::Bind,
            json!({"state":state,"node":text,"property":"text","direction":"one_way"}),
        );
        execute(
            &mut dispatcher,
            &mut backend,
            "8",
            Operation::Subscribe,
            json!({"node":button,"event":"click"}),
        );

        let window_key = backend.window_keys().next().unwrap();
        let initial = backend.paint(window_key).unwrap();
        let initial_glyphs = text_glyph_count(&initial);
        let updated = execute(
            &mut dispatcher,
            &mut backend,
            "9",
            Operation::StateSet,
            json!({"state":state,"value":{"type":"string","value":"Updated tree content"}}),
        );
        assert_eq!(updated["changed"], true);
        let repainted = backend.paint(window_key).unwrap();
        assert_ne!(text_glyph_count(&repainted), initial_glyphs);

        let ui = backend.ui_mut(window_key).unwrap();
        let root_id = ui.tree.root().unwrap();
        let button_id = *ui.tree.children(root_id).last().unwrap();
        let rect = ui.tree.node(button_id).rect;
        let pos = Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        ui.handle_event(UiEvent::PointerMoved { pos });
        ui.handle_event(UiEvent::PointerDown {
            pos,
            button: MouseButton::Left,
        });
        ui.handle_event(UiEvent::PointerUp {
            pos,
            button: MouseButton::Left,
        });
        let event: Value =
            serde_json::from_slice(&control.next(Duration::ZERO).unwrap().unwrap()).unwrap();
        assert_eq!(event["event"], "click");
        assert_eq!(event["source"], button);
    }
}
