use grafikr::{App, GridColumn, Signal, Theme, View, bind, column, data_grid, text};

fn demo() -> impl View {
    let selected = Signal::new(0usize);
    let status = selected.clone();
    let pick = selected.clone();

    column((
        text("DataGrid 50.000 baris").size(24.0),
        text(bind(move || {
            format!("Baris terpilih: {}", status.get() + 1)
        }))
        .size(12.0),
        data_grid(
            [
                GridColumn::new("Nama", 220.0),
                GridColumn::new("Status", 110.0),
                GridColumn::new("Nilai", 120.0),
            ],
            50_000,
            28.0,
            |index| {
                vec![
                    format!("Item {:05}", index + 1),
                    if index % 3 == 0 { "Aktif" } else { "Tertunda" }.to_owned(),
                    format!("Rp {}", (index * 137) % 1_000_000),
                ]
            },
        )
        .width(620.0)
        .height(400.0)
        .selected(0)
        .on_select(move |index| pick.set(index)),
    ))
    .padding(24.0)
    .gap(12.0)
}

fn main() {
    App::new(demo)
        .title("Grafikr — DataGrid")
        .theme(Theme::dark())
        .size(720.0, 540.0)
        .run()
        .expect("event loop");
}
