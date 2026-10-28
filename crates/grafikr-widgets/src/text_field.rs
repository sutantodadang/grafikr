//! Single-line text field with selection, clipboard, and IME via Parley.

use std::rc::Rc;

use grafikr_core::{
    Color, EventCx, EventStatus, Key, MeasureCx, MountCx, NodeId, PaintCx, Point, Rect, Size,
    Style, UiEvent, View, Widget,
};
use parley::{PlainEditor, StyleProperty};

use crate::into_views;

type ChangeHandler = Rc<dyn Fn(String)>;

pub struct TextField {
    initial: String,
    width: Option<f32>,
    on_change: Option<ChangeHandler>,
    multiline: bool,
    password: bool,
}

pub fn text_field(initial: impl Into<String>) -> TextField {
    TextField {
        initial: initial.into(),
        width: None,
        on_change: None,
        multiline: false,
        password: false,
    }
}

impl TextField {
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    pub fn password(mut self) -> Self {
        self.password = true;
        self
    }

    pub fn on_change(mut self, handler: impl Fn(String) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl View for TextField {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        let theme = cx.theme.clone();
        let mut editor = PlainEditor::<Color>::new(theme.body_size);
        editor.set_text(&self.initial);
        editor
            .edit_styles()
            .insert(StyleProperty::Brush(theme.foreground));
        let style = Style {
            width: Some(self.width.unwrap_or(220.0)),
            height: Some(if self.multiline {
                120.0
            } else {
                theme.control_height
            }),
            ..Style::default()
        };
        let id = cx.add_node(style);
        cx.set_widget(
            id,
            TextFieldWidget {
                editor,
                on_change: self.on_change.clone(),
                last_reported: self.initial.clone(),
                dragging: false,
                layout_width: None,
                multiline: self.multiline,
                password: self.password,
            },
        );
        id
    }
}

struct TextFieldWidget {
    editor: PlainEditor<Color>,
    on_change: Option<ChangeHandler>,
    last_reported: String,
    dragging: bool,
    layout_width: Option<f32>,
    multiline: bool,
    password: bool,
}

const PADDING_X: f32 = 10.0;
const PADDING_Y: f32 = 8.0;

impl TextFieldWidget {
    fn sync_layout(&mut self, width: f32) {
        if self.layout_width != Some(width) {
            self.editor.set_width(Some(width.max(1.0)));
            self.layout_width = Some(width);
        }
    }

    fn report(&mut self) {
        let text = self.editor.text().to_string();
        if text != self.last_reported {
            self.last_reported = text.clone();
            if let Some(handler) = &self.on_change {
                handler(text);
            }
        }
    }

    fn with_driver(
        &mut self,
        cx: &mut EventCx,
        f: impl FnOnce(&mut parley::PlainEditorDriver<'_, Color>),
    ) {
        let (font_cx, layout_cx) = cx.text.parts();
        let mut driver = self.editor.driver(font_cx, layout_cx);
        f(&mut driver);
    }

    fn point_to_text(&mut self, cx: &mut EventCx, pos: Point) -> (f32, f32) {
        let width = (cx.rect.width - PADDING_X * 2.0).max(1.0);
        let height = {
            let (font_cx, layout_cx) = cx.text.parts();
            self.editor.set_width(Some(width));
            self.layout_width = Some(width);
            self.editor.layout(font_cx, layout_cx).height()
        };
        let origin_y = if self.multiline {
            cx.rect.y + PADDING_Y
        } else {
            cx.rect.y + (cx.rect.height - height) * 0.5
        };
        (pos.x - (cx.rect.x + PADDING_X), pos.y - origin_y)
    }
}

impl Widget for TextFieldWidget {
    fn measure(&mut self, cx: &mut MeasureCx, available: Size) -> Size {
        let width =
            self.layout_width
                .unwrap_or(available.width)
                .min(if available.width.is_finite() {
                    available.width
                } else {
                    220.0
                });
        let height = {
            let (font_cx, layout_cx) = cx.text.parts();
            self.editor.set_width(Some(width.max(1.0)));
            self.layout_width = Some(width);
            self.editor.layout(font_cx, layout_cx).height()
        };
        Size::new(width, height.max(cx.theme.control_height))
    }

    fn paint(&mut self, cx: &mut PaintCx, rect: Rect) {
        let radius = cx.theme.control_radius();
        cx.push_bordered_rect(
            rect,
            cx.theme.field_background(cx.state),
            radius,
            Some((cx.theme.border_width, cx.theme.field_border(cx.state))),
        );

        let content = Rect::new(
            rect.x + PADDING_X,
            if self.multiline {
                rect.y + PADDING_Y
            } else {
                rect.y
            },
            (rect.width - PADDING_X * 2.0).max(1.0),
            if self.multiline {
                (rect.height - PADDING_Y * 2.0).max(1.0)
            } else {
                rect.height
            },
        );
        let clip = cx
            .clip
            .map(|clip| clip.intersect(content))
            .unwrap_or(content);

        if self.layout_width != Some(content.width) {
            self.editor.set_width(Some(content.width));
            self.layout_width = Some(content.width);
        }

        let (font_cx, layout_cx) = cx.text.parts();
        self.editor.refresh_layout(font_cx, layout_cx);
        let layout = self.editor.try_layout().expect("layout was refreshed");
        let text_height = layout.height();
        let origin = Point::new(
            content.x,
            if self.multiline {
                content.y
            } else {
                rect.y + (rect.height - text_height) * 0.5
            },
        );

        let selection: Vec<_> = self.editor.selection_geometry();
        for (bounds, _) in selection {
            let selection = Rect::new(
                origin.x + bounds.x0 as f32,
                origin.y + bounds.y0 as f32,
                (bounds.x1 - bounds.x0) as f32,
                (bounds.y1 - bounds.y0) as f32,
            );
            cx.display
                .push_rect(selection, cx.theme.accent.with_alpha(80), 0.0, Some(clip));
        }

        if self.password {
            cx.display.push_layout_masked(layout, origin, Some(clip));
        } else {
            cx.display.push_layout(layout, origin, Some(clip));
        }

        if cx.state.focused
            && let Some(bounds) = self.editor.cursor_geometry(1.5)
        {
            let cursor = Rect::new(
                origin.x + bounds.x0 as f32,
                origin.y + bounds.y0 as f32,
                ((bounds.x1 - bounds.x0) as f32).max(1.0),
                (bounds.y1 - bounds.y0) as f32,
            );
            cx.display
                .push_rect(cursor, cx.theme.accent, 0.0, Some(clip));
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> EventStatus {
        match event {
            UiEvent::PointerDown { pos, .. } => {
                self.dragging = true;
                let width = cx.rect.width - PADDING_X * 2.0;
                self.sync_layout(width);
                let (x, y) = self.point_to_text(cx, *pos);
                self.with_driver(cx, |driver| driver.move_to_point(x, y));
                cx.request_paint();
                EventStatus::Handled
            }
            UiEvent::PointerMoved { pos } if self.dragging => {
                let (x, y) = self.point_to_text(cx, *pos);
                self.with_driver(cx, |driver| driver.extend_selection_to_point(x, y));
                cx.request_paint();
                EventStatus::Handled
            }
            UiEvent::PointerUp { .. } => {
                self.dragging = false;
                EventStatus::Handled
            }
            UiEvent::Text(text) => {
                if !text.is_empty() {
                    self.with_driver(cx, |driver| driver.insert_or_replace_selection(text));
                    self.report();
                    cx.request_layout();
                }
                EventStatus::Handled
            }
            UiEvent::SetValue(text) if !self.password => {
                self.with_driver(cx, |driver| {
                    driver.select_all();
                    driver.insert_or_replace_selection(text);
                });
                self.report();
                cx.request_layout();
                EventStatus::Handled
            }
            UiEvent::KeyDown { key, modifiers, .. } => {
                if self.multiline && *key == Key::Enter && !modifiers.ctrl && !modifiers.alt {
                    self.with_driver(cx, |driver| driver.insert_or_replace_selection("\n"));
                    self.report();
                    cx.request_layout();
                    return EventStatus::Handled;
                }
                self.handle_key(cx, *key, *modifiers)
            }
            UiEvent::ImePreedit { text, cursor } => {
                if text.is_empty() {
                    self.with_driver(cx, |driver| driver.clear_compose());
                } else {
                    self.with_driver(cx, |driver| driver.set_compose(text, *cursor));
                }
                cx.request_layout();
                EventStatus::Handled
            }
            UiEvent::ImeCommit(text) => {
                if self.editor.is_composing() {
                    if text.is_empty() {
                        self.with_driver(cx, |driver| driver.clear_compose());
                    } else {
                        self.with_driver(cx, |driver| {
                            driver.set_compose(text, Some((text.len(), text.len())));
                            driver.finish_compose();
                        });
                    }
                } else if !text.is_empty() {
                    self.with_driver(cx, |driver| driver.insert_or_replace_selection(text));
                }
                self.report();
                cx.request_layout();
                EventStatus::Handled
            }
            UiEvent::FocusLost => {
                self.with_driver(cx, |driver| driver.clear_compose());
                cx.request_paint();
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn accepts_text_input(&self) -> bool {
        true
    }

    fn cursor(&self) -> grafikr_core::CursorIcon {
        grafikr_core::CursorIcon::Text
    }

    fn accessibility(&mut self, node: &mut accesskit::Node) {
        node.set_role(if self.password {
            accesskit::Role::PasswordInput
        } else if self.multiline {
            accesskit::Role::MultilineTextInput
        } else {
            accesskit::Role::TextInput
        });
        let value = self.editor.text().to_string();
        if self.password {
            node.set_value("\u{2022}".repeat(value.chars().count()));
        } else {
            node.set_value(value);
        }
        node.add_action(accesskit::Action::Focus);
        if !self.password {
            node.add_action(accesskit::Action::SetValue);
        }
    }
}

impl TextFieldWidget {
    fn handle_key(
        &mut self,
        cx: &mut EventCx,
        key: Key,
        modifiers: grafikr_core::Modifiers,
    ) -> EventStatus {
        let ctrl = modifiers.ctrl || modifiers.super_key;
        match key {
            Key::Character('a') if ctrl => {
                self.with_driver(cx, |driver| driver.select_all());
            }
            Key::Character('c') if ctrl => {
                if let Some(text) = self.editor.selected_text().map(str::to_owned) {
                    cx.clipboard.set_text(text);
                }
            }
            Key::Character('x') if ctrl => {
                if let Some(text) = self.editor.selected_text().map(str::to_owned) {
                    cx.clipboard.set_text(text);
                    self.with_driver(cx, |driver| driver.delete_selection());
                }
            }
            Key::Character('v') if ctrl => {
                if let Some(text) = cx.clipboard.get_text() {
                    self.with_driver(cx, |driver| driver.insert_or_replace_selection(&text));
                }
            }
            Key::Backspace => {
                if ctrl {
                    self.with_driver(cx, |driver| driver.backdelete_word());
                } else {
                    self.with_driver(cx, |driver| driver.backdelete());
                }
            }
            Key::Delete => {
                if ctrl {
                    self.with_driver(cx, |driver| driver.delete_word());
                } else {
                    self.with_driver(cx, |driver| driver.delete());
                }
            }
            Key::Left => {
                if modifiers.shift {
                    self.with_driver(cx, |driver| driver.select_left());
                } else {
                    self.with_driver(cx, |driver| driver.move_left());
                }
            }
            Key::Right => {
                if modifiers.shift {
                    self.with_driver(cx, |driver| driver.select_right());
                } else {
                    self.with_driver(cx, |driver| driver.move_right());
                }
            }
            Key::Up => {
                if modifiers.shift {
                    self.with_driver(cx, |driver| driver.select_up());
                } else {
                    self.with_driver(cx, |driver| driver.move_up());
                }
            }
            Key::Down => {
                if modifiers.shift {
                    self.with_driver(cx, |driver| driver.select_down());
                } else {
                    self.with_driver(cx, |driver| driver.move_down());
                }
            }
            Key::Home => {
                if modifiers.shift {
                    self.with_driver(cx, |driver| driver.select_to_line_start());
                } else {
                    self.with_driver(cx, |driver| driver.move_to_line_start());
                }
            }
            Key::End => {
                if modifiers.shift {
                    self.with_driver(cx, |driver| driver.select_to_line_end());
                } else {
                    self.with_driver(cx, |driver| driver.move_to_line_end());
                }
            }
            _ => return EventStatus::Ignored,
        }
        self.report();
        cx.request_layout();
        EventStatus::Handled
    }
}

into_views!(TextField);
