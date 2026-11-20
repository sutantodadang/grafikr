use std::cell::RefCell;
use std::rc::Rc;

use grafikr_core::{
    Key, Modifiers, MountCx, MouseButton, Point, Size, Style, Theme, Ui, UiEvent, View,
};
use grafikr_widgets::{
    ButtonVariant, DynamicEvent, DynamicValue, LayoutKind, TreeNode, WidgetError, WidgetPatch,
    WidgetSpec, mount,
};

struct Root;
impl View for Root {
    fn mount(&self, cx: &mut MountCx) -> grafikr_core::NodeId {
        cx.add_node(Style::default())
    }
}

#[test]
fn dynamic_text_patches_keep_the_mounted_node_and_validate_patch_kind() {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(400.0, 300.0), "test");
    ui.mount(&Root);
    let root = ui.tree.root().unwrap();
    let mut text = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::Text {
                    text: "before".into(),
                    size: None,
                    color: None,
                },
                cx,
                Rc::new(|_| {}),
            )
            .unwrap()
        })
        .unwrap();
    let node = text.root();
    text.apply(
        WidgetPatch::Text {
            text: Some("after".into()),
            size: Some(18.0),
            color: None,
        },
        &mut ui,
    )
    .unwrap();
    assert_eq!(text.root(), node);
    assert_eq!(ui.tree.node_count(), 2);
    assert_eq!(
        text.apply(
            WidgetPatch::Button {
                label: Some("wrong".into()),
                variant: None,
                icon: None,
                disabled: None
            },
            &mut ui
        ),
        Err(WidgetError::WrongWidget)
    );
}

#[test]
fn dynamic_tabs_expose_a_named_pane_slot_and_accept_active_updates() {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(400.0, 300.0), "test");
    ui.mount(&Root);
    let root = ui.tree.root().unwrap();
    let mut tabs = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::Tabs {
                    labels: vec!["One".into(), "Two".into()],
                    active: 0,
                    width: Some(300.0),
                    height: Some(180.0),
                },
                cx,
                Rc::new(|_| {}),
            )
            .unwrap()
        })
        .unwrap();
    assert_eq!(tabs.child_slot(), Some("panes"));
    assert_ne!(tabs.child_parent(), tabs.root());
    tabs.apply(
        WidgetPatch::Tabs {
            active: Some(1),
            width: None,
            height: None,
        },
        &mut ui,
    )
    .unwrap();
    tabs.children_changed(&mut ui).unwrap();
}

#[test]
fn dynamic_tooltip_exposes_content_slot_and_updates_label_and_delay() {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(400.0, 300.0), "test");
    ui.mount(&Root);
    let root = ui.tree.root().unwrap();
    let mut tooltip = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::Tooltip {
                    label: "Hint".into(),
                    delay_ms: 450,
                },
                cx,
                Rc::new(|_| {}),
            )
            .unwrap()
        })
        .unwrap();
    assert_eq!(tooltip.child_slot(), Some("content"));
    assert_eq!(tooltip.child_parent(), tooltip.root());
    ui.runtime.clear_frame();
    tooltip
        .apply(
            WidgetPatch::Tooltip {
                label: Some("Updated".into()),
                delay_ms: Some(0),
            },
            &mut ui,
        )
        .unwrap();
    ui.runtime.clear_frame();
    tooltip.children_changed(&mut ui).unwrap();
    assert!(ui.runtime.needs_frame());
}

#[test]
fn dynamic_image_patches_resource_and_geometry_in_place() {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(400.0, 300.0), "test");
    ui.mount(&Root);
    let root = ui.tree.root().unwrap();
    let first = grafikr_core::ImageData::new_rgba(1, 1, vec![255, 0, 0, 255]);
    let second = grafikr_core::ImageData::new_rgba(2, 1, vec![0, 0, 255, 255, 0, 0, 255, 255]);
    let mut image = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::Image {
                    data: first,
                    label: "before".into(),
                    width: None,
                    height: None,
                },
                cx,
                Rc::new(|_| {}),
            )
            .unwrap()
        })
        .unwrap();
    let id = image.root();
    image
        .apply(
            WidgetPatch::Image {
                data: Some(second),
                label: Some("after".into()),
                width: Some(20.0),
                height: Some(10.0),
            },
            &mut ui,
        )
        .unwrap();
    assert_eq!(image.root(), id);
    assert_eq!(ui.tree.node(id).style.width, Some(20.0));
    assert_eq!(ui.tree.node(id).style.height, Some(10.0));
}

#[test]
fn dynamic_button_click_emits_one_event_and_disabled_state_blocks_activation() {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(400.0, 300.0), "test");
    ui.mount(&Root);
    let root = ui.tree.root().unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink_events = events.clone();
    let mut button = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::Button {
                    label: "Run".into(),
                    variant: ButtonVariant::Secondary,
                    icon: false,
                },
                cx,
                Rc::new(move |event| sink_events.borrow_mut().push(event)),
            )
            .unwrap()
        })
        .unwrap();
    let id = button.root();
    ui.prepare_frame();
    let rect = ui.tree.node(id).rect;
    let pos = Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
    ui.handle_event(UiEvent::PointerDown {
        pos,
        button: MouseButton::Left,
    });
    ui.handle_event(UiEvent::PointerUp {
        pos,
        button: MouseButton::Left,
    });
    assert_eq!(
        events.borrow().as_slice(),
        &[DynamicEvent::Clicked { node: id }]
    );
    button
        .apply(
            WidgetPatch::Button {
                label: None,
                variant: None,
                icon: None,
                disabled: Some(true),
            },
            &mut ui,
        )
        .unwrap();
    ui.handle_event(UiEvent::PointerDown {
        pos,
        button: MouseButton::Left,
    });
    ui.handle_event(UiEvent::PointerUp {
        pos,
        button: MouseButton::Left,
    });
    assert_eq!(events.borrow().len(), 1);
}

#[test]
fn layout_specs_accept_valid_grid_columns_and_reject_zero() {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(100.0, 100.0), "test");
    ui.mount(&Root);
    let root = ui.tree.root().unwrap();
    let mut grid = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::Layout {
                    kind: LayoutKind::Grid(2),
                    style: Style::default(),
                },
                cx,
                Rc::new(|_| {}),
            )
            .unwrap()
        })
        .unwrap();
    assert!(grid.child_slot().is_some());
    assert_eq!(
        grid.apply(
            WidgetPatch::Layout(Style {
                grid_columns: Some(0),
                ..Style::default()
            }),
            &mut ui
        ),
        Err(WidgetError::InvalidProperty)
    );
}

#[test]
fn unchanged_field_value_and_sibling_patch_preserve_ime_composition() {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(400.0, 300.0), "test");
    ui.mount(&Root);
    let root = ui.tree.root().unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink_events = events.clone();
    let field = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::TextField {
                    text: "start".into(),
                    width: None,
                    multiline: false,
                    password: false,
                },
                cx,
                Rc::new(move |event| sink_events.borrow_mut().push(event)),
            )
            .unwrap()
        })
        .unwrap();
    let mut sibling = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::Text {
                    text: "label".into(),
                    size: None,
                    color: None,
                },
                cx,
                Rc::new(|_| {}),
            )
            .unwrap()
        })
        .unwrap();
    ui.focus = Some(field.root());
    ui.handle_event(UiEvent::ImePreedit {
        text: "かな".into(),
        cursor: Some((2, 2)),
    });
    ui.set_node_text_value(field.root(), "start").unwrap();
    sibling
        .apply(
            WidgetPatch::Text {
                text: Some("updated sibling".into()),
                size: None,
                color: None,
            },
            &mut ui,
        )
        .unwrap();
    ui.handle_event(UiEvent::ImeCommit("かな".into()));
    assert_eq!(
        events.borrow().as_slice(),
        &[DynamicEvent::ValueChanged {
            node: field.root(),
            value: DynamicValue::Text("かなstart".into())
        }]
    );
}

#[test]
fn control_events_are_reported_once_and_programmatic_state_patches_are_silent() {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(400.0, 300.0), "test");
    ui.mount(&Root);
    let root = ui.tree.root().unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink_events = events.clone();
    let mut checkbox = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::Checkbox {
                    label: "Enabled".into(),
                    checked: false,
                },
                cx,
                Rc::new(move |event| sink_events.borrow_mut().push(event)),
            )
            .unwrap()
        })
        .unwrap();
    ui.prepare_frame();
    let rect = ui.tree.node(checkbox.root()).rect;
    let pos = Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
    ui.handle_event(UiEvent::PointerDown {
        pos,
        button: MouseButton::Left,
    });
    ui.handle_event(UiEvent::PointerUp {
        pos,
        button: MouseButton::Left,
    });
    assert_eq!(
        events.borrow().as_slice(),
        &[DynamicEvent::ValueChanged {
            node: checkbox.root(),
            value: DynamicValue::Bool(true)
        }]
    );
    checkbox
        .apply(WidgetPatch::Checkbox { checked: false }, &mut ui)
        .unwrap();
    assert_eq!(events.borrow().len(), 1);
}

#[test]
fn slider_range_is_checked_and_keyboard_input_emits_typed_value() {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(400.0, 300.0), "test");
    ui.mount(&Root);
    let root = ui.tree.root().unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink_events = events.clone();
    let mut slider = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::Slider {
                    value: 0.5,
                    min: 0.0,
                    max: 1.0,
                    step: 0.1,
                    width: 200.0,
                },
                cx,
                Rc::new(move |event| sink_events.borrow_mut().push(event)),
            )
            .unwrap()
        })
        .unwrap();
    assert_eq!(
        slider.apply(WidgetPatch::Slider { value: f32::NAN }, &mut ui),
        Err(WidgetError::InvalidProperty)
    );
    ui.prepare_frame();
    let rect = ui.tree.node(slider.root()).rect;
    let pos = Point::new(rect.x + rect.width * 0.65, rect.y + rect.height * 0.5);
    ui.handle_event(UiEvent::PointerDown {
        pos,
        button: MouseButton::Left,
    });
    ui.handle_event(UiEvent::PointerUp {
        pos,
        button: MouseButton::Left,
    });
    ui.handle_event(UiEvent::KeyDown {
        key: Key::Right,
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert!(events.borrow().iter().any(|event| matches!(event, DynamicEvent::ValueChanged { node, value: DynamicValue::F32(value) } if *node == slider.root() && *value > 0.65)));
}

#[test]
fn scroll_and_modal_expose_their_real_content_slots_without_remounting() {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(400.0, 300.0), "test");
    ui.mount(&Root);
    let root = ui.tree.root().unwrap();
    let scroll = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::ScrollView {
                    width: None,
                    height: Some(120.0),
                    grow: false,
                },
                cx,
                Rc::new(|_| {}),
            )
            .unwrap()
        })
        .unwrap();
    assert_eq!(scroll.child_slot(), Some("content"));
    assert_ne!(scroll.child_parent(), scroll.root());
    let mut modal = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::Modal {
                    open: true,
                    width: Some(300.0),
                },
                cx,
                Rc::new(|_| {}),
            )
            .unwrap()
        })
        .unwrap();
    assert_eq!(modal.internal_node_count(), 2);
    assert_eq!(modal.child_slot(), Some("content"));
    let card = modal.child_parent();
    let text = ui
        .with_mount_cx(card, |cx| {
            mount(
                WidgetSpec::Text {
                    text: "inside".into(),
                    size: None,
                    color: None,
                },
                cx,
                Rc::new(|_| {}),
            )
            .unwrap()
        })
        .unwrap();
    modal
        .apply(
            WidgetPatch::Modal {
                open: Some(false),
                width: Some(320.0),
            },
            &mut ui,
        )
        .unwrap();
    assert!(ui.tree.get(text.root()).is_some());
    modal.children_changed(&mut ui).unwrap();
}

#[test]
fn tree_selection_is_preserved_by_key_and_explicit_selection_patches_apply() {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(400.0, 300.0), "test");
    ui.mount(&Root);
    let root = ui.tree.root().unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink_events = events.clone();
    let mut tree = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::TreeView {
                    roots: vec![
                        TreeNode::with_key("a", "A"),
                        TreeNode::with_key("b", "B"),
                        TreeNode::with_key("c", "C"),
                    ],
                    keys: vec!["a".into(), "b".into(), "c".into()],
                    selected: None,
                    width: None,
                    height: 130.0,
                },
                cx,
                Rc::new(move |event| sink_events.borrow_mut().push(event)),
            )
            .unwrap()
        })
        .unwrap();

    ui.prepare_frame();
    let rect = ui.tree.node(tree.root()).rect;
    ui.handle_event(UiEvent::PointerDown {
        pos: Point::new(rect.x + 40.0, rect.y + 26.0 + 10.0),
        button: MouseButton::Left,
    });
    assert_eq!(
        events.borrow().as_slice(),
        &[DynamicEvent::KeySelected { key: "b".into() }]
    );

    tree.apply(
        WidgetPatch::TreeView {
            roots: Some(vec![
                TreeNode::with_key("a", "A"),
                TreeNode::with_key("c", "C"),
                TreeNode::with_key("b", "B"),
            ]),
            selected: None,
            width: None,
            height: None,
        },
        &mut ui,
    )
    .unwrap();
    ui.focus = Some(tree.root());
    ui.handle_event(UiEvent::KeyDown {
        key: Key::Down,
        modifiers: Modifiers::default(),
        repeat: false,
    });

    assert_eq!(
        events.borrow().as_slice(),
        &[DynamicEvent::KeySelected { key: "b".into() }]
    );

    tree.apply(
        WidgetPatch::TreeView {
            roots: None,
            selected: Some("a".into()),
            width: None,
            height: None,
        },
        &mut ui,
    )
    .unwrap();
    ui.prepare_frame();
    ui.handle_event(UiEvent::KeyDown {
        key: Key::Down,
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(
        events.borrow().as_slice(),
        &[
            DynamicEvent::KeySelected { key: "b".into() },
            DynamicEvent::KeySelected { key: "c".into() },
        ]
    );
}

#[test]
fn tree_expansion_survives_reordering_when_its_key_remains() {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(400.0, 300.0), "test");
    ui.mount(&Root);
    let root = ui.tree.root().unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink_events = events.clone();
    let mut tree = ui
        .with_mount_cx(root, |cx| {
            mount(
                WidgetSpec::TreeView {
                    roots: vec![
                        TreeNode::with_key("folder", "Folder")
                            .child(TreeNode::with_key("leaf", "Leaf")),
                        TreeNode::with_key("other", "Other"),
                    ],
                    keys: vec!["folder".into(), "leaf".into(), "other".into()],
                    selected: None,
                    width: None,
                    height: 130.0,
                },
                cx,
                Rc::new(move |event| sink_events.borrow_mut().push(event)),
            )
            .unwrap()
        })
        .unwrap();

    ui.prepare_frame();
    let rect = ui.tree.node(tree.root()).rect;
    ui.handle_event(UiEvent::PointerDown {
        pos: Point::new(rect.x + 10.0, rect.y + 10.0),
        button: MouseButton::Left,
    });
    ui.handle_event(UiEvent::PointerUp {
        pos: Point::new(rect.x + 10.0, rect.y + 10.0),
        button: MouseButton::Left,
    });
    tree.apply(
        WidgetPatch::TreeView {
            roots: Some(vec![
                TreeNode::with_key("other", "Other"),
                TreeNode::with_key("folder", "Folder").child(TreeNode::with_key("leaf", "Leaf")),
            ]),
            selected: None,
            width: None,
            height: None,
        },
        &mut ui,
    )
    .unwrap();
    ui.prepare_frame();

    for _ in 0..3 {
        ui.handle_event(UiEvent::KeyDown {
            key: Key::Down,
            modifiers: Modifiers::default(),
            repeat: false,
        });
    }
    assert_eq!(
        events.borrow().last(),
        Some(&DynamicEvent::KeySelected { key: "leaf".into() })
    );
}
