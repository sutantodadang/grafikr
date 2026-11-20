use std::collections::HashMap;
use std::sync::{Arc, Weak};

use grafikr_core::ImageData;

use crate::handles::{Handle, Object, ObjectKind, Registry};
use crate::protocol::BridgeError;

pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_IMAGE_BYTES_PER_SESSION: usize = 256 * 1024 * 1024;
pub const MAX_IMAGE_CHUNK_BYTES: usize = 1024 * 1024;

struct Upload {
    width: u32,
    height: u32,
    bytes: Vec<u8>,
    expected: usize,
}

pub struct Resources {
    registry: Registry,
    uploads: HashMap<Handle, Upload>,
    images: HashMap<Handle, Arc<ImageData>>,
    retired: Vec<(Weak<ImageData>, usize)>,
    reserved_bytes: usize,
}

impl Default for Resources {
    fn default() -> Self {
        Self::new()
    }
}

impl Resources {
    pub fn new() -> Self {
        Self {
            registry: Registry::new(),
            uploads: HashMap::new(),
            images: HashMap::new(),
            retired: Vec::new(),
            reserved_bytes: 0,
        }
    }
    pub fn begin(&mut self, width: u32, height: u32, bytes: u64) -> Result<Handle, BridgeError> {
        self.reap();
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or_else(|| BridgeError::new("invalid_value", "image dimensions overflow"))?;
        if width == 0
            || height == 0
            || expected != usize::try_from(bytes).unwrap_or(usize::MAX)
            || expected > MAX_IMAGE_BYTES
        {
            return Err(BridgeError::new(
                "invalid_value",
                "RGBA byte length must match dimensions and stay within 64 MiB",
            ));
        }
        if self.reserved_bytes.saturating_add(expected) > MAX_IMAGE_BYTES_PER_SESSION {
            return Err(BridgeError::new(
                "quota_exceeded",
                "image session quota exceeded",
            ));
        }
        let handle = self.registry.insert(ObjectKind::Upload, Object::Marker)?;
        self.uploads.insert(
            handle,
            Upload {
                width,
                height,
                bytes: Vec::with_capacity(expected),
                expected,
            },
        );
        self.reserved_bytes += expected;
        Ok(handle)
    }
    pub fn append(&mut self, upload: Handle, offset: u64, bytes: &[u8]) -> Result<(), BridgeError> {
        if bytes.is_empty() || bytes.len() > MAX_IMAGE_CHUNK_BYTES {
            return Err(BridgeError::new(
                "invalid_value",
                "image chunk must contain 1 byte to 1 MiB",
            ));
        }
        let part = self
            .uploads
            .get_mut(&upload)
            .ok_or_else(|| BridgeError::new("invalid_handle", "upload is stale"))?;
        if usize::try_from(offset).ok() != Some(part.bytes.len()) {
            return Err(BridgeError::new(
                "invalid_offset",
                "image chunks must be contiguous and ordered",
            ));
        }
        if part
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|end| end > part.expected)
        {
            return Err(BridgeError::new(
                "invalid_value",
                "image chunk exceeds declared length",
            ));
        }
        part.bytes.extend_from_slice(bytes);
        Ok(())
    }
    pub fn finalize(&mut self, upload: Handle) -> Result<Handle, BridgeError> {
        let part = self
            .uploads
            .get(&upload)
            .ok_or_else(|| BridgeError::new("invalid_handle", "upload is stale"))?;
        if part.bytes.len() != part.expected {
            return Err(BridgeError::new(
                "incomplete_upload",
                "received bytes do not match declared image size",
            ));
        }
        let image_handle = self.registry.insert(ObjectKind::Image, Object::Marker)?;
        let part = self.uploads.remove(&upload).expect("upload validated");
        self.registry.remove(upload, ObjectKind::Upload)?;
        self.images.insert(
            image_handle,
            ImageData::new_rgba(part.width, part.height, part.bytes),
        );
        Ok(image_handle)
    }
    pub fn image(&mut self, id: Handle) -> Result<Arc<ImageData>, BridgeError> {
        self.reap();
        self.images
            .get(&id)
            .cloned()
            .ok_or_else(|| BridgeError::new("invalid_handle", "image handle is stale"))
    }
    pub fn release_upload(&mut self, id: Handle) -> Result<(), BridgeError> {
        let upload = self
            .uploads
            .remove(&id)
            .ok_or_else(|| BridgeError::new("invalid_handle", "upload is stale"))?;
        self.reserved_bytes -= upload.expected;
        self.registry.remove(id, ObjectKind::Upload)?;
        Ok(())
    }
    pub fn release(&mut self, id: Handle) -> Result<(), BridgeError> {
        let image = self
            .images
            .remove(&id)
            .ok_or_else(|| BridgeError::new("invalid_handle", "image handle is stale"))?;
        let bytes = image.pixels.len();
        self.registry.remove(id, ObjectKind::Image)?;
        if Arc::strong_count(&image) > 1 {
            self.retired.push((Arc::downgrade(&image), bytes));
        } else {
            self.reserved_bytes -= bytes;
        }
        Ok(())
    }
    pub fn reserved_bytes(&self) -> usize {
        self.reserved_bytes
    }
    fn reap(&mut self) {
        let mut freed = 0;
        self.retired.retain(|(weak, bytes)| {
            if weak.strong_count() == 0 {
                freed += *bytes;
                false
            } else {
                true
            }
        });
        self.reserved_bytes -= freed;
    }
}
