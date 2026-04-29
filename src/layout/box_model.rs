// src/layout/box_model.rs
//! CSS Box 模型实现

use crate::geometry::{Rect, Size, types::RoundedRect};

/// 边距结构（用于 margin / padding / border）
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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

    pub fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    pub fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub fn horizontal(value: f32) -> Self {
        Self {
            left: value,
            right: value,
            top: 0.0,
            bottom: 0.0,
        }
    }

    pub fn vertical(value: f32) -> Self {
        Self {
            top: value,
            bottom: value,
            left: 0.0,
            right: 0.0,
        }
    }

    /// 对称边距（水平/垂直）
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            top: vertical,
            bottom: vertical,
        }
    }

    /// 水平方向总和
    pub fn horizontal_sum(&self) -> f32 {
        self.left + self.right
    }

    /// 垂直方向总和
    pub fn vertical_sum(&self) -> f32 {
        self.top + self.bottom
    }

    /// 从 Size 中减去边距
    pub fn deflate(&self, size: Size) -> Size {
        Size::new(
            (size.width - self.horizontal_sum()).max(0.0),
            (size.height - self.vertical_sum()).max(0.0),
        )
    }
}

/// 计算后的布局结果
#[derive(Debug, Clone, Copy)]
pub struct ComputedLayout {
    pub margin_box: Rect,
    pub border_box: Rect,
    pub padding_box: Rect,
    pub content_box: Rect,
    pub hit_shape: Option<RoundedRect>,
}

impl ComputedLayout {
    /// 获取内容边界（便捷方法）
    pub fn bounds(&self) -> Rect {
        self.content_box
    }
}

/// Widget 的盒子模型约束
#[derive(Debug, Clone)]
pub struct BoxStyle {
    pub margin: EdgeInsets,
    pub padding: EdgeInsets,
    pub border: EdgeInsets,
    pub min_size: Size,
    pub max_size: Size,
}

impl Default for BoxStyle {
    fn default() -> Self {
        Self {
            margin: EdgeInsets::ZERO,
            padding: EdgeInsets::ZERO,
            border: EdgeInsets::ZERO,
            min_size: Size::ZERO,
            max_size: Size::new(f32::INFINITY, f32::INFINITY),
        }
    }
}

impl BoxStyle {
    /// 计算内容区域的可用空间
    pub fn content_available(&self, available: Size) -> Size {
        let with_margin = self.margin.deflate(available);
        let with_border = self.border.deflate(with_margin);
        self.padding.deflate(with_border)
    }
}
