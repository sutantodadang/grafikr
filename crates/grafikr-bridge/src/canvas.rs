use serde::Serialize;

use grafikr_core::{Color, ImageData, Point, Rect, TextCache};
use grafikr_widgets::CanvasPainter;
use std::sync::Arc;
use std::{cell::RefCell, rc::Rc};

use crate::handles::Handle;
use crate::protocol::BridgeError;

pub const MAX_CANVAS_COMMANDS: usize = 10_000;
pub const MAX_CANVAS_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub enum CanvasCommand {
    Rect {
        rect: Rect,
        color: Color,
        radius: f32,
    },
    BorderedRect {
        rect: Rect,
        color: Color,
        radius: f32,
        width: f32,
        border: Color,
    },
    Shadow {
        rect: Rect,
        radius: f32,
        blur: f32,
        color: Color,
    },
    Text {
        x: f32,
        y: f32,
        text: String,
        size: f32,
        color: Color,
    },
    Image {
        rect: Rect,
        handle: Handle,
        data: Arc<ImageData>,
    },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CanvasWire<'a> {
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: [u8; 4],
        radius: f32,
    },
    BorderedRect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: [u8; 4],
        radius: f32,
        border_width: f32,
        border_color: [u8; 4],
    },
    Shadow {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        blur: f32,
        color: [u8; 4],
    },
    Text {
        x: f32,
        y: f32,
        text: &'a str,
        size: f32,
        color: [u8; 4],
    },
    Image {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        image: String,
    },
}

#[derive(Default)]
struct CanvasData {
    commands: Vec<CanvasCommand>,
    canonical_bytes: usize,
}

#[derive(Clone, Default)]
pub struct CanvasModel(Rc<RefCell<CanvasData>>);

impl CanvasModel {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn replace(&self, commands: Vec<CanvasCommand>) -> Result<(), BridgeError> {
        if commands.len() > MAX_CANVAS_COMMANDS {
            return Err(BridgeError::new(
                "quota_exceeded",
                "canvas command quota exceeded",
            ));
        }
        let mut bytes = 0usize;
        for command in &commands {
            validate_command(command)?;
            bytes = bytes.checked_add(command_bytes(command)?).ok_or_else(|| {
                BridgeError::new("quota_exceeded", "canvas payload size overflow")
            })?;
        }
        if bytes > MAX_CANVAS_BYTES {
            return Err(BridgeError::new(
                "quota_exceeded",
                "canvas payload exceeds 8 MiB",
            ));
        }
        let mut data = self.0.borrow_mut();
        data.commands = commands;
        data.canonical_bytes = bytes;
        Ok(())
    }
    pub fn command_count(&self) -> usize {
        self.0.borrow().commands.len()
    }
    pub fn canonical_bytes(&self) -> usize {
        self.0.borrow().canonical_bytes
    }
    pub fn painter(&self) -> CanvasPainter {
        let data = self.0.clone();
        let text_cache = Rc::new(RefCell::new(TextCache::default()));
        Rc::new(move |cx, canvas| {
            let clip = cx.clip.map(|clip| clip.intersect(canvas)).unwrap_or(canvas);
            for command in &data.borrow().commands {
                let offset = |rect: Rect| {
                    Rect::new(
                        canvas.x + rect.x,
                        canvas.y + rect.y,
                        rect.width,
                        rect.height,
                    )
                };
                match command {
                    CanvasCommand::Rect {
                        rect,
                        color,
                        radius,
                    } => cx
                        .display
                        .push_rect(offset(*rect), *color, *radius, Some(clip)),
                    CanvasCommand::BorderedRect {
                        rect,
                        color,
                        radius,
                        width,
                        border,
                    } => {
                        cx.display.push_bordered_rect(
                            offset(*rect),
                            *color,
                            *radius,
                            Some((*width, *border)),
                            Some(clip),
                        );
                    }
                    CanvasCommand::Shadow {
                        rect,
                        radius,
                        blur,
                        color,
                    } => {
                        cx.display
                            .push_shadow(offset(*rect), *radius, *blur, *color, Some(clip));
                    }
                    CanvasCommand::Text {
                        x,
                        y,
                        text,
                        size,
                        color,
                    } => {
                        let layout = text_cache
                            .borrow_mut()
                            .get(cx.text, text, *size, *color, None)
                            .layout
                            .clone();
                        cx.display.push_layout(
                            &layout,
                            Point::new(canvas.x + x, canvas.y + y),
                            Some(clip),
                        );
                    }
                    CanvasCommand::Image { rect, data, .. } => {
                        cx.display
                            .push_image(data.clone(), offset(*rect), Some(clip))
                    }
                }
            }
        })
    }
}

fn validate_command(command: &CanvasCommand) -> Result<(), BridgeError> {
    let finite = |values: &[f32]| values.iter().all(|value| value.is_finite());
    let rect =
        |r: Rect| finite(&[r.x, r.y, r.width, r.height]) && r.width >= 0.0 && r.height >= 0.0;
    let valid = match command {
        CanvasCommand::Rect {
            rect: r, radius, ..
        } => rect(*r) && finite(&[*radius]) && *radius >= 0.0,
        CanvasCommand::BorderedRect {
            rect: r,
            radius,
            width,
            ..
        } => rect(*r) && finite(&[*radius, *width]) && *radius >= 0.0 && *width >= 0.0,
        CanvasCommand::Shadow {
            rect: r,
            radius,
            blur,
            ..
        } => rect(*r) && finite(&[*radius, *blur]) && *radius >= 0.0 && *blur >= 0.0,
        CanvasCommand::Text { x, y, size, .. } => finite(&[*x, *y, *size]) && *size > 0.0,
        CanvasCommand::Image { rect: r, .. } => rect(*r),
    };
    if valid {
        Ok(())
    } else {
        Err(BridgeError::new(
            "invalid_value",
            "canvas geometry must be finite and nonnegative",
        ))
    }
}

fn command_bytes(command: &CanvasCommand) -> Result<usize, BridgeError> {
    let wire = match command {
        CanvasCommand::Rect {
            rect,
            color,
            radius,
        } => CanvasWire::Rect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            color: rgba(*color),
            radius: *radius,
        },
        CanvasCommand::BorderedRect {
            rect,
            color,
            radius,
            width,
            border,
        } => CanvasWire::BorderedRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            color: rgba(*color),
            radius: *radius,
            border_width: *width,
            border_color: rgba(*border),
        },
        CanvasCommand::Shadow {
            rect,
            radius,
            blur,
            color,
        } => CanvasWire::Shadow {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            radius: *radius,
            blur: *blur,
            color: rgba(*color),
        },
        CanvasCommand::Text {
            x,
            y,
            text,
            size,
            color,
        } => CanvasWire::Text {
            x: *x,
            y: *y,
            text,
            size: *size,
            color: rgba(*color),
        },
        CanvasCommand::Image { rect, handle, .. } => CanvasWire::Image {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            image: handle.0.to_string(),
        },
    };
    serde_json::to_vec(&wire)
        .map(|bytes| bytes.len())
        .map_err(|e| BridgeError::new("invalid_value", e.to_string()))
}
fn rgba(color: Color) -> [u8; 4] {
    [color.r, color.g, color.b, color.a]
}
