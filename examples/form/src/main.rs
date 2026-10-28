use grafikr::{App, Signal, Theme, View, bind, button, column, text, text_field};

fn form() -> impl View {
    let name = Signal::new(String::new());
    let greeting = name.clone();
    let saved = Signal::new("Belum disimpan".to_owned());
    let status = saved.clone();
    let save_name = name.clone();
    let field_name = name.clone();

    column((
        text("Form sederhana").size(24.0),
        text("Nama").color(Theme::dark().muted).size(12.0),
        text_field("")
            .width(300.0)
            .on_change(move |value| field_name.set(value)),
        text(bind(move || {
            let name = greeting.get();
            if name.is_empty() {
                "Halo!".to_owned()
            } else {
                format!("Halo, {name}")
            }
        })),
        button("Simpan").on_click(move |_| {
            saved.set(format!("Tersimpan untuk {}", save_name.get()));
        }),
        text(bind(move || status.get())).size(12.0),
    ))
    .padding(24.0)
    .gap(10.0)
}

fn main() {
    App::new(form)
        .title("Grafikr — Form")
        .theme(Theme::dark())
        .size(560.0, 420.0)
        .run()
        .expect("event loop");
}
