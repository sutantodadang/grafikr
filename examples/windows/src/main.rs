use grafikr::{App, Signal, Theme, View, bind, button, column, progress, slider, switch, text};

fn counter() -> impl View {
    let count = Signal::new(0);
    let value = count.clone();
    let less = count.clone();
    let more = count.clone();
    column((
        text(bind(move || format!("Nilai: {}", value.get()))).size(28.0),
        button("Kurang").on_click(move |_| less.update(|n| *n -= 1)),
        button("Tambah").on_click(move |_| more.update(|n| *n += 1)),
    ))
    .padding(24.0)
    .gap(12.0)
}

fn inspector() -> impl View {
    let level = Signal::new(0.5f32);
    let label = level.clone();
    column((
        text("Inspector").size(22.0),
        switch("Aktif", Signal::new(true)),
        slider(level).width(280.0).step(0.05),
        text(bind(move || format!("Level: {:.0}%", label.get() * 100.0))).size(12.0),
        progress(0.5).width(280.0),
    ))
    .padding(20.0)
    .gap(12.0)
}

fn main() {
    App::new(counter)
        .title("Grafikr — Utama")
        .theme(Theme::dark())
        .size(420.0, 320.0)
        .window("Grafikr — Inspector", 360.0, 300.0, inspector)
        .run()
        .expect("event loop");
}
