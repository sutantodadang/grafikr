use std::collections::BTreeMap;

use serde_json::Value;

use super::wire::BridgeError;

pub type PropertyMap = BTreeMap<String, PropertyValue>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetKind {
    Column,
    Row,
    Grid,
    Stack,
    Text,
    Button,
    TextField,
    Checkbox,
    Radio,
    Switch,
    Slider,
    ProgressBar,
    Spinner,
    ScrollView,
    VirtualList,
    DataGrid,
    TreeView,
    Modal,
    Popup,
    Tabs,
    ComboBox,
    Tooltip,
    Image,
    Canvas,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    Bool(bool),
    I64(i64),
    F64(f64),
    Text(String),
    Color(String),
    Handle(String),
    Array(Vec<PropertyValue>),
    Object(BTreeMap<String, PropertyValue>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct WidgetPatch(pub PropertyMap);

pub fn validate_patch(kind: WidgetKind, fields: &Value) -> Result<WidgetPatch, BridgeError> {
    let fields = fields.as_object().ok_or_else(|| {
        BridgeError::new("invalid_type", "properties must be an object").field("properties")
    })?;
    let allowed: &[(&str, PropertyType)] = match kind {
        WidgetKind::Column | WidgetKind::Row | WidgetKind::Grid | WidgetKind::Stack => &[
            ("gap", PropertyType::Number),
            ("padding", PropertyType::Number),
            ("width", PropertyType::Number),
            ("height", PropertyType::Number),
            ("background", PropertyType::Color),
            ("radius", PropertyType::Number),
            ("transform", PropertyType::Object),
            ("columns", PropertyType::Integer),
        ],
        WidgetKind::Text => &[
            ("text", PropertyType::Text),
            ("size", PropertyType::Number),
            ("color", PropertyType::Color),
        ],
        WidgetKind::Button => &[
            ("label", PropertyType::Text),
            ("variant", PropertyType::Text),
            ("disabled", PropertyType::Bool),
        ],
        WidgetKind::TextField => &[
            ("text", PropertyType::Text),
            ("placeholder", PropertyType::Text),
            ("width", PropertyType::Number),
            ("multiline", PropertyType::Bool),
            ("password", PropertyType::Bool),
        ],
        WidgetKind::Checkbox | WidgetKind::Switch => &[
            ("label", PropertyType::Text),
            ("checked", PropertyType::Bool),
        ],
        WidgetKind::Radio => &[
            ("label", PropertyType::Text),
            ("group", PropertyType::Handle),
            ("value", PropertyType::Integer),
        ],
        WidgetKind::Slider => &[
            ("value", PropertyType::Number),
            ("min", PropertyType::Number),
            ("max", PropertyType::Number),
            ("step", PropertyType::Number),
            ("width", PropertyType::Number),
        ],
        WidgetKind::ProgressBar => &[
            ("value", PropertyType::Number),
            ("width", PropertyType::Number),
        ],
        WidgetKind::Spinner => &[
            ("size", PropertyType::Number),
            ("label", PropertyType::Text),
        ],
        WidgetKind::ScrollView => &[
            ("width", PropertyType::Number),
            ("height", PropertyType::Number),
            ("grow", PropertyType::Bool),
        ],
        WidgetKind::VirtualList => &[
            ("row_count", PropertyType::Integer),
            ("row_height", PropertyType::Number),
            ("width", PropertyType::Number),
            ("height", PropertyType::Number),
            ("selected", PropertyType::Integer),
        ],
        WidgetKind::DataGrid => &[
            ("columns", PropertyType::Array),
            ("row_count", PropertyType::Integer),
            ("row_height", PropertyType::Number),
            ("width", PropertyType::Number),
            ("height", PropertyType::Number),
            ("selected", PropertyType::Integer),
            ("label", PropertyType::Text),
        ],
        WidgetKind::TreeView => &[
            ("roots", PropertyType::Array),
            ("width", PropertyType::Number),
            ("height", PropertyType::Number),
            ("selected", PropertyType::Text),
        ],
        WidgetKind::Modal => &[
            ("open", PropertyType::Bool),
            ("width", PropertyType::Number),
        ],
        WidgetKind::Popup => &[
            ("open", PropertyType::Bool),
            ("x", PropertyType::Number),
            ("y", PropertyType::Number),
            ("width", PropertyType::Number),
        ],
        WidgetKind::Tabs => &[
            ("labels", PropertyType::Array),
            ("active", PropertyType::Integer),
            ("width", PropertyType::Number),
            ("height", PropertyType::Number),
        ],
        WidgetKind::ComboBox => &[
            ("options", PropertyType::Array),
            ("selected", PropertyType::Integer),
            ("width", PropertyType::Number),
        ],
        WidgetKind::Tooltip => &[
            ("label", PropertyType::Text),
            ("delay_ms", PropertyType::Integer),
        ],
        WidgetKind::Image => &[
            ("image", PropertyType::Handle),
            ("width", PropertyType::Number),
            ("height", PropertyType::Number),
            ("label", PropertyType::Text),
        ],
        WidgetKind::Canvas => &[
            ("commands", PropertyType::Array),
            ("label", PropertyType::Text),
            ("width", PropertyType::Number),
            ("height", PropertyType::Number),
        ],
    };
    let mut patch = PropertyMap::new();
    // Validate the entire patch before returning any values to the dispatcher.
    for (key, value) in fields {
        let Some((_, expected)) = allowed.iter().find(|(name, _)| *name == key) else {
            return Err(BridgeError::new(
                "unknown_property",
                "property is not supported by this widget",
            )
            .field(key));
        };
        patch.insert(
            key.clone(),
            convert(value, *expected).map_err(|e| e.field(key))?,
        );
    }
    Ok(WidgetPatch(patch))
}

#[derive(Clone, Copy)]
enum PropertyType {
    Bool,
    Integer,
    Number,
    Text,
    Color,
    Handle,
    Array,
    Object,
}

fn convert(value: &Value, expected: PropertyType) -> Result<PropertyValue, BridgeError> {
    match expected {
        PropertyType::Bool => value.as_bool().map(PropertyValue::Bool),
        PropertyType::Integer => value.as_i64().map(PropertyValue::I64),
        PropertyType::Number => value
            .as_f64()
            .filter(|n| n.is_finite())
            .map(PropertyValue::F64),
        PropertyType::Text => value.as_str().map(|s| PropertyValue::Text(s.to_owned())),
        PropertyType::Color => value
            .as_str()
            .filter(|s| s.starts_with('#') && matches!(s.len(), 7 | 9))
            .map(|s| PropertyValue::Color(s.to_owned())),
        PropertyType::Handle => value
            .as_str()
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .map(|s| PropertyValue::Handle(s.to_owned())),
        PropertyType::Array => value
            .as_array()
            .map(|values| PropertyValue::Array(values.iter().map(from_json).collect())),
        PropertyType::Object => value.as_object().map(|values| {
            PropertyValue::Object(
                values
                    .iter()
                    .map(|(k, v)| (k.clone(), from_json(v)))
                    .collect(),
            )
        }),
    }
    .ok_or_else(|| {
        BridgeError::new(
            "invalid_type",
            "property value has an invalid type or range",
        )
    })
}

fn from_json(value: &Value) -> PropertyValue {
    match value {
        Value::Null => PropertyValue::Object(BTreeMap::new()),
        Value::Bool(v) => PropertyValue::Bool(*v),
        Value::Number(v) => v
            .as_i64()
            .map(PropertyValue::I64)
            .or_else(|| v.as_f64().filter(|n| n.is_finite()).map(PropertyValue::F64))
            .unwrap_or(PropertyValue::I64(0)),
        Value::String(v) => PropertyValue::Text(v.clone()),
        Value::Array(v) => PropertyValue::Array(v.iter().map(from_json).collect()),
        Value::Object(v) => {
            PropertyValue::Object(v.iter().map(|(k, v)| (k.clone(), from_json(v))).collect())
        }
    }
}
