//! Text shaping and layout via Parley.

use std::sync::Arc;

use parley::{
    Alignment, AlignmentOptions, FontContext, Layout, LayoutContext, PositionedLayoutItem,
    StyleProperty,
};

use crate::color::{Color, Point, Rect};
use crate::display::{DisplayList, GlyphPos, TextRun};

pub struct TextEngine {
    font_cx: FontContext,
    layout_cx: LayoutContext<Color>,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    pub fn new() -> Self {
        Self {
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
        }
    }

    /// Split borrow for `PlainEditor`, which needs both contexts at once.
    pub fn parts(&mut self) -> (&mut FontContext, &mut LayoutContext<Color>) {
        (&mut self.font_cx, &mut self.layout_cx)
    }

    /// Shape in logical pixels (DIP). The renderer applies the window scale.
    pub fn shape(
        &mut self,
        text: &str,
        font_size: f32,
        color: Color,
        max_width: Option<f32>,
    ) -> Arc<Layout<Color>> {
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, false);
        builder.push_default(StyleProperty::FontSize(font_size));
        builder.push_default(StyleProperty::Brush(color));
        let mut layout = builder.build(text);
        layout.break_all_lines(max_width);
        layout.align(Alignment::Start, AlignmentOptions::default());
        Arc::new(layout)
    }
}

#[derive(Clone, PartialEq)]
pub struct ShapeKey {
    text: String,
    font_size: u32,
    color: Color,
    max_width: Option<u32>,
}

pub fn shape_key(text: &str, font_size: f32, color: Color, max_width: Option<f32>) -> ShapeKey {
    ShapeKey {
        text: text.to_owned(),
        font_size: font_size.to_bits(),
        color,
        max_width: max_width.map(f32::to_bits),
    }
}

#[derive(Clone)]
pub struct TextShape {
    pub layout: Arc<Layout<Color>>,
    pub width: f32,
    pub height: f32,
}

impl TextShape {
    fn new(layout: Arc<Layout<Color>>) -> Self {
        let width = layout.width();
        let height = layout.height();
        Self {
            layout,
            width,
            height,
        }
    }
}

/// Per-widget shape cache. Reshapes only when a key input changes.
#[derive(Default)]
pub struct TextCache {
    entry: Option<(ShapeKey, TextShape)>,
}

impl TextCache {
    pub fn get(
        &mut self,
        engine: &mut TextEngine,
        text: &str,
        font_size: f32,
        color: Color,
        max_width: Option<f32>,
    ) -> &TextShape {
        let key = shape_key(text, font_size, color, max_width);
        let stale = self.entry.as_ref().is_none_or(|(cached, _)| *cached != key);
        if stale {
            let layout = engine.shape(text, font_size, color, max_width);
            self.entry = Some((key, TextShape::new(layout)));
        }
        &self.entry.as_ref().expect("shape was just built").1
    }

    pub fn cached(&self) -> Option<&TextShape> {
        self.entry.as_ref().map(|(_, shape)| shape)
    }

    pub fn clear(&mut self) {
        self.entry = None;
    }
}

/// Resolve a shaped layout into glyph runs for the renderer.
pub fn push_layout(
    list: &mut DisplayList,
    layout: &Layout<Color>,
    origin: Point,
    clip: Option<Rect>,
) {
    push_layout_impl(list, layout, origin, clip, false);
}

/// Same as [`push_layout`], but the renderer draws bullets instead of glyphs.
pub fn push_layout_masked(
    list: &mut DisplayList,
    layout: &Layout<Color>,
    origin: Point,
    clip: Option<Rect>,
) {
    push_layout_impl(list, layout, origin, clip, true);
}

fn push_layout_impl(
    list: &mut DisplayList,
    layout: &Layout<Color>,
    origin: Point,
    clip: Option<Rect>,
    mask: bool,
) {
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let style = glyph_run.style();
            let run = glyph_run.run();
            let mut glyphs = Vec::with_capacity(glyph_run.glyphs().count());
            for glyph in glyph_run.positioned_glyphs() {
                glyphs.push(GlyphPos {
                    id: glyph.id,
                    x: origin.x + glyph.x,
                    y: origin.y + glyph.y,
                });
            }
            list.push_text(TextRun {
                font: run.font().clone(),
                font_size: run.font_size(),
                color: style.brush,
                glyphs,
                clip,
                mask,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_ascii_and_unicode() {
        let mut engine = TextEngine::new();
        let shape = engine.shape("Halo, dunia — Привет", 14.0, Color::BLACK, None);
        assert!(shape.width() > 0.0);
        assert!(shape.height() > 0.0);
        let glyphs: usize = shape
            .lines()
            .flat_map(|line| line.items())
            .map(|item| match item {
                PositionedLayoutItem::GlyphRun(run) => run.glyphs().count(),
                PositionedLayoutItem::InlineBox(_) => 0,
            })
            .sum();
        assert!(glyphs > 0, "layout produced no glyphs");
        let font_bytes = shape
            .lines()
            .flat_map(|line| line.items())
            .find_map(|item| match item {
                PositionedLayoutItem::GlyphRun(run) => Some(run.run().font().data.data().len()),
                _ => None,
            })
            .unwrap_or(0);
        assert!(font_bytes > 0, "font data is empty");
    }

    #[test]
    fn shapes_arabic_and_emoji() {
        let mut engine = TextEngine::new();
        let arabic = engine.shape("مرحبا بالعالم", 16.0, Color::BLACK, None);
        assert!(arabic.width() > 0.0);
        let glyphs: usize = arabic
            .lines()
            .flat_map(|line| line.items())
            .map(|item| match item {
                PositionedLayoutItem::GlyphRun(run) => run.glyphs().count(),
                PositionedLayoutItem::InlineBox(_) => 0,
            })
            .sum();
        assert!(glyphs > 0);

        let emoji = engine.shape("Halo 🌍", 16.0, Color::BLACK, None);
        assert!(emoji.width() > 0.0);
    }

    #[test]
    fn layout_is_in_logical_pixels() {
        let mut engine = TextEngine::new();
        let a = engine.shape("Hello", 14.0, Color::BLACK, None).width();
        let b = engine.shape("Hello", 28.0, Color::BLACK, None).width();
        assert!((b / a - 2.0).abs() < 0.05, "font size must scale linearly");
    }

    #[test]
    fn wraps_at_max_width() {
        let mut engine = TextEngine::new();
        let wide = engine.shape(
            "one two three four five six seven",
            14.0,
            Color::BLACK,
            None,
        );
        let narrow = engine.shape(
            "one two three four five six seven",
            14.0,
            Color::BLACK,
            Some(60.0),
        );
        assert!(narrow.height() > wide.height());
        assert!(narrow.width() <= 60.5);
    }

    #[test]
    fn cache_reuses_key() {
        let mut engine = TextEngine::new();
        let mut cache = TextCache::default();
        let first = cache
            .get(&mut engine, "same", 14.0, Color::BLACK, None)
            .layout
            .clone();
        let second = cache
            .get(&mut engine, "same", 14.0, Color::BLACK, None)
            .layout
            .clone();
        assert!(Arc::ptr_eq(&first, &second));
    }
}
