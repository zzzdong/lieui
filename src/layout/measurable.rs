//! Measurable trait — 将尺寸计算逻辑从 ViewNode 解耦
//!
//! 设计要点：
//! - Measurable 只关心“给定约束能占多少空间”，不关心 ViewNode 变体。
//! - ViewNode 内部把具体字段转交给对应的测量器实现。
//! - 新增 widget 不需要修改这里，只需新增一个 Measurable 实现并在 ViewNode::measure 中调用。

use crate::layout::box_model::{IntrinsicSize, LayoutConstraint};
use crate::text::TextEngine;

/// 可测量尺寸 trait
pub trait Measurable: Send + Sync {
    fn measure(&self, constraint: &LayoutConstraint) -> IntrinsicSize;
    fn clone_box(&self) -> Box<dyn Measurable>;
}

impl Clone for Box<dyn Measurable> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

impl std::fmt::Debug for dyn Measurable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Measurable").finish()
    }
}

/// 文本测量器
#[derive(Clone, Debug)]
pub struct TextMeasure {
    pub content: String,
    pub font_size: f64,
    /// 水平方向额外增量（如 padding、图标宽度）
    pub extra_width: f32,
    /// 垂直方向固定高度
    pub fixed_height: Option<f32>,
}

impl TextMeasure {
    pub fn new(content: impl Into<String>, font_size: f64) -> Self {
        Self {
            content: content.into(),
            font_size,
            extra_width: 0.0,
            fixed_height: None,
        }
    }

    pub fn with_extra_width(mut self, w: f32) -> Self {
        self.extra_width = w;
        self
    }

    pub fn with_fixed_height(mut self, h: f32) -> Self {
        self.fixed_height = Some(h);
        self
    }
}

impl Measurable for TextMeasure {
    fn measure(&self, constraint: &LayoutConstraint) -> IntrinsicSize {
        let max_w = if constraint.max_width < f32::MAX {
            Some((constraint.max_width - self.extra_width.max(0.0)) as f64)
        } else {
            None
        };
        let (mw, mh) = TextEngine::measure_text(&self.content, self.font_size, max_w);
        let w = (mw as f32 + self.extra_width).clamp(constraint.min_width, constraint.max_width);
        let h = match self.fixed_height {
            Some(h) => h.clamp(constraint.min_height, constraint.max_height),
            None => mh as f32,
        };
        IntrinsicSize::new(w, h)
    }

    fn clone_box(&self) -> Box<dyn Measurable> {
        Box::new(self.clone())
    }
}

/// 固定尺寸测量器
#[derive(Clone, Copy, Debug)]
pub struct FixedMeasure {
    pub size: IntrinsicSize,
}

impl FixedMeasure {
    pub fn new(size: IntrinsicSize) -> Self {
        Self { size }
    }
}

impl Measurable for FixedMeasure {
    fn measure(&self, constraint: &LayoutConstraint) -> IntrinsicSize {
        IntrinsicSize::new(
            self.size
                .width
                .clamp(constraint.min_width, constraint.max_width),
            self.size
                .height
                .clamp(constraint.min_height, constraint.max_height),
        )
    }

    fn clone_box(&self) -> Box<dyn Measurable> {
        Box::new(*self)
    }
}

/// 空测量器（Divider 等占位元素）
#[derive(Clone, Copy, Debug)]
pub struct EmptyMeasure;

impl Measurable for EmptyMeasure {
    fn measure(&self, _constraint: &LayoutConstraint) -> IntrinsicSize {
        IntrinsicSize::zero()
    }

    fn clone_box(&self) -> Box<dyn Measurable> {
        Box::new(*self)
    }
}
