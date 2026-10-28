//! MVP controls: checkbox, radio, switch, slider, progress.

use std::rc::Rc;

use grafikr_core::{
    Color, DirtyFlags, EventCx, EventStatus, Key, MeasureCx, MountCx, NodeId, PaintCx, Point, Rect,
    Runtime, Signal, Size, Style, TextCache, UiEvent, View, Widget, observe,
};

use crate::into_views;

fn track<T: Clone + PartialEq + 'static>(
    signal: &Signal<T>,
    node: NodeId,
    runtime: &Runtime,
) -> Rc<dyn Fn()> {
    let runtime = runtime.clone();
    let subscriber: Rc<dyn Fn()> = Rc::new(move || {
        runtime.invalidate(node, DirtyFlags::PAINT);
    });
    let _ = observe(&subscriber, || signal.get());
    subscriber
}

const BOX: f32 = 18.0;
const GAP: f32 = 8.0;

fn label_origin(rect: Rect, label_height: f32) -> Point {
    Point::new(
        rect.x + BOX + GAP,
        rect.y + (rect.height - label_height).max(0.0) * 0.5,
    )
}

fn toggle_box(cx: &mut PaintCx, rect: Rect, checked: bool, focused: bool) {
    let box_rect = Rect::new(
        rect.x,
        rect.y + (rect.height - BOX).max(0.0) * 0.5,
        BOX,
        BOX,
    );
    let border_color = if focused {
        if checked {
            Color::WHITE.with_alpha(220)
        } else {
            cx.theme.accent
        }
    } else {
        cx.theme.muted
    };
    cx.push_bordered_rect(
        box_rect,
        if checked {
            cx.theme.accent
        } else {
            cx.theme.surface
        },
        4.0,
        Some((if focused { 2.0 } else { 1.0 }, border_color)),
    );
}

// ------------------------------------------------------------------ Checkbox

pub struct Checkbox {
    label: String,
    checked: Signal<bool>,
}

pub fn checkbox(label: impl Into<String>, checked: Signal<bool>) -> Checkbox {
    Checkbox {
        label: label.into(),
        checked,
    }
}

impl View for Checkbox {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style::default());
        let subscriber = track(&self.checked, id, cx.runtime);
        cx.set_widget(
            id,
            CheckboxWidget {
                label: self.label.clone(),
                checked: self.checked.clone(),
                cache: TextCache::default(),
                _subscriber: subscriber,
            },
        );
        id
    }
}

struct CheckboxWidget {
    label: String,
    checked: Signal<bool>,
    cache: TextCache,
    _subscriber: Rc<dyn Fn()>,
}

impl CheckboxWidget {
    fn toggle(&self) {
        self.checked.update(|value| *value = !*value);
    }
}

impl Widget for CheckboxWidget {
    fn measure(&mut self, cx: &mut MeasureCx, _available: Size) -> Size {
        let shape = self.cache.get(
            cx.text,
            &self.label,
            cx.theme.body_size,
            cx.theme.foreground,
            None,
        );
        Size::new(
            BOX + GAP + shape.width,
            shape.height.max(24.0).max(cx.theme.control_height * 0.7),
        )
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        let checked = self.checked.get();
        toggle_box(cx, rect, checked, cx.state.focused);
        if checked {
            let box_rect = Rect::new(
                rect.x,
                rect.y + (rect.height - BOX).max(0.0) * 0.5,
                BOX,
                BOX,
            );
            cx.push_rect(
                Rect::new(box_rect.x + 5.0, box_rect.y + 5.0, 8.0, 8.0),
                Color::WHITE,
                2.0,
            );
        }
        let shape = self.cache.get(
            cx.text,
            &self.label,
            cx.theme.body_size,
            cx.theme.foreground,
            None,
        );
        cx.display
            .push_layout(&shape.layout, label_origin(rect, shape.height), cx.clip);
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::PointerDown { .. } => {
                self.toggle();
                EventStatus::Handled
            }
            UiEvent::KeyDown {
                key: Key::Space, ..
            } if cx.state.focused => {
                self.toggle();
                EventStatus::Handled
            }
            UiEvent::AccessibilityClick => {
                self.toggle();
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn cursor(&self) -> grafikr_core::CursorIcon {
        grafikr_core::CursorIcon::Pointer
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::CheckBox);
        node.set_label(self.label.clone());
        node.set_toggled(self.checked.get().into());
        node.add_action(accesskit::Action::Click);
    }
}

// --------------------------------------------------------------------- Radio

pub struct Radio {
    label: String,
    group: Signal<usize>,
    value: usize,
}

pub fn radio(label: impl Into<String>, group: Signal<usize>, value: usize) -> Radio {
    Radio {
        label: label.into(),
        group,
        value,
    }
}

impl View for Radio {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style::default());
        let subscriber = track(&self.group, id, cx.runtime);
        cx.set_widget(
            id,
            RadioWidget {
                label: self.label.clone(),
                group: self.group.clone(),
                value: self.value,
                cache: TextCache::default(),
                _subscriber: subscriber,
            },
        );
        id
    }
}

struct RadioWidget {
    label: String,
    group: Signal<usize>,
    value: usize,
    cache: TextCache,
    _subscriber: Rc<dyn Fn()>,
}

impl Widget for RadioWidget {
    fn measure(&mut self, cx: &mut MeasureCx, _available: Size) -> Size {
        let shape = self.cache.get(
            cx.text,
            &self.label,
            cx.theme.body_size,
            cx.theme.foreground,
            None,
        );
        Size::new(
            BOX + GAP + shape.width,
            shape.height.max(24.0).max(cx.theme.control_height * 0.7),
        )
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        let selected = self.group.get() == self.value;
        let box_rect = Rect::new(
            rect.x,
            rect.y + (rect.height - BOX).max(0.0) * 0.5,
            BOX,
            BOX,
        );
        let radius = BOX * 0.5;
        cx.push_bordered_rect(
            box_rect,
            cx.theme.surface,
            radius,
            Some((
                if cx.state.focused { 2.0 } else { 1.0 },
                if cx.state.focused || selected {
                    cx.theme.accent
                } else {
                    cx.theme.muted
                },
            )),
        );
        if selected {
            cx.push_rect(
                Rect::new(box_rect.x + 5.0, box_rect.y + 5.0, 8.0, 8.0),
                cx.theme.accent,
                4.0,
            );
        }
        let shape = self.cache.get(
            cx.text,
            &self.label,
            cx.theme.body_size,
            cx.theme.foreground,
            None,
        );
        cx.display
            .push_layout(&shape.layout, label_origin(rect, shape.height), cx.clip);
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        let select = || {
            if self.group.get() != self.value {
                self.group.set(self.value);
            }
        };
        match event {
            UiEvent::PointerDown { .. } => {
                select();
                EventStatus::Handled
            }
            UiEvent::KeyDown {
                key: Key::Space, ..
            } if cx.state.focused => {
                select();
                EventStatus::Handled
            }
            UiEvent::AccessibilityClick => {
                select();
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn cursor(&self) -> grafikr_core::CursorIcon {
        grafikr_core::CursorIcon::Pointer
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::RadioButton);
        node.set_label(self.label.clone());
        if self.group.get() == self.value {
            node.set_selected(true);
        }
        node.add_action(accesskit::Action::Click);
    }
}

// -------------------------------------------------------------------- Switch

pub struct Switch {
    label: String,
    checked: Signal<bool>,
}

pub fn switch(label: impl Into<String>, checked: Signal<bool>) -> Switch {
    Switch {
        label: label.into(),
        checked,
    }
}

impl View for Switch {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style::default());
        let subscriber = track(&self.checked, id, cx.runtime);
        cx.set_widget(
            id,
            SwitchWidget {
                label: self.label.clone(),
                checked: self.checked.clone(),
                cache: TextCache::default(),
                _subscriber: subscriber,
            },
        );
        id
    }
}

struct SwitchWidget {
    label: String,
    checked: Signal<bool>,
    cache: TextCache,
    _subscriber: Rc<dyn Fn()>,
}

impl SwitchWidget {
    fn toggle(&self) {
        self.checked.update(|value| *value = !*value);
    }
}

impl Widget for SwitchWidget {
    fn measure(&mut self, cx: &mut MeasureCx, _available: Size) -> Size {
        let shape = self.cache.get(
            cx.text,
            &self.label,
            cx.theme.body_size,
            cx.theme.foreground,
            None,
        );
        Size::new(
            38.0 + GAP + shape.width,
            shape.height.max(24.0).max(cx.theme.control_height * 0.7),
        )
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        let on = self.checked.get();
        let track = Rect::new(
            rect.x,
            rect.y + (rect.height - 22.0).max(0.0) * 0.5,
            38.0,
            22.0,
        );
        cx.push_rect(
            track,
            if on {
                cx.theme.accent
            } else {
                cx.theme.muted.with_alpha(90)
            },
            11.0,
        );
        let knob_x = if on { track.x + 19.0 } else { track.x + 3.0 };
        cx.push_rect(
            Rect::new(knob_x, track.y + 3.0, 16.0, 16.0),
            Color::WHITE,
            8.0,
        );
        if cx.state.focused {
            cx.push_bordered_rect(
                track,
                Color::TRANSPARENT,
                11.0,
                Some((2.0, cx.theme.accent)),
            );
        }
        let shape = self.cache.get(
            cx.text,
            &self.label,
            cx.theme.body_size,
            cx.theme.foreground,
            None,
        );
        cx.display.push_layout(
            &shape.layout,
            Point::new(
                rect.x + 38.0 + GAP,
                rect.y + (rect.height - shape.height).max(0.0) * 0.5,
            ),
            cx.clip,
        );
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::PointerDown { .. } => {
                self.toggle();
                EventStatus::Handled
            }
            UiEvent::KeyDown {
                key: Key::Space, ..
            } if cx.state.focused => {
                self.toggle();
                EventStatus::Handled
            }
            UiEvent::AccessibilityClick => {
                self.toggle();
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn cursor(&self) -> grafikr_core::CursorIcon {
        grafikr_core::CursorIcon::Pointer
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Switch);
        node.set_label(self.label.clone());
        node.set_toggled(self.checked.get().into());
        node.add_action(accesskit::Action::Click);
    }
}

// -------------------------------------------------------------------- Slider

pub struct Slider {
    value: Signal<f32>,
    min: f32,
    max: f32,
    step: f32,
    width: f32,
}

pub fn slider(value: Signal<f32>) -> Slider {
    Slider {
        value,
        min: 0.0,
        max: 1.0,
        step: 0.0,
        width: 200.0,
    }
}

impl Slider {
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
}

impl View for Slider {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style {
            width: Some(self.width),
            height: Some(28.0),
            ..Style::default()
        });
        let subscriber = track(&self.value, id, cx.runtime);
        cx.set_widget(
            id,
            SliderWidget {
                value: self.value.clone(),
                min: self.min,
                max: self.max.max(self.min + f32::EPSILON),
                step: self.step,
                dragging: false,
                _subscriber: subscriber,
            },
        );
        id
    }
}

struct SliderWidget {
    value: Signal<f32>,
    min: f32,
    max: f32,
    step: f32,
    dragging: bool,
    _subscriber: Rc<dyn Fn()>,
}

impl SliderWidget {
    fn fraction(&self) -> f32 {
        ((self.value.get() - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    fn track(&self, rect: Rect) -> Rect {
        Rect::new(
            rect.x + 8.0,
            rect.y + rect.height * 0.5 - 2.0,
            (rect.width - 16.0).max(1.0),
            4.0,
        )
    }

    fn set_from_x(&self, rect: Rect, x: f32) {
        let track = self.track(rect);
        let fraction = ((x - track.x) / track.width).clamp(0.0, 1.0);
        let mut value = self.min + fraction * (self.max - self.min);
        if self.step > 0.0 {
            value = self.min + ((value - self.min) / self.step).round() * self.step;
        }
        self.value.set(value.clamp(self.min, self.max));
    }

    fn nudge(&self, direction: f32) {
        let step = if self.step > 0.0 {
            self.step
        } else {
            (self.max - self.min) / 100.0
        };
        let value = (self.value.get() + direction * step).clamp(self.min, self.max);
        self.value.set(value);
    }
}

impl Widget for SliderWidget {
    fn measure(&mut self, cx: &mut MeasureCx, _available: Size) -> Size {
        Size::new(200.0, cx.theme.control_height.min(28.0))
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        let fraction = self.fraction();
        let track = self.track(rect);
        cx.push_rect(track, cx.theme.muted.with_alpha(70), 2.0);
        let filled = Rect::new(track.x, track.y, track.width * fraction, track.height);
        cx.push_rect(filled, cx.theme.accent, 2.0);
        let knob_x = track.x + track.width * fraction;
        let knob = Rect::new(knob_x - 8.0, rect.y + rect.height * 0.5 - 8.0, 16.0, 16.0);
        cx.push_rect(knob, Color::WHITE, 8.0);
        if cx.state.focused {
            cx.push_bordered_rect(knob, Color::TRANSPARENT, 8.0, Some((2.0, cx.theme.accent)));
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::PointerDown { pos, .. } => {
                self.dragging = true;
                self.set_from_x(cx.rect, pos.x);
                EventStatus::Handled
            }
            UiEvent::PointerMoved { pos } if self.dragging => {
                self.set_from_x(cx.rect, pos.x);
                EventStatus::Handled
            }
            UiEvent::PointerUp { .. } => {
                self.dragging = false;
                EventStatus::Handled
            }
            UiEvent::KeyDown { key, modifiers, .. } if cx.state.focused => {
                let large = if modifiers.shift { 10.0 } else { 1.0 };
                match key {
                    Key::Left => self.nudge(-large),
                    Key::Right => self.nudge(large),
                    Key::Down => self.nudge(-large),
                    Key::Up => self.nudge(large),
                    Key::PageDown => self.nudge(-10.0),
                    Key::PageUp => self.nudge(10.0),
                    Key::Home => self.value.set(self.min),
                    Key::End => self.value.set(self.max),
                    _ => return EventStatus::Ignored,
                }
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn cursor(&self) -> grafikr_core::CursorIcon {
        grafikr_core::CursorIcon::Pointer
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Slider);
        node.set_numeric_value(self.value.get() as f64);
        node.set_min_numeric_value(self.min as f64);
        node.set_max_numeric_value(self.max as f64);
        node.set_numeric_value_step(if self.step > 0.0 {
            self.step as f64
        } else {
            ((self.max - self.min) / 100.0) as f64
        });
        node.add_action(accesskit::Action::Focus);
    }
}

// ------------------------------------------------------------------ Progress

pub struct ProgressBar {
    value: f32,
    width: f32,
}

pub fn progress(value: f32) -> ProgressBar {
    ProgressBar {
        value: value.clamp(0.0, 1.0),
        width: 220.0,
    }
}

impl ProgressBar {
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
}

impl View for ProgressBar {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let id = cx.add_node(Style {
            width: Some(self.width),
            height: Some(8.0),
            ..Style::default()
        });
        cx.set_widget(id, ProgressWidget { value: self.value });
        id
    }
}

struct ProgressWidget {
    value: f32,
}

impl Widget for ProgressWidget {
    fn measure(&mut self, _cx: &mut MeasureCx, _available: Size) -> Size {
        Size::new(220.0, 8.0)
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        cx.push_rect(rect, cx.theme.muted.with_alpha(60), 4.0);
        let filled = Rect::new(
            rect.x,
            rect.y,
            rect.width * self.value.clamp(0.0, 1.0),
            rect.height,
        );
        cx.push_rect(filled, cx.theme.accent, 4.0);
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::ProgressIndicator);
        node.set_numeric_value((self.value * 100.0) as f64);
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(100.0);
    }
}

// A few helpers are shared with the canvas module.

into_views!(Checkbox, Radio, Switch, Slider, ProgressBar);
