//! Built-in widgets. Composition over inheritance; each widget is also a view.

mod button;
mod canvas;
mod combo;
mod containers;
mod controls;
mod data_grid;
mod image;
mod modal;
mod popup;
mod scroll;
mod spinner;
mod tabs;
mod text;
mod text_field;
mod tooltip;
mod tree_view;

pub use button::{Button, ButtonVariant, ClickEvent, button};
pub use canvas::{Canvas, canvas};
pub use combo::{ComboBox, combo_box};
pub use containers::{Column, Grid, Row, Stack, column, grid, row, stack};
pub use controls::{
    Checkbox, ProgressBar, Radio, Slider, Switch, checkbox, progress, radio, slider, switch,
};
pub use data_grid::{DataGrid, GridColumn, data_grid};
pub use image::{Image, image};
pub use modal::{Modal, modal};
pub use popup::{Popup, popup};
pub use scroll::{ScrollView, VirtualList, scroll_view, virtual_list};
pub use spinner::{Spinner, spinner};
pub use tabs::{Tabs, tabs};
pub use text::{Text, TextContent, text};
pub use text_field::{TextField, text_field};
pub use tooltip::{Tooltip, tooltip};
pub use tree_view::{TreeNode, TreeView, tree_view};

/// Implemented for every built-in widget so containers accept them as children.
macro_rules! into_views {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl grafikr_core::IntoViews for $ty {
                fn mount_into(&self, cx: &mut grafikr_core::MountCx) {
                    let _ = grafikr_core::View::mount(self, cx);
                }
            }
        )+
    };
}

pub(crate) use into_views;
