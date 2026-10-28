//! Raw RGBA image handles. Decoding (PNG/JPEG) is left to the application.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub struct ImageData {
    id: u64,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl ImageData {
    /// `pixels` must be tightly packed RGBA8: `width * height * 4` bytes.
    pub fn new_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Arc<Self> {
        assert!(width > 0 && height > 0, "image dimensions must be non-zero");
        assert_eq!(
            pixels.len(),
            width as usize * height as usize * 4,
            "image pixel buffer does not match dimensions"
        );
        Arc::new(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            pixels,
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }
}

impl std::fmt::Debug for ImageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageData")
            .field("id", &self.id)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl PartialEq for ImageData {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
