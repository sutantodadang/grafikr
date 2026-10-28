use grafikr::{App, Theme, View, button, column, text, tooltip};

fn demo() -> impl View {
    column((
        text("Tooltip").size(26.0),
        text("Arahkan pointer ke tombol dan tunggu 450 ms.").size(12.0),
        tooltip("Simpan perubahan", button("Simpan")),
    ))
    .padding(24.0)
    .gap(14.0)
}

fn main() {
    App::new(demo)
        .title("Grafikr — Tooltip")
        .theme(Theme::dark())
        .size(440.0, 260.0)
        .run()
        .expect("event loop");
}
