// src/widgets/text.rs

//! Text Widget
//!
//! 简单的文本 widget，支持单一样式的文本显示。
//! 使用 parley 的 StyleProperty 进行样式设置。
//!
//! 改进：
//! - 添加 TextLayout 缓存，避免每帧重复布局
//! - 只在内容/样式/宽度变化时重新布局

use crate::core::WidgetId;
use crate::geometry::{Rect, Size};
use crate::layout::{IntrinsicSize, LayoutNode, TextMeasure};
use crate::prelude::ViewContext;
use crate::render::visual::{LayeredElement, VisualElement};
use crate::text::{TextLayout, TextStyle};
use crate::widget::Widget;

/// Text Widget
///
/// 简单的文本 widget，支持单一样式的文本显示。
/// 使用 parley 的 StyleProperty 进行样式设置。
#[derive(Clone)]
pub struct Text {
    content: String,
    style: TextStyle,
    dirty: bool,
    /// 缓存的文本布局
    cached_layout: Option<TextLayout>,
    /// 缓存时的最大宽度（用于检测是否需要重新布局）
    cached_max_width: Option<f32>,
}

impl Text {
    /// 创建新的 Text
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: TextStyle::default(),
            dirty: true, // 新创建时需要布局
            cached_layout: None,
            cached_max_width: None,
        }
    }

    /// 获取字体大小
    pub fn get_font_size(&self) -> f32 {
        self.style.0.font_size
    }

    /// 设置文本内容
    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self.dirty = true;
        self.cached_layout = None; // 内容变化，清除缓存
        self
    }

    /// 设置文本内容（可变）
    pub fn set_content(&mut self, content: impl Into<String>) {
        self.content = content.into();
        self.dirty = true;
        self.cached_layout = None; // 内容变化，清除缓存
    }

    pub fn font_size(mut self, size: f32) -> Self {
        self.style.0.font_size = size;
        self.dirty = true;
        self.cached_layout = None; // 样式变化，清除缓存
        self
    }

    /// 设置字体大小（可变）
    pub fn set_font_size(&mut self, size: f32) {
        self.style.0.font_size = size;
        self.dirty = true;
        self.cached_layout = None;
    }

    pub fn text_color(mut self, color: crate::text::TextColor) -> Self {
        self.style.0.brush = color;
        self.dirty = true;
        self.cached_layout = None; // 样式变化，清除缓存
        self
    }

    /// 设置文本颜色（可变）
    pub fn set_text_color(&mut self, color: crate::text::TextColor) {
        self.style.0.brush = color;
        self.dirty = true;
        self.cached_layout = None;
    }

    /// 直接替换整个文本样式（可变）
    ///
    /// 复制 `TextStyle` 中的所有属性到当前 widget。
    /// 每次调用都会使缓存失效，触发重新布局。
    pub fn set_style(&mut self, style: &TextStyle) {
        self.style.0.font_size = style.0.font_size;
        self.style.0.brush = style.0.brush.clone();
        self.dirty = true;
        self.cached_layout = None;
    }

    /// 获取文本内容
    pub fn text_content(&self) -> &str {
        &self.content
    }

    /// 获取文本样式
    pub fn style(&self) -> &TextStyle {
        &self.style
    }

    /// 获取或创建文本布局（带缓存）
    ///
    /// 只在以下情况重新布局：
    /// 1. dirty 标记为 true（内容或样式变化）
    /// 2. 缓存不存在
    /// 3. max_width 与缓存时的宽度不同
    pub fn get_or_create_layout(&mut self, max_width: Option<f32>) -> &TextLayout {
        // 检查是否需要重新布局
        let needs_layout =
            self.dirty || self.cached_layout.is_none() || self.cached_max_width != max_width;

        if needs_layout {
            // 使用 TextMeasure 进行布局
            let measure = TextMeasure::new(&self.content, self.style.clone());
            let layout = measure.create_layout(max_width);
            self.cached_layout = Some(layout);
            self.cached_max_width = max_width;
            self.dirty = false;
        }

        self.cached_layout.as_ref().unwrap()
    }

    /// 强制重新布局（清除缓存）
    pub fn invalidate_layout(&mut self) {
        self.dirty = true;
        self.cached_layout = None;
    }

    /// 测量文本尺寸（使用缓存）
    pub fn measure(&mut self, max_width: Option<f32>) -> Size {
        let layout = self.get_or_create_layout(max_width);
        Size::new(layout.width(), layout.height())
    }
}

impl Widget for Text {
    crate::impl_widget_any!(Text);

    fn type_name(&self) -> &'static str {
        "Text"
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        // 使用 TextMeasure 来测量文本尺寸
        let measure = TextMeasure::new(self.content.clone(), self.style.clone());

        LayoutNode::new(id)
            .with_intrinsic_size(IntrinsicSize::Measurable(Box::new(measure)))
            .with_dirty(self.dirty)
    }

    fn render(&mut self, layout: &LayoutNode, _ctx: &ViewContext) -> Vec<LayeredElement> {
        let mut elements = Vec::new();

        let content_box = layout.computed.content_box;

        // 先获取所有需要的数据（避免借用冲突）
        let content = self.content.clone();
        let color = self.style.0.brush;
        let font_size = self.get_font_size() as f64;

        // 使用缓存的布局（不再重复创建！）
        let text_layout = self.get_or_create_layout(Some(content_box.width)).clone();

        // 创建文本元素（使用缓存的布局）
        let text_elem = VisualElement::TextRun {
            text: content,
            position: kurbo::Point::new(content_box.x as f64, content_box.y as f64),
            color,
            font_size,
            font_family: "sans-serif".to_string(),
            rotation: 0.0,
            max_width: Some(content_box.width as f64),
            layout: Some(Box::new(text_layout)),
        };

        elements.push(LayeredElement::default_layer(text_elem));

        elements
    }

    fn bounds(&self) -> Option<Rect> {
        None
    }
}
