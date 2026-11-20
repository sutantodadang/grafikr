use std::collections::HashMap;
use std::time::Duration;

use grafikr_bridge::backend::{WindowBackend, WindowKey};
use grafikr_bridge::dispatch::Dispatcher;
use grafikr_bridge::protocol::{Outbound, decode};
use grafikr_bridge::session::Session;
use grafikr_core::{MountCx, MouseButton, NodeId, Point, Size, Style, Theme, Ui, UiEvent, View};
use grafikr_shell::ShellWindowId;
use serde_json::{Value, json};

struct Root;
impl View for Root {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        cx.add_node(Style::default())
    }
}

struct TestWindow {
    ui: Ui,
    root: NodeId,
}
struct MemoryBackend {
    next: u64,
    windows: HashMap<WindowKey, TestWindow>,
}
impl MemoryBackend {
    fn new() -> Self {
        Self {
            next: 1,
            windows: HashMap::new(),
        }
    }

    fn paint(&mut self, key: WindowKey) -> Vec<grafikr_core::DisplayItem> {
        let ui = self.ui_mut(key).unwrap();
        ui.prepare_frame();
        ui.display.items.clone()
    }
}
impl WindowBackend for MemoryBackend {
    fn create_window(
        &mut self,
        title: &str,
        size: Size,
        theme: Theme,
    ) -> Result<(WindowKey, NodeId), grafikr_bridge::protocol::BridgeError> {
        let mut ui = Ui::new(theme, 1.0, size, title);
        ui.mount(&Root);
        let root = ui.tree.root().unwrap();
        let key = WindowKey(ShellWindowId::from_raw(self.next));
        self.next += 1;
        self.windows.insert(key, TestWindow { ui, root });
        Ok((key, root))
    }
    fn close_window(
        &mut self,
        key: WindowKey,
    ) -> Result<(), grafikr_bridge::protocol::BridgeError> {
        self.windows
            .remove(&key)
            .map(|_| ())
            .ok_or_else(invalid_window)
    }
    fn set_title(
        &mut self,
        _key: WindowKey,
        _title: &str,
    ) -> Result<(), grafikr_bridge::protocol::BridgeError> {
        Ok(())
    }
    fn set_size(
        &mut self,
        _key: WindowKey,
        _size: Size,
    ) -> Result<(), grafikr_bridge::protocol::BridgeError> {
        Ok(())
    }
    fn set_theme(
        &mut self,
        key: WindowKey,
        theme: Theme,
    ) -> Result<(), grafikr_bridge::protocol::BridgeError> {
        self.windows
            .get_mut(&key)
            .map(|window| window.ui.theme = theme)
            .ok_or_else(invalid_window)
    }
    fn ui_mut(&mut self, key: WindowKey) -> Option<&mut Ui> {
        self.windows.get_mut(&key).map(|window| &mut window.ui)
    }
}
fn invalid_window() -> grafikr_bridge::protocol::BridgeError {
    grafikr_bridge::protocol::BridgeError::new("invalid_handle", "unknown test window")
}

#[test]
fn hello_reports_all_supported_features_and_protocol_resource_limits() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let hello = send(
        &mut dispatcher,
        &mut backend,
        "hello",
        "hello",
        json!({"protocol_major":1,"protocol_minor":0,"sdk_version":"test"}),
    );
    assert_eq!(
        hello["capabilities"],
        json!([
            "windows",
            "nodes",
            "properties",
            "state",
            "bindings",
            "events",
            "resources",
            "canvas",
            "collections",
            "tree_view",
            "transitions"
        ])
    );
    assert_eq!(
        hello["limits"],
        json!({
            "frame_bytes": 8 * 1024 * 1024,
            "json_depth": 64,
            "request_id_bytes": 64,
            "command_queue_messages": 1024,
            "command_queue_bytes": 16 * 1024 * 1024,
            "output_queue_messages": 1024,
            "output_queue_bytes": 16 * 1024 * 1024,
            "windows": 16,
            "nodes": 100_000,
            "states": 100_000,
            "bindings": 100_000,
            "subscriptions": 100_000,
            "image_bytes": 64 * 1024 * 1024,
            "image_session_bytes": 256 * 1024 * 1024,
            "image_chunk_bytes": 1024 * 1024,
            "canvas_commands": 10_000,
            "canvas_bytes": 8 * 1024 * 1024,
            "model_bytes": 128 * 1024 * 1024,
            "collection_rows_per_node": 2048,
            "collection_bytes_per_node": 8 * 1024 * 1024,
            "collection_bytes_session": 64 * 1024 * 1024,
            "collection_request_rows": 512,
            "tree_items": 50_000,
            "tree_depth": 64
        })
    );
}

fn send(
    dispatcher: &mut Dispatcher,
    backend: &mut MemoryBackend,
    id: &str,
    op: &str,
    args: Value,
) -> Value {
    let request =
        decode(&serde_json::to_vec(&json!({"id":id,"op":op,"args":args})).unwrap()).unwrap();
    match dispatcher.execute(request, backend) {
        Outbound::Response {
            ok: true,
            result: Some(value),
            ..
        } => value,
        Outbound::Response { error, .. } => panic!("operation {op} failed: {error:?}"),
        _ => panic!("expected response"),
    }
}

#[test]
fn dispatcher_mounts_updates_and_removes_real_retained_widgets() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    send(
        &mut dispatcher,
        &mut backend,
        "1",
        "hello",
        json!({"protocol_major":1,"protocol_minor":0,"sdk_version":"test"}),
    );
    let window = send(
        &mut dispatcher,
        &mut backend,
        "2",
        "window.create",
        json!({"title":"test","width":400,"height":300}),
    );
    let window_id = window["window"].as_str().unwrap();
    let root = window["root"].as_str().unwrap();
    let column = send(
        &mut dispatcher,
        &mut backend,
        "3",
        "node.create",
        json!({"window":window_id,"widget":"Column","properties":{"gap":4}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    let field = send(
        &mut dispatcher,
        &mut backend,
        "4",
        "node.create",
        json!({"window":window_id,"widget":"TextField","properties":{"text":"before","width":160}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "5",
        "node.attach",
        json!({"node":column,"parent":root}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "6",
        "node.attach",
        json!({"node":field,"parent":column}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "7",
        "property.update",
        json!({"node":field,"properties":{"text":"after"}}),
    );
    let key = WindowKey(ShellWindowId::from_raw(1));
    let root_id = backend.windows[&key].root;
    let ui = backend.ui_mut(key).unwrap();
    assert_eq!(ui.tree.node_count(), 4); // shell root, parking, column, and field
    let column_handle = grafikr_bridge::handles::Handle(column.parse().unwrap());
    assert_eq!(
        dispatcher.document.node(column_handle).unwrap().parent,
        Some(grafikr_bridge::handles::Handle(root.parse().unwrap()))
    );
    assert_eq!(ui.tree.children(root_id).len(), 2);
    send(
        &mut dispatcher,
        &mut backend,
        "8",
        "node.destroy",
        json!({"node":column}),
    );
    assert_eq!(backend.ui_mut(key).unwrap().tree.node_count(), 2);
    assert!(
        dispatcher
            .document
            .node(grafikr_bridge::handles::Handle(field.parse().unwrap()))
            .is_err()
    );
}

#[test]
fn invalid_property_patch_is_atomic_and_window_close_drops_owned_nodes() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let window = send(
        &mut dispatcher,
        &mut backend,
        "1",
        "window.create",
        json!({"title":"test","width":400,"height":300}),
    );
    let id = window["window"].as_str().unwrap();
    let root = window["root"].as_str().unwrap();
    let label = send(
        &mut dispatcher,
        &mut backend,
        "2",
        "node.create",
        json!({"window":id,"widget":"Text","properties":{"text":"stable"}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "3",
        "node.attach",
        json!({"node":label,"parent":root}),
    );
    let invalid = decode(&serde_json::to_vec(&json!({"id":"4","op":"property.update","args":{"node":label,"properties":{"text":"changed","unknown":true}}})).unwrap()).unwrap();
    let response = dispatcher.execute(invalid, &mut backend);
    assert!(matches!(response, Outbound::Response { ok: false, .. }));
    let invalid_value = decode(&serde_json::to_vec(&json!({"id":"4b","op":"property.update","args":{"node":label,"properties":{"size":0}}})).unwrap()).unwrap();
    let response = dispatcher.execute(invalid_value, &mut backend);
    assert!(matches!(response, Outbound::Response { ok: false, .. }));
    assert_eq!(
        dispatcher
            .document
            .node(grafikr_bridge::handles::Handle(label.parse().unwrap()))
            .unwrap()
            .properties["text"],
        json!("stable")
    );
    assert!(
        !dispatcher
            .document
            .node(grafikr_bridge::handles::Handle(label.parse().unwrap()))
            .unwrap()
            .properties
            .contains_key("size")
    );
    send(
        &mut dispatcher,
        &mut backend,
        "5",
        "window.close",
        json!({"window":id}),
    );
    assert!(backend.windows.is_empty());
    assert!(dispatcher.document.node_count() == 0);
}

#[test]
fn radio_controls_share_a_bound_state_group() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let state = send(
        &mut dispatcher,
        &mut backend,
        "1",
        "state.create",
        json!({"value":{"type":"i64","value":"1"}}),
    )["state"]
        .as_str()
        .unwrap()
        .to_owned();
    let window = send(
        &mut dispatcher,
        &mut backend,
        "2",
        "window.create",
        json!({"title":"radio group","width":400,"height":300}),
    );
    let column = send(
        &mut dispatcher,
        &mut backend,
        "3",
        "node.create",
        json!({"window":window["window"],"widget":"Column","properties":{}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    let first = send(
        &mut dispatcher,
        &mut backend,
        "4",
        "node.create",
        json!({"window":window["window"],"widget":"Radio","properties":{"group":state,"value":1,"label":"One"}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    let second = send(
        &mut dispatcher,
        &mut backend,
        "5",
        "node.create",
        json!({"window":window["window"],"widget":"Radio","properties":{"group":state,"value":2,"label":"Two"}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "6",
        "node.attach",
        json!({"node":column,"parent":window["root"]}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "7",
        "node.attach",
        json!({"node":first,"parent":column}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "8",
        "node.attach",
        json!({"node":second,"parent":column}),
    );

    let key = WindowKey(ShellWindowId::from_raw(1));
    let (second_rect, root) = {
        let ui = backend.ui_mut(key).unwrap();
        ui.prepare_frame();
        let root = ui.tree.root().unwrap();
        let column = ui
            .tree
            .children(root)
            .iter()
            .copied()
            .find(|candidate| ui.tree.children(*candidate).len() == 2)
            .unwrap();
        let second = ui.tree.children(column)[1];
        (ui.tree.node(second).rect, root)
    };
    backend
        .ui_mut(key)
        .unwrap()
        .handle_event(UiEvent::PointerDown {
            pos: Point::new(second_rect.x + 8.0, second_rect.y + 12.0),
            button: MouseButton::Left,
        });
    dispatcher.drain_intents(&mut backend);
    let state_handle = grafikr_bridge::handles::Handle(state.parse().unwrap());
    assert_eq!(
        dispatcher.states.get(state_handle).unwrap(),
        &grafikr_bridge::state::Scalar::I64(2)
    );

    let invalidation = decode(
        &serde_json::to_vec(&json!({"id":"9","op":"state.destroy","args":{"state":state}}))
            .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        dispatcher.execute(invalidation, &mut backend),
        Outbound::Response { ok: false, .. }
    ));
    send(
        &mut dispatcher,
        &mut backend,
        "10",
        "node.destroy",
        json!({"node":column}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "11",
        "state.destroy",
        json!({"state":state}),
    );
    let _ = root;
}

#[test]
fn transition_start_is_supported_for_finite_numeric_state() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let state = send(
        &mut dispatcher,
        &mut backend,
        "1",
        "state.create",
        json!({"value":{"type":"f64","value":0.0}}),
    )["state"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "2",
        "transition.start",
        json!({"state":state,"to":{"type":"f64","value":1.0},"duration_ms":100,"easing":"linear"}),
    );
    assert!(dispatcher.is_animating());
    dispatcher.tick_transitions(0.05, &mut backend);
    assert_eq!(
        send(
            &mut dispatcher,
            &mut backend,
            "3",
            "state.get",
            json!({"state":state})
        )["value"],
        0.5
    );
    dispatcher.tick_transitions(0.05, &mut backend);
    assert_eq!(
        send(
            &mut dispatcher,
            &mut backend,
            "4",
            "state.get",
            json!({"state":state})
        )["value"],
        1.0
    );
    assert!(!dispatcher.is_animating());
    send(
        &mut dispatcher,
        &mut backend,
        "5",
        "transition.start",
        json!({"state":state,"to":{"type":"f64","value":2.0},"duration_ms":0,"easing":"linear"}),
    );
    assert!(!dispatcher.is_animating());
    assert_eq!(
        send(
            &mut dispatcher,
            &mut backend,
            "6",
            "state.get",
            json!({"state":state})
        )["value"],
        2.0
    );
    send(
        &mut dispatcher,
        &mut backend,
        "7",
        "transition.start",
        json!({"state":state,"to":{"type":"f64","value":3.0},"duration_ms":100,"easing":"linear"}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "8",
        "state.set",
        json!({"state":state,"value":{"type":"f64","value":4.0}}),
    );
    assert!(!dispatcher.is_animating());
    assert_eq!(
        send(
            &mut dispatcher,
            &mut backend,
            "9",
            "state.get",
            json!({"state":state})
        )["value"],
        4.0
    );
    send(
        &mut dispatcher,
        &mut backend,
        "10",
        "transition.start",
        json!({"state":state,"to":{"type":"f64","value":5.0},"duration_ms":100,"easing":"linear"}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "11",
        "state.destroy",
        json!({"state":state}),
    );
    assert!(!dispatcher.is_animating());
}

#[test]
fn transition_requires_numeric_state_and_supported_easing() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let integer_state = send(
        &mut dispatcher,
        &mut backend,
        "1",
        "state.create",
        json!({"value":{"type":"i64","value":"0"}}),
    )["state"]
        .as_str()
        .unwrap()
        .to_owned();
    let numeric_state = send(
        &mut dispatcher,
        &mut backend,
        "2",
        "state.create",
        json!({"value":{"type":"f64","value":0.0}}),
    )["state"]
        .as_str()
        .unwrap()
        .to_owned();
    let request = decode(
        &serde_json::to_vec(&json!({
            "id":"3",
            "op":"transition.start",
            "args":{"state":integer_state,"to":{"type":"f64","value":1.0},"duration_ms":100,"easing":"linear"}
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        dispatcher.execute(request, &mut backend),
        Outbound::Response { ok: false, .. }
    ));
    assert_eq!(
        decode(
        &serde_json::to_vec(&json!({
            "id":"4",
            "op":"transition.start",
            "args":{"state":numeric_state,"to":{"type":"f64","value":1.0},"duration_ms":100,"easing":"spring"}
        }))
        .unwrap(),
        )
        .unwrap_err()
        .field
        .as_deref(),
        Some("easing")
    );
    let target_error = decode(
        &serde_json::to_vec(&json!({
            "id":"5",
            "op":"transition.start",
            "args":{"state":numeric_state,"to":{"type":"i64","value":"1"},"duration_ms":100,"easing":"linear"}
        }))
        .unwrap(),
    )
    .unwrap_err();
    assert_eq!(target_error.field.as_deref(), Some("to"));
    assert!(!dispatcher.is_animating());
}

#[test]
fn reduced_motion_completes_bound_transition_immediately() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let window = send(
        &mut dispatcher,
        &mut backend,
        "1",
        "window.create",
        json!({"title":"reduced motion","width":400,"height":300}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "2",
        "window.update",
        json!({"window":window["window"],"reduced_motion":true}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "3",
        "window.update",
        json!({"window":window["window"],"theme":"light"}),
    );
    let node = send(
        &mut dispatcher,
        &mut backend,
        "4",
        "node.create",
        json!({"window":window["window"],"widget":"Text","properties":{"text":"animated","size":14.0}}),
    );
    let state = send(
        &mut dispatcher,
        &mut backend,
        "5",
        "state.create",
        json!({"value":{"type":"f64","value":14.0}}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "6",
        "binding.bind",
        json!({"state":state["state"],"node":node["node"],"property":"size","direction":"one_way"}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "7",
        "transition.start",
        json!({"state":state["state"],"to":{"type":"f64","value":24.0},"duration_ms":500,"easing":"ease_out_cubic"}),
    );

    assert!(!dispatcher.is_animating());
    assert_eq!(
        send(
            &mut dispatcher,
            &mut backend,
            "8",
            "state.get",
            json!({"state":state["state"]})
        )["value"],
        24.0
    );
}

#[test]
fn state_binding_updates_only_its_node_and_unbind_releases_state() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let window = send(
        &mut dispatcher,
        &mut backend,
        "1",
        "window.create",
        json!({"title":"test","width":400,"height":300}),
    );
    let node = send(
        &mut dispatcher,
        &mut backend,
        "2",
        "node.create",
        json!({"window":window["window"],"widget":"Text","properties":{"text":"initial"}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    let state = send(
        &mut dispatcher,
        &mut backend,
        "3",
        "state.create",
        json!({"value":{"type":"string","value":"bound"}}),
    )["state"]
        .as_str()
        .unwrap()
        .to_owned();
    let binding = send(
        &mut dispatcher,
        &mut backend,
        "4",
        "binding.bind",
        json!({"state":state,"node":node,"property":"text","direction":"one_way"}),
    )["binding"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        dispatcher
            .document
            .node(grafikr_bridge::handles::Handle(node.parse().unwrap()))
            .unwrap()
            .properties["text"],
        json!("bound")
    );

    send(
        &mut dispatcher,
        &mut backend,
        "5",
        "state.set",
        json!({"state":state,"value":{"type":"string","value":"updated"}}),
    );
    assert_eq!(
        dispatcher
            .document
            .node(grafikr_bridge::handles::Handle(node.parse().unwrap()))
            .unwrap()
            .properties["text"],
        json!("updated")
    );
    send(
        &mut dispatcher,
        &mut backend,
        "6",
        "binding.unbind",
        json!({"binding":binding}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "7",
        "state.destroy",
        json!({"state":state}),
    );
}

#[test]
fn tabs_use_pane_slot_and_switch_child_visibility_without_remounting() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let window = send(
        &mut dispatcher,
        &mut backend,
        "1",
        "window.create",
        json!({"title":"test","width":400,"height":300}),
    );
    let tabs = send(&mut dispatcher, &mut backend, "2", "node.create", json!({"window":window["window"],"widget":"Tabs","properties":{"labels":["One","Two"],"active":0}}))["node"].as_str().unwrap().to_owned();
    let first = send(
        &mut dispatcher,
        &mut backend,
        "3",
        "node.create",
        json!({"window":window["window"],"widget":"Text","properties":{"text":"first"}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    let second = send(
        &mut dispatcher,
        &mut backend,
        "4",
        "node.create",
        json!({"window":window["window"],"widget":"Text","properties":{"text":"second"}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "5",
        "node.attach",
        json!({"node":tabs,"parent":window["root"]}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "6",
        "node.attach",
        json!({"node":first,"parent":tabs}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "7",
        "node.attach",
        json!({"node":second,"parent":tabs}),
    );
    let key = WindowKey(ShellWindowId::from_raw(1));
    let tab_handle = grafikr_bridge::handles::Handle(tabs.parse().unwrap());
    let shell_root = backend.windows[&key].root;
    let ui = backend.ui_mut(key).unwrap();
    let tabs_engine = ui.tree.children(shell_root)[1];
    let pane_parent = ui.tree.children(tabs_engine)[0];
    let pane_children = ui.tree.children(pane_parent).to_vec();
    let first_engine = pane_children[0];
    let second_engine = pane_children[1];
    assert!(ui.tree.node(first_engine).visible.as_ref().unwrap().get());
    assert!(!ui.tree.node(second_engine).visible.as_ref().unwrap().get());

    send(
        &mut dispatcher,
        &mut backend,
        "8",
        "property.update",
        json!({"node":tabs,"properties":{"active":1}}),
    );
    assert!(
        !backend
            .ui_mut(key)
            .unwrap()
            .tree
            .node(first_engine)
            .visible
            .as_ref()
            .unwrap()
            .get()
    );
    assert!(
        backend
            .ui_mut(key)
            .unwrap()
            .tree
            .node(second_engine)
            .visible
            .as_ref()
            .unwrap()
            .get()
    );
    assert!(dispatcher.document.node(tab_handle).is_ok());
}

#[test]
fn uploaded_image_can_be_mounted_and_keeps_pixels_after_owner_release() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let window = send(
        &mut dispatcher,
        &mut backend,
        "1",
        "window.create",
        json!({"title":"test","width":400,"height":300}),
    );
    let upload = send(
        &mut dispatcher,
        &mut backend,
        "2",
        "image.begin",
        json!({"width":1,"height":1,"byte_length":4}),
    )["upload"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "3",
        "image.chunk",
        json!({"upload":upload,"offset":0,"data":"AQIDBA=="}),
    );
    let image = send(
        &mut dispatcher,
        &mut backend,
        "4",
        "image.finalize",
        json!({"upload":upload}),
    )["image"]
        .as_str()
        .unwrap()
        .to_owned();
    let node = send(&mut dispatcher, &mut backend, "5", "node.create", json!({"window":window["window"],"widget":"Image","properties":{"image":image,"label":"one pixel"}}))["node"].as_str().unwrap().to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "6",
        "node.attach",
        json!({"node":node,"parent":window["root"]}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "7",
        "image.destroy",
        json!({"image":image}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "8",
        "property.update",
        json!({"node":node,"properties":{"label":"retained"}}),
    );
    let handle = grafikr_bridge::handles::Handle(node.parse().unwrap());
    assert_eq!(
        dispatcher.document.node(handle).unwrap().properties["label"],
        json!("retained")
    );
    send(
        &mut dispatcher,
        &mut backend,
        "9",
        "node.destroy",
        json!({"node":node}),
    );
}

#[test]
fn canvas_update_paints_local_commands_and_reuses_the_mounted_canvas() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let (control, seed) = Session::channel();
    drop(seed);
    dispatcher.set_publisher(control.clone());
    let window = send(
        &mut dispatcher,
        &mut backend,
        "1",
        "window.create",
        json!({"title":"canvas","width":400,"height":300}),
    );
    let node = send(
        &mut dispatcher,
        &mut backend,
        "2",
        "node.create",
        json!({"window":window["window"],"widget":"Canvas","properties":{"width":80,"height":60,"label":"chart","commands":[{"kind":"rect","rect":{"x":1,"y":1,"width":4,"height":4},"color":"#0000ff","radius":0}]}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "3",
        "node.attach",
        json!({"node":node,"parent":window["root"]}),
    );
    let key = WindowKey(ShellWindowId::from_raw(1));
    let initial = backend.paint(key);
    assert!(initial.iter().any(|item| matches!(item, grafikr_core::DisplayItem::Rect { color, .. } if *color == grafikr_core::Color::hex(0x0000ff))));
    let upload = send(
        &mut dispatcher,
        &mut backend,
        "image-begin",
        "image.begin",
        json!({"width":1,"height":1,"byte_length":4}),
    )["upload"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "image-chunk",
        "image.chunk",
        json!({"upload":upload,"offset":0,"data":"BQYHCA=="}),
    );
    let image = send(
        &mut dispatcher,
        &mut backend,
        "image-finalize",
        "image.finalize",
        json!({"upload":upload}),
    )["image"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "4",
        "canvas.update",
        json!({"node":node,"commands":[
            {"kind":"rect","rect":{"x":2,"y":3,"width":12,"height":9},"color":"#ff0000","radius":0},
            {"kind":"bordered_rect","rect":{"x":16,"y":3,"width":12,"height":9},"color":"#00ff00","radius":1,"border_width":1,"border_color":"#000000"},
            {"kind":"shadow","rect":{"x":30,"y":3,"width":12,"height":9},"color":"#000000","radius":1,"blur":4},
            {"kind":"text","x":2,"y":30,"text":"chart","size":12,"color":"#ffffff"},
            {"kind":"image","rect":{"x":44,"y":3,"width":1,"height":1},"image":image}
        ]}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "image-destroy",
        "image.destroy",
        json!({"image":image}),
    );

    let items = backend.paint(key);
    assert!(items.iter().any(|item| matches!(item, grafikr_core::DisplayItem::Rect { rect, color, .. } if *rect == grafikr_core::Rect::new(2.0, 3.0, 12.0, 9.0) && *color == grafikr_core::Color::hex(0xff0000))));
    assert!(items.iter().any(|item| matches!(item, grafikr_core::DisplayItem::Rect { rect, clip: Some(clip), .. } if rect.x == 2.0 && *clip == grafikr_core::Rect::new(0.0, 0.0, 80.0, 60.0))));
    assert!(items.iter().any(|item| matches!(
        item,
        grafikr_core::DisplayItem::Rect {
            border: Some(_),
            ..
        }
    )));
    assert!(
        items
            .iter()
            .any(|item| matches!(item, grafikr_core::DisplayItem::Shadow { .. }))
    );
    assert!(items.iter().any(|item| matches!(item, grafikr_core::DisplayItem::Image { data, .. } if data.pixels == [5,6,7,8])));
    assert!(
        items
            .iter()
            .any(|item| matches!(item, grafikr_core::DisplayItem::Text(_)))
    );

    send(
        &mut dispatcher,
        &mut backend,
        "7",
        "event.subscribe",
        json!({"node":node,"event":"resize"}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "8",
        "property.update",
        json!({"node":node,"properties":{"width":100}}),
    );
    backend.paint(key);
    let event: Value =
        serde_json::from_slice(&control.next(Duration::ZERO).unwrap().unwrap()).unwrap();
    assert_eq!(event["event"], "resize");
    assert_eq!(event["payload"]["width"], 100.0);
}

#[test]
fn combo_box_mounts_and_updates_its_selected_option() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let (control, seed) = Session::channel();
    drop(seed);
    dispatcher.set_publisher(control.clone());
    let window = send(
        &mut dispatcher,
        &mut backend,
        "1",
        "window.create",
        json!({"title":"combo","width":400,"height":300}),
    );
    let node = send(
        &mut dispatcher,
        &mut backend,
        "2",
        "node.create",
        json!({"window":window["window"],"widget":"ComboBox","properties":{"options":["one","two"],"selected":0}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "3",
        "node.attach",
        json!({"node":node,"parent":window["root"]}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "4",
        "property.update",
        json!({"node":node,"properties":{"selected":1}}),
    );
    assert_eq!(
        dispatcher
            .document
            .node(grafikr_bridge::handles::Handle(node.parse().unwrap()))
            .unwrap()
            .properties["selected"],
        json!(1)
    );
    send(
        &mut dispatcher,
        &mut backend,
        "5",
        "property.update",
        json!({"node":node,"properties":{"selected":0}}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "6",
        "event.subscribe",
        json!({"node":node,"event":"select"}),
    );
    let key = WindowKey(ShellWindowId::from_raw(1));
    let ui = backend.ui_mut(key).unwrap();
    ui.prepare_frame();
    let root = ui.tree.root().unwrap();
    let combo = ui
        .tree
        .children(root)
        .iter()
        .copied()
        .find(|id| ui.tree.node(*id).widget.is_some())
        .unwrap();
    let rect = ui.tree.node(combo).rect;
    let open_at = Point::new(rect.x + 10.0, rect.y + 10.0);
    ui.handle_event(UiEvent::PointerDown {
        pos: open_at,
        button: MouseButton::Left,
    });
    ui.handle_event(UiEvent::PointerUp {
        pos: open_at,
        button: MouseButton::Left,
    });
    ui.prepare_frame();
    let dropdown = ui.tree.children(combo)[1];
    let list = ui.tree.children(dropdown)[0];
    let list_rect = ui.tree.node(list).rect;
    let row = Point::new(list_rect.x + 10.0, list_rect.y + 42.0);
    ui.handle_event(UiEvent::PointerDown {
        pos: row,
        button: MouseButton::Left,
    });
    ui.handle_event(UiEvent::PointerUp {
        pos: row,
        button: MouseButton::Left,
    });
    let event: Value =
        serde_json::from_slice(&control.next(Duration::ZERO).unwrap().unwrap()).unwrap();
    assert_eq!(event["event"], "select");
    assert_eq!(event["payload"]["index"], "1");
}

#[test]
fn virtual_list_requests_visible_rows_and_paints_provider_responses() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let (control, seed) = Session::channel();
    drop(seed);
    dispatcher.set_publisher(control.clone());
    let window = send(
        &mut dispatcher,
        &mut backend,
        "1",
        "window.create",
        json!({"title":"virtual","width":400,"height":300}),
    );
    let node = send(
        &mut dispatcher,
        &mut backend,
        "2",
        "node.create",
        json!({"window":window["window"],"widget":"VirtualList","properties":{"row_count":50000,"row_height":24,"height":120}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "3",
        "node.attach",
        json!({"node":node,"parent":window["root"]}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "4",
        "event.subscribe",
        json!({"node":node,"event":"data.request"}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "5",
        "event.subscribe",
        json!({"node":node,"event":"data.request"}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "6",
        "event.subscribe",
        json!({"node":node,"event":"data.request"}),
    );
    let key = WindowKey(ShellWindowId::from_raw(1));
    backend.ui_mut(key).unwrap().runtime.clear_frame();
    send(
        &mut dispatcher,
        &mut backend,
        "7",
        "collection.create",
        json!({"node":node,"row_count":50000,"row_height":24,"columns":["label"]}),
    );
    assert!(backend.ui_mut(key).unwrap().runtime.needs_frame());

    backend.paint(key);
    assert_eq!(backend.ui_mut(key).unwrap().stats().text_runs, 0);
    let request: Value =
        serde_json::from_slice(&control.next(Duration::ZERO).unwrap().unwrap()).unwrap();
    assert_eq!(request["event"], "data.request");
    let count = request["payload"]["count"].as_u64().unwrap() as usize;
    let revision = request["payload"]["revision"].as_str().unwrap();
    let request_id = request["payload"]["request"].as_str().unwrap();
    let start = request["payload"]["start"].as_u64().unwrap();
    let rows: Vec<Vec<Value>> = (0..count)
        .map(|offset| vec![json!(format!("row-{}", start + offset as u64))])
        .collect();
    send(
        &mut dispatcher,
        &mut backend,
        "8",
        "data.provide",
        json!({"node":node,"revision":revision,"request":request_id,"start":start,"rows":rows}),
    );
    backend.paint(key);
    assert_eq!(
        dispatcher
            .collections
            .row_cloned(
                grafikr_bridge::handles::Handle(node.parse().unwrap()),
                start as u32
            )
            .unwrap(),
        vec![json!(format!("row-{start}"))]
    );
    {
        let ui = backend.ui_mut(key).unwrap();
        assert!(ui.stats().text_runs > 0);
    }
    send(
        &mut dispatcher,
        &mut backend,
        "7",
        "collection.update",
        json!({"node":node,"metadata":{"row_count":2,"row_height":30,"columns":["label"]}}),
    );
    assert_eq!(
        dispatcher
            .collections
            .cache_rows(grafikr_bridge::handles::Handle(node.parse().unwrap())),
        0
    );
    let ui = backend.ui_mut(key).unwrap();
    let root = ui.tree.root().unwrap();
    let list = ui
        .tree
        .children(root)
        .iter()
        .copied()
        .find(|id| ui.tree.node(*id).widget.is_some())
        .unwrap();
    assert_eq!(ui.tree.node(list).scroll.as_ref().unwrap().content(), 60.0);
}

#[test]
fn data_grid_requests_and_paints_visible_collection_rows() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let (control, seed) = Session::channel();
    drop(seed);
    dispatcher.set_publisher(control.clone());
    let window = send(
        &mut dispatcher,
        &mut backend,
        "1",
        "window.create",
        json!({"title":"grid","width":400,"height":300}),
    );
    let node = send(
        &mut dispatcher,
        &mut backend,
        "2",
        "node.create",
        json!({"window":window["window"],"widget":"DataGrid","properties":{"columns":[{"title":"Name","width":160}],"row_count":50000,"row_height":24,"height":140}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "3",
        "node.attach",
        json!({"node":node,"parent":window["root"]}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "4",
        "event.subscribe",
        json!({"node":node,"event":"data.request"}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "5",
        "collection.create",
        json!({"node":node,"row_count":50000,"row_height":24,"columns":["name"]}),
    );

    let key = WindowKey(ShellWindowId::from_raw(1));
    backend.paint(key);
    let before = backend.ui_mut(key).unwrap().stats().text_runs;
    let request: Value =
        serde_json::from_slice(&control.next(Duration::ZERO).unwrap().unwrap()).unwrap();
    assert_eq!(request["event"], "data.request");
    let count = request["payload"]["count"].as_u64().unwrap() as usize;
    let start = request["payload"]["start"].as_u64().unwrap();
    let rows: Vec<Vec<Value>> = (0..count)
        .map(|offset| vec![json!(format!("person-{}", start + offset as u64))])
        .collect();
    send(
        &mut dispatcher,
        &mut backend,
        "6",
        "data.provide",
        json!({"node":node,"revision":request["payload"]["revision"],"request":request["payload"]["request"],"start":start,"rows":rows}),
    );
    backend.paint(key);
    let after = backend.ui_mut(key).unwrap().stats().text_runs;
    assert!(before > 0, "the header is painted before row data arrives");
    assert!(after > before, "provider rows add visible cell text");
    send(
        &mut dispatcher,
        &mut backend,
        "7",
        "collection.update",
        json!({"node":node,"metadata":{"row_count":2,"row_height":30,"columns":["name"]}}),
    );
    assert_eq!(
        dispatcher
            .collections
            .cache_rows(grafikr_bridge::handles::Handle(node.parse().unwrap())),
        0
    );
    backend.paint(key);
    let request: Value =
        serde_json::from_slice(&control.next(Duration::ZERO).unwrap().unwrap()).unwrap();
    assert_eq!(request["payload"]["revision"], "2");
    assert_eq!(request["payload"]["count"], 2);
}

#[test]
fn tree_view_selects_and_updates_by_stable_string_key() {
    let mut dispatcher = Dispatcher::new();
    let mut backend = MemoryBackend::new();
    let (control, seed) = Session::channel();
    drop(seed);
    dispatcher.set_publisher(control.clone());
    let window = send(
        &mut dispatcher,
        &mut backend,
        "1",
        "window.create",
        json!({"title":"tree","width":400,"height":300}),
    );
    let node = send(
        &mut dispatcher,
        &mut backend,
        "2",
        "node.create",
        json!({"window":window["window"],"widget":"TreeView","properties":{"roots":[{"key":"root","label":"Root","children":[{"key":"child","label":"Child"},{"key":"other","label":"Other"}]}],"height":180}}),
    )["node"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut dispatcher,
        &mut backend,
        "3",
        "node.attach",
        json!({"node":node,"parent":window["root"]}),
    );
    send(
        &mut dispatcher,
        &mut backend,
        "4",
        "event.subscribe",
        json!({"node":node,"event":"select"}),
    );

    let key = WindowKey(ShellWindowId::from_raw(1));
    let ui = backend.ui_mut(key).unwrap();
    ui.prepare_frame();
    let root = ui.tree.root().unwrap();
    let tree = ui
        .tree
        .children(root)
        .iter()
        .copied()
        .find(|id| ui.tree.node(*id).widget.is_some())
        .unwrap();
    let rect = ui.tree.node(tree).rect;
    let root_label = Point::new(rect.x + 36.0, rect.y + 13.0);
    ui.handle_event(UiEvent::PointerDown {
        pos: root_label,
        button: MouseButton::Left,
    });
    ui.handle_event(UiEvent::PointerUp {
        pos: root_label,
        button: MouseButton::Left,
    });
    let root_event: Value =
        serde_json::from_slice(&control.next(Duration::ZERO).unwrap().unwrap()).unwrap();
    assert_eq!(root_event["event"], "select");
    assert_eq!(root_event["payload"]["key"], "root");

    let expander = Point::new(rect.x + 12.0, rect.y + 13.0);
    ui.handle_event(UiEvent::PointerDown {
        pos: expander,
        button: MouseButton::Left,
    });
    ui.handle_event(UiEvent::PointerUp {
        pos: expander,
        button: MouseButton::Left,
    });
    ui.prepare_frame();
    let child_label = Point::new(rect.x + 52.0, rect.y + 39.0);
    ui.handle_event(UiEvent::PointerDown {
        pos: child_label,
        button: MouseButton::Left,
    });
    ui.handle_event(UiEvent::PointerUp {
        pos: child_label,
        button: MouseButton::Left,
    });
    let child_event: Value =
        serde_json::from_slice(&control.next(Duration::ZERO).unwrap().unwrap()).unwrap();
    assert_eq!(child_event["payload"]["key"], "child");
    send(
        &mut dispatcher,
        &mut backend,
        "5",
        "property.update",
        json!({"node":node,"properties":{"roots":[{"key":"root","label":"Updated root","children":[{"key":"other","label":"Other"},{"key":"child","label":"Child"}]}]}}),
    );
    let ui = backend.ui_mut(key).unwrap();
    ui.prepare_frame();
    let rect = ui.tree.node(tree).rect;
    let first_child = Point::new(rect.x + 52.0, rect.y + 39.0);
    ui.handle_event(UiEvent::PointerDown {
        pos: first_child,
        button: MouseButton::Left,
    });
    ui.handle_event(UiEvent::PointerUp {
        pos: first_child,
        button: MouseButton::Left,
    });
    let reordered_event: Value =
        serde_json::from_slice(&control.next(Duration::ZERO).unwrap().unwrap()).unwrap();
    assert_eq!(reordered_event["payload"]["key"], "other");
}
