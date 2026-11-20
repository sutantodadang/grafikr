use std::cell::Cell;
use std::rc::Rc;

use grafikr_core::{MountCx, NodeId, Size, Style, Theme, Ui, View};

struct TestTree;

impl View for TestTree {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let root = cx.add_node(Style::default());
        cx.children(root, |cx| {
            let left = cx.add_node(Style::default());
            cx.children(left, |cx| {
                cx.add_node(Style::default());
            });
            cx.add_node(Style::default());
            let parking = cx.add_node(Style::default());
            cx.tree.set_visibility(parking, Rc::new(Cell::new(false)));
        });
        root
    }
}

fn ui() -> (Ui, NodeId, NodeId, NodeId, NodeId, NodeId) {
    let mut ui = Ui::new(Theme::dark(), 1.0, Size::new(640.0, 480.0), "test");
    ui.mount(&TestTree);
    let root = ui.tree.root().unwrap();
    let left = ui.tree.children(root)[0];
    let child = ui.tree.children(left)[0];
    let sibling = ui.tree.children(root)[1];
    let parking = ui.tree.children(root)[2];
    (ui, root, left, child, sibling, parking)
}

#[test]
fn reparent_keeps_identity_and_invalid_moves_leave_tree_unchanged() {
    let (mut ui, _root, left, child, sibling, _parking) = ui();
    ui.tree.node_mut(child).interaction.focused = true;
    assert_eq!(
        ui.tree.reparent(child, sibling, 1),
        Err(grafikr_core::TreeEditError::InvalidIndex)
    );
    assert_eq!(
        ui.tree.reparent(left, child, 0),
        Err(grafikr_core::TreeEditError::Cycle)
    );
    assert_eq!(ui.tree.children(left), &[child]);
    ui.tree.reparent(child, sibling, 0).unwrap();
    assert_eq!(ui.tree.node(child).parent, Some(sibling));
    assert!(ui.tree.node(child).interaction.focused);
}

#[test]
fn detach_to_hidden_parking_removes_focus_capture_and_visibility() {
    let (mut ui, _root, _left, child, _sibling, parking) = ui();
    ui.focus = Some(child);
    ui.capture = Some(child);
    ui.detach_to(child, parking).unwrap();
    assert_eq!(ui.focus, None);
    assert_eq!(ui.capture, None);
    assert_eq!(ui.tree.node(child).parent, Some(parking));
    assert!(!ui.tree.is_visible(child));
}

#[test]
fn removed_subtree_clears_ui_references_and_stales_node_ids() {
    let (mut ui, _root, _left, child, _sibling, _parking) = ui();
    ui.focus = Some(child);
    ui.capture = Some(child);
    ui.remove_subtree(child).unwrap();
    assert_eq!(ui.focus, None);
    assert_eq!(ui.capture, None);
    assert!(ui.tree.get(child).is_none());
    assert_eq!(
        ui.remove_subtree(child),
        Err(grafikr_core::TreeEditError::StaleNode)
    );
}
