use grafikr::{App, Signal, Theme, View, bind, checkbox, column, tabs, text, text_field};

fn demo() -> impl View {
    let active = Signal::new(0usize);
    let status = active.clone();
    let notify = Signal::new(true);

    column((
        text("Tabs").size(26.0),
        tabs(
            ["Umum", "Akun", "Lanjutan"],
            active.clone(),
            (
                column((
                    text("Pengaturan umum").size(18.0),
                    text("Nama tampilan").size(12.0),
                    text_field("Grafikr").width(320.0),
                ))
                .gap(10.0),
                column((
                    text("Akun").size(18.0),
                    text("Email").size(12.0),
                    text_field("hello@example.com").width(320.0),
                ))
                .gap(10.0),
                column((
                    text("Lanjutan").size(18.0),
                    checkbox("Kirim notifikasi", notify),
                    text("Hanya pane aktif yang ikut layout dan hit-test.").size(12.0),
                ))
                .gap(10.0),
            ),
        )
        .width(480.0)
        .height(280.0),
        text(bind(move || format!("Tab aktif: {}", status.get()))).size(12.0),
    ))
    .padding(24.0)
    .gap(12.0)
}

fn main() {
    App::new(demo)
        .title("Grafikr — Tabs")
        .theme(Theme::dark())
        .size(640.0, 460.0)
        .run()
        .expect("event loop");
}
