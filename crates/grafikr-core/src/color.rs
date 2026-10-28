//! Colors and geometry in logical pixels (DIP).

use std::ops::{Add, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn hex(value: u32) -> Self {
        Self::rgb(
            ((value >> 16) & 0xff) as u8,
            ((value >> 8) & 0xff) as u8,
            (value & 0xff) as u8,
        )
    }

    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// Linear interpolation in sRGB space. `t` is clamped to `0..=1`.
    pub fn mix(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Self {
            r: lerp(self.r, other.r),
            g: lerp(self.g, other.g),
            b: lerp(self.b, other.b),
            a: lerp(self.a, other.a),
        }
    }

    pub fn to_array(self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }

    /// Linear-space value for a wgpu clear color on an sRGB target.
    pub fn to_linear(self) -> [f64; 4] {
        let f = |c: u8| {
            let c = c as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        [f(self.r), f(self.g), f(self.b), self.a as f64 / 255.0]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

impl Add for Point {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Point {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

/// Translation and uniform scale applied visually to a node subtree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translate: Point,
    pub scale: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translate: Point::ZERO,
            scale: 1.0,
        }
    }
}

impl Transform {
    pub const IDENTITY: Self = Self {
        translate: Point::ZERO,
        scale: 1.0,
    };

    pub const fn translate(x: f32, y: f32) -> Self {
        Self {
            translate: Point::new(x, y),
            scale: 1.0,
        }
    }

    pub const fn scale(scale: f32) -> Self {
        Self {
            translate: Point::ZERO,
            scale,
        }
    }

    pub fn is_identity(self) -> bool {
        self.scale == 1.0 && self.translate.x == 0.0 && self.translate.y == 0.0
    }

    /// Map a point from local space into window space.
    pub fn apply(self, point: Point, origin: Point) -> Point {
        Point::new(
            origin.x + (point.x - origin.x) * self.scale + self.translate.x,
            origin.y + (point.y - origin.y) * self.scale + self.translate.y,
        )
    }

    /// Map a window-space point back into local space.
    pub fn inverse(self, point: Point, origin: Point) -> Point {
        if self.scale == 0.0 {
            return point;
        }
        Point::new(
            origin.x + (point.x - origin.x - self.translate.x) / self.scale,
            origin.y + (point.y - origin.y - self.translate.y) / self.scale,
        )
    }

    pub fn apply_rect(self, rect: Rect, origin: Point) -> Rect {
        let min = self.apply(Point::new(rect.x, rect.y), origin);
        Rect::new(
            min.x,
            min.y,
            rect.width * self.scale,
            rect.height * self.scale,
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Self = Self {
        width: 0.0,
        height: 0.0,
    };

    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn from_origin_size(origin: Point, size: Size) -> Self {
        Self::new(origin.x, origin.y, size.width, size.height)
    }

    pub fn right(self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.right() && p.y < self.bottom()
    }

    pub fn intersect(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Self::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
    }

    pub fn is_empty(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    pub fn inset(self, insets: EdgeInsets) -> Self {
        Self::new(
            self.x + insets.left,
            self.y + insets.top,
            (self.width - insets.left - insets.right).max(0.0),
            (self.height - insets.top - insets.bottom).max(0.0),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl EdgeInsets {
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    pub const fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub top: Option<f32>,
    pub right: Option<f32>,
    pub bottom: Option<f32>,
    pub left: Option<f32>,
}

impl Insets {
    pub const NONE: Self = Self {
        top: None,
        right: None,
        bottom: None,
        left: None,
    };

    pub const fn all(value: f32) -> Self {
        Self {
            top: Some(value),
            right: Some(value),
            bottom: Some(value),
            left: Some(value),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub width: f32,
    pub color: Color,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub blur: f32,
    pub color: Color,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_roundtrips_points() {
        let transform = Transform {
            translate: Point::new(12.0, -4.0),
            scale: 1.5,
        };
        let origin = Point::new(20.0, 10.0);
        let point = Point::new(52.0, 38.0);
        let mapped = transform.apply(point, origin);
        let back = transform.inverse(mapped, origin);
        assert!((back.x - point.x).abs() < 1e-4);
        assert!((back.y - point.y).abs() < 1e-4);
    }

    #[test]
    fn transform_scales_rect_about_origin() {
        let transform = Transform::scale(2.0);
        let rect = transform.apply_rect(Rect::new(10.0, 20.0, 30.0, 40.0), Point::ZERO);
        assert_eq!(rect, Rect::new(20.0, 40.0, 60.0, 80.0));
    }
}
