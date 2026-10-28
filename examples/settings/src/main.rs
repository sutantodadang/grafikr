use grafikr::{
    App, Signal, Theme, View, bind, button, checkbox, column, radio, row, scroll_view, slider,
    switch, text, text_field,
};

fn settings() -> impl View {
    let name = Signal::new("Sutan".to_owned());
    let greeting = name.clone();
    let field = name.clone();

    let notifications = Signal::new(true);
    let notif = notifications.clone();

    let plan = Signal::new(1usize);
    let plan_text = plan.clone();

    let volume = Signal::new(0.35f32);
    let volume_text = volume.clone();
    let volume_slider = volume.clone();

    scroll_view(
        column((
            text("Settings").size(26.0),
            text("Nama").size(12.0),
            text_field("Sutan")
                .width(360.0)
                .on_change(move |value| field.set(value)),
            text("Password").size(12.0),
            text_field("rahasia").password().width(360.0),
            text(bind(move || format!("Halo, {}", greeting.get()))),
            checkbox("Aktifkan notifikasi", notifications.clone()),
            text(bind(move || {
                format!(
                    "Notifikasi: {}",
                    if notif.get() { "aktif" } else { "nonaktif" }
                )
            }))
            .size(12.0),
            switch("Mode hemat daya", Signal::new(false)),
            text("Paket").size(12.0),
            row((
                radio("Gratis", plan.clone(), 0),
                radio("Pro", plan.clone(), 1),
                radio("Tim", plan.clone(), 2),
            ))
            .gap(16.0),
            text(bind(move || {
                let label = match plan_text.get() {
                    0 => "Gratis",
                    1 => "Pro",
                    _ => "Tim",
                };
                format!("Paket dipilih: {label}")
            }))
            .size(12.0),
            text("Volume").size(12.0),
            slider(volume_slider)
                .range(0.0, 1.0)
                .step(0.01)
                .width(360.0),
            text(bind(move || {
                format!("Volume: {:.0}%", volume_text.get() * 100.0)
            }))
            .size(12.0),
            row((
                button("Simpan").on_click(|_| {}),
                button("Reset").secondary().on_click(|_| {}),
            ))
            .gap(8.0),
        ))
        .gap(10.0)
        .padding(24.0),
    )
    .height(460.0)
}

fn main() {
    App::new(settings)
        .title("Grafikr — Settings")
        .theme(Theme::dark())
        .size(640.0, 560.0)
        .run()
        .expect("event loop");
}
