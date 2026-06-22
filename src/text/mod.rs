// src/text/engine.rs

use parley::style::{FontFamily, FontStyle, FontWeight, LineHeight};
use parley::{FontContext, LayoutContext, RangedBuilder};
use std::cell::RefCell;

use crate::geometry::Color;

// TextColor 现在是 geometry::Color 的类型别名
// parley 的 Brush trait 是 blanket implementation: Clone + PartialEq + Default + Debug
// geometry::Color 已经满足所有这些要求
pub type TextColor = Color;

thread_local! {
    /// 全局字体上下文 - 线程本地存储
    pub static FONT_CONTEXT: RefCell<FontContext> = RefCell::new(FontContext::default());
    /// 全局布局上下文 - 线程本地存储
    pub static LAYOUT_CONTEXT: RefCell<LayoutContext<TextColor>> = RefCell::new(LayoutContext::default());
}

/// 访问字体上下文的便捷函数
pub fn with_font_context<R, F: FnOnce(&mut FontContext) -> R>(f: F) -> R {
    FONT_CONTEXT.with(|cx| f(&mut cx.borrow_mut()))
}

/// 访问布局上下文的便捷函数
pub fn with_layout_context<R, F: FnOnce(&mut LayoutContext<TextColor>) -> R>(f: F) -> R {
    LAYOUT_CONTEXT.with(|cx| f(&mut cx.borrow_mut()))
}

/// 同时访问两个上下文的便捷函数
pub fn with_text_contexts<R, F: FnOnce(&mut FontContext, &mut LayoutContext<TextColor>) -> R>(
    f: F,
) -> R {
    FONT_CONTEXT.with(|font_cx| {
        LAYOUT_CONTEXT.with(|layout_cx| f(&mut font_cx.borrow_mut(), &mut layout_cx.borrow_mut()))
    })
}

pub struct TextEngine;

impl TextEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn layout(text: &str, style: &TextStyle, scale: f32, max_width: Option<f32>) -> TextLayout {
        with_text_contexts(|font_cx, layout_cx| {
            let mut builder = layout_cx.ranged_builder(font_cx, text, scale, true);

            style.apply(&mut builder);

            let mut layout = builder.build(text);

            layout.break_all_lines(max_width);

            layout.align(
                parley::Alignment::Start,
                parley::AlignmentOptions::default(),
            );

            layout
        })
    }
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ========== TextStyle ==========

#[derive(Clone)]
pub struct TextStyle(pub parley::TextStyle<'static, 'static, TextColor>);

impl TextStyle {
    pub fn new() -> Self {
        Self(parley::TextStyle::default())
    }

    pub fn font_size(mut self, size: f32) -> Self {
        self.0.font_size = size;
        self
    }

    pub fn font_family(mut self, family: FontFamily<'static>) -> Self {
        self.0.font_family = family;
        self
    }

    pub fn font_weight(mut self, weight: FontWeight) -> Self {
        self.0.font_weight = weight;
        self
    }

    pub fn font_style(mut self, style: FontStyle) -> Self {
        self.0.font_style = style;
        self
    }

    pub fn text_color(mut self, color: TextColor) -> Self {
        self.0.brush = color;
        self
    }

    pub fn line_height(mut self, height: f32) -> Self {
        self.0.line_height = LineHeight::MetricsRelative(height);
        self
    }

    pub fn letter_spacing(mut self, spacing: f32) -> Self {
        self.0.letter_spacing = spacing;
        self
    }

    pub fn word_spacing(mut self, spacing: f32) -> Self {
        self.0.word_spacing = spacing;
        self
    }

    pub fn underline(mut self, has: bool) -> Self {
        self.0.has_underline = has;
        self
    }

    pub fn strikethrough(mut self, has: bool) -> Self {
        self.0.has_strikethrough = has;
        self
    }

    fn apply(&self, builder: &mut RangedBuilder<'_, TextColor>) {
        use parley::StyleProperty;

        builder.push_default(StyleProperty::Brush(self.0.brush));
        builder.push_default(StyleProperty::FontFamily(self.0.font_family.clone()));
        builder.push_default(StyleProperty::FontSize(self.0.font_size));
        builder.push_default(StyleProperty::FontWeight(self.0.font_weight));
        builder.push_default(StyleProperty::FontStyle(self.0.font_style));
        builder.push_default(StyleProperty::LineHeight(self.0.line_height));
        builder.push_default(StyleProperty::LetterSpacing(self.0.letter_spacing));
        builder.push_default(StyleProperty::WordSpacing(self.0.word_spacing));

        // 下划线
        if self.0.has_underline {
            builder.push_default(StyleProperty::Underline(true));
            builder.push_default(StyleProperty::UnderlineBrush(self.0.underline_brush));
            builder.push_default(StyleProperty::UnderlineOffset(self.0.underline_offset));
            builder.push_default(StyleProperty::UnderlineSize(self.0.underline_size));
        }

        // 删除线
        if self.0.has_strikethrough {
            builder.push_default(StyleProperty::Strikethrough(true));
            builder.push_default(StyleProperty::StrikethroughOffset(
                self.0.strikethrough_offset,
            ));
            builder.push_default(StyleProperty::StrikethroughSize(self.0.strikethrough_size));
            builder.push_default(StyleProperty::StrikethroughBrush(
                self.0.strikethrough_brush,
            ));
        }

        builder.push_default(StyleProperty::FontVariations(
            self.0.font_variations.clone(),
        ));

        builder.push_default(StyleProperty::FontFeatures(self.0.font_features.clone()));

        // 换行控制
        builder.push_default(StyleProperty::WordBreak(self.0.word_break));
        builder.push_default(StyleProperty::OverflowWrap(self.0.overflow_wrap));
        builder.push_default(StyleProperty::TextWrapMode(self.0.text_wrap_mode));
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        Self::new()
    }
}

// ========== 类型别名 ==========

pub type TextLayout = parley::Layout<TextColor>;
