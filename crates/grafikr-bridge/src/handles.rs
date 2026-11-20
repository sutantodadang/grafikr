use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::protocol::BridgeError;

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
pub const MAX_OBJECTS_PER_KIND: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Window,
    Node,
    State,
    Binding,
    Subscription,
    Image,
    Upload,
    Collection,
}

#[derive(Debug)]
pub enum Object {
    Marker,
    State(crate::state::Scalar),
    Node,
    Window,
    Bytes(Vec<u8>),
}

pub struct Registry {
    entries: HashMap<Handle, (ObjectKind, Object)>,
    kind_counts: HashMap<ObjectKind, usize>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            kind_counts: HashMap::new(),
        }
    }

    pub fn insert(&mut self, kind: ObjectKind, object: Object) -> Result<Handle, BridgeError> {
        let count = self.kind_counts.get(&kind).copied().unwrap_or_default();
        if count >= MAX_OBJECTS_PER_KIND {
            return Err(BridgeError::new(
                "quota_exceeded",
                "object kind handle quota exceeded",
            ));
        }
        let raw = NEXT_TOKEN
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| BridgeError::new("quota_exceeded", "global handle space is exhausted"))?;
        let handle = Handle(raw);
        self.entries.insert(handle, (kind, object));
        self.kind_counts.insert(kind, count + 1);
        Ok(handle)
    }

    pub fn get(&self, handle: Handle, kind: ObjectKind) -> Result<&Object, BridgeError> {
        match self.entries.get(&handle) {
            Some((actual, object)) if *actual == kind => Ok(object),
            Some(_) => Err(BridgeError::new(
                "wrong_handle_kind",
                "handle refers to another object kind",
            )),
            None => Err(BridgeError::new(
                "invalid_handle",
                "handle is stale or belongs to another session",
            )),
        }
    }

    pub fn get_mut(
        &mut self,
        handle: Handle,
        kind: ObjectKind,
    ) -> Result<&mut Object, BridgeError> {
        match self.entries.get_mut(&handle) {
            Some((actual, object)) if *actual == kind => Ok(object),
            Some(_) => Err(BridgeError::new(
                "wrong_handle_kind",
                "handle refers to another object kind",
            )),
            None => Err(BridgeError::new(
                "invalid_handle",
                "handle is stale or belongs to another session",
            )),
        }
    }

    pub fn remove(&mut self, handle: Handle, kind: ObjectKind) -> Result<Object, BridgeError> {
        self.get(handle, kind)?;
        let object = self
            .entries
            .remove(&handle)
            .expect("validated entry exists")
            .1;
        let count = self
            .kind_counts
            .get_mut(&kind)
            .expect("registered object kind has a count");
        *count -= 1;
        if *count == 0 {
            self.kind_counts.remove(&kind);
        }
        Ok(object)
    }
}
