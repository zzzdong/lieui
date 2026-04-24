// src/layout/measurable.rs
//! 可测量尺寸 trait 和实现

use crate::geometry::Size;
use crate::text::{TextEngine, TextStyle};

/// 可测量尺寸 trait
///
/// 用于需要动态计算尺寸的 widget（如 Text）
pub trait Measurable: Send + Sync {
    /// 测量尺寸
    ///
    /// # 参数
    /// - `max_width`: 最大可用宽度，None 表示无限制
    fn measure(&self, max_width: Option<f32>) -> Size;

    /// 克隆自身
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
///
/// 专门用于 Text widget 的尺寸测量
#[derive(Clone)]
pub struct TextMeasure {
    /// 文本内容
    pub content: String,
    /// 文本样式
    pub style: TextStyle,
}

impl TextMeasure {
    /// 创建新的文本测量器
    pub fn new(content: impl Into<String>, style: TextStyle) -> Self {
        Self {
            content: content.into(),
            style,
        }
    }
}

impl Measurable for TextMeasure {
    fn measure(&self, max_width: Option<f32>) -> Size {
        TextEngine::with(|engine| {
            let layout = engine.layout(&self.content, &self.style, 1.0, max_width);
            Size::new(layout.width(), layout.height())
        })
    }

    fn clone_box(&self) -> Box<dyn Measurable> {
        Box::new(self.clone())
    }
}

/// 固定尺寸测量器
///
/// 用于尺寸固定的 widget
#[derive(Clone, Copy, Debug)]
pub struct FixedMeasure {
    pub size: Size,
}

impl FixedMeasure {
    pub fn new(size: Size) -> Self {
        Self { size }
    }
}

impl Measurable for FixedMeasure {
    fn measure(&self, _max_width: Option<f32>) -> Size {
        self.size
    }

    fn clone_box(&self) -> Box<dyn Measurable> {
        Box::new(*self)
    }
}
