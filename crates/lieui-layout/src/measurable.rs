//! Measurable —— 叶子内容测量的可选实现集合
//!
//! 布局引擎只通过 `LayoutTree::measure` 取得叶子尺寸；本模块提供几个通用实现，
//! 供控件（或测试）直接组合使用。真实文本测量由 `lieui-text` 提供。

use crate::box_model::{IntrinsicSize, LayoutConstraint};

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

/// 固定尺寸测量器
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixedMeasure {
    pub size: IntrinsicSize,
}

impl FixedMeasure {
    pub fn new(size: IntrinsicSize) -> Self {
        Self { size }
    }
    pub fn of(width: f32, height: f32) -> Self {
        Self::new(IntrinsicSize::new(width, height))
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
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyMeasure;

impl Measurable for EmptyMeasure {
    fn measure(&self, _constraint: &LayoutConstraint) -> IntrinsicSize {
        IntrinsicSize::zero()
    }
    fn clone_box(&self) -> Box<dyn Measurable> {
        Box::new(*self)
    }
}

/// 带最大宽度的固定尺寸测量器：宽度受 `max_width` 约束时截断
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClampedMeasure {
    pub size: IntrinsicSize,
}

impl Measurable for ClampedMeasure {
    fn measure(&self, constraint: &LayoutConstraint) -> IntrinsicSize {
        IntrinsicSize::new(
            self.size.width.clamp(
                constraint.min_width,
                constraint.max_width.min(self.size.width),
            ),
            self.size
                .height
                .clamp(constraint.min_height, constraint.max_height),
        )
    }
    fn clone_box(&self) -> Box<dyn Measurable> {
        Box::new(*self)
    }
}
