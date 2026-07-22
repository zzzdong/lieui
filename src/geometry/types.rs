//! 基础几何类型

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub const fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}
impl Size {
    pub const fn new(w: f32, h: f32) -> Self {
        Self {
            width: w,
            height: h,
        }
    }
    pub const fn zero() -> Self {
        Self {
            width: 0.0,
            height: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            x,
            y,
            width: w,
            height: h,
        }
    }
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.x + self.width && p.y >= self.y && p.y <= self.y + self.height
    }
}

/// RGBA 颜色 (u8 值)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}
impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    pub fn from_hex(hex: &str) -> Self {
        let h = hex.trim_start_matches('#');
        if h.len() == 6 {
            Self::new(
                u8::from_str_radix(&h[0..2], 16).unwrap_or(0),
                u8::from_str_radix(&h[2..4], 16).unwrap_or(0),
                u8::from_str_radix(&h[4..6], 16).unwrap_or(0),
            )
        } else if h.len() == 8 {
            Self::rgba(
                u8::from_str_radix(&h[0..2], 16).unwrap_or(0),
                u8::from_str_radix(&h[2..4], 16).unwrap_or(0),
                u8::from_str_radix(&h[4..6], 16).unwrap_or(0),
                u8::from_str_radix(&h[6..8], 16).unwrap_or(255),
            )
        } else {
            Self::BLACK
        }
    }
    pub fn as_vello(&self) -> vello_cpu::color::AlphaColor<vello_cpu::color::Srgb> {
        vello_cpu::color::AlphaColor::from_rgba8(self.r, self.g, self.b, self.a)
    }
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const RED: Color = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
}
impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}
