use grafikr::{App, Signal, Theme, View, bind, button, column, popup, text};

fn demo() -> impl View {
    let open = Signal::new(false);
    let status = Signal::new("Belum ada aksi".to_owned());
    let open_button = open.clone();
    let on_rename = open.clone();
    let on_duplicate = open.clone();
    let on_delete = open.clone();
    let rename_status = status.clone();
    let duplicate_status = status.clone();
    let delete_status = status.clone();
    let status_text = status.clone();

    column((
        text("Menu demo").size(26.0),
        button("Aksi").on_click(move |_| open_button.set(true)),
        text(bind(move || status_text.get())).size(12.0),
        popup(
            open.clone(),
            24.0,
            104.0,
            (
                button("Rename").secondary().on_click(move |_| {
                    rename_status.set("Rename dipilih".to_owned());
                    on_rename.set(false);
                }),
                button("Duplicate").secondary().on_click(move |_| {
                    duplicate_status.set("Duplicate dipilih".to_owned());
                    on_duplicate.set(false);
                }),
                button("Delete").secondary().on_click(move |_| {
                    delete_status.set("Delete dipilih".to_owned());
                    on_delete.set(false);
                }),
            ),
        )
        .width(180.0),
    ))
    .padding(24.0)
    .gap(12.0)
}

fn main() {
    App::new(demo)
        .title("Grafikr — Menu")
        .theme(Theme::dark())
        .size(560.0, 400.0)
        .run()
        .expect("event loop");
}
