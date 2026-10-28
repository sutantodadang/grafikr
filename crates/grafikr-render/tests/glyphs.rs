use grafikr_core::{Color, TextEngine};
use parley::PositionedLayoutItem;
use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;

#[test]
fn swash_rasterizes_glyphs() {
    let mut engine = TextEngine::new();
    let layout = engine.shape("Hello", 14.0, Color::WHITE, None);
    let mut scale_context = ScaleContext::new();
    let mut rasterized = 0;

    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            let font = run.font();
            let font_ref =
                FontRef::from_index(font.data.data(), font.index as usize).expect("font ref");
            let mut scaler = scale_context
                .builder(font_ref)
                .size(14.0)
                .hint(true)
                .build();
            let sources = [Source::Outline];
            let mut render = Render::new(&sources);
            render.format(Format::Alpha);
            for glyph in glyph_run.glyphs() {
                let image = render
                    .render(&mut scaler, glyph.id as u16)
                    .expect("glyph image");
                assert!(image.placement.width > 0, "zero-width glyph");
                assert!(!image.data.is_empty(), "empty glyph mask");
                rasterized += 1;
            }
        }
    }
    assert!(rasterized > 0);
}
