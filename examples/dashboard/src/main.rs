use grafikr::{App, Signal, Theme, View, bind, canvas, column, progress, text, virtual_list};

fn dashboard() -> impl View {
    let selected = Signal::new("Belum ada baris dipilih".to_owned());
    let status = selected.clone();
    let pick = selected.clone();

    column((
        text("Dashboard").size(26.0),
        text(bind(move || status.get())).size(12.0),
        progress(0.62).width(920.0),
        canvas(920.0, 120.0, |cx, rect| {
            let bars = 36;
            let gap = 4.0;
            let width = (rect.width - gap * (bars as f32 - 1.0)) / bars as f32;
            for index in 0..bars {
                let value = ((index * 37 + 11) % 100) as f32 / 100.0;
                let height = (rect.height - 12.0) * value.max(0.08);
                let x = rect.x + index as f32 * (width + gap);
                let y = rect.y + rect.height - height;
                let color = if index % 7 == 0 {
                    cx.theme.accent
                } else {
                    cx.theme.accent.with_alpha(110)
                };
                cx.push_rect(grafikr::Rect::new(x, y, width, height), color, 3.0);
            }
            cx.push_rect(
                grafikr::Rect::new(rect.x, rect.bottom() - 1.0, rect.width, 1.0),
                cx.theme.muted.with_alpha(80),
                0.0,
            );
        }),
        virtual_list(50_000, 32.0, |index| {
            format!(
                "Transaksi #{:05}   Rp {:>9}   {}",
                index + 1,
                (index * 137) % 1_000_000,
                if index % 3 == 0 {
                    "selesai"
                } else {
                    "tertunda"
                }
            )
        })
        .width(920.0)
        .height(380.0)
        .selected(0)
        .on_select(move |index| {
            pick.set(format!("Baris {} dipilih dari 50.000", index + 1));
        }),
    ))
    .padding(24.0)
    .gap(12.0)
}

fn main() {
    App::new(dashboard)
        .title("Grafikr — Dashboard 50k")
        .theme(Theme::dark())
        .size(1000.0, 680.0)
        .run()
        .expect("event loop");
}
