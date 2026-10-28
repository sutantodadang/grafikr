//! Column and Row containers.

use grafikr_core::{
    Color, Direction, EdgeInsets, IntoViews, MountCx, NodeId, Style, Transform, View,
};

use crate::into_views;

pub struct Column {
    children: Box<dyn IntoViews>,
    gap: f32,
    padding: EdgeInsets,
    background: Option<Color>,
    radius: f32,
    width: Option<f32>,
    transform: Transform,
}

pub fn column(children: impl IntoViews + 'static) -> Column {
    Column {
        children: Box::new(children),
        gap: 0.0,
        padding: EdgeInsets::ZERO,
        background: None,
        radius: 0.0,
        width: None,
        transform: Transform::IDENTITY,
    }
}

impl Column {
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = EdgeInsets::all(padding);
        self
    }

    pub fn padding_edges(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }

    fn make_style(&self) -> Style {
        Style {
            direction: Direction::Column,
            gap: self.gap,
            padding: self.padding,
            background: self.background,
            radius: self.radius,
            width: self.width,
            transform: self.transform,
            ..Style::default()
        }
    }
}

impl View for Column {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(self.make_style());
        cx.children(id, |cx| self.children.mount_into(cx));
        id
    }
}

pub struct Row {
    children: Box<dyn IntoViews>,
    gap: f32,
    padding: EdgeInsets,
    background: Option<Color>,
    radius: f32,
    height: Option<f32>,
    transform: Transform,
}
pub fn row(children: impl IntoViews + 'static) -> Row {
    Row {
        children: Box::new(children),
        gap: 0.0,
        padding: EdgeInsets::ZERO,
        background: None,
        radius: 0.0,
        height: None,
        transform: Transform::IDENTITY,
    }
}

impl Row {
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = EdgeInsets::all(padding);
        self
    }

    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    pub fn transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }

    fn make_style(&self) -> Style {
        let mut style = Style::row();
        style.gap = self.gap;
        style.padding = self.padding;
        style.background = self.background;
        style.radius = self.radius;
        style.height = self.height;
        style.transform = self.transform;
        style
    }
}

impl View for Row {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(self.make_style());
        cx.children(id, |cx| self.children.mount_into(cx));
        id
    }
}

pub struct Grid {
    children: Box<dyn IntoViews>,
    columns: u16,
    gap: f32,
    padding: EdgeInsets,
    width: Option<f32>,
    transform: Transform,
}

pub fn grid(columns: u16, children: impl IntoViews + 'static) -> Grid {
    Grid {
        children: Box::new(children),
        columns: columns.max(1),
        gap: 0.0,
        padding: EdgeInsets::ZERO,
        width: None,
        transform: Transform::IDENTITY,
    }
}

impl Grid {
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = EdgeInsets::all(padding);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }
}

impl View for Grid {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style {
            grid_columns: Some(self.columns),
            gap: self.gap,
            padding: self.padding,
            width: self.width,
            transform: self.transform,
            ..Style::default()
        });
        cx.children(id, |cx| self.children.mount_into(cx));
        id
    }
}

pub struct Stack {
    children: Box<dyn IntoViews>,
    width: Option<f32>,
    height: Option<f32>,
    transform: Transform,
}

pub fn stack(children: impl IntoViews + 'static) -> Stack {
    Stack {
        children: Box::new(children),
        width: None,
        height: None,
        transform: Transform::IDENTITY,
    }
}

impl Stack {
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    pub fn transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }
}

impl View for Stack {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style {
            width: self.width,
            height: self.height,
            transform: self.transform,
            ..Style::default()
        });
        cx.children(id, |cx| self.children.mount_into(cx));
        let children = cx.tree.children(id).to_vec();
        for child in children {
            cx.tree.set_absolute_fill(child);
        }
        id
    }
}

into_views!(Column, Row, Grid, Stack);
