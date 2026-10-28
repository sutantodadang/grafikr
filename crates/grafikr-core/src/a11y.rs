//! AccessKit tree projection. Built on demand from the retained tree.

use accesskit::{Node, NodeId as A11yId, Role, TreeInfo, TreeUpdate};

use crate::tree::{NodeId, Tree};

fn a11y_id(id: NodeId) -> A11yId {
    A11yId(id.to_u64())
}

pub fn tree_update(tree: &mut Tree, focus: Option<NodeId>, title: &str) -> TreeUpdate {
    let Some(root) = tree.root() else {
        return TreeUpdate {
            nodes: Vec::new(),
            tree: Some(TreeInfo::new(A11yId(0))),
            tree_id: accesskit::TreeId::ROOT,
            focus: A11yId(0),
        };
    };

    let mut nodes = Vec::new();
    build(tree, root, title, &mut nodes);
    let root_id = a11y_id(root);
    TreeUpdate {
        nodes,
        tree: Some(TreeInfo::new(root_id)),
        tree_id: accesskit::TreeId::ROOT,
        focus: focus.map(a11y_id).unwrap_or(root_id),
    }
}

fn build(tree: &mut Tree, id: NodeId, title: &str, nodes: &mut Vec<(A11yId, Node)>) {
    let (rect, children, mut builder) = {
        let Some(node) = tree.get_mut(id) else {
            return;
        };
        if !node.applied_visible {
            return;
        }
        let rect = node.rect;
        let children = node.children.clone();
        let is_root = node.parent.is_none();
        let mut builder = Node::new(if is_root {
            Role::Window
        } else {
            Role::GenericContainer
        });
        if node.modal_scope {
            builder.set_modal();
        }
        builder.set_bounds(accesskit::Rect {
            x0: rect.x as f64,
            y0: rect.y as f64,
            x1: rect.right() as f64,
            y1: rect.bottom() as f64,
        });
        if is_root {
            builder.set_label(title.to_owned());
        }
        if let Some(widget) = node.widget.as_mut() {
            widget.accessibility(&mut builder);
        }
        (rect, children, builder)
    };
    let _ = rect;

    for child in &children {
        build(tree, *child, title, nodes);
    }
    let child_ids: Vec<A11yId> = children.iter().map(|child| a11y_id(*child)).collect();
    builder.set_children(child_ids);
    nodes.push((a11y_id(id), builder));
}
