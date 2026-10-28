//! Declarative views and mounting into the retained tree.

use crate::runtime::Runtime;
use crate::style::{Style, Theme};
use crate::text::TextEngine;
use crate::tree::{NodeId, Tree, Widget};

/// A declarative description that mounts itself into the retained tree.
pub trait View {
    fn mount(&self, cx: &mut MountCx) -> NodeId;
}

/// Multiple children accepted by containers.
///
/// A widget implements `IntoViews` once (usually forwarding to [`View`]);
/// tuples, vectors, and `()` mount their members into the current parent.
pub trait IntoViews {
    fn mount_into(&self, cx: &mut MountCx);
}

pub struct MountCx<'a> {
    pub tree: &'a mut Tree,
    pub text: &'a mut TextEngine,
    pub theme: &'a Theme,
    pub runtime: &'a Runtime,
    pub parent: Option<NodeId>,
}

impl MountCx<'_> {
    pub fn add_node(&mut self, style: Style) -> NodeId {
        self.tree.create_node(self.parent, style)
    }

    pub fn set_widget(&mut self, id: NodeId, widget: impl Widget + 'static) {
        self.tree.set_widget(id, Box::new(widget));
    }

    /// Mount children under `parent` and update Taffy once.
    pub fn children(&mut self, parent: NodeId, f: impl FnOnce(&mut Self)) {
        let previous = self.parent.replace(parent);
        f(self);
        self.parent = previous;
        self.tree.sync_children(parent);
    }
}

impl IntoViews for () {
    fn mount_into(&self, _cx: &mut MountCx) {}
}

impl<V: IntoViews> IntoViews for Vec<V> {
    fn mount_into(&self, cx: &mut MountCx) {
        for child in self {
            child.mount_into(cx);
        }
    }
}

impl<V: IntoViews, const N: usize> IntoViews for [V; N] {
    fn mount_into(&self, cx: &mut MountCx) {
        for child in self {
            child.mount_into(cx);
        }
    }
}

macro_rules! tuple_views {
    ($($name:ident),+) => {
        impl<$($name: IntoViews),+> IntoViews for ($($name,)+) {
            #[allow(non_snake_case)]
            fn mount_into(&self, cx: &mut MountCx) {
                let ($($name,)+) = self;
                $($name.mount_into(cx);)+
            }
        }
    };
}

tuple_views!(A);
tuple_views!(A, B);
tuple_views!(A, B, C);
tuple_views!(A, B, C, D);
tuple_views!(A, B, C, D, E);
tuple_views!(A, B, C, D, E, F);
tuple_views!(A, B, C, D, E, F, G);
tuple_views!(A, B, C, D, E, F, G, H);
tuple_views!(A, B, C, D, E, F, G, H, I);
tuple_views!(A, B, C, D, E, F, G, H, I, J);
tuple_views!(A, B, C, D, E, F, G, H, I, J, K);
tuple_views!(A, B, C, D, E, F, G, H, I, J, K, L);
tuple_views!(A, B, C, D, E, F, G, H, I, J, K, L, M);
tuple_views!(A, B, C, D, E, F, G, H, I, J, K, L, M, N);
tuple_views!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O);
tuple_views!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P);
