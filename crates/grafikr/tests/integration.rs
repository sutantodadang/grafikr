use std::cell::Cell;
use std::rc::Rc;

use grafikr::{
    Color, DisplayItem, GridColumn, Key, Modifiers, Point, Signal, Size, Theme, TreeNode, Ui,
    UiEvent, View, bind, button, canvas, checkbox, column, combo_box, data_grid, grid, image,
    modal, popup, radio, scroll_view, slider, spinner, stack, switch, tabs, text, text_field,
    tooltip, tree_view, virtual_list,
};

fn ui_for(view: &dyn View) -> Ui {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(480.0, 320.0), "Test");
    ui.mount(view);
    ui.prepare_frame();
    ui
}

fn center(ui: &Ui, id: grafikr::NodeId) -> Point {
    let rect = ui.tree.node(id).rect;
    Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5)
}

#[test]
fn column_layout_applies_padding_and_gap() {
    let view = column((text("first"), text("second")))
        .padding(16.0)
        .gap(10.0);
    let ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let children = ui.tree.children(root).to_vec();
    assert_eq!(children.len(), 2);

    let first = ui.tree.node(children[0]).rect;
    let second = ui.tree.node(children[1]).rect;
    assert_eq!(first.x, 16.0);
    assert_eq!(first.y, 16.0);
    assert!(first.width > 0.0 && first.height > 0.0);
    assert!(second.y >= first.bottom() + 10.0 - 0.1);
    assert!(
        ui.display
            .items
            .iter()
            .any(|item| matches!(item, DisplayItem::Text(_)))
    );
}

#[test]
fn button_click_updates_signal_once() {
    let clicks = Rc::new(Cell::new(0));
    let handler = {
        let clicks = clicks.clone();
        move |_| clicks.set(clicks.get() + 1)
    };
    let view = column((button("Go").on_click(handler),)).padding(10.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let button = ui.tree.children(root)[0];
    let point = center(&ui, button);

    ui.handle_event(UiEvent::PointerMoved { pos: point });
    ui.handle_event(UiEvent::PointerDown {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    assert_eq!(ui.focus, Some(button));
    ui.handle_event(UiEvent::PointerUp {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    assert_eq!(clicks.get(), 1);
}

#[test]
fn bound_text_marks_dirty_without_rebuilding() {
    let value = Signal::new(0);
    let bound = value.clone();
    let view = column((
        text("static"),
        text(bind(move || format!("value {}", bound.get()))),
    ))
    .padding(8.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let children_before = ui.tree.children(root).to_vec();

    value.update(|value| *value += 1);
    assert!(ui.runtime.needs_frame());
    ui.prepare_frame();
    assert!(!ui.runtime.needs_frame());

    let children_after = ui.tree.children(root).to_vec();
    assert_eq!(children_before, children_after, "tree must be retained");
}

#[test]
fn tab_moves_focus_between_controls() {
    let view = column((button("One"), button("Two").on_click(|_| {})))
        .padding(8.0)
        .gap(8.0);
    let ui_view = view;
    let mut ui = ui_for(&ui_view);

    ui.handle_event(UiEvent::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers::default(),
        repeat: false,
    });
    let first = ui.focus.expect("first control focused");
    ui.handle_event(UiEvent::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers::default(),
        repeat: false,
    });
    let second = ui.focus.expect("second control focused");
    assert_ne!(first, second);
}

#[test]
fn text_field_edits_and_reports() {
    let value = Signal::new(String::new());
    let target = value.clone();
    let view = column((text_field("")
        .width(200.0)
        .on_change(move |text| target.set(text)),))
    .padding(10.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let field = ui.tree.children(root)[0];
    let point = center(&ui, field);

    ui.handle_event(UiEvent::PointerDown {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    ui.handle_event(UiEvent::Text("hi".to_owned()));
    assert_eq!(value.get(), "hi");

    ui.handle_event(UiEvent::KeyDown {
        key: Key::Backspace,
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(value.get(), "h");
}

#[test]
fn resize_refills_root() {
    let view = column((text("hello"),)).padding(8.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    ui.set_viewport(Size::new(1024.0, 768.0));
    ui.prepare_frame();
    let rect = ui.tree.node(root).rect;
    assert_eq!(rect.width, 1024.0);
    assert_eq!(rect.height, 768.0);
}

#[test]
fn checkbox_and_switch_toggle() {
    let flag = Signal::new(false);
    let view = column((
        checkbox("Notify", flag.clone()),
        switch("Power", flag.clone()),
    ))
    .padding(8.0)
    .gap(8.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let checkbox_id = ui.tree.children(root)[0];
    let point = center(&ui, checkbox_id);
    ui.handle_event(UiEvent::PointerDown {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    assert!(flag.get());

    let switch_id = ui.tree.children(root)[1];
    let point = center(&ui, switch_id);
    ui.handle_event(UiEvent::PointerDown {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    assert!(!flag.get());
}

#[test]
fn radio_group_and_slider() {
    let plan = Signal::new(0usize);
    let volume = Signal::new(0.0f32);
    let view = column((
        radio("Free", plan.clone(), 0),
        radio("Pro", plan.clone(), 1),
        slider(volume.clone()).width(200.0),
    ))
    .padding(8.0)
    .gap(8.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let pro = ui.tree.children(root)[1];
    let point = center(&ui, pro);
    ui.handle_event(UiEvent::PointerDown {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    assert_eq!(plan.get(), 1);

    let slider = ui.tree.children(root)[2];
    let rect = ui.tree.node(slider).rect;
    let point = Point::new(rect.x + rect.width * 0.75, rect.y + rect.height * 0.5);
    ui.handle_event(UiEvent::PointerDown {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    assert!(volume.get() > 0.6 && volume.get() < 0.9);
}

#[test]
fn scroll_view_translates_content_and_clamps() {
    let children: Vec<_> = (0..30).map(|index| text(format!("row {index}"))).collect();
    let view = scroll_view(column(children).gap(4.0))
        .width(200.0)
        .height(100.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let content = ui.tree.children(root)[0];
    let first = ui.tree.children(content)[0];
    let before = ui.tree.node(first).rect;

    let point = Point::new(20.0, 40.0);
    ui.handle_event(UiEvent::PointerMoved { pos: point });
    ui.handle_event(UiEvent::Wheel {
        delta: Point::new(0.0, 60.0),
    });
    ui.prepare_frame();

    let state = ui.tree.node(root).scroll.clone().unwrap();
    assert!(state.offset() > 0.0);
    let after = ui.tree.node(first).rect;
    assert!((before.y - after.y - state.offset()).abs() < 0.6);

    ui.handle_event(UiEvent::Wheel {
        delta: Point::new(0.0, 10_000.0),
    });
    ui.prepare_frame();
    assert!(state.at_end());
}

#[test]
fn virtual_list_paints_only_visible_rows() {
    let view = virtual_list(50_000, 24.0, |index| format!("row {index}"))
        .width(300.0)
        .height(200.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let painted = ui
        .display
        .items
        .iter()
        .filter(|item| matches!(item, DisplayItem::Text(_)))
        .count();
    assert!(painted > 0 && painted < 20, "painted {painted} rows");

    ui.handle_event(UiEvent::PointerMoved {
        pos: Point::new(100.0, 60.0),
    });
    ui.handle_event(UiEvent::Wheel {
        delta: Point::new(0.0, 2_000.0),
    });
    ui.prepare_frame();
    let state = ui.tree.node(root).scroll.clone().unwrap();
    assert!(state.offset() > 0.0);
    let painted_after = ui
        .display
        .items
        .iter()
        .filter(|item| matches!(item, DisplayItem::Text(_)))
        .count();
    assert!(painted_after < 20, "painted {painted_after} rows");
}

#[test]
fn canvas_custom_paint_emits_rects() {
    let view = canvas(120.0, 60.0, |cx, rect| {
        cx.push_rect(rect, Color::WHITE, 4.0);
    });
    let ui = ui_for(&view);
    assert!(
        ui.display
            .items
            .iter()
            .any(|item| matches!(item, DisplayItem::Rect { .. }))
    );
}

#[test]
fn modal_traps_and_restores_focus() {
    let open = Signal::new(false);
    let opener = open.clone();
    let view = column((
        button("Open").on_click(move |_| opener.set(true)),
        modal(
            open.clone(),
            (
                text("Dialog"),
                button("Close").on_click({
                    let open = open.clone();
                    move |_| open.set(false)
                }),
            ),
        )
        .width(320.0),
    ))
    .padding(12.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let open_button = ui.tree.children(root)[0];
    let overlay = ui.tree.children(root)[1];

    let point = center(&ui, open_button);
    ui.handle_event(UiEvent::PointerDown {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    ui.handle_event(UiEvent::PointerUp {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    ui.prepare_frame();
    assert!(open.get());
    assert!(ui.tree.node(overlay).applied_visible);

    let focus = ui.focus.expect("dialog control focused");
    assert!(
        ui.tree.is_within(focus, overlay),
        "focus must be inside modal"
    );
    assert!(
        ui.display
            .items
            .iter()
            .any(|item| matches!(item, DisplayItem::Shadow { .. })),
        "modal card should cast a shadow"
    );

    ui.handle_event(UiEvent::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers::default(),
        repeat: false,
    });
    let focus = ui.focus.expect("still focused");
    assert!(ui.tree.is_within(focus, overlay), "Tab must stay in modal");

    ui.handle_event(UiEvent::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
        repeat: false,
    });
    ui.prepare_frame();
    assert!(!open.get());
    assert!(!ui.tree.node(overlay).applied_visible);
    assert_eq!(ui.focus, Some(open_button), "focus restored");
}

#[test]
fn tabs_switch_visible_pane() {
    let active = Signal::new(0usize);
    let view = column((
        tabs(["A", "B"], active.clone(), (text("pane A"), text("pane B")))
            .width(300.0)
            .height(200.0),
    ));
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let tab_root = ui.tree.children(root)[0];
    let panes = ui.tree.children(tab_root)[0];
    let pane_a = ui.tree.children(panes)[0];
    let pane_b = ui.tree.children(panes)[1];
    assert!(ui.tree.node(pane_a).applied_visible);
    assert!(!ui.tree.node(pane_b).applied_visible);

    let rect = ui.tree.node(tab_root).rect;
    let point = Point::new(rect.x + rect.width * 0.75, rect.y + 20.0);
    ui.handle_event(UiEvent::PointerDown {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    ui.prepare_frame();
    assert_eq!(active.get(), 1);
    assert!(!ui.tree.node(pane_a).applied_visible);
    assert!(ui.tree.node(pane_b).applied_visible);
}

#[test]
fn popup_closes_on_outside_click() {
    let open = Signal::new(false);
    let opener = open.clone();
    let view = column((
        button("Menu").on_click(move |_| opener.set(true)),
        popup(open.clone(), 10.0, 60.0, (button("One"), button("Two"))).width(160.0),
    ))
    .padding(8.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let menu_button = ui.tree.children(root)[0];
    let layer = ui.tree.children(root)[1];

    let point = center(&ui, menu_button);
    ui.handle_event(UiEvent::PointerDown {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    ui.handle_event(UiEvent::PointerUp {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    ui.prepare_frame();
    assert!(open.get());
    assert!(ui.tree.node(layer).applied_visible);
    let focus = ui.focus.expect("menu item focused");
    assert!(ui.tree.is_within(focus, layer));

    ui.handle_event(UiEvent::PointerDown {
        pos: Point::new(2.0, 2.0),
        button: grafikr::MouseButton::Left,
    });
    ui.prepare_frame();
    assert!(!open.get());
    assert!(!ui.tree.node(layer).applied_visible);
    assert_eq!(ui.focus, Some(menu_button));
}

#[test]
fn multiline_field_inserts_newline() {
    let value = Signal::new(String::new());
    let target = value.clone();
    let view = column((text_field("")
        .width(200.0)
        .multiline()
        .on_change(move |text| target.set(text)),))
    .padding(10.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let field = ui.tree.children(root)[0];
    let point = center(&ui, field);

    ui.handle_event(UiEvent::PointerDown {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    ui.handle_event(UiEvent::Text("a".to_owned()));
    ui.handle_event(UiEvent::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::default(),
        repeat: false,
    });
    ui.handle_event(UiEvent::Text("b".to_owned()));
    assert_eq!(value.get(), "a\nb");
}

#[test]
fn grid_places_items_in_columns() {
    let view = grid(2, (text("a"), text("b"), text("c"), text("d")))
        .padding(10.0)
        .gap(10.0);
    let ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let children = ui.tree.children(root).to_vec();
    assert_eq!(children.len(), 4);
    let a = ui.tree.node(children[0]).rect;
    let b = ui.tree.node(children[1]).rect;
    let c = ui.tree.node(children[2]).rect;
    assert!(b.x > a.x, "second item should be in the second column");
    assert!((b.y - a.y).abs() < 1.0, "first row items share y");
    assert!(c.y > a.y, "third item should be on the next row");
}

#[test]
fn image_emits_display_item() {
    let data = grafikr::ImageData::new_rgba(2, 2, vec![255; 16]);
    let view = column((image(data).width(100.0).height(50.0),));
    let ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let node = ui.tree.children(root)[0];
    assert_eq!(ui.tree.node(node).rect.width, 100.0);
    assert!(
        ui.display
            .items
            .iter()
            .any(|item| matches!(item, DisplayItem::Image { .. }))
    );
}

#[test]
fn spinner_requests_animation_frames() {
    let view = column((spinner(),));
    let mut ui = ui_for(&view);
    assert!(ui.runtime.is_animating());
    assert!(
        ui.display
            .items
            .iter()
            .filter(|item| matches!(item, DisplayItem::Rect { .. }))
            .count()
            >= 8
    );

    ui.prepare_frame();
    assert!(ui.runtime.is_animating());
    assert!(!ui.display.is_empty());
}

#[test]
fn combo_box_selects_option() {
    let selected = Signal::new(0usize);
    let view = column((combo_box(["Alpha", "Beta", "Gamma"], selected.clone()).width(200.0),))
        .padding(10.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let control = ui.tree.children(root)[0];
    let card = ui.tree.children(control)[1];
    let list = ui.tree.children(card)[0];

    let point = center(&ui, control);
    ui.handle_event(UiEvent::PointerDown {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    ui.prepare_frame();
    assert!(ui.tree.node(card).applied_visible);
    let focus = ui.focus.expect("option list focused");
    assert!(ui.tree.is_within(focus, card));

    let list_rect = ui.tree.node(list).rect;
    let point = Point::new(list_rect.x + 10.0, list_rect.y + 2.0 * 28.0 + 14.0);
    ui.handle_event(UiEvent::PointerDown {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    ui.prepare_frame();
    assert_eq!(selected.get(), 2);
    assert!(!ui.tree.node(card).applied_visible);
    assert_eq!(ui.focus, Some(control));
}

#[test]
fn data_grid_virtualizes_rows_and_selects() {
    let picked = Rc::new(Cell::new(usize::MAX));
    let target = picked.clone();
    let view = column((data_grid(
        [
            GridColumn::new("Name", 160.0),
            GridColumn::new("Value", 100.0),
        ],
        50_000,
        26.0,
        |index| vec![format!("row {index}"), format!("value {index}")],
    )
    .width(300.0)
    .height(240.0)
    .on_select(move |index| target.set(index)),))
    .padding(8.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let grid = ui.tree.children(root)[0];
    let painted = ui
        .display
        .items
        .iter()
        .filter(|item| matches!(item, DisplayItem::Text(_)))
        .count();
    assert!(painted > 0 && painted < 100, "painted {painted} cells");

    let rect = ui.tree.node(grid).rect;
    let point = Point::new(rect.x + 20.0, rect.y + 32.0 + 3.0 * 26.0 + 13.0);
    ui.handle_event(UiEvent::PointerDown {
        pos: point,
        button: grafikr::MouseButton::Left,
    });
    assert_eq!(picked.get(), 3);
}

#[test]
fn tree_view_expands_and_selects() {
    let picked = Rc::new(Cell::new(usize::MAX));
    let target = picked.clone();
    let view = column((tree_view([
        TreeNode::new("A")
            .child(TreeNode::new("A1"))
            .child(TreeNode::new("A2")),
        TreeNode::new("B"),
    ])
    .width(240.0)
    .height(200.0)
    .on_select(move |id| target.set(id)),))
    .padding(8.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let tree = ui.tree.children(root)[0];
    let rect = ui.tree.node(tree).rect;

    let marker = Point::new(rect.x + 10.0, rect.y + 13.0);
    ui.handle_event(UiEvent::PointerDown {
        pos: marker,
        button: grafikr::MouseButton::Left,
    });
    ui.prepare_frame();
    let painted = ui
        .display
        .items
        .iter()
        .filter(|item| matches!(item, DisplayItem::Text(_)))
        .count();
    assert!(painted >= 4, "expanded tree should show 4 rows: {painted}");

    let child = Point::new(rect.x + 50.0, rect.y + 26.0 + 13.0);
    ui.handle_event(UiEvent::PointerDown {
        pos: child,
        button: grafikr::MouseButton::Left,
    });
    assert_eq!(picked.get(), 1);
}

#[test]
fn tooltip_shows_after_hover_and_hides() {
    let view = column((tooltip("Hint", button("Hover")).delay_ms(0),)).padding(10.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let wrapper = ui.tree.children(root)[0];
    let button_id = ui.tree.children(wrapper)[0];
    let before = ui
        .display
        .items
        .iter()
        .filter(|item| matches!(item, DisplayItem::Text(_)))
        .count();

    let point = center(&ui, button_id);
    ui.handle_event(UiEvent::PointerMoved { pos: point });
    ui.prepare_frame();
    let after = ui
        .display
        .items
        .iter()
        .filter(|item| matches!(item, DisplayItem::Text(_)))
        .count();
    assert!(after > before, "tooltip text should be painted");

    ui.handle_event(UiEvent::PointerLeft);
    ui.prepare_frame();
    let hidden = ui
        .display
        .items
        .iter()
        .filter(|item| matches!(item, DisplayItem::Text(_)))
        .count();
    assert_eq!(hidden, before);
}

#[test]
fn icon_button_is_compact() {
    let view = column((button("X").icon(),)).padding(8.0);
    let ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let button = ui.tree.children(root)[0];
    let rect = ui.tree.node(button).rect;
    assert!(rect.width <= 80.0);
    assert!(rect.width >= rect.height);
}

#[test]
fn password_field_marks_text_as_masked() {
    let view = column((text_field("secret").password().width(200.0),)).padding(8.0);
    let ui = ui_for(&view);
    let masked = ui.display.items.iter().any(|item| match item {
        DisplayItem::Text(run) => run.mask,
        _ => false,
    });
    assert!(masked, "password text run must be masked");
}

#[test]
fn stack_overlays_children() {
    let view = column((stack((text("a"), text("b"))).width(200.0).height(100.0),));
    let ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let stack_node = ui.tree.children(root)[0];
    let children = ui.tree.children(stack_node).to_vec();
    assert_eq!(children.len(), 2);
    let a = ui.tree.node(children[0]).rect;
    let b = ui.tree.node(children[1]).rect;
    assert_eq!(a, b);
    assert_eq!(a.width, 200.0);
}

#[test]
fn accessibility_tree_exposes_controls() {
    let view = column((text("Name"), text_field(""), button("Save"))).padding(8.0);
    let mut ui = ui_for(&view);
    let update = ui.a11y_update();
    assert!(update.nodes.len() >= 3);
    assert_eq!(update.focus, update.tree.as_ref().unwrap().root);

    let roles: Vec<accesskit::Role> = update.nodes.iter().map(|(_, node)| node.role()).collect();
    assert!(roles.contains(&accesskit::Role::Button));
    assert!(roles.contains(&accesskit::Role::TextInput));
    assert!(roles.contains(&accesskit::Role::Label));
}

#[test]
fn hover_changes_button_pixels_without_layout() {
    let view = column((button("Hover"),)).padding(8.0);
    let mut ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let button_id = ui.tree.children(root)[0];
    let point = center(&ui, button_id);
    let before = ui.display.items.first().cloned();
    ui.handle_event(UiEvent::PointerMoved { pos: point });
    std::thread::sleep(std::time::Duration::from_millis(30));
    ui.prepare_frame();
    let after = ui.display.items.first().cloned();
    assert_ne!(before, after);
    assert!(
        ui.runtime.is_animating(),
        "hover fade keeps requesting frames"
    );
}

#[test]
fn transform_shifts_paint_and_hit_testing() {
    let view = column((stack((button("Go"),))
        .width(100.0)
        .height(40.0)
        .transform(grafikr::Transform::translate(40.0, 0.0)),))
    .padding(10.0);
    let ui = ui_for(&view);
    let root = ui.tree.root().unwrap();
    let stack_node = ui.tree.children(root)[0];
    let button_node = ui.tree.children(stack_node)[0];

    assert_eq!(ui.tree.hit_test(Point::new(80.0, 30.0)), Some(button_node));
    assert_eq!(ui.tree.hit_test(Point::new(20.0, 30.0)), None);

    let text = ui
        .display
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Text(run) => Some(run),
            _ => None,
        })
        .expect("button label");
    assert!(text.glyphs.iter().all(|glyph| glyph.x >= 50.0));
}

#[test]
fn remount_replaces_the_previous_tree() {
    let mut ui = ui_for(&column((text("old a"), text("old b"))));
    let old_root = ui.tree.root().unwrap();
    ui.mount(&column((text("new"),)));
    assert!(ui.tree.get(old_root).is_none(), "old tree must be removed");
    let new_root = ui.tree.root().unwrap();
    assert_ne!(old_root, new_root);
}

#[test]
fn accessibility_set_value_updates_text_field() {
    let value = Signal::new(String::new());
    let sink = value.clone();
    let view = column((text_field("a").on_change(move |text| sink.set(text)),)).padding(8.0);
    let mut ui = ui_for(&view);
    ui.handle_event(UiEvent::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers::default(),
        repeat: false,
    });
    let field = ui.focus.expect("text field focused");
    let request = accesskit::ActionRequest {
        action: accesskit::Action::SetValue,
        target_tree: accesskit::TreeId::ROOT,
        target_node: accesskit::NodeId(field.to_u64()),
        data: Some(accesskit::ActionData::Value("bar".into())),
    };
    assert!(ui.handle_a11y_action(&request));
    assert_eq!(value.get(), "bar");
}
