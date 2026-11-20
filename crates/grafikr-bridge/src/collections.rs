use std::cell::{RefCell, RefMut};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::handles::Handle;
use crate::protocol::BridgeError;

pub const MAX_REQUEST_ROWS: u32 = 512;
pub const MAX_CACHE_ROWS_PER_NODE: usize = 2_048;
pub const MAX_CACHE_BYTES_PER_NODE: usize = 8 * 1024 * 1024;
pub const MAX_CACHE_BYTES_PER_SESSION: usize = 64 * 1024 * 1024;
pub const MAX_TREE_ITEMS: usize = 50_000;
pub const MAX_TREE_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRequest {
    pub node: Handle,
    pub revision: u64,
    pub request_id: String,
    pub start: u32,
    pub count: u32,
}

#[derive(Debug)]
struct RowEntry {
    data: Vec<Value>,
    bytes: usize,
    last_used: u64,
}

#[derive(Debug)]
struct Collection {
    revision: u64,
    row_count: u64,
    row_height: f32,
    columns: Vec<String>,
    rows: BTreeMap<u32, RowEntry>,
    active: Option<DataRequest>,
    bytes: usize,
}

#[derive(Clone, Default)]
pub struct Collections(Rc<RefCell<CollectionStore>>);

#[derive(Default)]
struct CollectionStore {
    models: HashMap<Handle, Collection>,
    total_bytes: usize,
    clock: u64,
}

impl Collections {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn create(
        &self,
        node: Handle,
        row_count: u64,
        row_height: f32,
        columns: Vec<String>,
    ) -> Result<(), BridgeError> {
        self.0
            .borrow_mut()
            .create(node, row_count, row_height, columns)
    }
    pub fn update(
        &self,
        node: Handle,
        row_count: u64,
        row_height: f32,
        columns: Vec<String>,
    ) -> Result<u64, BridgeError> {
        self.0
            .borrow_mut()
            .update(node, row_count, row_height, columns)
    }
    pub fn request_visible(
        &self,
        node: Handle,
        start: u64,
        visible: u32,
        request_id: impl Into<String>,
    ) -> Result<Option<DataRequest>, BridgeError> {
        self.0
            .borrow_mut()
            .request_visible(node, start, visible, request_id)
    }
    pub fn provide(&self, request: &DataRequest, rows: Vec<Vec<Value>>) -> Result<(), BridgeError> {
        self.0.borrow_mut().provide(request, rows)
    }
    pub fn provide_active(
        &self,
        node: Handle,
        revision: u64,
        request_id: &str,
        start: u32,
        rows: Vec<Vec<Value>>,
    ) -> Result<(), BridgeError> {
        self.0
            .borrow_mut()
            .provide_active(node, revision, request_id, start, rows)
    }
    pub fn cancel(&self, request: &DataRequest) {
        self.0.borrow_mut().cancel(request);
    }
    pub fn row(&self, node: Handle, index: u32) -> Option<RefMut<'_, [Value]>> {
        let mut store = self.0.borrow_mut();
        if !store.touch_row(node, index) {
            return None;
        }
        RefMut::filter_map(store, |store| {
            store
                .models
                .get_mut(&node)?
                .rows
                .get_mut(&index)
                .map(|row| row.data.as_mut_slice())
        })
        .ok()
    }
    pub fn row_cloned(&self, node: Handle, index: u32) -> Option<Vec<Value>> {
        self.row(node, index).map(|row| row.to_vec())
    }
    pub fn set_revision(&self, node: Handle, revision: u64) -> Result<(), BridgeError> {
        self.0.borrow_mut().set_revision(node, revision)
    }
    pub fn remove(&self, node: Handle) {
        self.0.borrow_mut().remove(node);
    }
    pub fn cache_rows(&self, node: Handle) -> usize {
        self.0.borrow().cache_rows(node)
    }
    pub fn cache_bytes(&self, node: Handle) -> usize {
        self.0.borrow().cache_bytes(node)
    }
    pub fn total_bytes(&self) -> usize {
        self.0.borrow().total_bytes()
    }
}

impl CollectionStore {
    pub fn create(
        &mut self,
        node: Handle,
        row_count: u64,
        row_height: f32,
        columns: Vec<String>,
    ) -> Result<(), BridgeError> {
        validate_row_count(row_count)?;
        if !row_height.is_finite() || row_height <= 0.0 {
            return Err(BridgeError::new(
                "invalid_value",
                "row height must be positive and finite",
            ));
        }
        if columns.is_empty() || columns.len() > 256 {
            return Err(BridgeError::new(
                "invalid_value",
                "collection requires 1 to 256 columns",
            ));
        }
        if self.models.contains_key(&node) {
            return Err(BridgeError::new(
                "invalid_lifecycle",
                "collection already exists for node",
            ));
        }
        self.models.insert(
            node,
            Collection {
                revision: 1,
                row_count,
                row_height,
                columns,
                rows: BTreeMap::new(),
                active: None,
                bytes: 0,
            },
        );
        Ok(())
    }
    pub fn update(
        &mut self,
        node: Handle,
        row_count: u64,
        row_height: f32,
        columns: Vec<String>,
    ) -> Result<u64, BridgeError> {
        validate_row_count(row_count)?;
        if !row_height.is_finite() || row_height <= 0.0 {
            return Err(BridgeError::new(
                "invalid_value",
                "row height must be positive and finite",
            ));
        }
        if columns.is_empty() || columns.len() > 256 {
            return Err(BridgeError::new(
                "invalid_value",
                "collection requires 1 to 256 columns",
            ));
        }
        let collection = self.models.get_mut(&node).ok_or_else(invalid_node)?;
        collection.revision = collection
            .revision
            .checked_add(1)
            .ok_or_else(|| BridgeError::new("quota_exceeded", "collection revision exhausted"))?;
        self.total_bytes -= collection.bytes;
        collection.rows.clear();
        collection.bytes = 0;
        collection.active = None;
        collection.row_count = row_count;
        collection.row_height = row_height;
        collection.columns = columns;
        Ok(collection.revision)
    }
    pub fn request_visible(
        &mut self,
        node: Handle,
        start: u64,
        visible: u32,
        request_id: impl Into<String>,
    ) -> Result<Option<DataRequest>, BridgeError> {
        let collection = self.models.get_mut(&node).ok_or_else(invalid_node)?;
        let request_id = request_id.into();
        if request_id.is_empty() || request_id.len() > 64 {
            return Err(BridgeError::new(
                "invalid_value",
                "request id must contain 1 to 64 bytes",
            ));
        }
        if visible == 0 || start >= collection.row_count {
            return Ok(None);
        }
        let from = start.saturating_sub(visible as u64);
        let to = collection
            .row_count
            .min(start.saturating_add((visible as u64).saturating_mul(2)));
        if let Some(active) = &collection.active {
            let active_end = active.start as u64 + active.count as u64;
            if (active.start as u64) < to && from < active_end {
                return Ok(None);
            }
        }
        collection.active = None;
        let missing_start = (from..to).find(|row| !collection.rows.contains_key(&(*row as u32)));
        let Some(missing_start) = missing_start else {
            return Ok(None);
        };
        let count = (missing_start..to)
            .take_while(|row| !collection.rows.contains_key(&(*row as u32)))
            .count()
            .min(MAX_REQUEST_ROWS as usize) as u32;
        let request = DataRequest {
            node,
            revision: collection.revision,
            request_id,
            start: missing_start as u32,
            count,
        };
        collection.active = Some(request.clone());
        Ok(Some(request))
    }
    pub fn provide(
        &mut self,
        request: &DataRequest,
        rows: Vec<Vec<Value>>,
    ) -> Result<(), BridgeError> {
        let collection = self.models.get(&request.node).ok_or_else(invalid_node)?;
        if request.revision != collection.revision || collection.active.as_ref() != Some(request) {
            return Err(BridgeError::new(
                "stale_data",
                "data response no longer matches the active request",
            ));
        }
        if rows.len() != request.count as usize
            || (request.start as u64).saturating_add(rows.len() as u64) > collection.row_count
        {
            return Err(
                BridgeError::new("invalid_value", "row count does not match data request")
                    .field("rows"),
            );
        }
        if rows.iter().any(|row| row.len() != collection.columns.len()) {
            return Err(BridgeError::new(
                "invalid_value",
                "row column count does not match collection metadata",
            )
            .field("rows"));
        }
        let encoded = rows
            .iter()
            .map(|row| {
                serde_json::to_vec(row)
                    .map(|v| v.len())
                    .map_err(|e| BridgeError::new("invalid_value", e.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let incoming = encoded
            .iter()
            .try_fold(0usize, |total, size| total.checked_add(*size))
            .ok_or_else(|| BridgeError::new("quota_exceeded", "row payload size overflow"))?;
        if incoming > MAX_CACHE_BYTES_PER_NODE {
            return Err(BridgeError::new(
                "quota_exceeded",
                "response exceeds per-node row cache",
            ));
        }

        let collection = self
            .models
            .get_mut(&request.node)
            .expect("model validated above");
        collection.active = None;
        for (offset, (row, bytes)) in rows.into_iter().zip(encoded).enumerate() {
            let index = request.start + offset as u32;
            if let Some(old) = collection.rows.remove(&index) {
                collection.bytes -= old.bytes;
                self.total_bytes -= old.bytes;
            }
            self.clock = self.clock.saturating_add(1);
            collection.rows.insert(
                index,
                RowEntry {
                    data: row,
                    bytes,
                    last_used: self.clock,
                },
            );
            collection.bytes += bytes;
            self.total_bytes += bytes;
        }
        self.trim_node(request.node);
        while self.total_bytes > MAX_CACHE_BYTES_PER_SESSION {
            self.evict_oldest(None);
        }
        Ok(())
    }
    pub fn provide_active(
        &mut self,
        node: Handle,
        revision: u64,
        request_id: &str,
        start: u32,
        rows: Vec<Vec<Value>>,
    ) -> Result<(), BridgeError> {
        let request = self
            .models
            .get(&node)
            .and_then(|collection| collection.active.as_ref())
            .cloned()
            .ok_or_else(|| BridgeError::new("stale_data", "collection has no active request"))?;
        if request.revision != revision
            || request.request_id != request_id
            || request.start != start
        {
            return Err(BridgeError::new(
                "stale_data",
                "data response no longer matches the active request",
            ));
        }
        self.provide(&request, rows)
    }
    fn cancel(&mut self, request: &DataRequest) {
        if let Some(collection) = self.models.get_mut(&request.node)
            && collection.active.as_ref() == Some(request)
        {
            collection.active = None;
        }
    }
    fn touch_row(&mut self, node: Handle, index: u32) -> bool {
        self.clock = self.clock.saturating_add(1);
        let Some(entry) = self
            .models
            .get_mut(&node)
            .and_then(|model| model.rows.get_mut(&index))
        else {
            return false;
        };
        entry.last_used = self.clock;
        true
    }
    pub fn set_revision(&mut self, node: Handle, revision: u64) -> Result<(), BridgeError> {
        let collection = self.models.get_mut(&node).ok_or_else(invalid_node)?;
        if revision <= collection.revision {
            return Err(BridgeError::new("invalid_value", "revision must increase"));
        }
        self.total_bytes -= collection.bytes;
        collection.revision = revision;
        collection.rows.clear();
        collection.bytes = 0;
        collection.active = None;
        Ok(())
    }
    pub fn remove(&mut self, node: Handle) {
        if let Some(collection) = self.models.remove(&node) {
            self.total_bytes -= collection.bytes;
        }
    }
    pub fn cache_rows(&self, node: Handle) -> usize {
        self.models.get(&node).map_or(0, |model| model.rows.len())
    }
    pub fn cache_bytes(&self, node: Handle) -> usize {
        self.models.get(&node).map_or(0, |model| model.bytes)
    }
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }
    fn trim_node(&mut self, node: Handle) {
        while self.models.get(&node).is_some_and(|m| {
            m.bytes > MAX_CACHE_BYTES_PER_NODE || m.rows.len() > MAX_CACHE_ROWS_PER_NODE
        }) {
            self.evict_oldest(Some(node));
        }
    }
    fn evict_oldest(&mut self, only: Option<Handle>) {
        let candidate = self
            .models
            .iter()
            .filter(|(node, _)| only.is_none_or(|only| only == **node))
            .flat_map(|(node, model)| {
                model
                    .rows
                    .iter()
                    .map(move |(index, row)| (*node, *index, row.last_used))
            })
            .min_by_key(|(_, _, age)| *age);
        if let Some((node, index, _)) = candidate
            && let Some(model) = self.models.get_mut(&node)
            && let Some(row) = model.rows.remove(&index)
        {
            model.bytes -= row.bytes;
            self.total_bytes -= row.bytes;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TreeItem {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub children: Vec<TreeItem>,
}

#[derive(Default)]
pub struct TreeSnapshot {
    roots: Vec<TreeItem>,
    selected: Option<String>,
    count: usize,
}

impl TreeSnapshot {
    pub fn replace(&mut self, roots: Vec<TreeItem>) -> Result<(), BridgeError> {
        let mut seen = std::collections::HashSet::new();
        let mut count = 0;
        for root in &roots {
            validate_item(root, 1, &mut seen, &mut count)?;
        }
        if count > MAX_TREE_ITEMS {
            return Err(BridgeError::new(
                "quota_exceeded",
                "tree item quota exceeded",
            ));
        }
        let selected = self.selected.clone().filter(|key| seen.contains(key));
        self.roots = roots;
        self.selected = selected;
        self.count = count;
        Ok(())
    }
    pub fn select(&mut self, key: Option<String>) -> Result<(), BridgeError> {
        if key.as_ref().is_some_and(|key| !self.contains(key)) {
            return Err(BridgeError::new(
                "invalid_handle",
                "tree item key does not exist",
            ));
        }
        self.selected = key;
        Ok(())
    }
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }
    pub fn item_count(&self) -> usize {
        self.count
    }
    pub fn roots(&self) -> &[TreeItem] {
        &self.roots
    }
    fn contains(&self, key: &str) -> bool {
        fn visit(items: &[TreeItem], key: &str) -> bool {
            items
                .iter()
                .any(|i| i.key == key || visit(&i.children, key))
        }
        visit(&self.roots, key)
    }
}

fn validate_item(
    item: &TreeItem,
    depth: usize,
    seen: &mut std::collections::HashSet<String>,
    count: &mut usize,
) -> Result<(), BridgeError> {
    if depth > MAX_TREE_DEPTH {
        return Err(BridgeError::new("depth_exceeded", "tree depth exceeds 64"));
    }
    if item.key.is_empty() || !seen.insert(item.key.clone()) {
        return Err(BridgeError::new(
            "invalid_value",
            "tree keys must be nonempty and unique",
        ));
    }
    *count += 1;
    if *count > MAX_TREE_ITEMS {
        return Err(BridgeError::new(
            "quota_exceeded",
            "tree item quota exceeded",
        ));
    }
    for child in &item.children {
        validate_item(child, depth + 1, seen, count)?;
    }
    Ok(())
}
fn invalid_node() -> BridgeError {
    BridgeError::new("invalid_handle", "collection node does not exist")
}

fn validate_row_count(row_count: u64) -> Result<(), BridgeError> {
    if row_count > u32::MAX as u64 + 1 {
        return Err(BridgeError::new(
            "invalid_value",
            "row count exceeds protocol index range",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CollectionStore;
    use crate::handles::Handle;
    use serde_json::Value;

    #[test]
    fn overlapping_visible_request_keeps_the_active_response_valid() {
        let mut collections = CollectionStore::default();
        let node = Handle(1);
        collections
            .create(node, 50_000, 24.0, vec!["label".to_owned()])
            .unwrap();
        let request = collections
            .request_visible(node, 0, 15, "first")
            .unwrap()
            .unwrap();
        assert_eq!((request.start, request.count), (0, 30));
        assert!(
            collections
                .request_visible(node, 0, 15, "duplicate")
                .unwrap()
                .is_none()
        );

        collections
            .provide_active(
                node,
                request.revision,
                &request.request_id,
                request.start,
                (0..request.count)
                    .map(|index| vec![Value::String(format!("Row {index}"))])
                    .collect(),
            )
            .unwrap();
    }
}
