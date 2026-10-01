//! lieui-geom —— 基础几何与颜色类型（零依赖）
//!
//! 从 `feature/mvp` 的 `src/geometry/types.rs` 抽出。唯一改动：
//! `Color::as_vello()` 移到渲染层（`lieui::render`，M3），避免 geom 依赖 `vello_cpu`。

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
        Rect::new(
            self.x - v,
            self.y - v,
            self.width + v * 2.0,
            self.height + v * 2.0,
        )
    }
}

impl Default for Rect {
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
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
