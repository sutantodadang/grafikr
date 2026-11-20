use std::cell::Cell;
use std::rc::Rc;

use grafikr_core::{
    Color, Direction, DirtyFlags, MountCx, NodeId, Signal, Style, Ui, View, observe,
};

use crate::button::{ButtonController, ButtonVariant, button};
use crate::canvas::{CanvasController, CanvasPainter};
use crate::combo::combo_box;
use crate::controls::{checkbox, progress, radio, slider, switch};
use crate::data_grid::{DataGridController, GridColumn, data_grid};
use crate::image::{ImageController, image};
use crate::modal::modal;
use crate::popup::popup;
use crate::scroll::{VirtualListController, scroll_view, virtual_list};
use crate::spinner::{SpinnerController, spinner};
use crate::tabs::tabs;
use crate::text::{TextController, dynamic_text};
use crate::text_field::text_field;
use crate::tooltip::{TooltipController, tooltip};
use crate::tree_view::{TreeNode, TreeViewController, tree_view};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    Column,
    Row,
    Grid(u16),
    Stack,
}

pub enum WidgetSpec {
    Layout {
        kind: LayoutKind,
        style: Style,
    },
    Text {
        text: String,
        size: Option<f32>,
        color: Option<Color>,
    },
    Button {
        label: String,
        variant: ButtonVariant,
        icon: bool,
    },
    TextField {
        text: String,
        width: Option<f32>,
        multiline: bool,
        password: bool,
    },
    Checkbox {
        label: String,
        checked: bool,
    },
    Radio {
        label: String,
        selected: u64,
        value: u64,
    },
    Switch {
        label: String,
        checked: bool,
    },
    Slider {
        value: f32,
        min: f32,
        max: f32,
        step: f32,
        width: f32,
    },
    ProgressBar {
        value: f32,
        width: f32,
    },
    Spinner {
        size: f32,
        label: String,
    },
    ScrollView {
        width: Option<f32>,
        height: Option<f32>,
        grow: bool,
    },
    Modal {
        open: bool,
        width: Option<f32>,
    },
    Popup {
        open: bool,
        x: f32,
        y: f32,
        width: Option<f32>,
    },
    Tabs {
        labels: Vec<String>,
        active: usize,
        width: Option<f32>,
        height: Option<f32>,
    },
    Tooltip {
        label: String,
        delay_ms: u64,
    },
    Image {
        data: std::sync::Arc<grafikr_core::ImageData>,
        label: String,
        width: Option<f32>,
        height: Option<f32>,
    },
    Canvas {
        width: f32,
        height: f32,
        label: String,
        painter: CanvasPainter,
    },
    ComboBox {
        options: Vec<String>,
        selected: usize,
        width: Option<f32>,
    },
    VirtualList {
        rows: usize,
        row_height: f32,
        height: f32,
        width: Option<f32>,
        selected: Option<usize>,
        label: Rc<dyn Fn(usize) -> String>,
        on_visible: Rc<dyn Fn(usize, u32) -> bool>,
    },
    DataGrid {
        columns: Vec<GridColumn>,
        rows: usize,
        row_height: f32,
        height: f32,
        width: Option<f32>,
        selected: Option<usize>,
        label: String,
        row: Rc<dyn Fn(usize) -> Vec<String>>,
        on_visible: Rc<dyn Fn(usize, u32) -> bool>,
    },
    TreeView {
        roots: Vec<TreeNode>,
        keys: Vec<String>,
        selected: Option<usize>,
        width: Option<f32>,
        height: f32,
    },
}

pub enum WidgetPatch {
    Layout(Style),
    Text {
        text: Option<String>,
        size: Option<f32>,
        color: Option<Color>,
    },
    Button {
        label: Option<String>,
        variant: Option<ButtonVariant>,
        icon: Option<bool>,
        disabled: Option<bool>,
    },
    TextField {
        text: Option<String>,
        width: Option<f32>,
    },
    Checkbox {
        checked: bool,
    },
    Radio {
        selected: u64,
    },
    Switch {
        checked: bool,
    },
    Slider {
        value: f32,
    },
    ProgressBar {
        value: f32,
    },
    Spinner {
        size: Option<f32>,
        label: Option<String>,
    },
    ScrollView {
        width: Option<f32>,
        height: Option<f32>,
        grow: Option<bool>,
    },
    Modal {
        open: Option<bool>,
        width: Option<f32>,
    },
    Popup {
        open: Option<bool>,
        x: Option<f32>,
        y: Option<f32>,
        width: Option<f32>,
    },
    Tabs {
        active: Option<usize>,
        width: Option<f32>,
        height: Option<f32>,
    },
    Tooltip {
        label: Option<String>,
        delay_ms: Option<u64>,
    },
    Image {
        data: Option<std::sync::Arc<grafikr_core::ImageData>>,
        label: Option<String>,
        width: Option<f32>,
        height: Option<f32>,
    },
    Canvas {
        label: Option<String>,
        width: Option<f32>,
        height: Option<f32>,
    },
    ComboBox {
        selected: Option<usize>,
        width: Option<f32>,
    },
    CollectionRefresh,
    CollectionMetadata {
        rows: usize,
        row_height: f32,
    },
    TreeView {
        roots: Option<Vec<TreeNode>>,
        selected: Option<String>,
        width: Option<f32>,
        height: Option<f32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetError {
    InvalidProperty,
    WrongWidget,
    StaleNode,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DynamicEvent {
    Clicked {
        node: NodeId,
    },
    ValueChanged {
        node: NodeId,
        value: DynamicValue,
    },
    Resized {
        node: NodeId,
        width: f32,
        height: f32,
    },
    Selected {
        index: usize,
    },
    KeySelected {
        key: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DynamicValue {
    Bool(bool),
    Index(u64),
    F32(f32),
    Text(String),
}

enum Controller {
    Layout(LayoutKind),
    Text(TextController),
    Button(ButtonController),
    Image(ImageController),
    Canvas(CanvasController),
    ComboBox {
        selected: Signal<usize>,
        count: usize,
        suppressed: Rc<Cell<bool>>,
        _observer: Rc<dyn Fn()>,
    },
    VirtualList(VirtualListController),
    DataGrid(DataGridController),
    TreeView(TreeViewController),
    TextField,
    Checkbox {
        value: Signal<bool>,
        suppressed: Rc<Cell<bool>>,
        _observer: Rc<dyn Fn()>,
    },
    Tooltip(TooltipController),
    Radio {
        value: Signal<usize>,
        suppressed: Rc<Cell<bool>>,
        _observer: Rc<dyn Fn()>,
    },
    Switch {
        value: Signal<bool>,
        suppressed: Rc<Cell<bool>>,
        _observer: Rc<dyn Fn()>,
    },
    Slider {
        value: Signal<f32>,
        min: f32,
        max: f32,
        suppressed: Rc<Cell<bool>>,
        _observer: Rc<dyn Fn()>,
    },
    ProgressBar {
        value: Signal<f32>,
        suppressed: Rc<Cell<bool>>,
        _observer: Rc<dyn Fn()>,
    },
    Spinner(SpinnerController),
    ScrollView {
        content: NodeId,
    },
    Modal {
        open: Signal<bool>,
        card: NodeId,
        suppressed: Rc<Cell<bool>>,
        _observer: Rc<dyn Fn()>,
    },
    Popup {
        open: Signal<bool>,
        card: NodeId,
        suppressed: Rc<Cell<bool>>,
        _observer: Rc<dyn Fn()>,
    },
    Tabs {
        active: Signal<usize>,
        pane_parent: NodeId,
        visibility: Rc<std::cell::RefCell<Vec<Rc<Cell<bool>>>>>,
        count: usize,
        suppressed: Rc<Cell<bool>>,
        _observer: Rc<dyn Fn()>,
    },
}

pub struct MountedWidget {
    root: NodeId,
    internal_nodes: usize,
    controller: Controller,
}

fn watch<T: Clone + PartialEq + 'static>(
    signal: &Signal<T>,
    suppressed: Rc<Cell<bool>>,
    notify: impl Fn(T) + 'static,
) -> Rc<dyn Fn()> {
    let signal = signal.clone();
    let watched = signal.clone();
    let callback: Rc<dyn Fn()> = Rc::new(move || {
        if !suppressed.get() {
            notify(watched.get());
        }
    });
    let _ = observe(&callback, || signal.get());
    callback
}

fn set_signal<T: Clone + PartialEq + 'static>(
    signal: &Signal<T>,
    suppressed: &Cell<bool>,
    value: T,
) {
    suppressed.set(true);
    signal.set(value);
    suppressed.set(false);
}

impl MountedWidget {
    pub fn root(&self) -> NodeId {
        self.root
    }
    pub fn child_slot(&self) -> Option<&'static str> {
        match self.controller {
            Controller::Layout(_) => Some("children"),
            Controller::ScrollView { .. } => Some("content"),
            Controller::Modal { .. } | Controller::Popup { .. } | Controller::Tooltip(_) => {
                Some("content")
            }
            Controller::Tabs { .. } => Some("panes"),
            _ => None,
        }
    }
    pub fn child_parent(&self) -> NodeId {
        match self.controller {
            Controller::ScrollView { content } => content,
            Controller::Modal { card, .. } | Controller::Popup { card, .. } => card,
            Controller::Tabs { pane_parent, .. } => pane_parent,
            _ => self.root,
        }
    }
    pub fn internal_node_count(&self) -> usize {
        self.internal_nodes
    }
    pub fn apply(&mut self, patch: WidgetPatch, ui: &mut Ui) -> Result<(), WidgetError> {
        if ui.tree.get(self.root).is_none() {
            return Err(WidgetError::StaleNode);
        }
        match (&self.controller, patch) {
            (Controller::Layout(_), WidgetPatch::Layout(style)) => {
                validate_style(&style)?;
                ui.tree
                    .set_style(self.root, style)
                    .map_err(|_| WidgetError::StaleNode)?;
            }
            (Controller::Text(controller), WidgetPatch::Text { text, size, color }) => {
                if size.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                    return Err(WidgetError::InvalidProperty);
                }
                if let Some(value) = text {
                    controller.set_text(value);
                }
                if let Some(value) = size {
                    controller.set_size(value);
                }
                if let Some(value) = color {
                    controller.set_color(value);
                }
                ui.tree.mark_dirty(
                    self.root,
                    DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY,
                );
            }
            (
                Controller::Button(controller),
                WidgetPatch::Button {
                    label,
                    variant,
                    icon,
                    disabled,
                },
            ) => {
                if let Some(value) = label {
                    controller.set_label(value);
                }
                if let Some(value) = variant {
                    controller.set_variant(value);
                }
                if let Some(value) = icon {
                    controller.set_icon(value);
                }
                if let Some(value) = disabled {
                    ui.tree.node_mut(self.root).interaction.disabled = value;
                }
                ui.tree.mark_dirty(
                    self.root,
                    DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY,
                );
            }
            (Controller::TextField, WidgetPatch::TextField { text, width }) => {
                if width.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                    return Err(WidgetError::InvalidProperty);
                }
                if let Some(width) = width {
                    let mut style = ui.tree.node(self.root).style.clone();
                    style.width = Some(width);
                    ui.tree
                        .set_style(self.root, style)
                        .map_err(|_| WidgetError::StaleNode)?;
                }
                if let Some(text) = text {
                    ui.set_node_text_value(self.root, &text)
                        .map_err(|_| WidgetError::StaleNode)?;
                }
            }
            (
                Controller::Checkbox {
                    value, suppressed, ..
                },
                WidgetPatch::Checkbox { checked },
            ) => set_signal(value, suppressed, checked),
            (
                Controller::Radio {
                    value, suppressed, ..
                },
                WidgetPatch::Radio { selected: next },
            ) => {
                if next > usize::MAX as u64 {
                    return Err(WidgetError::InvalidProperty);
                }
                set_signal(value, suppressed, next as usize);
            }
            (
                Controller::Switch {
                    value, suppressed, ..
                },
                WidgetPatch::Switch { checked },
            ) => set_signal(value, suppressed, checked),
            (
                Controller::Slider {
                    value,
                    min,
                    max,
                    suppressed,
                    ..
                },
                WidgetPatch::Slider { value: next },
            ) => {
                if !next.is_finite() || next < *min || next > *max {
                    return Err(WidgetError::InvalidProperty);
                }
                set_signal(value, suppressed, next);
            }
            (
                Controller::ProgressBar {
                    value, suppressed, ..
                },
                WidgetPatch::ProgressBar { value: next },
            ) => {
                if !next.is_finite() || !(0.0..=1.0).contains(&next) {
                    return Err(WidgetError::InvalidProperty);
                }
                set_signal(value, suppressed, next);
            }
            (Controller::Spinner(controller), WidgetPatch::Spinner { size, label }) => {
                if size.is_some_and(|value| !value.is_finite() || value < 12.0) {
                    return Err(WidgetError::InvalidProperty);
                }
                let mut style = ui.tree.node(self.root).style.clone();
                if let Some(size) = size {
                    style.width = Some(size);
                    style.height = Some(size);
                    controller.set_size(size);
                }
                if let Some(label) = label {
                    controller.set_label(label);
                }
                ui.tree
                    .set_style(self.root, style)
                    .map_err(|_| WidgetError::StaleNode)?;
            }
            (
                Controller::ScrollView { .. },
                WidgetPatch::ScrollView {
                    width,
                    height,
                    grow,
                },
            ) => {
                if width.is_some_and(|value| !value.is_finite() || value <= 0.0)
                    || height.is_some_and(|value| !value.is_finite() || value <= 0.0)
                {
                    return Err(WidgetError::InvalidProperty);
                }
                let mut style = ui.tree.node(self.root).style.clone();
                if let Some(width) = width {
                    style.width = Some(width);
                }
                if let Some(height) = height {
                    style.height = Some(height);
                }
                if let Some(true) = grow {
                    style.height = None;
                    style.flex_grow = 1.0;
                }
                if let Some(false) = grow {
                    style.flex_grow = 0.0;
                    if style.height.is_none() {
                        style.height = Some(240.0);
                    }
                }
                ui.tree
                    .set_style(self.root, style)
                    .map_err(|_| WidgetError::StaleNode)?;
            }
            (
                Controller::Modal {
                    open,
                    card,
                    suppressed,
                    ..
                },
                WidgetPatch::Modal { open: next, width },
            ) => {
                if width.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                    return Err(WidgetError::InvalidProperty);
                }
                if let Some(next) = next {
                    set_signal(open, suppressed, next);
                }
                if let Some(width) = width {
                    let mut style = ui.tree.node(*card).style.clone();
                    style.width = Some(width);
                    ui.tree
                        .set_style(*card, style)
                        .map_err(|_| WidgetError::StaleNode)?;
                }
                ui.tree.mark_dirty(
                    self.root,
                    DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY,
                );
            }
            (
                Controller::Popup {
                    open,
                    card,
                    suppressed,
                    ..
                },
                WidgetPatch::Popup {
                    open: next,
                    x,
                    y,
                    width,
                },
            ) => {
                if x.is_some_and(|value| !value.is_finite())
                    || y.is_some_and(|value| !value.is_finite())
                    || width.is_some_and(|value| !value.is_finite() || value <= 0.0)
                {
                    return Err(WidgetError::InvalidProperty);
                }
                if let Some(next) = next {
                    set_signal(open, suppressed, next);
                }
                if x.is_some() || y.is_some() || width.is_some() {
                    let mut style = ui.tree.node(*card).style.clone();
                    if let Some(x) = x {
                        style.inset.left = Some(x);
                    }
                    if let Some(y) = y {
                        style.inset.top = Some(y);
                    }
                    if let Some(width) = width {
                        style.width = Some(width);
                    }
                    ui.tree
                        .set_style(*card, style)
                        .map_err(|_| WidgetError::StaleNode)?;
                }
                ui.tree.mark_dirty(
                    self.root,
                    DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY,
                );
            }
            (
                Controller::Tabs {
                    active,
                    visibility,
                    count,
                    suppressed,
                    ..
                },
                WidgetPatch::Tabs {
                    active: next,
                    width,
                    height,
                },
            ) => {
                if next.is_some_and(|value| value >= *count && *count > 0)
                    || width.is_some_and(|value| !value.is_finite() || value <= 0.0)
                    || height.is_some_and(|value| !value.is_finite() || value <= 0.0)
                {
                    return Err(WidgetError::InvalidProperty);
                }
                if let Some(next) = next {
                    set_signal(active, suppressed, next);
                    update_tab_visibility(active.get(), &visibility.borrow());
                }
                if width.is_some() || height.is_some() {
                    let mut style = ui.tree.node(self.root).style.clone();
                    if let Some(width) = width {
                        style.width = Some(width);
                    }
                    if let Some(height) = height {
                        style.height = Some(height);
                    }
                    ui.tree
                        .set_style(self.root, style)
                        .map_err(|_| WidgetError::StaleNode)?;
                }
                ui.tree
                    .mark_dirty(self.root, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
            }
            (Controller::Tooltip(controller), WidgetPatch::Tooltip { label, delay_ms }) => {
                if let Some(label) = label {
                    controller.set_label(label);
                }
                if let Some(delay_ms) = delay_ms {
                    controller.set_delay_ms(delay_ms);
                }
                ui.tree
                    .mark_dirty(self.root, DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY);
            }
            (
                Controller::Image(controller),
                WidgetPatch::Image {
                    data,
                    label,
                    width,
                    height,
                },
            ) => {
                if width.is_some_and(|value| !value.is_finite() || value <= 0.0)
                    || height.is_some_and(|value| !value.is_finite() || value <= 0.0)
                {
                    return Err(WidgetError::InvalidProperty);
                }
                if let Some(data) = data {
                    controller.set_data(data);
                }
                if let Some(label) = label {
                    controller.set_label(label);
                }
                if width.is_some() || height.is_some() {
                    let mut style = ui.tree.node(self.root).style.clone();
                    if let Some(width) = width {
                        style.width = Some(width);
                    }
                    if let Some(height) = height {
                        style.height = Some(height);
                    }
                    ui.tree
                        .set_style(self.root, style)
                        .map_err(|_| WidgetError::StaleNode)?;
                }
            }
            (
                Controller::Canvas(controller),
                WidgetPatch::Canvas {
                    label,
                    width,
                    height,
                },
            ) => {
                if width.is_some_and(|value| !value.is_finite() || value <= 0.0)
                    || height.is_some_and(|value| !value.is_finite() || value <= 0.0)
                {
                    return Err(WidgetError::InvalidProperty);
                }
                if let Some(label) = label {
                    controller.set_label(label);
                }
                if width.is_some() || height.is_some() {
                    let mut style = ui.tree.node(self.root).style.clone();
                    if let Some(width) = width {
                        style.width = Some(width);
                    }
                    if let Some(height) = height {
                        style.height = Some(height);
                    }
                    let new_width = style.width.unwrap_or(0.0);
                    let new_height = style.height.unwrap_or(0.0);
                    ui.tree
                        .set_style(self.root, style)
                        .map_err(|_| WidgetError::StaleNode)?;
                    controller.set_size(new_width, new_height);
                }
                ui.tree.mark_dirty(
                    self.root,
                    DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY,
                );
            }
            (
                Controller::ComboBox {
                    selected,
                    count,
                    suppressed,
                    ..
                },
                WidgetPatch::ComboBox {
                    selected: next,
                    width,
                },
            ) => {
                if next.is_some_and(|value| value >= *count)
                    || width.is_some_and(|value| !value.is_finite() || value <= 0.0)
                {
                    return Err(WidgetError::InvalidProperty);
                }
                if let Some(next) = next {
                    set_signal(selected, suppressed, next);
                }
                if let Some(width) = width {
                    let mut style = ui.tree.node(self.root).style.clone();
                    style.width = Some(width);
                    ui.tree
                        .set_style(self.root, style)
                        .map_err(|_| WidgetError::StaleNode)?;
                }
                ui.tree.mark_dirty(
                    self.root,
                    DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY,
                );
            }
            (Controller::VirtualList(controller), WidgetPatch::CollectionRefresh) => {
                controller.refresh();
                ui.tree
                    .mark_dirty(self.root, DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY);
            }
            (
                Controller::VirtualList(controller),
                WidgetPatch::CollectionMetadata { rows, row_height },
            ) => {
                if !row_height.is_finite() || row_height <= 0.0 {
                    return Err(WidgetError::InvalidProperty);
                }
                controller.set_metadata(rows, row_height);
                ui.tree.mark_dirty(
                    self.root,
                    DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY,
                );
            }
            (Controller::DataGrid(controller), WidgetPatch::CollectionRefresh) => {
                controller.refresh();
                ui.tree
                    .mark_dirty(self.root, DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY);
            }
            (
                Controller::DataGrid(controller),
                WidgetPatch::CollectionMetadata { rows, row_height },
            ) => {
                if !row_height.is_finite() || row_height <= 0.0 {
                    return Err(WidgetError::InvalidProperty);
                }
                controller.set_metadata(rows, row_height);
                ui.tree.mark_dirty(
                    self.root,
                    DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY,
                );
            }
            (
                Controller::TreeView(controller),
                WidgetPatch::TreeView {
                    roots,
                    selected: next,
                    width,
                    height,
                },
            ) => {
                if width.is_some_and(|value| !value.is_finite() || value <= 0.0)
                    || height.is_some_and(|value| !value.is_finite() || value <= 0.0)
                {
                    return Err(WidgetError::InvalidProperty);
                }
                if let Some(roots) = roots {
                    if next
                        .as_ref()
                        .is_some_and(|key| !TreeViewController::roots_contain_key(&roots, key))
                    {
                        return Err(WidgetError::InvalidProperty);
                    }
                    controller
                        .set_roots(roots)
                        .map_err(|_| WidgetError::InvalidProperty)?;
                } else if next
                    .as_ref()
                    .is_some_and(|key| !controller.contains_key(key))
                {
                    return Err(WidgetError::InvalidProperty);
                }
                if let Some(next) = next {
                    controller.set_selected_key(Some(next));
                }
                if width.is_some() || height.is_some() {
                    let mut style = ui.tree.node(self.root).style.clone();
                    if let Some(width) = width {
                        style.width = Some(width);
                    }
                    if let Some(height) = height {
                        style.height = Some(height);
                    }
                    ui.tree
                        .set_style(self.root, style)
                        .map_err(|_| WidgetError::StaleNode)?;
                }
                ui.tree.mark_dirty(
                    self.root,
                    DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::ACCESSIBILITY,
                );
            }
            _ => return Err(WidgetError::WrongWidget),
        }
        ui.runtime.request_frame();
        Ok(())
    }
    pub fn children_changed(&mut self, ui: &mut Ui) -> Result<(), WidgetError> {
        if ui.tree.get(self.root).is_none() {
            return Err(WidgetError::StaleNode);
        }
        ui.runtime.request_frame();
        if let Controller::Layout(kind) = self.controller {
            if kind == LayoutKind::Stack {
                for child in ui.tree.children(self.root).to_vec() {
                    ui.tree.set_absolute_fill(child);
                }
            }
            ui.tree.mark_dirty(self.root, DirtyFlags::LAYOUT);
            Ok(())
        } else if let Controller::ScrollView { content } = self.controller {
            ui.tree.mark_dirty(content, DirtyFlags::LAYOUT);
            Ok(())
        } else if let Controller::Modal { card, .. } | Controller::Popup { card, .. } =
            self.controller
        {
            ui.tree.mark_dirty(card, DirtyFlags::LAYOUT);
            Ok(())
        } else if let Controller::Tabs {
            active,
            pane_parent,
            visibility,
            ..
        } = &mut self.controller
        {
            let children = ui.tree.children(*pane_parent).to_vec();
            let cells: Vec<_> = children
                .iter()
                .map(|child| {
                    let visible = Rc::new(Cell::new(false));
                    ui.tree.set_visibility(*child, visible.clone());
                    visible
                })
                .collect();
            update_tab_visibility(active.get(), &cells);
            *visibility.borrow_mut() = cells;
            ui.tree.mark_dirty(*pane_parent, DirtyFlags::LAYOUT);
            Ok(())
        } else if let Controller::Tooltip(_) = self.controller {
            ui.tree.mark_dirty(self.root, DirtyFlags::LAYOUT);
            Ok(())
        } else {
            Err(WidgetError::WrongWidget)
        }
    }
}

pub fn mount(
    spec: WidgetSpec,
    cx: &mut MountCx<'_>,
    sink: Rc<dyn Fn(DynamicEvent)>,
) -> Result<MountedWidget, WidgetError> {
    let before = cx.tree.node_count();
    let (root, controller) = match spec {
        WidgetSpec::Layout { kind, mut style } => {
            match kind {
                LayoutKind::Column => style.direction = Direction::Column,
                LayoutKind::Row => style.direction = Direction::Row,
                LayoutKind::Grid(columns) => style.grid_columns = Some(columns),
                LayoutKind::Stack => {}
            }
            validate_style(&style)?;
            (cx.add_node(style), Controller::Layout(kind))
        }
        WidgetSpec::Text { text, size, color } => {
            if size.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                return Err(WidgetError::InvalidProperty);
            }
            let (mut view, controller) = dynamic_text(text);
            if let Some(size) = size {
                view = view.size(size);
            }
            if let Some(color) = color {
                view = view.color(color);
            }
            (view.mount(cx), Controller::Text(controller))
        }
        WidgetSpec::Button {
            label,
            variant,
            icon,
        } => {
            let mut view = button(label);
            if variant == ButtonVariant::Secondary {
                view = view.secondary();
            }
            if icon {
                view = view.icon();
            }
            let controller = view.controller();
            let clicked = Rc::new(std::cell::Cell::new(None));
            let clicked_sink = sink.clone();
            let clicked_node = clicked.clone();
            view = view.on_click(move |_| {
                if let Some(node) = clicked_node.get() {
                    clicked_sink(DynamicEvent::Clicked { node });
                }
            });
            let root = view.mount(cx);
            clicked.set(Some(root));
            (root, Controller::Button(controller))
        }
        WidgetSpec::TextField {
            text,
            width,
            multiline,
            password,
        } => {
            if width.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                return Err(WidgetError::InvalidProperty);
            }
            let callback_node = Rc::new(std::cell::Cell::new(None));
            let callback_sink = sink.clone();
            let callback_id = callback_node.clone();
            let mut view = text_field(text).on_change(move |value| {
                if let Some(node) = callback_id.get() {
                    callback_sink(DynamicEvent::ValueChanged {
                        node,
                        value: DynamicValue::Text(value),
                    });
                }
            });
            if let Some(width) = width {
                view = view.width(width);
            }
            if multiline {
                view = view.multiline();
            }
            if password {
                view = view.password();
            }
            let root = view.mount(cx);
            callback_node.set(Some(root));
            (root, Controller::TextField)
        }
        WidgetSpec::Checkbox { label, checked } => {
            let value = Signal::new(checked);
            let root = checkbox(label, value.clone()).mount(cx);
            let suppressed = Rc::new(Cell::new(false));
            let event_sink = sink.clone();
            let observer = watch(&value, suppressed.clone(), move |value| {
                event_sink(DynamicEvent::ValueChanged {
                    node: root,
                    value: DynamicValue::Bool(value),
                })
            });
            (
                root,
                Controller::Checkbox {
                    value,
                    suppressed,
                    _observer: observer,
                },
            )
        }
        WidgetSpec::Radio {
            label,
            selected,
            value,
        } => {
            let selected = usize::try_from(selected).map_err(|_| WidgetError::InvalidProperty)?;
            let value = usize::try_from(value).map_err(|_| WidgetError::InvalidProperty)?;
            let group = Signal::new(selected);
            let root = radio(label, group.clone(), value).mount(cx);
            let suppressed = Rc::new(Cell::new(false));
            let event_sink = sink.clone();
            let observer = watch(&group, suppressed.clone(), move |value| {
                event_sink(DynamicEvent::ValueChanged {
                    node: root,
                    value: DynamicValue::Index(value as u64),
                })
            });
            (
                root,
                Controller::Radio {
                    value: group,
                    suppressed,
                    _observer: observer,
                },
            )
        }
        WidgetSpec::Switch { label, checked } => {
            let value = Signal::new(checked);
            let root = switch(label, value.clone()).mount(cx);
            let suppressed = Rc::new(Cell::new(false));
            let event_sink = sink.clone();
            let observer = watch(&value, suppressed.clone(), move |value| {
                event_sink(DynamicEvent::ValueChanged {
                    node: root,
                    value: DynamicValue::Bool(value),
                })
            });
            (
                root,
                Controller::Switch {
                    value,
                    suppressed,
                    _observer: observer,
                },
            )
        }
        WidgetSpec::Slider {
            value,
            min,
            max,
            step,
            width,
        } => {
            if !value.is_finite()
                || !min.is_finite()
                || !max.is_finite()
                || !step.is_finite()
                || !width.is_finite()
                || min >= max
                || !(min..=max).contains(&value)
                || step < 0.0
                || width <= 0.0
                || !(max - min).is_finite()
            {
                return Err(WidgetError::InvalidProperty);
            }
            let signal = Signal::new(value);
            let root = slider(signal.clone())
                .range(min, max)
                .step(step)
                .width(width)
                .mount(cx);
            let suppressed = Rc::new(Cell::new(false));
            let event_sink = sink.clone();
            let observer = watch(&signal, suppressed.clone(), move |value| {
                event_sink(DynamicEvent::ValueChanged {
                    node: root,
                    value: DynamicValue::F32(value),
                })
            });
            (
                root,
                Controller::Slider {
                    value: signal,
                    min,
                    max,
                    suppressed,
                    _observer: observer,
                },
            )
        }
        WidgetSpec::ProgressBar { value, width } => {
            if !value.is_finite()
                || !(0.0..=1.0).contains(&value)
                || !width.is_finite()
                || width <= 0.0
            {
                return Err(WidgetError::InvalidProperty);
            }
            let view = progress(value).width(width);
            let signal = view.controller();
            let root = view.mount(cx);
            let suppressed = Rc::new(Cell::new(false));
            let event_sink = sink.clone();
            let observer = watch(&signal, suppressed.clone(), move |value| {
                event_sink(DynamicEvent::ValueChanged {
                    node: root,
                    value: DynamicValue::F32(value),
                })
            });
            (
                root,
                Controller::ProgressBar {
                    value: signal,
                    suppressed,
                    _observer: observer,
                },
            )
        }
        WidgetSpec::Spinner { size, label } => {
            if !size.is_finite() || size < 12.0 {
                return Err(WidgetError::InvalidProperty);
            }
            let view = spinner().size(size).label(label);
            let controller = view.controller();
            (view.mount(cx), Controller::Spinner(controller))
        }
        WidgetSpec::ScrollView {
            width,
            height,
            grow,
        } => {
            if width.is_some_and(|value| !value.is_finite() || value <= 0.0)
                || height.is_some_and(|value| !value.is_finite() || value <= 0.0)
            {
                return Err(WidgetError::InvalidProperty);
            }
            let mut view = scroll_view(());
            if let Some(width) = width {
                view = view.width(width);
            }
            if let Some(height) = height {
                view = view.height(height);
            }
            if grow {
                view = view.grow();
            }
            let root = view.mount(cx);
            let content = cx.tree.children(root)[0];
            (root, Controller::ScrollView { content })
        }
        WidgetSpec::Modal { open, width } => {
            if width.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                return Err(WidgetError::InvalidProperty);
            }
            let signal = Signal::new(open);
            let mut view = modal(signal.clone(), ());
            if let Some(width) = width {
                view = view.width(width);
            }
            let root = view.mount(cx);
            let card = cx.tree.children(root)[0];
            let suppressed = Rc::new(Cell::new(false));
            let event_sink = sink.clone();
            let observer = watch(&signal, suppressed.clone(), move |value| {
                event_sink(DynamicEvent::ValueChanged {
                    node: root,
                    value: DynamicValue::Bool(value),
                })
            });
            (
                root,
                Controller::Modal {
                    open: signal,
                    card,
                    suppressed,
                    _observer: observer,
                },
            )
        }
        WidgetSpec::Popup { open, x, y, width } => {
            if !x.is_finite()
                || !y.is_finite()
                || width.is_some_and(|value| !value.is_finite() || value <= 0.0)
            {
                return Err(WidgetError::InvalidProperty);
            }
            let signal = Signal::new(open);
            let mut view = popup(signal.clone(), x, y, ());
            if let Some(width) = width {
                view = view.width(width);
            }
            let root = view.mount(cx);
            let card = cx.tree.children(root)[0];
            let suppressed = Rc::new(Cell::new(false));
            let event_sink = sink.clone();
            let observer = watch(&signal, suppressed.clone(), move |value| {
                event_sink(DynamicEvent::ValueChanged {
                    node: root,
                    value: DynamicValue::Bool(value),
                })
            });
            (
                root,
                Controller::Popup {
                    open: signal,
                    card,
                    suppressed,
                    _observer: observer,
                },
            )
        }
        WidgetSpec::Tabs {
            labels,
            active,
            width,
            height,
        } => {
            if labels.is_empty()
                || active >= labels.len()
                || width.is_some_and(|value| !value.is_finite() || value <= 0.0)
                || height.is_some_and(|value| !value.is_finite() || value <= 0.0)
            {
                return Err(WidgetError::InvalidProperty);
            }
            let active = Signal::new(active);
            let mut view = tabs(labels.clone(), active.clone(), ());
            if let Some(width) = width {
                view = view.width(width);
            }
            if let Some(height) = height {
                view = view.height(height);
            }
            let root = view.mount(cx);
            let pane_parent = cx.tree.children(root)[0];
            let visibility = Rc::new(std::cell::RefCell::new(Vec::new()));
            let suppressed = Rc::new(Cell::new(false));
            let event_sink = sink.clone();
            let watched_visibility = visibility.clone();
            let observer = watch(&active, suppressed.clone(), move |index| {
                update_tab_visibility(index, &watched_visibility.borrow());
                event_sink(DynamicEvent::ValueChanged {
                    node: root,
                    value: DynamicValue::Index(index as u64),
                });
            });
            (
                root,
                Controller::Tabs {
                    active,
                    pane_parent,
                    visibility,
                    count: labels.len(),
                    suppressed,
                    _observer: observer,
                },
            )
        }
        WidgetSpec::Tooltip { label, delay_ms } => {
            let view = tooltip(label, ()).delay_ms(delay_ms);
            let controller = view.controller();
            (view.mount(cx), Controller::Tooltip(controller))
        }
        WidgetSpec::Image {
            data,
            label,
            width,
            height,
        } => {
            if width.is_some_and(|value| !value.is_finite() || value <= 0.0)
                || height.is_some_and(|value| !value.is_finite() || value <= 0.0)
            {
                return Err(WidgetError::InvalidProperty);
            }
            let mut view = image(data).label(label);
            if let Some(width) = width {
                view = view.width(width);
            }
            if let Some(height) = height {
                view = view.height(height);
            }
            let controller = view.controller();
            (view.mount(cx), Controller::Image(controller))
        }
        WidgetSpec::Canvas {
            width,
            height,
            label,
            painter,
        } => {
            if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
                return Err(WidgetError::InvalidProperty);
            }
            let resize_sink = sink.clone();
            let (view, controller) = crate::canvas::Canvas::controlled(
                width,
                height,
                label,
                painter,
                move |node, width, height| {
                    resize_sink(DynamicEvent::Resized {
                        node,
                        width,
                        height,
                    })
                },
            );
            (view.mount(cx), Controller::Canvas(controller))
        }
        WidgetSpec::ComboBox {
            options,
            selected,
            width,
        } => {
            if options.is_empty()
                || selected >= options.len()
                || width.is_some_and(|value| !value.is_finite() || value <= 0.0)
            {
                return Err(WidgetError::InvalidProperty);
            }
            let selected = Signal::new(selected);
            let mut view = combo_box(options.clone(), selected.clone());
            if let Some(width) = width {
                view = view.width(width);
            }
            let root = view.mount(cx);
            let suppressed = Rc::new(Cell::new(false));
            let event_sink = sink.clone();
            let observer = watch(&selected, suppressed.clone(), move |index| {
                event_sink(DynamicEvent::Selected { index });
            });
            (
                root,
                Controller::ComboBox {
                    selected,
                    count: options.len(),
                    suppressed,
                    _observer: observer,
                },
            )
        }
        WidgetSpec::VirtualList {
            rows,
            row_height,
            height,
            width,
            selected,
            label,
            on_visible,
        } => {
            if !row_height.is_finite()
                || row_height <= 0.0
                || !height.is_finite()
                || height <= 0.0
                || width.is_some_and(|value| !value.is_finite() || value <= 0.0)
                || selected.is_some_and(|value| value >= rows)
            {
                return Err(WidgetError::InvalidProperty);
            }
            let mut view = virtual_list(rows, row_height, move |index| label(index))
                .height(height)
                .on_visible_range(move |start, count| on_visible(start, count));
            if let Some(width) = width {
                view = view.width(width);
            }
            if let Some(selected) = selected {
                view = view.selected(selected);
            }
            let event_sink = sink.clone();
            view = view.on_select(move |index| event_sink(DynamicEvent::Selected { index }));
            let controller = view.controller();
            let root = view.mount(cx);
            (root, Controller::VirtualList(controller))
        }
        WidgetSpec::DataGrid {
            columns,
            rows,
            row_height,
            height,
            width,
            selected,
            label,
            row,
            on_visible,
        } => {
            if columns.is_empty()
                || !row_height.is_finite()
                || row_height <= 0.0
                || !height.is_finite()
                || height <= 0.0
                || width.is_some_and(|value| !value.is_finite() || value <= 0.0)
                || selected.is_some_and(|value| value >= rows)
            {
                return Err(WidgetError::InvalidProperty);
            }
            let mut view = data_grid(columns, rows, row_height, move |index| row(index))
                .height(height)
                .label(label)
                .on_visible_range(move |start, count| on_visible(start, count));
            if let Some(width) = width {
                view = view.width(width);
            }
            if let Some(selected) = selected {
                view = view.selected(selected);
            }
            let event_sink = sink.clone();
            view = view.on_select(move |index| event_sink(DynamicEvent::Selected { index }));
            let controller = view.controller();
            let root = view.mount(cx);
            (root, Controller::DataGrid(controller))
        }
        WidgetSpec::TreeView {
            roots,
            keys,
            selected,
            width,
            height,
        } => {
            if keys.is_empty()
                || selected.is_some_and(|index| index >= keys.len())
                || width.is_some_and(|value| !value.is_finite() || value <= 0.0)
                || !height.is_finite()
                || height <= 0.0
            {
                return Err(WidgetError::InvalidProperty);
            }
            let mut view = tree_view(roots).height(height);
            if let Some(width) = width {
                view = view.width(width);
            }
            if let Some(selected) = selected {
                view = view.selected(selected);
            }
            let event_sink = sink.clone();
            view = view.on_select_key(move |key| event_sink(DynamicEvent::KeySelected { key }));
            let controller = view.controller();
            let root = view.mount(cx);
            (root, Controller::TreeView(controller))
        }
    };
    let internal_nodes = cx.tree.node_count().saturating_sub(before);
    Ok(MountedWidget {
        root,
        internal_nodes,
        controller,
    })
}

fn update_tab_visibility(active: usize, cells: &[Rc<Cell<bool>>]) {
    for (index, cell) in cells.iter().enumerate() {
        cell.set(index == active);
    }
}

fn validate_style(style: &Style) -> Result<(), WidgetError> {
    let values = [
        style.gap,
        style.padding.top,
        style.padding.right,
        style.padding.bottom,
        style.padding.left,
        style.flex_grow,
        style.radius,
        style.transform.scale,
        style.transform.translate.x,
        style.transform.translate.y,
    ];
    if values.iter().any(|value| !value.is_finite())
        || style.gap < 0.0
        || style.flex_grow < 0.0
        || style.radius < 0.0
        || style.transform.scale <= 0.0
        || [style.width, style.height, style.min_width, style.min_height]
            .into_iter()
            .flatten()
            .any(|value| !value.is_finite() || value < 0.0)
        || style.grid_columns == Some(0)
    {
        return Err(WidgetError::InvalidProperty);
    }
    Ok(())
}
