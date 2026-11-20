use std::collections::HashSet;
use std::fmt;

use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_JSON_DEPTH: usize = 64;
pub const MAX_REQUEST_ID_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeError {
    pub code: &'static str,
    pub message: String,
    pub field: Option<String>,
    pub handle: Option<String>,
}

impl BridgeError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            field: None,
            handle: None,
        }
    }

    pub fn field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for BridgeError {}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: String,
    pub op: Operation,
    pub args: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    Hello,
    Shutdown,
    WindowCreate,
    WindowUpdate,
    WindowClose,
    NodeCreate,
    NodeAttach,
    NodeDetach,
    NodeReplaceChildren,
    NodeDestroy,
    PropertyUpdate,
    StateCreate,
    StateGet,
    StateSet,
    StateDestroy,
    Bind,
    Unbind,
    Subscribe,
    Unsubscribe,
    ImageBegin,
    ImageChunk,
    ImageFinalize,
    ImageDestroy,
    CanvasUpdate,
    CollectionCreate,
    CollectionUpdate,
    DataProvide,
    Transition,
}

impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hello => "hello",
            Self::Shutdown => "shutdown",
            Self::WindowCreate => "window.create",
            Self::WindowUpdate => "window.update",
            Self::WindowClose => "window.close",
            Self::NodeCreate => "node.create",
            Self::NodeAttach => "node.attach",
            Self::NodeDetach => "node.detach",
            Self::NodeReplaceChildren => "node.replace_children",
            Self::NodeDestroy => "node.destroy",
            Self::PropertyUpdate => "property.update",
            Self::StateCreate => "state.create",
            Self::StateGet => "state.get",
            Self::StateSet => "state.set",
            Self::StateDestroy => "state.destroy",
            Self::Bind => "binding.bind",
            Self::Unbind => "binding.unbind",
            Self::Subscribe => "event.subscribe",
            Self::Unsubscribe => "event.unsubscribe",
            Self::ImageBegin => "image.begin",
            Self::ImageChunk => "image.chunk",
            Self::ImageFinalize => "image.finalize",
            Self::ImageDestroy => "image.destroy",
            Self::CanvasUpdate => "canvas.update",
            Self::CollectionCreate => "collection.create",
            Self::CollectionUpdate => "collection.update",
            Self::DataProvide => "data.provide",
            Self::Transition => "transition.start",
        }
    }
}

impl TryFrom<&str> for Operation {
    type Error = BridgeError;
    fn try_from(op: &str) -> Result<Self, Self::Error> {
        let ops: &[(&str, Operation)] = &[
            ("hello", Self::Hello),
            ("shutdown", Self::Shutdown),
            ("window.create", Self::WindowCreate),
            ("window.update", Self::WindowUpdate),
            ("window.close", Self::WindowClose),
            ("node.create", Self::NodeCreate),
            ("node.attach", Self::NodeAttach),
            ("node.detach", Self::NodeDetach),
            ("node.replace_children", Self::NodeReplaceChildren),
            ("node.destroy", Self::NodeDestroy),
            ("property.update", Self::PropertyUpdate),
            ("state.create", Self::StateCreate),
            ("state.get", Self::StateGet),
            ("state.set", Self::StateSet),
            ("state.destroy", Self::StateDestroy),
            ("binding.bind", Self::Bind),
            ("binding.unbind", Self::Unbind),
            ("event.subscribe", Self::Subscribe),
            ("event.unsubscribe", Self::Unsubscribe),
            ("image.begin", Self::ImageBegin),
            ("image.chunk", Self::ImageChunk),
            ("image.finalize", Self::ImageFinalize),
            ("image.destroy", Self::ImageDestroy),
            ("canvas.update", Self::CanvasUpdate),
            ("collection.create", Self::CollectionCreate),
            ("collection.update", Self::CollectionUpdate),
            ("data.provide", Self::DataProvide),
            ("transition.start", Self::Transition),
        ];
        ops.iter()
            .find_map(|(name, value)| (*name == op).then_some(*value))
            .ok_or_else(|| {
                BridgeError::new("unknown_operation", "operation is not supported").field("op")
            })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outbound {
    Response {
        id: String,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<WireError>,
    },
    Event {
        sequence: String,
        event: String,
        source: String,
        payload: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
}

pub fn decode(bytes: &[u8]) -> Result<Request, BridgeError> {
    if bytes
        .len()
        .saturating_add(usize::from(!bytes.ends_with(b"\n")))
        > MAX_FRAME_BYTES
    {
        return Err(BridgeError::new("frame_too_large", "request exceeds 8 MiB"));
    }
    let body = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    if body.contains(&b'\n') || body.contains(&b'\r') {
        return Err(BridgeError::new(
            "invalid_frame",
            "expected one JSON object per LF-terminated frame",
        ));
    }
    let mut de = serde_json::Deserializer::from_slice(body);
    let value = UniqueValueSeed
        .deserialize(&mut de)
        .map_err(|e| BridgeError::new("invalid_json", e.to_string()))?;
    de.end()
        .map_err(|e| BridgeError::new("invalid_json", e.to_string()))?;
    check_depth(&value, 1)?;
    let object = value
        .as_object()
        .ok_or_else(|| BridgeError::new("invalid_request", "request must be an object"))?;
    if object
        .keys()
        .any(|k| !matches!(k.as_str(), "id" | "op" | "args"))
        || object.len() != 3
    {
        return Err(BridgeError::new(
            "invalid_request",
            "request requires only id, op, and args",
        ));
    }
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| BridgeError::new("invalid_request", "id must be a string").field("id"))?;
    if id.is_empty() || id.len() > MAX_REQUEST_ID_BYTES {
        return Err(
            BridgeError::new("invalid_request", "id must contain 1 to 64 UTF-8 bytes").field("id"),
        );
    }
    let op = object
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| BridgeError::new("invalid_request", "op must be a string").field("op"))?;
    let op = Operation::try_from(op)?;
    let args = object
        .get("args")
        .filter(|v| v.is_object())
        .cloned()
        .ok_or_else(|| {
            BridgeError::new("invalid_request", "args must be an object").field("args")
        })?;
    validate_args(op, &args)?;
    Ok(Request {
        id: id.to_owned(),
        op,
        args,
    })
}

pub fn encode(message: &Outbound) -> Result<Vec<u8>, BridgeError> {
    let mut bytes = serde_json::to_vec(message)
        .map_err(|e| BridgeError::new("internal_failure", e.to_string()))?;
    bytes.push(b'\n');
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(BridgeError::new(
            "frame_too_large",
            "outbound message exceeds 8 MiB",
        ));
    }
    Ok(bytes)
}

fn validate_args(op: Operation, args: &Value) -> Result<(), BridgeError> {
    let args = args.as_object().expect("object checked by decode");
    let (required, optional): (&[&str], &[&str]) = match op {
        Operation::Hello => (&["protocol_major", "protocol_minor", "sdk_version"], &[]),
        Operation::Shutdown => (&[], &[]),
        Operation::WindowCreate => (&["title", "width", "height"], &["theme", "reduced_motion"]),
        Operation::WindowUpdate => (
            &["window"],
            &["title", "width", "height", "theme", "reduced_motion"],
        ),
        Operation::WindowClose => (&["window"], &[]),
        Operation::NodeCreate => (&["window", "widget", "properties"], &[]),
        Operation::NodeAttach => (&["node", "parent"], &["index"]),
        Operation::NodeDetach | Operation::NodeDestroy => (&["node"], &[]),
        Operation::CanvasUpdate => (&["node", "commands"], &[]),
        Operation::CollectionUpdate => (&["node", "metadata"], &[]),
        Operation::NodeReplaceChildren => (&["node", "children"], &[]),
        Operation::PropertyUpdate => (&["node", "properties"], &[]),
        Operation::StateCreate => (&["value"], &[]),
        Operation::StateGet | Operation::StateDestroy => (&["state"], &[]),
        Operation::StateSet => (&["state", "value"], &[]),
        Operation::Bind => (&["state", "node", "property", "direction"], &[]),
        Operation::Unbind => (&["binding"], &[]),
        Operation::Subscribe => (&["node", "event"], &[]),
        Operation::Unsubscribe => (&["subscription"], &[]),
        Operation::ImageBegin => (&["width", "height", "byte_length"], &[]),
        Operation::ImageChunk => (&["upload", "offset", "data"], &[]),
        Operation::ImageFinalize => (&["upload"], &[]),
        Operation::ImageDestroy => (&["image"], &[]),
        Operation::CollectionCreate => (&["node", "row_count", "row_height", "columns"], &[]),
        Operation::DataProvide => (&["node", "revision", "request", "start", "rows"], &[]),
        Operation::Transition => (&["state", "to", "duration_ms", "easing"], &[]),
    };
    if let Some(name) = args
        .keys()
        .find(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str()))
    {
        return Err(BridgeError::new(
            "unknown_field",
            "argument is not defined for this operation",
        )
        .field(name));
    }
    if let Some(name) = required.iter().find(|name| !args.contains_key(**name)) {
        return Err(BridgeError::new("missing_field", "required argument is missing").field(*name));
    }
    if matches!(op, Operation::WindowCreate | Operation::WindowUpdate)
        && args
            .get("reduced_motion")
            .is_some_and(|value| !value.is_boolean())
    {
        return Err(invalid_field("reduced_motion", "must be a boolean"));
    }
    if op == Operation::Transition {
        let target = args.get("to").and_then(Value::as_object);
        if !target.is_some_and(|target| {
            target.len() == 2
                && target.get("type").and_then(Value::as_str) == Some("f64")
                && target
                    .get("value")
                    .and_then(Value::as_f64)
                    .is_some_and(|value| {
                        value.is_finite() && value <= f32::MAX as f64 && value >= f32::MIN as f64
                    })
        }) {
            return Err(invalid_field("to", "must be a finite f64 within f32 range"));
        }
        if args
            .get("duration_ms")
            .and_then(Value::as_u64)
            .and_then(|duration| u32::try_from(duration).ok())
            .is_none()
        {
            return Err(invalid_field("duration_ms", "must be a u32 integer"));
        }
        if !matches!(
            args.get("easing").and_then(Value::as_str),
            Some("linear" | "ease_out_cubic" | "ease_in_out_cubic")
        ) {
            return Err(invalid_field("easing", "unsupported easing"));
        }
    }
    for key in [
        "window",
        "node",
        "state",
        "binding",
        "subscription",
        "upload",
        "image",
        "parent",
        "revision",
    ] {
        if let Some(value) = args.get(key) {
            let Some(token) = value.as_str() else {
                return Err(BridgeError::new(
                    "invalid_type",
                    "handle/sequence must be a decimal string",
                )
                .field(key));
            };
            if token.is_empty()
                || (token.len() > 1 && token.starts_with('0'))
                || !token.bytes().all(|b| b.is_ascii_digit())
                || token.parse::<u64>().is_err()
            {
                return Err(BridgeError::new(
                    "invalid_value",
                    "expected canonical unsigned decimal",
                )
                .field(key));
            }
        }
    }
    Ok(())
}

fn invalid_field(field: &str, message: &str) -> BridgeError {
    BridgeError::new("invalid_value", message).field(field)
}

fn check_depth(value: &Value, depth: usize) -> Result<(), BridgeError> {
    if depth > MAX_JSON_DEPTH {
        return Err(BridgeError::new(
            "depth_exceeded",
            "JSON nesting exceeds 64",
        ));
    }
    match value {
        Value::Array(items) => {
            for item in items {
                check_depth(item, depth + 1)?;
            }
        }
        Value::Object(items) => {
            for item in items.values() {
                check_depth(item, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

struct UniqueValueSeed;
impl<'de> DeserializeSeed<'de> for UniqueValueSeed {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(UniqueValueVisitor)
    }
}
struct UniqueValueVisitor;
impl<'de> Visitor<'de> for UniqueValueVisitor {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value with unique object keys")
    }
    fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite number"))
    }
    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }
    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_none<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = seq.next_element_seed(UniqueValueSeed)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut values = serde_json::Map::new();
        let mut seen = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate object key: {key}"
                )));
            }
            values.insert(key, map.next_value_seed(UniqueValueSeed)?);
        }
        Ok(Value::Object(values))
    }
}
