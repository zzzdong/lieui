// src/widgets/text.rs

//! Text Widget
//!
//! 简单的文本 widget，支持单一样式的文本显示。
//! 使用 parley 的 StyleProperty 进行样式设置。

use crate::core::WidgetId;
use crate::geometry::{Rect, Size};
use crate::layout::{IntrinsicSize, LayoutNode, TextMeasure};
use crate::prelude::ViewContext;
use crate::render::RenderNode;
use crate::text::{TextEngine, TextLayout, TextStyle};
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
}

impl Text {
    /// 创建新的 Text
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: TextStyle::default(),
            dirty: false,
        }
    }

    /// 设置文本内容
    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self.dirty = true;
        self
    }

    /// 设置文本内容（可变）
    pub fn set_content(&mut self, content: impl Into<String>) {
        self.content = content.into();
        self.dirty = true;
    }

    pub fn font_size(mut self, size: f32) -> Self {
        self.style.0.font_size = size;
        self
    }

    pub fn text_color(mut self, color: crate::text::TextColor) -> Self {
        self.style.0.brush = color;
        self
    }

    /// 获取文本内容
    pub fn text_content(&self) -> &str {
        &self.content
    }

    /// 执行文本布局
    pub fn do_layout(&self, max_width: Option<f32>) -> TextLayout {
        TextEngine::with(|engine| engine.layout(&self.content, &self.style, 1.0, max_width))
    }

    /// 测量文本尺寸
    pub fn measure(&self, max_width: Option<f32>) -> Size {
        let layout = self.do_layout(max_width);
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

        LayoutNode::new(id).with_intrinsic_size(IntrinsicSize::Measurable(Box::new(measure)))
    }

    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            let text_layout = self.do_layout(Some(computed.content_box.width));
            RenderNode::text(computed.content_box, text_layout)
        } else {
            // 如果还没有计算，使用零尺寸
            RenderNode::text(Rect::zero(), self.do_layout(None))
        }
    }

    fn bounds(&self) -> Option<Rect> {
        None
    }
}

// 注意：parley 的类型在 widgets/mod.rs 中重新导出
