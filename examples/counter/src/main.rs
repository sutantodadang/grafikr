use grafikr::{App, Signal, Theme, View, bind, button, column, row, text};

fn counter() -> impl View {
    let count = Signal::new(0);
    let value = count.clone();
    let less = count.clone();
    let more = count.clone();

    column((
        text(bind(move || format!("Nilai: {}", value.get()))).size(28.0),
        row((
            button("Kurang").on_click(move |_| less.update(|n| *n -= 1)),
            button("Tambah").on_click(move |_| more.update(|n| *n += 1)),
        ))
        .gap(8.0),
    ))
    .padding(24.0)
    .gap(16.0)
}

fn main() {
    App::new(counter)
        .title("Grafikr — Counter")
        .theme(Theme::dark())
        .run()
        .expect("event loop");
}
