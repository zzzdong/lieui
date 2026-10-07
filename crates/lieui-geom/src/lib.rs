//! lieui-geom —— 基础几何与颜色类型（零依赖）
//!
//! 从 `feature/mvp` 的 `src/geometry/types.rs` 抽出。与 `lievisual` 的几何层的取舍差异：
//! **坐标用 `f32` 而不是 kurbo 的 `f64` 直通**——UI 布局/命中/上屏全链路都是逻辑像素
//! （winit 逻辑坐标、软光栅化都是 f32 量级），f32 避免全工程的类型转换噪音；
//! kurbo 直通的价值在矢量场景（lierender/lievisual），GUI 布局用不上。
//!
//! 参考 lievisual 0.2（2026-10）补齐的实用面：
//! - `Color`：`Eq`/`Hash`（可作映射键）、CSS hex 输出、带透明度派生、线性插值（动画用）；
//! - `Rect`：`from_points`（两点构造）/ `translate`（平移）/ `inflate_xy`（非对称外扩）；
//! - `Point`：`distance` / `lerp`。

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
    /// 欧氏距离。
    pub fn distance(self, other: Self) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
    /// 线性插值（`t=0` ⇒ self，`t=1` ⇒ other；不钳制）。
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}
impl Size {
    pub const fn new(w: f32, h: f32) -> Self {
        Self { width: w, height: h }
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
    /// 由两个（可对角的）点构造：自动归一化 min/max。
    pub fn from_points(a: Point, b: Point) -> Self {
        let x0 = a.x.min(b.x);
        let y0 = a.y.min(b.y);
        Rect::new(x0, y0, a.x.max(b.x) - x0, a.y.max(b.y) - y0)
    }
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.x + self.width && p.y >= self.y && p.y <= self.y + self.height
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// 是否有交叠（面积为 0 的接触不算）
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }

    /// 交集；不相交或面积为 0 时返回 `None`
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        if x1 > x0 && y1 > y0 {
            Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
        } else {
            None
        }
    }

    /// 并集（含两者之间的空隙）
    pub fn union(&self, o: &Rect) -> Rect {
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = self.right().max(o.right());
        let y1 = self.bottom().max(o.bottom());
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// 四周外扩 `v`（可为负）
    pub fn inflate(&self, v: f32) -> Rect {
        self.inflate_xy(v, v)
    }

    /// 水平外扩 `x`、垂直外扩 `y`（可为负）
    pub fn inflate_xy(&self, x: f32, y: f32) -> Rect {
        Rect::new(self.x - x, self.y - y, self.width + x * 2.0, self.height + y * 2.0)
    }

    /// 平移（不改变尺寸）。
    pub fn translate(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }
}

impl Default for Rect {
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }
}

/// RGBA 颜色 (u8 值)
///
/// 8-bit 直存：对 SVG hex / vello `from_rgba8` / softbuffer 0x00RRGGBB 等后端无损精确，
/// 不引入 f64 存储的 `clamp×255+round` 舍入漂移（与 lievisual 的 Color 同一设计）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
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
    /// CSS hex 字符串：不透明输出 `#rrggbb`，带 alpha 输出 `#rrggbbaa`。
    pub fn to_hex(&self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }
    /// 同色、指定 alpha 的派生色（`a` 为 0–255）。
    pub const fn with_alpha(&self, a: u8) -> Color {
        Color {
            r: self.r,
            g: self.g,
            b: self.b,
            a,
        }
    }
    /// 线性插值（`t=0` ⇒ self，`t=1` ⇒ other；通道各自插值后四舍五入，不钳制 t）。
    pub fn lerp(&self, other: &Color, t: f32) -> Color {
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
        Color::rgba(
            mix(self.r, other.r),
            mix(self.g, other.g),
            mix(self.b, other.b),
            mix(self.a, other.a),
        )
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
    pub const GREEN: Color = Color {
        r: 0,
        g: 128,
        b: 0,
        a: 255,
    };
    pub const BLUE: Color = Color {
        r: 0,
        g: 0,
        b: 255,
        a: 255,
    };
    pub const GRAY: Color = Color {
        r: 128,
        g: 128,
        b: 128,
        a: 255,
    };
    pub const RED: Color = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const TRANSPARENT: Color = Color { r: 0, g: 0, b: 0, a: 0 };
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
    fn color_hex_roundtrip() {
        assert_eq!(Color::rgb(0, 102, 204).to_hex(), "#0066cc");
        assert_eq!(Color::rgba(0, 102, 204, 128).to_hex(), "#0066cc80");
        assert_eq!(Color::from_hex("#0066CC"), Color::rgb(0, 102, 204));
        assert_eq!(Color::from_hex("#0066cc80"), Color::rgba(0, 102, 204, 128));
    }

    #[test]
    fn color_derivatives() {
        let c = Color::rgb(10, 20, 30);
        assert_eq!(c.with_alpha(0), Color::rgba(10, 20, 30, 0));
        assert_eq!(c.with_alpha(255), c);
        // t=0 / t=1 是端点
        assert_eq!(c.lerp(&Color::WHITE, 0.0), c);
        assert_eq!(c.lerp(&Color::WHITE, 1.0), Color::WHITE);
        // 中点混色：10 与 255 的中点 132.5 → round(远离零) = 133
        let m = c.lerp(&Color::WHITE, 0.5);
        assert_eq!(m.r, 133);
        assert_eq!(m.a, 255);
    }

    #[test]
    fn color_is_hashable_and_eq() {
        use std::collections::HashMap;
        let mut m = HashMap::new();
        m.insert(Color::rgb(1, 2, 3), "a");
        assert_eq!(m.get(&Color::rgb(1, 2, 3)), Some(&"a"));
        assert_eq!(m.get(&Color::rgba(1, 2, 3, 255)), Some(&"a"), "Eq 按全通道");
        assert_eq!(m.get(&Color::rgba(1, 2, 3, 254)), None);
    }

    #[test]
    fn rect_from_points_normalizes_corners() {
        let r = Rect::from_points(Point::new(5.0, 7.0), Point::new(1.0, 3.0));
        assert_eq!(r, Rect::new(1.0, 3.0, 4.0, 4.0));
        // 同一点 ⇒ 零尺寸
        assert!(Rect::from_points(Point::new(2.0, 2.0), Point::new(2.0, 2.0)).is_empty());
    }

    #[test]
    fn rect_translate_and_inflate_xy() {
        let r = Rect::new(10.0, 10.0, 40.0, 20.0);
        assert_eq!(r.translate(5.0, -5.0), Rect::new(15.0, 5.0, 40.0, 20.0));
        assert_eq!(r.inflate_xy(2.0, 3.0), Rect::new(8.0, 7.0, 44.0, 26.0));
        assert_eq!(r.inflate(2.0), r.inflate_xy(2.0, 2.0), "inflate = inflate_xy 同值");
        // 负外扩
        assert_eq!(r.inflate_xy(-1.0, 0.0), Rect::new(11.0, 10.0, 38.0, 20.0));
    }

    #[test]
    fn point_distance_and_lerp() {
        let a = Point::new(1.0, 2.0);
        let b = Point::new(4.0, 6.0);
        assert!((a.distance(b) - 5.0).abs() < 1e-5, "3-4-5 三角形");
        assert_eq!(a.lerp(b, 0.5), Point::new(2.5, 4.0));
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
    }
}
