use std::sync::Arc;

use grafikr::{App, ImageData, Theme, View, column, image, text};

fn checker() -> Arc<ImageData> {
    let (width, height) = (256u32, 160u32);
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let cell = (x / 16 + y / 16) % 2 == 0;
            let glow = (x as f32 / width as f32 * 0.7 + y as f32 / height as f32 * 0.3) * 90.0;
            let (r, g, b) = if cell {
                (60.0 + glow, 120.0 + glow, 230.0)
            } else {
                (18.0, 24.0 + glow * 0.4, 42.0)
            };
            pixels.extend_from_slice(&[r as u8, g as u8, b as u8, 255]);
        }
    }
    ImageData::new_rgba(width, height, pixels)
}

fn demo() -> impl View {
    let data = ImageData::new_rgba(
        2,
        2,
        vec![
            255, 80, 80, 255, 80, 255, 120, 255, 80, 120, 255, 255, 255, 220, 80, 255,
        ],
    );
    let texture = checker();
    column((
        text("Image demo").size(26.0),
        text("RGBA mentah, upload texture per handle, discale oleh renderer.").size(12.0),
        image(texture)
            .width(512.0)
            .height(240.0)
            .label("Checker gradient"),
        image(data)
            .width(96.0)
            .height(96.0)
            .label("Four pixel image"),
    ))
    .padding(24.0)
    .gap(14.0)
}

fn main() {
    App::new(demo)
        .title("Grafikr — Image")
        .theme(Theme::dark())
        .size(640.0, 520.0)
        .run()
        .expect("event loop");
}
