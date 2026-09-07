//! 布局约束与盒模型

use crate::types::VALUE_UNDEFINED;

/// 轴对齐矩形（布局层自用，渲染层各自有等价类型）
#[derive(Debug, Clone, Copy, PartialEq, Default)]
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
    pub fn right(&self) -> f32 {
        self.x + self.width
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.right() && py >= self.y && py <= self.bottom()
    }
}

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

/// 计算后的布局结果（全局坐标）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComputedLayout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// 内容尺寸（滚动容器由子节点包围盒推导，非滚动容器等于自身尺寸）
    pub content_width: f32,
    pub content_height: f32,
    /// 钳制后的滚动偏移
    pub scroll_x: f32,
    pub scroll_y: f32,
    /// 在父内容盒内的局部偏移（= 全局位置 - 父内容盒原点）。
    /// 重排边界子树重算时用它还原原点，从而不必重算祖先。
    pub local_x: f32,
    pub local_y: f32,
    /// 父级传入的可用宽高回显（未扣 margin）。重排边界子树重算时原样回传，
    /// 保证父约束与上一帧完全一致（百分比尺寸也正确）。
    pub avail_w: f32,
    pub avail_h: f32,
    /// 是否为滚动容器。渲染层据此裁剪，布局层据此对子节点施加滚动偏移。
    pub overflow_scroll: bool,
}

impl ComputedLayout {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            content_width: width,
            content_height: height,
            scroll_x: 0.0,
            scroll_y: 0.0,
            local_x: 0.0,
            local_y: 0.0,
            avail_w: VALUE_UNDEFINED,
            avail_h: VALUE_UNDEFINED,
            overflow_scroll: false,
        }
    }
    /// 父内容盒原点（重排边界重算时用）
    pub const fn parent_origin(&self) -> (f32, f32) {
        (self.x - self.local_x, self.y - self.local_y)
    }

    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.width, self.height)
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

impl Default for ComputedLayout {
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }
}
