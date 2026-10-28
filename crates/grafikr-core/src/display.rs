//! Display list consumed by the GPU renderer.
//!
//! Coordinates are logical pixels (DIP). The renderer multiplies by the
//! window scale factor. Text is resolved to positioned glyphs here so the
//! renderer never needs Parley.

use std::sync::Arc;

use crate::FontData;
use crate::color::{Color, Point, Rect, Transform};
use crate::image::ImageData;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphPos {
    pub id: u32,
    /// Baseline origin, logical pixels.
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    pub font: FontData,
    pub font_size: f32,
    pub color: Color,
    pub glyphs: Vec<GlyphPos>,
    pub clip: Option<Rect>,
    /// Renderer substitutes every glyph with a bullet (password fields).
    pub mask: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DisplayItem {
    /// Filled, optionally rounded and bordered rectangle.
    Rect {
        rect: Rect,
        color: Color,
        radius: f32,
        border: Option<(f32, Color)>,
        clip: Option<Rect>,
    },
    Text(TextRun),
    Image {
        data: Arc<ImageData>,
        rect: Rect,
        clip: Option<Rect>,
    },
    Shadow {
        rect: Rect,
        radius: f32,
        blur: f32,
        color: Color,
        clip: Option<Rect>,
    },
}

impl DisplayItem {
    pub fn clip(&self) -> Option<Rect> {
        match self {
            Self::Rect { clip, .. } => *clip,
            Self::Text(run) => run.clip,
            Self::Image { clip, .. } => *clip,
            Self::Shadow { clip, .. } => *clip,
        }
    }

    /// Apply a visual transform about `origin` to this item.
    pub fn apply_transform(&mut self, origin: Point, transform: Transform) {
        if transform.is_identity() {
            return;
        }
        let scale = transform.scale;
        match self {
            Self::Rect {
                rect,
                radius,
                border,
                ..
            } => {
                *rect = transform.apply_rect(*rect, origin);
                *radius *= scale;
                if let Some((width, _)) = border {
                    *width *= scale;
                }
            }
            Self::Text(run) => {
                run.font_size *= scale;
                for glyph in &mut run.glyphs {
                    let point = transform.apply(Point::new(glyph.x, glyph.y), origin);
                    glyph.x = point.x;
                    glyph.y = point.y;
                }
            }
            Self::Image { rect, .. } => *rect = transform.apply_rect(*rect, origin),
            Self::Shadow {
                rect, radius, blur, ..
            } => {
                *rect = transform.apply_rect(*rect, origin);
                *radius *= scale;
                *blur *= scale;
            }
        }
    }
}

#[derive(Default)]
pub struct DisplayList {
    pub items: Vec<DisplayItem>,
}

impl DisplayList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn push_rect(&mut self, rect: Rect, color: Color, radius: f32, clip: Option<Rect>) {
        if rect.is_empty() || color.a == 0 {
            return;
        }
        self.items.push(DisplayItem::Rect {
            rect,
            color,
            radius,
            border: None,
            clip,
        });
    }

    pub fn push_bordered_rect(
        &mut self,
        rect: Rect,
        color: Color,
        radius: f32,
        border: Option<(f32, Color)>,
        clip: Option<Rect>,
    ) {
        if rect.is_empty() {
            return;
        }
        self.items.push(DisplayItem::Rect {
            rect,
            color,
            radius,
            border,
            clip,
        });
    }

    pub fn push_text(&mut self, run: TextRun) {
        if run.glyphs.is_empty() {
            return;
        }
        self.items.push(DisplayItem::Text(run));
    }

    pub fn push_image(&mut self, data: Arc<ImageData>, rect: Rect, clip: Option<Rect>) {
        if rect.is_empty() {
            return;
        }
        self.items.push(DisplayItem::Image { data, rect, clip });
    }

    pub fn push_shadow(
        &mut self,
        rect: Rect,
        radius: f32,
        blur: f32,
        color: Color,
        clip: Option<Rect>,
    ) {
        if rect.is_empty() || blur <= 0.0 || color.a == 0 {
            return;
        }
        self.items.push(DisplayItem::Shadow {
            rect,
            radius,
            blur,
            color,
            clip,
        });
    }

    /// Convenience for widgets that draw a text layout.
    pub fn push_layout(
        &mut self,
        layout: &parley::Layout<Color>,
        origin: Point,
        clip: Option<Rect>,
    ) {
        crate::text::push_layout(self, layout, origin, clip);
    }

    pub fn push_layout_masked(
        &mut self,
        layout: &parley::Layout<Color>,
        origin: Point,
        clip: Option<Rect>,
    ) {
        crate::text::push_layout_masked(self, layout, origin, clip);
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Transform every item appended since `start` about `origin`.
    pub fn apply_transform(&mut self, start: usize, origin: Point, transform: Transform) {
        for item in &mut self.items[start..] {
            item.apply_transform(origin, transform);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rect_is_skipped() {
        let mut list = DisplayList::new();
        list.push_rect(Rect::new(0.0, 0.0, 0.0, 10.0), Color::WHITE, 0.0, None);
        assert!(list.is_empty());
    }

    #[test]
    fn clips_intersect() {
        let outer = Rect::new(0.0, 0.0, 100.0, 100.0);
        let inner = Rect::new(50.0, 50.0, 100.0, 100.0);
        let clipped = outer.intersect(inner);
        assert_eq!(clipped, Rect::new(50.0, 50.0, 50.0, 50.0));
    }
}
