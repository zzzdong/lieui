//! 基础几何类型（Point, Size, Rect, Color）

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point { pub x: f32, pub y: f32 }
impl Point {
    pub const fn new(x: f32, y: f32) -> Self { Self { x, y } }
    pub const fn zero() -> Self { Self { x: 0.0, y: 0.0 } }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size { pub width: f32, pub height: f32 }
impl Size {
    pub const fn new(width: f32, height: f32) -> Self { Self { width, height } }
    pub const fn zero() -> Self { Self { width: 0.0, height: 0.0 } }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect { pub x: f32, pub y: f32, pub width: f32, pub height: f32 }
impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self { Self { x, y, width, height } }
    pub fn contains(&self, point: Point) -> bool { point.x >= self.x && point.x <= self.x + self.width && point.y >= self.y && point.y <= self.y + self.height }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color { pub r: f32, pub g: f32, pub b: f32, pub a: f32 }
impl Color {
    pub const fn from_rgb8(r: u8, g: u8, b: u8) -> Self { Self::from_rgba8(r, g, b, 255) }
    pub const fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self { Self { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: a as f32 / 255.0 } }
    pub fn from_hex(hex: &str) -> Self {
        let h = hex.trim_start_matches('#');
        if h.len() == 6 { Self::from_rgb8(u8::from_str_radix(&h[0..2], 16).unwrap_or(0), u8::from_str_radix(&h[2..4], 16).unwrap_or(0), u8::from_str_radix(&h[4..6], 16).unwrap_or(0)) }
        else if h.len() == 8 { Self::from_rgba8(u8::from_str_radix(&h[0..2], 16).unwrap_or(0), u8::from_str_radix(&h[2..4], 16).unwrap_or(0), u8::from_str_radix(&h[4..6], 16).unwrap_or(0), u8::from_str_radix(&h[6..8], 16).unwrap_or(255)) }
        else { Self::BLACK }
    }
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const RED: Color = Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
}
impl Default for Color { fn default() -> Self { Self::BLACK } }
