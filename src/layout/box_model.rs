//! 布局约束与盒模型

use crate::geometry::Rect;

/// 布局约束
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutConstraint {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl LayoutConstraint {
    pub const fn new(min_w: f32, max_w: f32, min_h: f32, max_h: f32) -> Self {
        Self {
            min_width: min_w,
            max_width: max_w,
            min_height: min_h,
            max_height: max_h,
        }
    }

    pub const fn tight(width: f32, height: f32) -> Self {
        Self {
            min_width: width,
            max_width: width,
            min_height: height,
            max_height: height,
        }
    }

    pub const fn loose(size: (f32, f32)) -> Self {
        Self {
            min_width: 0.0,
            max_width: size.0,
            min_height: 0.0,
            max_height: size.1,
        }
    }

    pub const fn tight_width(height: f32) -> Self {
        Self {
            min_width: f32::MAX,
            max_width: f32::MAX,
            min_height: height,
            max_height: height,
        }
    }
}

impl Default for LayoutConstraint {
    fn default() -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::MAX,
            min_height: 0.0,
            max_height: f32::MAX,
        }
    }
}

/// 边距
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeInsets {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl EdgeInsets {
    pub const fn new(left: f32, right: f32, top: f32, bottom: f32) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    pub const fn all(v: f32) -> Self {
        Self {
            left: v,
            right: v,
            top: v,
            bottom: v,
        }
    }

    pub const fn zero() -> Self {
        Self {
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
        }
    }

    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

impl Default for EdgeInsets {
    fn default() -> Self {
        Self::zero()
    }
}

/// 固有尺寸
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntrinsicSize {
    pub width: f32,
    pub height: f32,
}

impl IntrinsicSize {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    pub const fn zero() -> Self {
        Self {
            width: 0.0,
            height: 0.0,
        }
    }
}

/// 计算后的布局结果
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComputedLayout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// 是否为滚动容器（overflow_scroll）。渲染层据此自动裁剪，
    /// 布局层据此对子节点施加滚动偏移。
    pub overflow_scroll: bool,
}

impl ComputedLayout {
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.width, self.height)
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

impl Default for ComputedLayout {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            overflow_scroll: false,
        }
    }
}
