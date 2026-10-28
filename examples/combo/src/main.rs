use grafikr::{App, Signal, Theme, View, bind, column, combo_box, text};

fn demo() -> impl View {
    let selected = Signal::new(1usize);
    let status = selected.clone();
    column((
        text("ComboBox").size(26.0),
        text("Pilih paket").size(12.0),
        combo_box(["Gratis", "Pro", "Tim", "Enterprise"], selected.clone()).width(260.0),
        text(bind(move || match status.get() {
            0 => "Paket: Gratis".to_owned(),
            1 => "Paket: Pro".to_owned(),
            2 => "Paket: Tim".to_owned(),
            _ => "Paket: Enterprise".to_owned(),
        }))
        .size(12.0),
    ))
    .padding(24.0)
    .gap(12.0)
}

fn main() {
    App::new(demo)
        .title("Grafikr — ComboBox")
        .theme(Theme::dark())
        .size(520.0, 360.0)
        .run()
        .expect("event loop");
}
