use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;

use crate::handles::{Handle, Object, ObjectKind, Registry};
use crate::protocol::BridgeError;

pub const MAX_WINDOWS: usize = 16;
pub const MAX_NODES: usize = 100_000;
pub const MAX_CANONICAL_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug)]
pub struct NodeRecord {
    pub window: Handle,
    pub widget: String,
    pub parent: Option<Handle>,
    pub children: Vec<Handle>,
    pub properties: BTreeMap<String, Value>,
}

#[derive(Debug)]
pub struct WindowRecord {
    pub title: String,
    pub size: (f64, f64),
    pub theme: String,
}

pub struct Document {
    registry: Registry,
    windows: HashMap<Handle, WindowRecord>,
    nodes: HashMap<Handle, NodeRecord>,
    roots: HashMap<Handle, (Handle, Handle)>,
    payload_bytes: usize,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        Self {
            registry: Registry::new(),
            windows: HashMap::new(),
            nodes: HashMap::new(),
            roots: HashMap::new(),
            payload_bytes: 0,
        }
    }
    pub fn create_window(
        &mut self,
        title: String,
        width: f64,
        height: f64,
        theme: String,
    ) -> Result<Handle, BridgeError> {
        if self.windows.len() >= MAX_WINDOWS {
            return Err(BridgeError::new("quota_exceeded", "window quota exceeded"));
        }
        if self.nodes.len().saturating_add(2) > MAX_NODES {
            return Err(BridgeError::new("quota_exceeded", "node quota exceeded"));
        }
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err(BridgeError::new(
                "invalid_value",
                "window dimensions must be positive and finite",
            ));
        }
        let handle = self.registry.insert(ObjectKind::Window, Object::Window)?;
        let root = match self.registry.insert(ObjectKind::Node, Object::Node) {
            Ok(id) => id,
            Err(error) => {
                let _ = self.registry.remove(handle, ObjectKind::Window);
                return Err(error);
            }
        };
        let parking = match self.registry.insert(ObjectKind::Node, Object::Node) {
            Ok(id) => id,
            Err(error) => {
                let _ = self.registry.remove(root, ObjectKind::Node);
                let _ = self.registry.remove(handle, ObjectKind::Window);
                return Err(error);
            }
        };
        self.windows.insert(
            handle,
            WindowRecord {
                title,
                size: (width, height),
                theme,
            },
        );
        self.nodes.insert(
            root,
            NodeRecord {
                window: handle,
                widget: "__window_root__".into(),
                parent: None,
                children: vec![parking],
                properties: BTreeMap::new(),
            },
        );
        self.nodes.insert(
            parking,
            NodeRecord {
                window: handle,
                widget: "__parking__".into(),
                parent: Some(root),
                children: Vec::new(),
                properties: BTreeMap::new(),
            },
        );
        self.roots.insert(handle, (root, parking));
        Ok(handle)
    }
    pub fn root_node(&self, window: Handle) -> Result<Handle, BridgeError> {
        self.roots
            .get(&window)
            .map(|roots| roots.0)
            .ok_or_else(|| BridgeError::new("invalid_handle", "window handle is stale"))
    }
    pub fn parking_node(&self, window: Handle) -> Result<Handle, BridgeError> {
        self.roots
            .get(&window)
            .map(|roots| roots.1)
            .ok_or_else(|| BridgeError::new("invalid_handle", "window handle is stale"))
    }
    pub fn create_node(
        &mut self,
        window: Handle,
        widget: String,
        properties: BTreeMap<String, Value>,
    ) -> Result<Handle, BridgeError> {
        self.window(window)?;
        if self.nodes.len() >= MAX_NODES {
            return Err(BridgeError::new("quota_exceeded", "node quota exceeded"));
        }
        let bytes = canonical_size(&properties)?;
        self.reserve(bytes)?;
        let handle = match self.registry.insert(ObjectKind::Node, Object::Node) {
            Ok(id) => id,
            Err(error) => {
                self.payload_bytes -= bytes;
                return Err(error);
            }
        };
        self.nodes.insert(
            handle,
            NodeRecord {
                window,
                widget,
                parent: Some(self.parking_node(window)?),
                children: Vec::new(),
                properties,
            },
        );
        self.node_mut(self.parking_node(window)?)?
            .children
            .push(handle);
        Ok(handle)
    }
    pub fn update_window(
        &mut self,
        id: Handle,
        title: Option<String>,
        size: Option<(f64, f64)>,
        theme: Option<String>,
    ) -> Result<(), BridgeError> {
        let window = self.windows.get_mut(&id).ok_or_else(|| {
            BridgeError::new(
                "invalid_handle",
                "window handle is stale or belongs to another session",
            )
        })?;
        if let Some((width, height)) = size {
            if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
                return Err(BridgeError::new(
                    "invalid_value",
                    "window dimensions must be positive and finite",
                ));
            }
            window.size = (width, height);
        }
        if let Some(title) = title {
            window.title = title;
        }
        if let Some(theme) = theme {
            window.theme = theme;
        }
        Ok(())
    }
    pub fn update_properties(
        &mut self,
        node: Handle,
        patch: BTreeMap<String, Value>,
    ) -> Result<(), BridgeError> {
        self.validate_property_update(node, &patch)?;
        let target = self.node(node)?;
        let before = canonical_size(&target.properties)?;
        let mut merged = target.properties.clone();
        merged.extend(patch);
        let after = canonical_size(&merged)?;
        self.reserve(after.saturating_sub(before))?;
        if before > after {
            self.payload_bytes -= before - after;
        }
        self.node_mut(node)?.properties = merged;
        Ok(())
    }
    pub fn validate_property_update(
        &self,
        node: Handle,
        patch: &BTreeMap<String, Value>,
    ) -> Result<(), BridgeError> {
        let target = self.node(node)?;
        let before = canonical_size(&target.properties)?;
        let mut merged = target.properties.clone();
        merged.extend(patch.clone());
        let after = canonical_size(&merged)?;
        let growth = after.saturating_sub(before);
        if self.payload_bytes.saturating_add(growth) > MAX_CANONICAL_BYTES {
            return Err(BridgeError::new(
                "quota_exceeded",
                "session payload quota exceeded",
            ));
        }
        Ok(())
    }
    pub fn attach(
        &mut self,
        node: Handle,
        parent: Handle,
        index: usize,
    ) -> Result<(), BridgeError> {
        let child = self.node(node)?;
        let parent_record = self.node(parent)?;
        if child.window != parent_record.window {
            return Err(BridgeError::new(
                "cross_window_move",
                "nodes must remain within their window",
            ));
        }
        if node == parent || self.is_descendant(parent, node) {
            return Err(BridgeError::new(
                "tree_cycle",
                "attachment would create a cycle",
            ));
        }
        let old_parent = child.parent;
        let adjusted_len =
            self.node(parent)?.children.len() - usize::from(old_parent == Some(parent));
        if index > adjusted_len {
            return Err(
                BridgeError::new("invalid_value", "child index is out of range").field("index"),
            );
        }
        if let Some(old) = old_parent {
            self.node_mut(old)?.children.retain(|id| *id != node);
        }
        let record = self.node_mut(parent)?;
        record.children.insert(index, node);
        self.node_mut(node)?.parent = Some(parent);
        Ok(())
    }
    pub fn detach(&mut self, node: Handle) -> Result<(), BridgeError> {
        let record = self.node(node)?;
        if record.widget.starts_with("__") {
            return Err(BridgeError::new(
                "invalid_handle",
                "internal window nodes cannot be detached",
            ));
        }
        let parent = record.parent;
        let parking = self.parking_node(record.window)?;
        if let Some(parent) = parent {
            self.node_mut(parent)?.children.retain(|id| *id != node);
        }
        self.node_mut(parking)?.children.push(node);
        self.node_mut(node)?.parent = Some(parking);
        Ok(())
    }
    pub fn replace_children(
        &mut self,
        parent: Handle,
        children: &[Handle],
    ) -> Result<(), BridgeError> {
        let parent_window = self.node(parent)?.window;
        let mut unique = HashSet::new();
        for child in children {
            if !unique.insert(*child) {
                return Err(BridgeError::new(
                    "duplicate_child",
                    "child occurs more than once",
                ));
            }
            let record = self.node(*child)?;
            if record.window != parent_window
                || *child == parent
                || self.is_descendant(parent, *child)
            {
                return Err(BridgeError::new(
                    "invalid_tree",
                    "children must belong to this window and not create a cycle",
                ));
            }
        }
        let old_children = self.node(parent)?.children.clone();
        let parking = self.parking_node(parent_window)?;
        for old in old_children {
            if !children.contains(&old) {
                self.node_mut(old)?.parent = Some(parking);
                self.node_mut(parking)?.children.push(old);
            }
        }
        for child in children {
            self.detach(*child)?;
            self.node_mut(*child)?.parent = Some(parent);
        }
        self.node_mut(parent)?.children = children.to_vec();
        Ok(())
    }
    pub fn destroy_node(&mut self, node: Handle) -> Result<(), BridgeError> {
        self.node(node)?;
        if self.node(node)?.widget.starts_with("__") {
            return Err(BridgeError::new(
                "invalid_handle",
                "internal window nodes cannot be destroyed",
            ));
        }
        self.destroy_subtree(node)
    }
    pub fn subtree(&self, node: Handle) -> Result<Vec<Handle>, BridgeError> {
        self.node(node)?;
        let mut pending = vec![node];
        let mut result = Vec::new();
        while let Some(id) = pending.pop() {
            let record = self.node(id)?;
            pending.extend(record.children.iter().copied());
            result.push(id);
        }
        Ok(result)
    }
    fn destroy_subtree(&mut self, node: Handle) -> Result<(), BridgeError> {
        let mut stack = vec![node];
        let mut removal = Vec::new();
        while let Some(id) = stack.pop() {
            let record = self.node(id)?;
            stack.extend(record.children.iter().copied());
            removal.push(id);
        }
        if let Some(parent) = self.node(node)?.parent {
            self.node_mut(parent)?.children.retain(|id| *id != node);
        }
        for id in removal {
            let record = self.nodes.remove(&id).expect("node was validated");
            if !record.widget.starts_with("__") {
                self.payload_bytes -= canonical_size(&record.properties)?;
            }
            self.registry.remove(id, ObjectKind::Node)?;
        }
        Ok(())
    }
    pub fn close_window(&mut self, window: Handle) -> Result<(), BridgeError> {
        self.window(window)?;
        let (root, _) = self
            .roots
            .remove(&window)
            .ok_or_else(|| BridgeError::new("invalid_handle", "window has no root"))?;
        self.destroy_subtree(root)?;
        self.windows.remove(&window);
        self.registry.remove(window, ObjectKind::Window)?;
        Ok(())
    }
    pub fn node(&self, id: Handle) -> Result<&NodeRecord, BridgeError> {
        self.nodes.get(&id).ok_or_else(|| {
            BridgeError::new(
                "invalid_handle",
                "node handle is stale or belongs to another session",
            )
        })
    }
    pub fn node_mut(&mut self, id: Handle) -> Result<&mut NodeRecord, BridgeError> {
        self.nodes.get_mut(&id).ok_or_else(|| {
            BridgeError::new(
                "invalid_handle",
                "node handle is stale or belongs to another session",
            )
        })
    }
    pub fn window(&self, id: Handle) -> Result<&WindowRecord, BridgeError> {
        self.windows.get(&id).ok_or_else(|| {
            BridgeError::new(
                "invalid_handle",
                "window handle is stale or belongs to another session",
            )
        })
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn window_nodes(&self, window: Handle) -> Vec<Handle> {
        self.nodes
            .iter()
            .filter_map(|(id, node)| (node.window == window).then_some(*id))
            .collect()
    }
    pub fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }
    fn reserve(&mut self, bytes: usize) -> Result<(), BridgeError> {
        if self.payload_bytes.saturating_add(bytes) > MAX_CANONICAL_BYTES {
            return Err(BridgeError::new(
                "quota_exceeded",
                "session payload quota exceeded",
            ));
        }
        self.payload_bytes += bytes;
        Ok(())
    }
    fn is_descendant(&self, node: Handle, ancestor: Handle) -> bool {
        let mut current = self.nodes.get(&node).and_then(|n| n.parent);
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            current = self.nodes.get(&id).and_then(|n| n.parent);
        }
        false
    }
}

fn canonical_size(value: &impl serde::Serialize) -> Result<usize, BridgeError> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|e| BridgeError::new("invalid_value", e.to_string()))
}
