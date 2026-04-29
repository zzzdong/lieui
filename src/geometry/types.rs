// src/geometry/types.rs

use vello_cpu::color::{AlphaColor, Srgb};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Self = Self::new(0.0, 0.0);

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub const fn zero() -> Self {
        Self::new(0.0, 0.0)
    }

    // 两点距离
    pub fn distance_to(&self, other: Point) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Self = Self::new(0.0, 0.0);

    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    pub const fn zero() -> Self {
        Self::new(0.0, 0.0)
    }

    /// 将尺寸限制在 min 和 max 之间
    pub fn clamp(&self, min: Size, max: Size) -> Size {
        Size::new(
            self.width.clamp(min.width, max.width),
            self.height.clamp(min.height, max.height),
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.width
            && point.y >= self.y
            && point.y <= self.y + self.height
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.width, self.height)
    }
}

/// 带等圆角的矩形（四个角半径相同）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundedRect {
    pub rect: Rect,
    pub radius: f32,
}

impl RoundedRect {
    pub fn new(rect: Rect, radius: f32) -> Self {
        // 校正半径不超过矩形边长的一半
        let radius = radius.min(rect.width / 2.0).min(rect.height / 2.0);
        Self { rect, radius }
    }

    /// 精确命中测试：点是否在圆角矩形内
    pub fn contains(&self, point: Point) -> bool {
        let r = self.radius;
        let rect = &self.rect;

        // 1. 快速排除：在整体矩形外
        if !rect.contains(point) {
            return false;
        }

        // 2. 内部矩形（无圆角影响的区域）
        let inner_rect = Rect::new(
            rect.x + r,
            rect.y + r,
            rect.width - 2.0 * r,
            rect.height - 2.0 * r,
        );
        if inner_rect.contains(point) {
            return true;
        }

        // 3. 检查四个角
        // 左上角圆心
        if point.x < rect.x + r && point.y < rect.y + r {
            let cx = rect.x + r;
            let cy = rect.y + r;
            return Point::new(point.x, point.y).distance_to(Point::new(cx, cy)) <= r;
        }
        // 右上角
        if point.x > rect.x + rect.width - r && point.y < rect.y + r {
            let cx = rect.x + rect.width - r;
            let cy = rect.y + r;
            return Point::new(point.x, point.y).distance_to(Point::new(cx, cy)) <= r;
        }
        // 左下角
        if point.x < rect.x + r && point.y > rect.y + rect.height - r {
            let cx = rect.x + r;
            let cy = rect.y + rect.height - r;
            return Point::new(point.x, point.y).distance_to(Point::new(cx, cy)) <= r;
        }
        // 右下角
        if point.x > rect.x + rect.width - r && point.y > rect.y + rect.height - r {
            let cx = rect.x + rect.width - r;
            let cy = rect.y + rect.height - r;
            return Point::new(point.x, point.y).distance_to(Point::new(cx, cy)) <= r;
        }

        // 在四条边平直区域
        true
    }

    /// 获取内部矩形（用于布局内容放置）
    pub fn inner_rect(&self) -> Rect {
        let r = self.radius;
        Rect::new(
            self.rect.x + r,
            self.rect.y + r,
            self.rect.width - 2.0 * r,
            self.rect.height - 2.0 * r,
        )
    }

    /// 转为此形状的 kurbo 表示（用于渲染）
    pub fn to_kurbo_rounded_rect(&self) -> vello_cpu::kurbo::RoundedRect {
        vello_cpu::kurbo::RoundedRect::from_rect(
            vello_cpu::kurbo::Rect::new(
                self.rect.x as f64,
                self.rect.y as f64,
                (self.rect.x + self.rect.width) as f64,
                (self.rect.y + self.rect.height) as f64,
            ),
            self.radius as f64,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color(pub AlphaColor<Srgb>);

impl Color {
    pub const WHITE: Self = Self(AlphaColor::WHITE);
    pub const BLACK: Self = Self(AlphaColor::BLACK);
    pub const RED: Self = Self(AlphaColor::from_rgb8(255, 0, 0));
    pub const GREEN: Self = Self(AlphaColor::from_rgb8(0, 128, 0));
    pub const BLUE: Self = Self(AlphaColor::from_rgb8(0, 0, 255));
    pub const TRANSPARENT: Self = Self(AlphaColor::from_rgba8(0, 0, 0, 0));

    pub fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self(AlphaColor::from_rgb8(
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
        ))
    }

    pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self(AlphaColor::from_rgba8(
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            (a * 255.0) as u8,
        ))
    }

    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim_start_matches('#');
        match hex.len() {
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Self(AlphaColor::from_rgb8(r, g, b)))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some(Self(AlphaColor::from_rgba8(r, g, b, a)))
            }
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                Some(Self(AlphaColor::from_rgb8(r, g, b)))
            }
            _ => None,
        }
    }

    pub fn to_hex(&self) -> String {
        let rgba = self.0.to_rgba8();
        format!("#{:02X}{:02X}{:02X}", rgba.r, rgba.g, rgba.b)
    }

    pub fn with_alpha(&self, alpha: f32) -> Self {
        let rgba = self.0.to_rgba8();
        Self(AlphaColor::from_rgba8(
            rgba.r,
            rgba.g,
            rgba.b,
            (alpha * 255.0) as u8,
        ))
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rect_contains() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(rect.contains(Point::new(50.0, 50.0)));
        assert!(rect.contains(Point::new(0.0, 0.0)));
        assert!(rect.contains(Point::new(100.0, 100.0)));
        assert!(!rect.contains(Point::new(101.0, 50.0)));
        assert!(!rect.contains(Point::new(50.0, 101.0)));
    }

    #[test]
    fn test_color_hex() {
        // 使用可以精确表示的颜色值
        let color = Color::rgb(1.0, 0.0, 0.0);
        assert_eq!(color.to_hex(), "#FF0000");

        let parsed = Color::from_hex("#FF8000").unwrap();
        let rgba = parsed.0.to_rgba8();
        assert!((rgba.r as f32 / 255.0 - 1.0).abs() < 0.01);
        assert!((rgba.g as f32 / 255.0 - 0.5).abs() < 0.01);
        assert!((rgba.b as f32 / 255.0 - 0.0).abs() < 0.01);
    }
}
