use base64::Engine;
use grafikr_core::{
    Color, Easing, EdgeInsets, ImageData, NodeId, Rect, Size, Style, Theme, Transition,
};
use grafikr_widgets::{
    ButtonVariant, DynamicEvent, DynamicValue, GridColumn, LayoutKind, MountedWidget, WidgetPatch,
    WidgetSpec, mount,
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::backend::{WindowBackend, WindowKey};
use crate::canvas::{CanvasCommand, CanvasModel, MAX_CANVAS_BYTES, MAX_CANVAS_COMMANDS};
use crate::collections::{
    Collections, MAX_CACHE_BYTES_PER_NODE, MAX_CACHE_BYTES_PER_SESSION, MAX_CACHE_ROWS_PER_NODE,
    MAX_REQUEST_ROWS, MAX_TREE_DEPTH, MAX_TREE_ITEMS, TreeItem, TreeSnapshot,
};
use crate::document::{Document, MAX_CANONICAL_BYTES, MAX_NODES, MAX_WINDOWS};
use crate::handles::{Handle, MAX_OBJECTS_PER_KIND, Object, ObjectKind, Registry};
use crate::protocol::{
    BridgeError, MAX_FRAME_BYTES, MAX_JSON_DEPTH, MAX_REQUEST_ID_BYTES, Operation, Outbound,
    Request, WidgetKind, WireError, validate_patch,
};
use crate::queue::{MAX_QUEUE_BYTES, MAX_QUEUE_MESSAGES};
use crate::resources::{
    MAX_IMAGE_BYTES, MAX_IMAGE_BYTES_PER_SESSION, MAX_IMAGE_CHUNK_BYTES, Resources,
};
use crate::session::SessionControl;
use crate::state::{Scalar, StateStore};

const RADIO_GROUP_BINDING: &str = "__grafikr_radio_group";
const CAPABILITIES: &[&str] = &[
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
    "transitions",
];

pub struct Dispatcher {
    pub document: Document,
    pub states: StateStore,
    pub resources: Resources,
    pub collections: Collections,
    canvases: HashMap<Handle, CanvasModel>,
    windows: HashMap<Handle, ManagedWindow>,
    mounted: HashMap<Handle, MountedNode>,
    publisher: Option<SessionControl>,
    sequence: Arc<AtomicU64>,
    handles: Registry,
    subscriptions: Rc<RefCell<HashMap<Handle, (Handle, String)>>>,
    bindings: Rc<RefCell<HashMap<Handle, BindingRecord>>>,
    binding_intents: Rc<RefCell<Vec<(Handle, DynamicValue)>>>,
    transitions: HashMap<Handle, Transition>,
    stopped: bool,
    protocol_minor: u16,
}

#[derive(Clone)]
struct BindingRecord {
    state: Handle,
    node: Handle,
    property: String,
    two_way: bool,
}

struct ManagedWindow {
    key: WindowKey,
    root_handle: Handle,
    root: NodeId,
    parking: NodeId,
}
struct MountedNode {
    widget: MountedWidget,
}

impl Default for Dispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Dispatcher {
    pub fn new() -> Self {
        Self {
            document: Document::new(),
            states: StateStore::new(),
            resources: Resources::new(),
            collections: Collections::new(),
            windows: HashMap::new(),
            mounted: HashMap::new(),
            publisher: None,
            sequence: Arc::new(AtomicU64::new(1)),
            handles: Registry::new(),
            subscriptions: Rc::new(RefCell::new(HashMap::new())),
            bindings: Rc::new(RefCell::new(HashMap::new())),
            binding_intents: Rc::new(RefCell::new(Vec::new())),
            transitions: HashMap::new(),
            canvases: HashMap::new(),
            stopped: false,
            protocol_minor: 0,
        }
    }
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }
    pub fn is_animating(&self) -> bool {
        !self.transitions.is_empty()
    }
    pub fn tick_transitions(&mut self, dt: f32, backend: &mut dyn WindowBackend) {
        let mut finished = Vec::new();
        let mut updates = Vec::new();
        for (state, transition) in &self.transitions {
            if !transition.tick(dt) {
                finished.push(*state);
            }
            updates.push((*state, f64::from(transition.value())));
        }
        for state in finished {
            self.transitions.remove(&state);
        }
        for (state, value) in updates {
            let _ = self.set_bound_state(state, Scalar::F64(value), backend);
        }
    }
    fn reduced_motion_for_state(&self, state: Handle, backend: &mut dyn WindowBackend) -> bool {
        let nodes: Vec<_> = self
            .bindings
            .borrow()
            .values()
            .filter(|binding| binding.state == state)
            .map(|binding| binding.node)
            .collect();
        nodes.into_iter().any(|node| {
            self.document
                .node(node)
                .ok()
                .and_then(|node| self.windows.get(&node.window))
                .and_then(|window| backend.ui_mut(window.key))
                .is_some_and(|ui| ui.theme.reduced_motion)
        })
    }
    pub fn window_closed(&mut self, key: WindowKey) -> Option<Handle> {
        let id = self
            .windows
            .iter()
            .find_map(|(id, window)| (window.key == key).then_some(*id))?;
        self.windows.remove(&id);
        for node in self.document.window_nodes(id) {
            self.remove_subscriptions(node);
            self.remove_bindings(node);
            self.mounted.remove(&node);
            self.collections.remove(node);
            self.canvases.remove(&node);
        }
        let _ = self.document.close_window(id);
        Some(id)
    }
    pub fn set_publisher(&mut self, publisher: SessionControl) {
        self.publisher = Some(publisher);
    }
    pub fn drain_intents(&mut self, backend: &mut dyn WindowBackend) {
        let intents = std::mem::take(&mut *self.binding_intents.borrow_mut());
        for (state, value) in intents {
            let scalar = match value {
                DynamicValue::Bool(value) => Scalar::Bool(value),
                DynamicValue::Index(value) => match i64::try_from(value) {
                    Ok(value) => Scalar::I64(value),
                    Err(_) => continue,
                },
                DynamicValue::F32(value) => Scalar::F64(f64::from(value)),
                DynamicValue::Text(value) => Scalar::Text(value),
            };
            let _ = self.set_bound_state(state, scalar, backend);
        }
    }

    fn set_bound_state(
        &mut self,
        state: Handle,
        value: Scalar,
        backend: &mut dyn WindowBackend,
    ) -> Result<bool, BridgeError> {
        let has_radio_group = self
            .bindings
            .borrow()
            .values()
            .any(|binding| binding.state == state && binding.property == RADIO_GROUP_BINDING);
        let radio_selected = has_radio_group
            .then(|| radio_group_selection(&value))
            .transpose()?;
        if !self.states.set(state, value)? {
            return Ok(false);
        }
        let value = scalar_property(self.states.get(state)?)?;
        let bindings: Vec<_> = self
            .bindings
            .borrow()
            .values()
            .filter(|binding| binding.state == state)
            .cloned()
            .collect();
        for binding in bindings {
            if binding.property == RADIO_GROUP_BINDING {
                let window = self.document.node(binding.node)?.window;
                let managed = self.windows.get(&window).ok_or_else(|| {
                    BridgeError::new("invalid_handle", "window has no active shell window")
                })?;
                self.mounted
                    .get_mut(&binding.node)
                    .ok_or_else(|| {
                        BridgeError::new("invalid_handle", "radio group target is not mounted")
                    })?
                    .widget
                    .apply(
                        WidgetPatch::Radio {
                            selected: radio_selected.expect("radio group selection was validated"),
                        },
                        backend.ui_mut(managed.key).ok_or_else(|| {
                            BridgeError::new("internal_failure", "window UI is unavailable")
                        })?,
                    )
                    .map_err(widget_error)?;
                continue;
            }
            let properties = serde_json::Map::from_iter([(binding.property, value.clone())]);
            self.apply(
                Operation::PropertyUpdate,
                json!({"node":binding.node.0.to_string(),"properties":properties}),
                backend,
            )?;
        }
        Ok(true)
    }
    pub fn emit_event(&self, event: &str, source: Handle, payload: Value) {
        let Some(publisher) = &self.publisher else {
            return;
        };
        let message = Outbound::Event {
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed).to_string(),
            event: event.to_owned(),
            source: source.0.to_string(),
            payload,
        };
        if let Ok(frame) = crate::protocol::encode(&message) {
            let _ = publisher.publish(&frame);
        }
    }
    fn collection_request_sink(&self, source: Handle) -> Rc<dyn Fn(usize, u32) -> bool> {
        let collections = self.collections.clone();
        let publisher = self.publisher.clone();
        let sequence = self.sequence.clone();
        let subscriptions = self.subscriptions.clone();
        Rc::new(move |start, count| {
            if !subscriptions
                .borrow()
                .values()
                .any(|(node, event)| *node == source && event == "data.request")
            {
                return false;
            }
            let request_id = sequence.fetch_add(1, Ordering::Relaxed).to_string();
            let request = match collections.request_visible(source, start as u64, count, request_id)
            {
                Ok(Some(request)) => request,
                Ok(None) => return true,
                Err(_) => return false,
            };
            let Some(publisher) = &publisher else {
                collections.cancel(&request);
                return false;
            };
            let message = Outbound::Event {
                sequence: sequence.fetch_add(1, Ordering::Relaxed).to_string(),
                event: "data.request".to_owned(),
                source: source.0.to_string(),
                payload: json!({
                    "revision": request.revision.to_string(),
                    "request": request.request_id,
                    "start": request.start,
                    "count": request.count,
                }),
            };
            match crate::protocol::encode(&message)
                .map_err(|_| ())
                .and_then(|frame| publisher.publish(&frame).map_err(|_| ()))
            {
                Ok(()) => true,
                Err(()) => {
                    collections.cancel(&request);
                    false
                }
            }
        })
    }
    fn refresh_collection(
        &mut self,
        node: Handle,
        backend: &mut dyn WindowBackend,
    ) -> Result<(), BridgeError> {
        self.apply_collection_patch(node, WidgetPatch::CollectionRefresh, backend)
    }
    fn apply_collection_patch(
        &mut self,
        node: Handle,
        patch: WidgetPatch,
        backend: &mut dyn WindowBackend,
    ) -> Result<(), BridgeError> {
        let window = self.document.node(node)?.window;
        let key = self
            .windows
            .get(&window)
            .ok_or_else(|| BridgeError::new("invalid_handle", "window has no active shell window"))?
            .key;
        self.mounted
            .get_mut(&node)
            .ok_or_else(|| BridgeError::new("invalid_handle", "collection widget is not mounted"))?
            .widget
            .apply(
                patch,
                backend.ui_mut(key).ok_or_else(|| {
                    BridgeError::new("internal_failure", "window UI is unavailable")
                })?,
            )
            .map_err(widget_error)
    }
    pub fn execute(&mut self, request: Request, backend: &mut dyn WindowBackend) -> Outbound {
        let id = request.id.clone();
        match self.apply(request.op, request.args, backend) {
            Ok(result) => Outbound::Response {
                id,
                ok: true,
                result: Some(result),
                error: None,
            },
            Err(error) => Outbound::Response {
                id,
                ok: false,
                result: None,
                error: Some(WireError {
                    code: error.code.to_owned(),
                    message: error.message,
                    field: error.field,
                    handle: error.handle,
                }),
            },
        }
    }
    fn apply(
        &mut self,
        op: Operation,
        args: Value,
        backend: &mut dyn WindowBackend,
    ) -> Result<Value, BridgeError> {
        if self.stopped && op != Operation::Shutdown {
            return Err(BridgeError::new("stopped", "session is shutting down"));
        }
        let args = args
            .as_object()
            .ok_or_else(|| invalid("args", "must be an object"))?;
        match op {
            Operation::Hello => {
                let major = uint(args.get("protocol_major"), "protocol_major")?;
                let minor = uint(args.get("protocol_minor"), "protocol_minor")?;
                if major != 1 {
                    return Err(BridgeError::new(
                        "version_mismatch",
                        "protocol major version 1 is required",
                    )
                    .field("protocol_major"));
                }
                self.protocol_minor = 0;
                let _ = minor;
                Ok(json!({
                    "engine_version": env!("CARGO_PKG_VERSION"),
                    "protocol": {"major": 1, "minor": self.protocol_minor},
                    "capabilities": CAPABILITIES,
                    "limits": {
                        "frame_bytes": MAX_FRAME_BYTES,
                        "json_depth": MAX_JSON_DEPTH,
                        "request_id_bytes": MAX_REQUEST_ID_BYTES,
                        "command_queue_messages": MAX_QUEUE_MESSAGES,
                        "command_queue_bytes": MAX_QUEUE_BYTES,
                        "output_queue_messages": MAX_QUEUE_MESSAGES,
                        "output_queue_bytes": MAX_QUEUE_BYTES,
                        "windows": MAX_WINDOWS,
                        "nodes": MAX_NODES,
                        "states": MAX_OBJECTS_PER_KIND,
                        "bindings": MAX_OBJECTS_PER_KIND,
                        "subscriptions": MAX_OBJECTS_PER_KIND,
                        "image_bytes": MAX_IMAGE_BYTES,
                        "image_session_bytes": MAX_IMAGE_BYTES_PER_SESSION,
                        "image_chunk_bytes": MAX_IMAGE_CHUNK_BYTES,
                        "canvas_commands": MAX_CANVAS_COMMANDS,
                        "canvas_bytes": MAX_CANVAS_BYTES,
                        "model_bytes": MAX_CANONICAL_BYTES,
                        "collection_rows_per_node": MAX_CACHE_ROWS_PER_NODE,
                        "collection_bytes_per_node": MAX_CACHE_BYTES_PER_NODE,
                        "collection_bytes_session": MAX_CACHE_BYTES_PER_SESSION,
                        "collection_request_rows": MAX_REQUEST_ROWS,
                        "tree_items": MAX_TREE_ITEMS,
                        "tree_depth": MAX_TREE_DEPTH,
                    },
                }))
            }
            Operation::Shutdown => {
                self.stopped = true;
                Ok(json!({"stopped": true}))
            }
            Operation::WindowCreate => {
                let title = string(args.get("title"), "title")?.to_owned();
                let width = number(args.get("width"), "width")?;
                let height = number(args.get("height"), "height")?;
                let theme_name = args.get("theme").and_then(Value::as_str).unwrap_or("dark");
                let mut theme = parse_theme(theme_name)?;
                if let Some(reduced_motion) = args.get("reduced_motion") {
                    theme.reduced_motion = reduced_motion
                        .as_bool()
                        .ok_or_else(|| invalid("reduced_motion", "must be a boolean"))?;
                }
                let id = self.document.create_window(
                    title.clone(),
                    width,
                    height,
                    theme_name.to_owned(),
                )?;
                let root_handle = self.document.root_node(id)?;
                let (key, root) = match backend.create_window(
                    &title,
                    Size::new(width as f32, height as f32),
                    theme,
                ) {
                    Ok(window) => window,
                    Err(error) => {
                        let _ = self.document.close_window(id);
                        return Err(error);
                    }
                };
                let parking = match backend
                    .ui_mut(key)
                    .ok_or_else(|| {
                        BridgeError::new("internal_failure", "new window UI is unavailable")
                    })
                    .and_then(|ui| {
                        ui.with_mount_cx(root, |cx| {
                            let node = cx.add_node(Style::default());
                            cx.tree.set_visibility(node, Rc::new(Cell::new(false)));
                            node
                        })
                        .map_err(|_| {
                            BridgeError::new(
                                "internal_failure",
                                "could not create hidden detached-node parking",
                            )
                        })
                    }) {
                    Ok(id) => id,
                    Err(error) => {
                        let _ = backend.close_window(key);
                        let _ = self.document.close_window(id);
                        return Err(error);
                    }
                };
                self.windows.insert(
                    id,
                    ManagedWindow {
                        key,
                        root_handle,
                        root,
                        parking,
                    },
                );
                Ok(json!({"window": id.0.to_string(), "root": root_handle.0.to_string()}))
            }
            Operation::WindowUpdate => {
                let id = handle(args.get("window"), "window")?;
                let managed = self.windows.get(&id).ok_or_else(|| {
                    BridgeError::new("invalid_handle", "window has no active shell window")
                })?;
                let old = self.document.window(id)?;
                let title = args
                    .get("title")
                    .map(|v| string(Some(v), "title").map(str::to_owned))
                    .transpose()?;
                let size = if args.contains_key("width") || args.contains_key("height") {
                    Some((
                        args.get("width")
                            .map(|v| number(Some(v), "width"))
                            .transpose()?
                            .unwrap_or(old.size.0),
                        args.get("height")
                            .map(|v| number(Some(v), "height"))
                            .transpose()?
                            .unwrap_or(old.size.1),
                    ))
                } else {
                    None
                };
                let theme = args
                    .get("theme")
                    .map(|v| string(Some(v), "theme").map(str::to_owned))
                    .transpose()?;
                let reduced_motion = args
                    .get("reduced_motion")
                    .map(|value| {
                        value
                            .as_bool()
                            .ok_or_else(|| invalid("reduced_motion", "must be a boolean"))
                    })
                    .transpose()?;
                if let Some(title) = &title {
                    backend.set_title(managed.key, title)?;
                }
                if let Some((width, height)) = size {
                    backend.set_size(managed.key, Size::new(width as f32, height as f32))?;
                }
                if theme.is_some() || reduced_motion.is_some() {
                    let current_theme = backend
                        .ui_mut(managed.key)
                        .ok_or_else(|| {
                            BridgeError::new("internal_failure", "window UI is unavailable")
                        })?
                        .theme
                        .clone();
                    let current_reduced_motion = current_theme.reduced_motion;
                    let mut updated_theme = theme
                        .as_deref()
                        .map(parse_theme)
                        .transpose()?
                        .unwrap_or(current_theme);
                    updated_theme.reduced_motion = reduced_motion.unwrap_or(current_reduced_motion);
                    backend.set_theme(managed.key, updated_theme)?;
                }
                self.document.update_window(id, title, size, theme)?;
                Ok(json!({}))
            }
            Operation::WindowClose => {
                let id = handle(args.get("window"), "window")?;
                let managed = self.windows.get(&id).ok_or_else(|| {
                    BridgeError::new("invalid_handle", "window has no active shell window")
                })?;
                let nodes = self.document.window_nodes(id);
                backend.close_window(managed.key)?;
                self.windows.remove(&id);
                for node in nodes {
                    self.mounted.remove(&node);
                    self.collections.remove(node);
                    self.canvases.remove(&node);
                }
                self.document.close_window(id)?;
                Ok(json!({}))
            }
            Operation::NodeCreate => {
                let window = handle(args.get("window"), "window")?;
                let widget = string(args.get("widget"), "widget")?;
                let properties = object(args.get("properties"), "properties")?;
                let kind = widget_kind(widget)?;
                validate_patch(kind, &Value::Object(properties.clone()))?;
                let radio_group = if kind == WidgetKind::Radio {
                    let state = handle(properties.get("group"), "group")?;
                    let selected = radio_group_selection(self.states.get(state)?)?;
                    Some((state, selected))
                } else {
                    None
                };
                let image = if kind == WidgetKind::Image {
                    Some(
                        self.resources
                            .image(handle(properties.get("image"), "image")?)?,
                    )
                } else {
                    None
                };
                let canvas = (kind == WidgetKind::Canvas).then(CanvasModel::new);
                if let (Some(model), Some(commands)) = (canvas.as_ref(), properties.get("commands"))
                {
                    let commands = commands
                        .as_array()
                        .ok_or_else(|| invalid("commands", "must be an array"))?
                        .iter()
                        .map(|value| self.canvas_command(value))
                        .collect::<Result<Vec<_>, _>>()?;
                    model.replace(commands)?;
                }
                let disabled = properties
                    .get("disabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let id = self.document.create_node(
                    window,
                    widget.to_owned(),
                    properties.clone().into_iter().collect(),
                )?;
                let sink = self.event_sink(id);
                let on_visible = self.collection_request_sink(id);
                let spec = match widget_spec(
                    kind,
                    &properties,
                    WidgetSpecContext {
                        radio_selected: radio_group.map(|(_, selected)| selected),
                        image,
                        canvas: canvas.as_ref(),
                        node: id,
                        collections: &self.collections,
                        on_visible,
                    },
                ) {
                    Ok(spec) => spec,
                    Err(error) => {
                        let _ = self.document.destroy_node(id);
                        return Err(error);
                    }
                };
                let managed = self.windows.get(&window).ok_or_else(|| {
                    BridgeError::new("invalid_handle", "window has no active shell window")
                })?;
                let mounted = match backend
                    .ui_mut(managed.key)
                    .ok_or_else(|| BridgeError::new("internal_failure", "window UI is unavailable"))
                    .and_then(|ui| {
                        ui.with_mount_cx(managed.parking, |cx| mount(spec, cx, sink))
                            .map_err(|_| {
                                BridgeError::new("internal_failure", "dynamic widget mount failed")
                            })?
                            .map_err(widget_error)
                    }) {
                    Ok(widget) => widget,
                    Err(error) => {
                        let _ = self.document.destroy_node(id);
                        return Err(error);
                    }
                };
                if let Some((state, _)) = radio_group
                    && let Err(error) = self.states.bind(state, id)
                {
                    if let Some(ui) = backend.ui_mut(managed.key) {
                        let _ = ui.remove_subtree(mounted.root());
                    }
                    let _ = self.document.destroy_node(id);
                    return Err(error);
                }
                self.mounted.insert(id, MountedNode { widget: mounted });
                if let Some((state, _)) = radio_group {
                    self.bindings.borrow_mut().insert(
                        id,
                        BindingRecord {
                            state,
                            node: id,
                            property: RADIO_GROUP_BINDING.to_owned(),
                            two_way: true,
                        },
                    );
                }
                if let Some(canvas) = canvas {
                    self.canvases.insert(id, canvas);
                }
                if kind == WidgetKind::Button
                    && disabled
                    && let Some(ui) = backend.ui_mut(managed.key)
                {
                    ui.tree
                        .node_mut(self.mounted[&id].widget.root())
                        .interaction
                        .disabled = true;
                }
                Ok(json!({"node": id.0.to_string()}))
            }
            Operation::NodeAttach => {
                let node = handle(args.get("node"), "node")?;
                let parent = handle(args.get("parent"), "parent")?;
                let default_index =
                    self.document
                        .node(parent)?
                        .children
                        .len()
                        .saturating_sub(usize::from(
                            self.document.node(node)?.parent == Some(parent),
                        ));
                let index = args
                    .get("index")
                    .map(|v| uint(Some(v), "index").map(|n| n as usize))
                    .transpose()?
                    .unwrap_or(default_index);
                let window = self.document.node(node)?.window;
                let managed = self.windows.get(&window).ok_or_else(|| {
                    BridgeError::new("invalid_handle", "window has no active shell window")
                })?;
                let child_engine = self
                    .mounted
                    .get(&node)
                    .ok_or_else(|| {
                        BridgeError::new("invalid_handle", "node has no mounted widget")
                    })?
                    .widget
                    .root();
                let parent_engine = if parent == managed.root_handle {
                    managed.root
                } else {
                    let target = self.mounted.get(&parent).ok_or_else(|| {
                        BridgeError::new("invalid_handle", "parent has no child slot")
                    })?;
                    if target.widget.child_slot().is_none() {
                        return Err(BridgeError::new(
                            "invalid_property",
                            "parent widget cannot contain children",
                        ));
                    }
                    target.widget.child_parent()
                };
                self.document.attach(node, parent, index)?;
                let ui = backend.ui_mut(managed.key).ok_or_else(|| {
                    BridgeError::new("internal_failure", "window UI is unavailable")
                })?;
                ui.tree
                    .reparent(child_engine, parent_engine, index)
                    .map_err(|_| {
                        BridgeError::new("invalid_tree", "engine tree rejected node attachment")
                    })?;
                if let Some(parent_widget) = self.mounted.get_mut(&parent) {
                    parent_widget.widget.children_changed(ui).map_err(|_| {
                        BridgeError::new("invalid_property", "parent rejected child update")
                    })?;
                }
                Ok(json!({}))
            }
            Operation::NodeDetach => {
                let node = handle(args.get("node"), "node")?;
                let record = self.document.node(node)?;
                let window = record.window;
                let old_parent = record.parent;
                let managed = self.windows.get(&window).ok_or_else(|| {
                    BridgeError::new("invalid_handle", "window has no active shell window")
                })?;
                let engine_node = self
                    .mounted
                    .get(&node)
                    .ok_or_else(|| {
                        BridgeError::new("invalid_handle", "node has no mounted widget")
                    })?
                    .widget
                    .root();
                self.document.detach(node)?;
                let ui = backend.ui_mut(managed.key).ok_or_else(|| {
                    BridgeError::new("internal_failure", "window UI is unavailable")
                })?;
                ui.detach_to(engine_node, managed.parking)
                    .map_err(|_| BridgeError::new("invalid_tree", "engine tree rejected detach"))?;
                if let Some(parent) = old_parent.and_then(|id| self.mounted.get_mut(&id)) {
                    parent.widget.children_changed(ui).map_err(|_| {
                        BridgeError::new("invalid_property", "parent rejected child update")
                    })?;
                }
                Ok(json!({}))
            }
            Operation::NodeReplaceChildren => {
                let parent = handle(args.get("node"), "node")?;
                let children = args
                    .get("children")
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid("children", "must be an array"))?;
                let children = children
                    .iter()
                    .map(|v| handle(Some(v), "children"))
                    .collect::<Result<Vec<_>, _>>()?;
                let window = self.document.node(parent)?.window;
                let old_children = self.document.node(parent)?.children.clone();
                let managed = self.windows.get(&window).ok_or_else(|| {
                    BridgeError::new("invalid_handle", "window has no active shell window")
                })?;
                let (engine_parent, parent_widget) = if parent == managed.root_handle {
                    (managed.root, None)
                } else {
                    let mounted = self.mounted.get(&parent).ok_or_else(|| {
                        BridgeError::new("invalid_handle", "parent has no child slot")
                    })?;
                    if mounted.widget.child_slot().is_none() {
                        return Err(BridgeError::new(
                            "invalid_property",
                            "parent widget cannot contain children",
                        ));
                    }
                    (mounted.widget.child_parent(), Some(parent))
                };
                self.document.replace_children(parent, &children)?;
                let ui = backend.ui_mut(managed.key).ok_or_else(|| {
                    BridgeError::new("internal_failure", "window UI is unavailable")
                })?;
                for child in old_children.iter().filter(|id| !children.contains(id)) {
                    let child_root = self
                        .mounted
                        .get(child)
                        .ok_or_else(|| {
                            BridgeError::new("invalid_handle", "child has no mounted widget")
                        })?
                        .widget
                        .root();
                    ui.detach_to(child_root, managed.parking).map_err(|_| {
                        BridgeError::new("invalid_tree", "engine tree rejected child detach")
                    })?;
                }
                for (index, child) in children.iter().enumerate() {
                    let child_root = self
                        .mounted
                        .get(child)
                        .ok_or_else(|| {
                            BridgeError::new("invalid_handle", "child has no mounted widget")
                        })?
                        .widget
                        .root();
                    ui.tree
                        .reparent(child_root, engine_parent, index)
                        .map_err(|_| {
                            BridgeError::new(
                                "invalid_tree",
                                "engine tree rejected child replacement",
                            )
                        })?;
                }
                if let Some(parent) = parent_widget.and_then(|id| self.mounted.get_mut(&id)) {
                    parent.widget.children_changed(ui).map_err(|_| {
                        BridgeError::new("invalid_property", "parent rejected child update")
                    })?;
                }
                Ok(json!({}))
            }
            Operation::NodeDestroy => {
                let id = handle(args.get("node"), "node")?;
                let window = self.document.node(id)?.window;
                let managed = self.windows.get(&window).ok_or_else(|| {
                    BridgeError::new("invalid_handle", "window has no active shell window")
                })?;
                let nodes = self.document.subtree(id)?;
                let engine = self
                    .mounted
                    .get(&id)
                    .ok_or_else(|| {
                        BridgeError::new("invalid_handle", "node has no mounted widget")
                    })?
                    .widget
                    .root();
                let ui = backend.ui_mut(managed.key).ok_or_else(|| {
                    BridgeError::new("internal_failure", "window UI is unavailable")
                })?;
                ui.remove_subtree(engine).map_err(|_| {
                    BridgeError::new("invalid_tree", "engine rejected subtree destruction")
                })?;
                self.document.destroy_node(id)?;
                for node in nodes {
                    self.remove_subscriptions(node);
                    self.remove_bindings(node);
                    self.mounted.remove(&node);
                    self.canvases.remove(&node);
                    self.collections.remove(node);
                }
                self.canvases
                    .retain(|node, _| self.document.node(*node).is_ok());
                Ok(json!({}))
            }
            Operation::PropertyUpdate => {
                let id = handle(args.get("node"), "node")?;
                let kind = widget_kind(&self.document.node(id)?.widget)?;
                let properties = object(args.get("properties"), "properties")?;
                validate_patch(kind, &Value::Object(properties.clone()))?;
                let image = if kind == WidgetKind::Image && properties.contains_key("image") {
                    Some(
                        self.resources
                            .image(handle(properties.get("image"), "image")?)?,
                    )
                } else {
                    None
                };
                let patch = widget_patch(
                    kind,
                    &properties,
                    &self.document.node(id)?.properties,
                    image,
                )?;
                let window = self.document.node(id)?.window;
                let key = self
                    .windows
                    .get(&window)
                    .ok_or_else(|| {
                        BridgeError::new("invalid_handle", "window has no active shell window")
                    })?
                    .key;
                let properties: BTreeMap<_, _> = properties.into_iter().collect();
                self.document.validate_property_update(id, &properties)?;
                self.mounted
                    .get_mut(&id)
                    .ok_or_else(|| {
                        BridgeError::new("invalid_handle", "node has no mounted widget")
                    })?
                    .widget
                    .apply(
                        patch,
                        backend.ui_mut(key).ok_or_else(|| {
                            BridgeError::new("internal_failure", "window UI is unavailable")
                        })?,
                    )
                    .map_err(widget_error)?;
                self.document.update_properties(id, properties)?;
                Ok(json!({}))
            }
            Operation::ImageBegin => {
                let width = u32::try_from(uint(args.get("width"), "width")?)
                    .map_err(|_| invalid("width", "exceeds u32"))?;
                let height = u32::try_from(uint(args.get("height"), "height")?)
                    .map_err(|_| invalid("height", "exceeds u32"))?;
                let upload = self.resources.begin(
                    width,
                    height,
                    uint(args.get("byte_length"), "byte_length")?,
                )?;
                Ok(json!({"upload": upload.0.to_string()}))
            }
            Operation::ImageChunk => {
                let upload = handle(args.get("upload"), "upload")?;
                let data = string(args.get("data"), "data")?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|_| {
                        BridgeError::new("invalid_value", "image chunk is not valid base64")
                            .field("data")
                    })?;
                self.resources
                    .append(upload, uint(args.get("offset"), "offset")?, &bytes)?;
                Ok(json!({"accepted": bytes.len()}))
            }
            Operation::ImageFinalize => {
                let image = self
                    .resources
                    .finalize(handle(args.get("upload"), "upload")?)?;
                Ok(json!({"image": image.0.to_string()}))
            }
            Operation::ImageDestroy => {
                self.resources
                    .release(handle(args.get("image"), "image")?)?;
                Ok(json!({}))
            }
            Operation::Subscribe => {
                let node = handle(args.get("node"), "node")?;
                let kind = widget_kind(&self.document.node(node)?.widget)?;
                let event = string(args.get("event"), "event")?;
                let supported = match event {
                    "click" => kind == WidgetKind::Button,
                    "change" => matches!(
                        kind,
                        WidgetKind::TextField
                            | WidgetKind::Checkbox
                            | WidgetKind::Radio
                            | WidgetKind::Switch
                            | WidgetKind::Slider
                            | WidgetKind::Modal
                            | WidgetKind::Popup
                            | WidgetKind::Tabs
                    ),
                    "resize" => kind == WidgetKind::Canvas,
                    "select" => matches!(
                        kind,
                        WidgetKind::ComboBox
                            | WidgetKind::VirtualList
                            | WidgetKind::DataGrid
                            | WidgetKind::TreeView
                    ),
                    "data.request" => {
                        matches!(kind, WidgetKind::VirtualList | WidgetKind::DataGrid)
                    }
                    _ => false,
                };
                if !supported {
                    return Err(BridgeError::new(
                        "unsupported_capability",
                        "event is not emitted by this widget",
                    )
                    .field("event"));
                }
                let id = self
                    .handles
                    .insert(ObjectKind::Subscription, Object::Marker)?;
                self.subscriptions
                    .borrow_mut()
                    .insert(id, (node, event.to_owned()));
                if event == "data.request" {
                    let window = self.document.node(node)?.window;
                    if let (Some(managed), Some(mounted)) =
                        (self.windows.get(&window), self.mounted.get(&node))
                        && let Some(ui) = backend.ui_mut(managed.key)
                    {
                        ui.tree
                            .mark_dirty(mounted.widget.root(), grafikr_core::DirtyFlags::PAINT);
                    }
                }
                Ok(json!({"subscription": id.0.to_string()}))
            }
            Operation::Unsubscribe => {
                let id = handle(args.get("subscription"), "subscription")?;
                self.handles.remove(id, ObjectKind::Subscription)?;
                if self.subscriptions.borrow_mut().remove(&id).is_none() {
                    return Err(BridgeError::new("invalid_handle", "subscription is stale"));
                }
                Ok(json!({}))
            }
            Operation::Bind => {
                let state = handle(args.get("state"), "state")?;
                let node = handle(args.get("node"), "node")?;
                let property = string(args.get("property"), "property")?.to_owned();
                let two_way = match string(args.get("direction"), "direction")? {
                    "one_way" => false,
                    "two_way" => true,
                    _ => return Err(invalid("direction", "must be one_way or two_way")),
                };
                let value = scalar_property(self.states.get(state)?)?;
                let properties = serde_json::Map::from_iter([(property.clone(), value)]);
                let id = self.handles.insert(ObjectKind::Binding, Object::Marker)?;
                if let Err(error) = self.apply(
                    Operation::PropertyUpdate,
                    json!({"node":node.0.to_string(),"properties":properties}),
                    backend,
                ) {
                    let _ = self.handles.remove(id, ObjectKind::Binding);
                    return Err(error);
                }
                if let Err(error) = self.states.bind(state, node) {
                    let _ = self.handles.remove(id, ObjectKind::Binding);
                    return Err(error);
                }
                self.bindings.borrow_mut().insert(
                    id,
                    BindingRecord {
                        state,
                        node,
                        property,
                        two_way,
                    },
                );
                Ok(json!({"binding":id.0.to_string()}))
            }
            Operation::Unbind => {
                let id = handle(args.get("binding"), "binding")?;
                self.handles.remove(id, ObjectKind::Binding)?;
                let binding = self
                    .bindings
                    .borrow_mut()
                    .remove(&id)
                    .ok_or_else(|| BridgeError::new("invalid_handle", "binding is stale"))?;
                self.states.unbind(binding.state, binding.node);
                Ok(json!({}))
            }
            Operation::CanvasUpdate => {
                let node = handle(args.get("node"), "node")?;
                if widget_kind(&self.document.node(node)?.widget)? != WidgetKind::Canvas {
                    return Err(BridgeError::new(
                        "invalid_type",
                        "canvas.update requires a Canvas node",
                    )
                    .field("node"));
                }
                let values = args
                    .get("commands")
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid("commands", "must be an array"))?;
                let commands = values
                    .iter()
                    .map(|value| self.canvas_command(value))
                    .collect::<Result<Vec<_>, _>>()?;
                let canvas = self.canvases.get(&node).ok_or_else(|| {
                    BridgeError::new("invalid_handle", "canvas is not mounted").field("node")
                })?;
                let window = self.document.node(node)?.window;
                let key = self
                    .windows
                    .get(&window)
                    .ok_or_else(|| {
                        BridgeError::new("invalid_handle", "window has no active shell window")
                    })?
                    .key;
                self.mounted
                    .get_mut(&node)
                    .ok_or_else(|| BridgeError::new("invalid_handle", "canvas is not mounted"))?
                    .widget
                    .apply(
                        WidgetPatch::Canvas {
                            label: None,
                            width: None,
                            height: None,
                        },
                        backend.ui_mut(key).ok_or_else(|| {
                            BridgeError::new("internal_failure", "window UI is unavailable")
                        })?,
                    )
                    .map_err(widget_error)?;
                canvas.replace(commands)?;
                Ok(json!({"command_count": values.len()}))
            }
            Operation::CollectionCreate => {
                let node = handle(args.get("node"), "node")?;
                let kind = widget_kind(&self.document.node(node)?.widget)?;
                if !matches!(kind, WidgetKind::VirtualList | WidgetKind::DataGrid) {
                    return Err(BridgeError::new(
                        "invalid_type",
                        "collections require a VirtualList or DataGrid node",
                    ));
                }
                let row_count = uint(args.get("row_count"), "row_count")?;
                let row_height = f32_number(args.get("row_height"), "row_height")?;
                let columns = string_list(args.get("columns"), "columns")?;
                self.collections
                    .create(node, row_count, row_height, columns)?;
                if matches!(kind, WidgetKind::VirtualList | WidgetKind::DataGrid) {
                    self.apply_collection_patch(
                        node,
                        WidgetPatch::CollectionMetadata {
                            rows: usize::try_from(row_count)
                                .map_err(|_| invalid("row_count", "exceeds usize"))?,
                            row_height,
                        },
                        backend,
                    )?;
                }
                Ok(json!({"revision":"1"}))
            }
            Operation::CollectionUpdate => {
                let node = handle(args.get("node"), "node")?;
                let metadata = args
                    .get("metadata")
                    .and_then(Value::as_object)
                    .ok_or_else(|| invalid("metadata", "must be an object"))?;
                let row_count = uint(metadata.get("row_count"), "row_count")?;
                let row_height = f32_number(metadata.get("row_height"), "row_height")?;
                let columns = string_list(metadata.get("columns"), "columns")?;
                let revision = self
                    .collections
                    .update(node, row_count, row_height, columns)?;
                if matches!(
                    widget_kind(&self.document.node(node)?.widget)?,
                    WidgetKind::VirtualList | WidgetKind::DataGrid
                ) {
                    self.apply_collection_patch(
                        node,
                        WidgetPatch::CollectionMetadata {
                            rows: usize::try_from(row_count)
                                .map_err(|_| invalid("row_count", "exceeds usize"))?,
                            row_height,
                        },
                        backend,
                    )?;
                }
                Ok(json!({"revision":revision.to_string()}))
            }
            Operation::DataProvide => {
                let rows = args
                    .get("rows")
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid("rows", "must be an array"))?
                    .iter()
                    .map(|row| {
                        row.as_array()
                            .cloned()
                            .ok_or_else(|| invalid("rows", "each row must be an array"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let node = handle(args.get("node"), "node")?;
                self.collections.provide_active(
                    node,
                    wide_uint(args.get("revision"), "revision")?,
                    string(args.get("request"), "request")?,
                    u32::try_from(uint(args.get("start"), "start")?)
                        .map_err(|_| invalid("start", "exceeds u32"))?,
                    rows,
                )?;
                if matches!(
                    widget_kind(&self.document.node(node)?.widget)?,
                    WidgetKind::VirtualList | WidgetKind::DataGrid
                ) {
                    self.refresh_collection(node, backend)?;
                }
                Ok(json!({}))
            }
            Operation::StateCreate => {
                let value = scalar(args.get("value"))?;
                let id = self.states.create(value)?;
                Ok(json!({"state": id.0.to_string()}))
            }
            Operation::StateGet => {
                let value = scalar_json(self.states.get(handle(args.get("state"), "state")?)?);
                Ok(value)
            }
            Operation::StateSet => {
                let state = handle(args.get("state"), "state")?;
                let value = scalar(args.get("value"))?;
                let changed = self.set_bound_state(state, value, backend)?;
                self.transitions.remove(&state);
                Ok(json!({"changed": changed}))
            }
            Operation::StateDestroy => {
                let state = handle(args.get("state"), "state")?;
                self.states.destroy(state)?;
                self.transitions.remove(&state);
                Ok(json!({}))
            }
            Operation::Transition => {
                let state = handle(args.get("state"), "state")?;
                let from = match self.states.get(state)? {
                    Scalar::F64(value) => *value,
                    _ => return Err(invalid("state", "transition state must be f64")),
                };
                let target = match scalar(args.get("to"))? {
                    Scalar::F64(value) => value,
                    _ => return Err(invalid("to", "transition target must be f64")),
                };
                let from = finite_f32(from, "state")?;
                let target = finite_f32(target, "to")?;
                let duration_ms = uint(args.get("duration_ms"), "duration_ms")?;
                let duration = u32::try_from(duration_ms).map_err(|_| {
                    BridgeError::new("invalid_value", "duration exceeds u32 milliseconds")
                        .field("duration_ms")
                })?;
                let easing = match string(args.get("easing"), "easing")? {
                    "linear" => Easing::Linear,
                    "ease_out_cubic" => Easing::EaseOutCubic,
                    "ease_in_out_cubic" => Easing::EaseInOutCubic,
                    _ => return Err(invalid("easing", "unsupported easing")),
                };
                let transition = Transition::new(from)
                    .duration(duration as f32 / 1000.0)
                    .easing(easing);
                transition.set_target(target);
                self.transitions.remove(&state);
                if duration == 0 || from == target || self.reduced_motion_for_state(state, backend)
                {
                    self.set_bound_state(state, Scalar::F64(f64::from(target)), backend)?;
                } else {
                    self.transitions.insert(state, transition);
                }
                Ok(json!({}))
            }
        }
    }

    fn event_sink(&self, source: Handle) -> Rc<dyn Fn(DynamicEvent)> {
        let publisher = self.publisher.clone();
        let sequence = self.sequence.clone();
        let subscriptions = self.subscriptions.clone();
        let bindings = self.bindings.clone();
        let intents = self.binding_intents.clone();
        Rc::new(move |event| {
            if let DynamicEvent::ValueChanged { value, .. } = &event {
                for binding in bindings
                    .borrow()
                    .values()
                    .filter(|binding| binding.node == source && binding.two_way)
                {
                    intents.borrow_mut().push((binding.state, value.clone()));
                }
            }
            let Some(publisher) = &publisher else {
                return;
            };
            let (name, payload) = match event {
                DynamicEvent::Clicked { .. } => ("click", json!({})),
                DynamicEvent::ValueChanged { value, .. } => (
                    "change",
                    match value {
                        DynamicValue::Bool(value) => json!({"type":"bool", "value":value}),
                        DynamicValue::Index(value) => {
                            json!({"type":"index", "value":value.to_string()})
                        }
                        DynamicValue::F32(value) => json!({"type":"f32", "value":value}),
                        DynamicValue::Text(value) => json!({"type":"string", "value":value}),
                    },
                ),
                DynamicEvent::Resized { width, height, .. } => {
                    ("resize", json!({"width":width,"height":height}))
                }
                DynamicEvent::Selected { index, .. } => {
                    ("select", json!({"index":index.to_string()}))
                }
                DynamicEvent::KeySelected { key } => ("select", json!({"key":key})),
            };
            if !subscriptions
                .borrow()
                .values()
                .any(|(node, subscribed)| *node == source && subscribed == name)
            {
                return;
            }
            let message = Outbound::Event {
                sequence: sequence.fetch_add(1, Ordering::Relaxed).to_string(),
                event: name.to_owned(),
                source: source.0.to_string(),
                payload,
            };
            if let Ok(frame) = crate::protocol::encode(&message) {
                let _ = publisher.publish(&frame);
            }
        })
    }

    fn remove_subscriptions(&mut self, node: Handle) {
        let mut subscriptions = self.subscriptions.borrow_mut();
        let stale: Vec<_> = subscriptions
            .iter()
            .filter_map(|(id, (source, _))| (*source == node).then_some(*id))
            .collect();
        for id in stale {
            subscriptions.remove(&id);
            let _ = self.handles.remove(id, ObjectKind::Subscription);
        }
    }

    fn remove_bindings(&mut self, node: Handle) {
        let stale: Vec<_> = self
            .bindings
            .borrow()
            .iter()
            .filter_map(|(id, binding)| (binding.node == node).then_some((*id, binding.clone())))
            .collect();
        let mut bindings = self.bindings.borrow_mut();
        for (id, binding) in stale {
            bindings.remove(&id);
            self.states.unbind(binding.state, binding.node);
            let _ = self.handles.remove(id, ObjectKind::Binding);
        }
    }

    fn canvas_command(&mut self, value: &Value) -> Result<CanvasCommand, BridgeError> {
        let fields = value
            .as_object()
            .ok_or_else(|| invalid("commands", "each command must be an object"))?;
        let kind = string(fields.get("kind"), "kind")?;
        match kind {
            "rect" => Ok(CanvasCommand::Rect {
                rect: parse_rect(fields.get("rect"))?,
                color: parse_color(fields.get("color"), "color")?,
                radius: f32_number(fields.get("radius"), "radius")?,
            }),
            "bordered_rect" => Ok(CanvasCommand::BorderedRect {
                rect: parse_rect(fields.get("rect"))?,
                color: parse_color(fields.get("color"), "color")?,
                radius: f32_number(fields.get("radius"), "radius")?,
                width: f32_number(fields.get("border_width"), "border_width")?,
                border: parse_color(fields.get("border_color"), "border_color")?,
            }),
            "shadow" => Ok(CanvasCommand::Shadow {
                rect: parse_rect(fields.get("rect"))?,
                radius: f32_number(fields.get("radius"), "radius")?,
                blur: f32_number(fields.get("blur"), "blur")?,
                color: parse_color(fields.get("color"), "color")?,
            }),
            "text" => Ok(CanvasCommand::Text {
                x: f32_number(fields.get("x"), "x")?,
                y: f32_number(fields.get("y"), "y")?,
                text: string(fields.get("text"), "text")?.to_owned(),
                size: f32_number(fields.get("size"), "size")?,
                color: parse_color(fields.get("color"), "color")?,
            }),
            "image" => {
                let image = handle(fields.get("image"), "image")?;
                let data = self.resources.image(image)?;
                Ok(CanvasCommand::Image {
                    rect: parse_rect(fields.get("rect"))?,
                    handle: image,
                    data,
                })
            }
            _ => {
                Err(BridgeError::new("invalid_value", "unknown canvas command kind").field("kind"))
            }
        }
    }
}

fn widget_kind(name: &str) -> Result<WidgetKind, BridgeError> {
    match name {
        "Column" | "column" => Ok(WidgetKind::Column),
        "Row" | "row" => Ok(WidgetKind::Row),
        "Grid" | "grid" => Ok(WidgetKind::Grid),
        "Stack" | "stack" => Ok(WidgetKind::Stack),
        "Text" | "text" => Ok(WidgetKind::Text),
        "Button" | "button" => Ok(WidgetKind::Button),
        "TextField" | "text_field" => Ok(WidgetKind::TextField),
        "Checkbox" | "checkbox" => Ok(WidgetKind::Checkbox),
        "Radio" | "radio" => Ok(WidgetKind::Radio),
        "Switch" | "switch" => Ok(WidgetKind::Switch),
        "Slider" | "slider" => Ok(WidgetKind::Slider),
        "ProgressBar" | "progress" => Ok(WidgetKind::ProgressBar),
        "Spinner" | "spinner" => Ok(WidgetKind::Spinner),
        "ScrollView" | "scroll_view" => Ok(WidgetKind::ScrollView),
        "VirtualList" | "virtual_list" => Ok(WidgetKind::VirtualList),
        "DataGrid" | "data_grid" => Ok(WidgetKind::DataGrid),
        "TreeView" | "tree_view" => Ok(WidgetKind::TreeView),
        "Modal" | "modal" => Ok(WidgetKind::Modal),
        "Popup" | "popup" => Ok(WidgetKind::Popup),
        "Tabs" | "tabs" => Ok(WidgetKind::Tabs),
        "ComboBox" | "combo_box" => Ok(WidgetKind::ComboBox),
        "Tooltip" | "tooltip" => Ok(WidgetKind::Tooltip),
        "Image" | "image" => Ok(WidgetKind::Image),
        "Canvas" | "canvas" => Ok(WidgetKind::Canvas),
        _ => Err(BridgeError::new("invalid_value", "unknown widget kind").field("widget")),
    }
}

fn widget_error(error: grafikr_widgets::WidgetError) -> BridgeError {
    let code = match error {
        grafikr_widgets::WidgetError::InvalidProperty => "invalid_value",
        grafikr_widgets::WidgetError::WrongWidget => "invalid_type",
        grafikr_widgets::WidgetError::StaleNode => "invalid_handle",
    };
    BridgeError::new(code, "dynamic widget operation failed")
}

fn parse_theme(value: &str) -> Result<Theme, BridgeError> {
    match value {
        "dark" => Ok(Theme::dark()),
        "light" => Ok(Theme::light()),
        _ => Err(BridgeError::new("invalid_value", "theme must be dark or light").field("theme")),
    }
}

struct WidgetSpecContext<'a> {
    radio_selected: Option<u64>,
    image: Option<Arc<ImageData>>,
    canvas: Option<&'a CanvasModel>,
    node: Handle,
    collections: &'a Collections,
    on_visible: Rc<dyn Fn(usize, u32) -> bool>,
}

fn widget_spec(
    kind: WidgetKind,
    p: &serde_json::Map<String, Value>,
    context: WidgetSpecContext<'_>,
) -> Result<WidgetSpec, BridgeError> {
    let WidgetSpecContext {
        radio_selected,
        image,
        canvas,
        node,
        collections,
        on_visible,
    } = context;
    let text = |key| prop_text(p, key, "");
    let number = |key, default| prop_f32(p, key, default);
    let boolean = |key, default| prop_bool(p, key, default);
    Ok(match kind {
        WidgetKind::Column | WidgetKind::Row | WidgetKind::Grid | WidgetKind::Stack => {
            let kind = match kind {
                WidgetKind::Column => LayoutKind::Column,
                WidgetKind::Row => LayoutKind::Row,
                WidgetKind::Grid => LayoutKind::Grid(
                    u16::try_from(prop_u64(p, "columns", 1)?)
                        .map_err(|_| invalid("columns", "exceeds u16"))?,
                ),
                _ => LayoutKind::Stack,
            };
            WidgetSpec::Layout {
                kind,
                style: layout_style(kind, p)?,
            }
        }
        WidgetKind::Text => WidgetSpec::Text {
            text: text("text")?,
            size: optional_f32(p, "size")?,
            color: optional_color(p, "color")?,
        },
        WidgetKind::Button => {
            let (variant, icon) = match text("variant")?.as_str() {
                "" | "primary" => (ButtonVariant::Primary, false),
                "secondary" => (ButtonVariant::Secondary, false),
                "icon" => (ButtonVariant::Primary, true),
                _ => {
                    return Err(BridgeError::new("invalid_value", "unknown button variant")
                        .field("variant"));
                }
            };
            WidgetSpec::Button {
                label: text("label")?,
                variant,
                icon,
            }
        }
        WidgetKind::TextField => WidgetSpec::TextField {
            text: text("text")?,
            width: optional_f32(p, "width")?,
            multiline: boolean("multiline", false)?,
            password: boolean("password", false)?,
        },
        WidgetKind::Checkbox => WidgetSpec::Checkbox {
            label: text("label")?,
            checked: boolean("checked", false)?,
        },
        WidgetKind::Radio => WidgetSpec::Radio {
            label: text("label")?,
            selected: radio_selected
                .ok_or_else(|| invalid("group", "radio group state is required"))?,
            value: prop_u64(p, "value", 0)?,
        },
        WidgetKind::Switch => WidgetSpec::Switch {
            label: text("label")?,
            checked: boolean("checked", false)?,
        },
        WidgetKind::Slider => WidgetSpec::Slider {
            value: number("value", 0.0)?,
            min: number("min", 0.0)?,
            max: number("max", 1.0)?,
            step: number("step", 0.0)?,
            width: number("width", 200.0)?,
        },
        WidgetKind::ProgressBar => WidgetSpec::ProgressBar {
            value: number("value", 0.0)?,
            width: number("width", 220.0)?,
        },
        WidgetKind::Spinner => WidgetSpec::Spinner {
            size: number("size", 28.0)?,
            label: {
                let value = text("label")?;
                if value.is_empty() {
                    "Loading".to_owned()
                } else {
                    value
                }
            },
        },
        WidgetKind::ScrollView => WidgetSpec::ScrollView {
            width: optional_f32(p, "width")?,
            height: optional_f32(p, "height")?,
            grow: boolean("grow", false)?,
        },
        WidgetKind::VirtualList => {
            let rows = usize::try_from(prop_u64(p, "row_count", 0)?)
                .map_err(|_| invalid("row_count", "exceeds usize"))?;
            let collection = collections.clone();
            WidgetSpec::VirtualList {
                rows,
                row_height: prop_f32(p, "row_height", 24.0)?,
                height: prop_f32(p, "height", 240.0)?,
                width: optional_f32(p, "width")?,
                selected: p
                    .get("selected")
                    .map(|_| prop_u64(p, "selected", 0))
                    .transpose()?
                    .map(|value| {
                        usize::try_from(value).map_err(|_| invalid("selected", "exceeds usize"))
                    })
                    .transpose()?,
                label: Rc::new(move |index| {
                    collection
                        .row_cloned(node, index as u32)
                        .and_then(|row| row.first().cloned())
                        .map(cell_text)
                        .unwrap_or_default()
                }),
                on_visible,
            }
        }
        WidgetKind::DataGrid => {
            let collection = collections.clone();
            let columns = grid_columns(p.get("columns"))?;
            WidgetSpec::DataGrid {
                columns,
                rows: usize::try_from(prop_u64(p, "row_count", 0)?)
                    .map_err(|_| invalid("row_count", "exceeds usize"))?,
                row_height: prop_f32(p, "row_height", 24.0)?,
                height: prop_f32(p, "height", 320.0)?,
                width: optional_f32(p, "width")?,
                selected: p
                    .get("selected")
                    .map(|_| prop_u64(p, "selected", 0))
                    .transpose()?
                    .map(|value| {
                        usize::try_from(value).map_err(|_| invalid("selected", "exceeds usize"))
                    })
                    .transpose()?,
                label: prop_text(p, "label", "Data grid")?,
                row: Rc::new(move |index| {
                    collection
                        .row_cloned(node, index as u32)
                        .unwrap_or_default()
                        .into_iter()
                        .map(cell_text)
                        .collect()
                }),
                on_visible,
            }
        }
        WidgetKind::TreeView => {
            let (roots, keys) = parse_tree_roots(p.get("roots"))?;
            let selected = p
                .get("selected")
                .map(|_| {
                    let key = string(p.get("selected"), "selected")?;
                    keys.iter()
                        .position(|candidate| candidate == key)
                        .ok_or_else(|| {
                            BridgeError::new("invalid_value", "tree key does not exist")
                                .field("selected")
                        })
                })
                .transpose()?;
            WidgetSpec::TreeView {
                roots,
                keys,
                selected,
                width: optional_f32(p, "width")?,
                height: prop_f32(p, "height", 320.0)?,
            }
        }
        WidgetKind::Modal => WidgetSpec::Modal {
            open: boolean("open", false)?,
            width: optional_f32(p, "width")?,
        },
        WidgetKind::Popup => WidgetSpec::Popup {
            open: boolean("open", false)?,
            x: number("x", 0.0)?,
            y: number("y", 0.0)?,
            width: optional_f32(p, "width")?,
        },
        WidgetKind::Tabs => WidgetSpec::Tabs {
            labels: string_list(p.get("labels"), "labels")?,
            active: usize::try_from(prop_u64(p, "active", 0)?)
                .map_err(|_| invalid("active", "exceeds usize"))?,
            width: optional_f32(p, "width")?,
            height: optional_f32(p, "height")?,
        },
        WidgetKind::Tooltip => WidgetSpec::Tooltip {
            label: text("label")?,
            delay_ms: prop_u64(p, "delay_ms", 450)?,
        },
        WidgetKind::Image => WidgetSpec::Image {
            data: image
                .ok_or_else(|| BridgeError::new("invalid_handle", "image handle is required"))?,
            label: text("label")?,
            width: optional_f32(p, "width")?,
            height: optional_f32(p, "height")?,
        },
        WidgetKind::Canvas => WidgetSpec::Canvas {
            width: prop_f32(p, "width", 320.0)?,
            height: prop_f32(p, "height", 200.0)?,
            label: prop_text(p, "label", "Canvas")?,
            painter: canvas
                .ok_or_else(|| BridgeError::new("internal_failure", "canvas model is missing"))?
                .painter(),
        },
        WidgetKind::ComboBox => WidgetSpec::ComboBox {
            options: string_list(p.get("options"), "options")?,
            selected: usize::try_from(prop_u64(p, "selected", 0)?)
                .map_err(|_| invalid("selected", "exceeds usize"))?,
            width: optional_f32(p, "width")?,
        },
    })
}

fn widget_patch(
    kind: WidgetKind,
    p: &serde_json::Map<String, Value>,
    old: &BTreeMap<String, Value>,
    image: Option<std::sync::Arc<grafikr_core::ImageData>>,
) -> Result<WidgetPatch, BridgeError> {
    let text = |key| {
        p.get(key)
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| invalid(key, "must be a string"))
            })
            .transpose()
    };
    let number = |key| optional_f32(p, key);
    let boolean = |key| {
        p.get(key)
            .map(|v| v.as_bool().ok_or_else(|| invalid(key, "must be a boolean")))
            .transpose()
    };
    Ok(match kind {
        WidgetKind::Column | WidgetKind::Row | WidgetKind::Grid | WidgetKind::Stack => {
            let mut merged: serde_json::Map<String, Value> =
                old.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            merged.extend(p.clone());
            let layout_kind = match kind {
                WidgetKind::Column => LayoutKind::Column,
                WidgetKind::Row => LayoutKind::Row,
                WidgetKind::Grid => LayoutKind::Grid(
                    u16::try_from(prop_u64(&merged, "columns", 1)?)
                        .map_err(|_| invalid("columns", "exceeds u16"))?,
                ),
                _ => LayoutKind::Stack,
            };
            let style = layout_style(layout_kind, &merged)?;
            validate_bridge_style(&style)?;
            WidgetPatch::Layout(style)
        }
        WidgetKind::Text => WidgetPatch::Text {
            text: text("text")?,
            size: number("size")?,
            color: optional_color(p, "color")?,
        },
        WidgetKind::Button => {
            let variant = text("variant")?
                .map(|value| match value.as_str() {
                    "primary" | "icon" => Ok(ButtonVariant::Primary),
                    "secondary" => Ok(ButtonVariant::Secondary),
                    _ => Err(BridgeError::new("invalid_value", "unknown button variant")
                        .field("variant")),
                })
                .transpose()?;
            WidgetPatch::Button {
                label: text("label")?,
                variant,
                icon: text("variant")?.map(|v| v == "icon"),
                disabled: boolean("disabled")?,
            }
        }
        WidgetKind::TextField => {
            if p.contains_key("multiline") || p.contains_key("password") {
                return Err(BridgeError::new(
                    "immutable_property",
                    "multiline and password are creation-only",
                ));
            }
            WidgetPatch::TextField {
                text: text("text")?,
                width: number("width")?,
            }
        }
        WidgetKind::Checkbox => WidgetPatch::Checkbox {
            checked: boolean("checked")?.ok_or_else(|| invalid("checked", "is required"))?,
        },
        WidgetKind::Radio => WidgetPatch::Radio {
            selected: prop_u64(p, "selected", 0)?,
        },
        WidgetKind::Switch => WidgetPatch::Switch {
            checked: boolean("checked")?.ok_or_else(|| invalid("checked", "is required"))?,
        },
        WidgetKind::Slider => {
            if p.keys()
                .any(|key| matches!(key.as_str(), "min" | "max" | "step"))
            {
                return Err(BridgeError::new(
                    "immutable_property",
                    "slider range is creation-only",
                ));
            }
            WidgetPatch::Slider {
                value: number("value")?.ok_or_else(|| invalid("value", "is required"))?,
            }
        }
        WidgetKind::ProgressBar => WidgetPatch::ProgressBar {
            value: number("value")?.ok_or_else(|| invalid("value", "is required"))?,
        },
        WidgetKind::Spinner => WidgetPatch::Spinner {
            size: number("size")?,
            label: text("label")?,
        },
        WidgetKind::ScrollView => WidgetPatch::ScrollView {
            width: number("width")?,
            height: number("height")?,
            grow: boolean("grow")?,
        },
        WidgetKind::Modal => WidgetPatch::Modal {
            open: boolean("open")?,
            width: number("width")?,
        },
        WidgetKind::Popup => WidgetPatch::Popup {
            open: boolean("open")?,
            x: number("x")?,
            y: number("y")?,
            width: number("width")?,
        },
        WidgetKind::Tabs => {
            if p.contains_key("labels") {
                return Err(BridgeError::new(
                    "immutable_property",
                    "tab labels cannot be changed after node creation",
                )
                .field("labels"));
            }
            WidgetPatch::Tabs {
                active: p
                    .get("active")
                    .map(|_| {
                        usize::try_from(prop_u64(p, "active", 0)?)
                            .map_err(|_| invalid("active", "exceeds usize"))
                    })
                    .transpose()?,
                width: number("width")?,
                height: number("height")?,
            }
        }
        WidgetKind::Tooltip => WidgetPatch::Tooltip {
            label: text("label")?,
            delay_ms: p
                .get("delay_ms")
                .map(|_| prop_u64(p, "delay_ms", 450))
                .transpose()?,
        },
        WidgetKind::Image => WidgetPatch::Image {
            data: image,
            label: text("label")?,
            width: number("width")?,
            height: number("height")?,
        },
        WidgetKind::TreeView => {
            let parsed_roots = p
                .get("roots")
                .map(|value| parse_tree_roots(Some(value)))
                .transpose()?;
            let roots = parsed_roots.as_ref().map(|(roots, _)| roots.clone());
            let selected = text("selected")?;
            if let Some(key) = &selected {
                let exists = if let Some((_, keys)) = &parsed_roots {
                    keys.iter().any(|candidate| candidate == key)
                } else {
                    let (_, keys) = parse_tree_roots(old.get("roots"))?;
                    keys.iter().any(|candidate| candidate == key)
                };
                if !exists {
                    return Err(BridgeError::new("invalid_value", "tree key does not exist")
                        .field("selected"));
                }
            }
            WidgetPatch::TreeView {
                roots,
                selected,
                width: number("width")?,
                height: number("height")?,
            }
        }
        WidgetKind::Canvas => {
            if p.contains_key("commands") {
                return Err(BridgeError::new(
                    "immutable_property",
                    "canvas commands must be changed with canvas.update",
                ));
            }
            WidgetPatch::Canvas {
                label: text("label")?,
                width: number("width")?,
                height: number("height")?,
            }
        }
        WidgetKind::ComboBox => {
            if p.contains_key("options") {
                return Err(BridgeError::new(
                    "immutable_property",
                    "combo box options cannot be changed after node creation",
                ));
            }
            WidgetPatch::ComboBox {
                selected: p
                    .get("selected")
                    .map(|_| {
                        usize::try_from(prop_u64(p, "selected", 0)?)
                            .map_err(|_| invalid("selected", "exceeds usize"))
                    })
                    .transpose()?,
                width: number("width")?,
            }
        }
        _ => {
            return Err(BridgeError::new(
                "unsupported_capability",
                "widget adapter is not available yet",
            ));
        }
    })
}

fn layout_style(
    kind: LayoutKind,
    p: &serde_json::Map<String, Value>,
) -> Result<Style, BridgeError> {
    let mut style = match kind {
        LayoutKind::Row => Style::row(),
        _ => Style::default(),
    };
    style.gap = prop_f32(p, "gap", 0.0)?;
    let padding = prop_f32(p, "padding", 0.0)?;
    style.padding = EdgeInsets::all(padding);
    style.width = optional_f32(p, "width")?;
    style.height = optional_f32(p, "height")?;
    style.background = optional_color(p, "background")?;
    style.radius = prop_f32(p, "radius", 0.0)?;
    if let Some(columns) = p.get("columns") {
        style.grid_columns = Some(
            u16::try_from(
                columns
                    .as_u64()
                    .ok_or_else(|| invalid("columns", "must be unsigned integer"))?,
            )
            .map_err(|_| invalid("columns", "exceeds u16"))?,
        );
    }
    if let Some(transform) = p.get("transform") {
        let transform = transform
            .as_object()
            .ok_or_else(|| invalid("transform", "must be an object"))?;
        style.transform.translate.x = prop_f32(transform, "x", 0.0)?;
        style.transform.translate.y = prop_f32(transform, "y", 0.0)?;
        style.transform.scale = prop_f32(transform, "scale", 1.0)?;
    }
    Ok(style)
}

fn validate_bridge_style(style: &Style) -> Result<(), BridgeError> {
    let values = [
        style.gap,
        style.padding.top,
        style.padding.right,
        style.padding.bottom,
        style.padding.left,
        style.radius,
        style.transform.scale,
        style.transform.translate.x,
        style.transform.translate.y,
    ];
    if values.iter().any(|v| !v.is_finite())
        || style.gap < 0.0
        || style.radius < 0.0
        || style.transform.scale <= 0.0
        || [style.width, style.height]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || v <= 0.0)
        || style.grid_columns == Some(0)
    {
        return Err(BridgeError::new(
            "invalid_value",
            "layout values are outside their supported ranges",
        ));
    }
    Ok(())
}

fn prop_text(
    p: &serde_json::Map<String, Value>,
    key: &str,
    default: &str,
) -> Result<String, BridgeError> {
    p.get(key)
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid(key, "must be a string"))
        })
        .transpose()
        .map(|v| v.unwrap_or_else(|| default.to_owned()))
}
fn prop_bool(
    p: &serde_json::Map<String, Value>,
    key: &str,
    default: bool,
) -> Result<bool, BridgeError> {
    p.get(key)
        .map(|v| v.as_bool().ok_or_else(|| invalid(key, "must be a boolean")))
        .transpose()
        .map(|v| v.unwrap_or(default))
}
fn prop_f32(
    p: &serde_json::Map<String, Value>,
    key: &str,
    default: f32,
) -> Result<f32, BridgeError> {
    optional_f32(p, key).map(|v| v.unwrap_or(default))
}
fn optional_f32(p: &serde_json::Map<String, Value>, key: &str) -> Result<Option<f32>, BridgeError> {
    p.get(key)
        .map(|v| {
            let n = v
                .as_f64()
                .filter(|n| n.is_finite() && *n <= f32::MAX as f64 && *n >= f32::MIN as f64)
                .ok_or_else(|| invalid(key, "must be a finite f32"))?;
            Ok(n as f32)
        })
        .transpose()
}
fn prop_u64(
    p: &serde_json::Map<String, Value>,
    key: &str,
    default: u64,
) -> Result<u64, BridgeError> {
    p.get(key)
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| invalid(key, "must be unsigned integer"))
        })
        .transpose()
        .map(|v| v.unwrap_or(default))
}
fn optional_color(
    p: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<Color>, BridgeError> {
    p.get(key).map(|v| parse_color(Some(v), key)).transpose()
}
fn invalid(field: &str, message: &str) -> BridgeError {
    BridgeError::new("invalid_type", message).field(field)
}
fn string<'a>(value: Option<&'a Value>, field: &str) -> Result<&'a str, BridgeError> {
    value
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(field, "must be a string"))
}
fn number(value: Option<&Value>, field: &str) -> Result<f64, BridgeError> {
    value
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| invalid(field, "must be a finite number"))
}
fn f32_number(value: Option<&Value>, field: &str) -> Result<f32, BridgeError> {
    let value = number(value, field)?;
    if value > f32::MAX as f64 || value < f32::MIN as f64 {
        return Err(BridgeError::new("invalid_value", "number is outside f32 range").field(field));
    }
    Ok(value as f32)
}
fn finite_f32(value: f64, field: &str) -> Result<f32, BridgeError> {
    if value.is_finite() && value <= f32::MAX as f64 && value >= f32::MIN as f64 {
        Ok(value as f32)
    } else {
        Err(BridgeError::new("invalid_value", "number is outside f32 range").field(field))
    }
}
fn parse_rect(value: Option<&Value>) -> Result<Rect, BridgeError> {
    let fields = value
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("rect", "must be an object"))?;
    Ok(Rect::new(
        f32_number(fields.get("x"), "rect.x")?,
        f32_number(fields.get("y"), "rect.y")?,
        f32_number(fields.get("width"), "rect.width")?,
        f32_number(fields.get("height"), "rect.height")?,
    ))
}
fn parse_color(value: Option<&Value>, field: &str) -> Result<Color, BridgeError> {
    if let Some(values) = value.and_then(Value::as_array) {
        if values.len() != 4 {
            return Err(invalid(field, "RGBA color requires four byte components"));
        }
        let mut rgba = [0; 4];
        for (index, value) in values.iter().enumerate() {
            rgba[index] = u8::try_from(
                value
                    .as_u64()
                    .ok_or_else(|| invalid(field, "color components must be bytes"))?,
            )
            .map_err(|_| invalid(field, "color components must be bytes"))?;
        }
        return Ok(Color::rgba(rgba[0], rgba[1], rgba[2], rgba[3]));
    }
    let hex = string(value, field)?
        .strip_prefix('#')
        .ok_or_else(|| invalid(field, "expected #RRGGBB, #RRGGBBAA, or an RGBA array"))?;
    if !matches!(hex.len(), 6 | 8) {
        return Err(invalid(field, "invalid hexadecimal color length"));
    }
    let value =
        u32::from_str_radix(hex, 16).map_err(|_| invalid(field, "invalid hexadecimal color"))?;
    Ok(if hex.len() == 6 {
        Color::hex(value)
    } else {
        Color::rgba(
            (value >> 24) as u8,
            (value >> 16) as u8,
            (value >> 8) as u8,
            value as u8,
        )
    })
}
fn uint(value: Option<&Value>, field: &str) -> Result<u64, BridgeError> {
    value
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid(field, "must be a nonnegative integer"))
}
fn wide_uint(value: Option<&Value>, field: &str) -> Result<u64, BridgeError> {
    value
        .and_then(|value| match value {
            Value::String(value) => value.parse().ok(),
            Value::Number(value) => value.as_u64(),
            _ => None,
        })
        .ok_or_else(|| invalid(field, "must be an unsigned integer or decimal string"))
}
fn handle(value: Option<&Value>, field: &str) -> Result<Handle, BridgeError> {
    string(value, field)?
        .parse::<u64>()
        .map(Handle)
        .map_err(|_| BridgeError::new("invalid_value", "invalid handle").field(field))
}
fn object(
    value: Option<&Value>,
    field: &str,
) -> Result<serde_json::Map<String, Value>, BridgeError> {
    value
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| invalid(field, "must be an object"))
}
fn string_list(value: Option<&Value>, field: &str) -> Result<Vec<String>, BridgeError> {
    value
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(field, "must be an array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid(field, "items must be strings"))
        })
        .collect()
}
fn grid_columns(value: Option<&Value>) -> Result<Vec<GridColumn>, BridgeError> {
    value
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("columns", "must be an array"))?
        .iter()
        .map(|column| {
            let fields = column
                .as_object()
                .ok_or_else(|| invalid("columns", "each column must be an object"))?;
            let title = string(fields.get("title"), "columns.title")?;
            let width = f32_number(fields.get("width"), "columns.width")?;
            if width <= 0.0 {
                return Err(
                    BridgeError::new("invalid_value", "column width must be positive")
                        .field("columns.width"),
                );
            }
            Ok(GridColumn::new(title, width))
        })
        .collect()
}
fn parse_tree_roots(
    value: Option<&Value>,
) -> Result<(Vec<grafikr_widgets::TreeNode>, Vec<String>), BridgeError> {
    let roots: Vec<TreeItem> = serde_json::from_value(
        value
            .cloned()
            .ok_or_else(|| invalid("roots", "must be an array"))?,
    )
    .map_err(|_| invalid("roots", "tree nodes have an invalid shape"))?;
    let mut snapshot = TreeSnapshot::default();
    snapshot.replace(roots.clone())?;
    fn convert(item: TreeItem, keys: &mut Vec<String>) -> grafikr_widgets::TreeNode {
        keys.push(item.key.clone());
        let mut node = grafikr_widgets::TreeNode::with_key(item.key, item.label);
        for child in item.children {
            node = node.child(convert(child, keys));
        }
        node
    }
    let mut keys = Vec::with_capacity(snapshot.item_count());
    let roots = roots
        .into_iter()
        .map(|item| convert(item, &mut keys))
        .collect();
    Ok((roots, keys))
}
fn scalar(value: Option<&Value>) -> Result<Scalar, BridgeError> {
    let value = value
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("value", "must be a tagged scalar object"))?;
    let tag = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("value.type", "must be a scalar type tag"))?;
    let raw = value
        .get("value")
        .ok_or_else(|| invalid("value.value", "is required"))?;
    match tag {
        "bool" => raw.as_bool().map(Scalar::Bool),
        "i64" => raw.as_str().and_then(|s| s.parse().ok()).map(Scalar::I64),
        "f64" => raw.as_f64().filter(|n| n.is_finite()).map(Scalar::F64),
        "string" => raw.as_str().map(|s| Scalar::Text(s.to_owned())),
        _ => None,
    }
    .ok_or_else(|| BridgeError::new("invalid_value", "tagged scalar does not match its value"))
}
fn scalar_json(value: &Scalar) -> Value {
    match value {
        Scalar::Bool(v) => json!({"type":"bool", "value":v}),
        Scalar::I64(v) => json!({"type":"i64", "value":v.to_string()}),
        Scalar::F64(v) => json!({"type":"f64", "value":v}),
        Scalar::Text(v) => json!({"type":"string", "value":v}),
    }
}

fn cell_text(value: Value) -> String {
    match value {
        Value::String(value) => value,
        Value::Null => String::new(),
        value => value.to_string(),
    }
}

fn scalar_property(value: &Scalar) -> Result<Value, BridgeError> {
    match value {
        Scalar::Bool(value) => Ok(json!(value)),
        Scalar::I64(value) => Ok(json!(value)),
        Scalar::F64(value) if value.is_finite() => Ok(json!(value)),
        Scalar::Text(value) => Ok(json!(value)),
        Scalar::F64(_) => Err(BridgeError::new(
            "invalid_value",
            "state numbers must be finite",
        )),
    }
}

fn radio_group_selection(value: &Scalar) -> Result<u64, BridgeError> {
    match value {
        Scalar::I64(value) if *value >= 0 => {
            u64::try_from(*value).map_err(|_| invalid("group", "radio selection is out of range"))
        }
        _ => Err(invalid(
            "group",
            "radio group state must contain a nonnegative i64 selection",
        )),
    }
}

#[cfg(test)]
mod subscription_tests {
    use super::*;
    use crate::session::Session;
    use std::time::Duration;

    #[test]
    fn dynamic_events_are_published_only_while_subscribed() {
        let (control, seed) = Session::channel();
        drop(seed);
        let mut dispatcher = Dispatcher::new();
        dispatcher.set_publisher(control.clone());
        let source = Handle(44);
        let sink = dispatcher.event_sink(source);
        sink(DynamicEvent::Clicked {
            node: NodeId::new(0, 0),
        });
        assert!(control.next(Duration::ZERO).unwrap().is_none());

        dispatcher
            .subscriptions
            .borrow_mut()
            .insert(Handle(45), (source, "click".to_owned()));
        sink(DynamicEvent::Clicked {
            node: NodeId::new(0, 0),
        });
        let event = control.next(Duration::ZERO).unwrap().unwrap();
        let event: Value = serde_json::from_slice(&event).unwrap();
        assert_eq!(event["event"], "click");

        dispatcher.subscriptions.borrow_mut().clear();
        sink(DynamicEvent::Clicked {
            node: NodeId::new(0, 0),
        });
        assert!(control.next(Duration::ZERO).unwrap().is_none());
    }
}
