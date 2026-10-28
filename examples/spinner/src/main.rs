use grafikr::{App, Theme, View, column, progress, row, spinner, text};

fn demo() -> impl View {
    column((
        text("Spinner").size(26.0),
        row((
            spinner().size(44.0).label("Memuat data"),
            text("Memuat data...").size(14.0),
        ))
        .gap(16.0),
        text("Animasi memakai frame scheduler; idle tetap berhenti.").size(12.0),
        progress(0.42).width(280.0),
    ))
    .padding(24.0)
    .gap(14.0)
}

fn main() {
    App::new(demo)
        .title("Grafikr — Spinner")
        .theme(Theme::dark())
        .size(440.0, 260.0)
        .run()
        .expect("event loop");
}
