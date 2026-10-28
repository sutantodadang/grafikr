use grafikr::{App, Signal, Theme, View, bind, button, column, modal, row, text};

fn demo() -> impl View {
    let open = Signal::new(false);
    let open_button = open.clone();
    let status = open.clone();
    let cancel = open.clone();
    let confirm = open.clone();

    column((
        text("Dialog demo").size(26.0),
        text("Modal mengunci fokus, menutup dengan Escape, dan mengembalikan fokus.").size(12.0),
        button("Buka dialog").on_click(move |_| open_button.set(true)),
        text(bind(move || {
            format!(
                "Status: {}",
                if status.get() { "terbuka" } else { "tertutup" }
            )
        }))
        .size(12.0),
        modal(
            open.clone(),
            (
                text("Hapus data?").size(18.0),
                text("Tindakan ini tidak bisa dibatalkan.").size(12.0),
                row((
                    button("Batal").on_click(move |_| cancel.set(false)),
                    button("Hapus").on_click(move |_| confirm.set(false)),
                ))
                .gap(8.0),
            ),
        )
        .width(380.0),
    ))
    .padding(24.0)
    .gap(12.0)
}

fn main() {
    App::new(demo)
        .title("Grafikr — Dialog")
        .theme(Theme::dark())
        .size(640.0, 420.0)
        .run()
        .expect("event loop");
}
