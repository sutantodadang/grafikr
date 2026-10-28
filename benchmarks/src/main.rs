//! Micro-benchmarks for the core paths. Run with:
//! `cargo run -p grafikr-bench --release`

use std::hint::black_box;
use std::time::Instant;

use grafikr::{
    Color, DirtyFlags, Point, Signal, Size, TextEngine, Theme, Ui, UiEvent, bind, column, text,
    virtual_list,
};

fn measure(name: &str, iterations: u32, mut f: impl FnMut()) {
    // Warm up allocators and caches.
    f();
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();
    println!(
        "{name:<34} {:>10.2} us/iter ({iterations} iters)",
        elapsed.as_secs_f64() * 1_000_000.0 / iterations as f64
    );
}

fn main() {
    let signal = Signal::new(0u64);
    measure("signal update (notify)", 200_000, || {
        signal.update(|value| *value += 1);
        black_box(signal.get());
    });

    let mut engine = TextEngine::new();
    measure("text shape (cold)", 2_000, || {
        black_box(engine.shape(
            "Benchmark text — Unicode ✓",
            14.0,
            Color::WHITE,
            Some(320.0),
        ));
    });

    let mut cache = grafikr::TextCache::default();
    let _ = cache.get(&mut engine, "Benchmark text", 14.0, Color::WHITE, None);
    measure("text shape (cache hit)", 200_000, || {
        black_box(
            cache
                .get(&mut engine, "Benchmark text", 14.0, Color::WHITE, None)
                .width,
        );
    });

    let value = Signal::new(0u64);
    let bound = value.clone();
    let children: Vec<_> = (0..1_000)
        .map(|index| {
            if index == 500 {
                let bound = bound.clone();
                text(bind(move || format!("value {}", bound.get())))
            } else {
                text(format!("row {index}"))
            }
        })
        .collect();
    let view = column(children).padding(8.0).gap(2.0);
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(960.0, 600.0), "bench");
    ui.mount(&view);
    ui.prepare_frame();
    let root = ui.tree.root().expect("root");

    measure("layout 1000 text nodes", 20, || {
        ui.tree.mark_dirty(root, DirtyFlags::LAYOUT);
        ui.prepare_frame();
        black_box(ui.display.items.len());
    });

    measure("update 1 label in 1000", 20, || {
        value.update(|value| *value += 1);
        ui.prepare_frame();
        black_box(ui.display.items.len());
    });

    measure("remount 1000-node tree", 20, || {
        ui.mount(&view);
        ui.prepare_frame();
        black_box(ui.tree.node_count());
    });
    let stats = ui.stats();
    println!(
        "tree after remount: {} nodes, {} slots, {} display items, {} text runs",
        stats.nodes, stats.slots, stats.display_items, stats.text_runs
    );

    let list = virtual_list(50_000, 24.0, |index| format!("row {index}"))
        .width(800.0)
        .height(400.0);
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(960.0, 600.0), "bench");
    ui.mount(&list);
    ui.prepare_frame();
    ui.handle_event(UiEvent::PointerMoved {
        pos: Point::new(100.0, 100.0),
    });
    measure("scroll virtual list 50k rows", 2_000, || {
        ui.handle_event(UiEvent::Wheel {
            delta: Point::new(0.0, 24.0),
        });
        ui.prepare_frame();
        black_box(ui.display.items.len());
    });
}
