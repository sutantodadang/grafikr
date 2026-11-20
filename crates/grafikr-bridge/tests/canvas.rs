use grafikr_bridge::canvas::{CanvasCommand, CanvasModel};
use grafikr_core::{Color, Rect};

#[test]
fn canvas_replacement_is_atomic_and_rejects_non_finite_geometry() {
    let model = CanvasModel::new();
    model
        .replace(vec![CanvasCommand::Rect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::WHITE,
            radius: 0.0,
        }])
        .unwrap();
    let old_bytes = model.canonical_bytes();
    assert!(
        model
            .replace(vec![CanvasCommand::Rect {
                rect: Rect::new(f32::NAN, 0.0, 1.0, 1.0),
                color: Color::WHITE,
                radius: 0.0
            }])
            .is_err()
    );
    assert_eq!(model.command_count(), 1);
    assert_eq!(model.canonical_bytes(), old_bytes);
}
