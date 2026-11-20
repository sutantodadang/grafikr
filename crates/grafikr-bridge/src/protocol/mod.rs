mod properties;
mod wire;

pub use properties::{PropertyMap, PropertyValue, WidgetKind, WidgetPatch, validate_patch};
pub use wire::{
    BridgeError, MAX_FRAME_BYTES, MAX_JSON_DEPTH, MAX_REQUEST_ID_BYTES, Operation, Outbound,
    Request, WireError, decode, encode,
};
