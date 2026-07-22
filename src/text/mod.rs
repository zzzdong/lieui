//! 文本引擎 — 基于 parley 0.11.0 排版

use std::cell::RefCell;

use parley::{style::StyleProperty, Alignment, AlignmentOptions, FontContext, LayoutContext};

use crate::geometry::Color;

/// 文本布局类型
pub type TextLayout = parley::Layout<Color>;

thread_local! {
    pub static FONT_CONTEXT: RefCell<FontContext> = RefCell::new(FontContext::default());
    pub static LAYOUT_CONTEXT: RefCell<LayoutContext<Color>> = RefCell::new(LayoutContext::new());
}

/// 同时访问字体和布局上下文
pub fn with_text_contexts<R, F: FnOnce(&mut FontContext, &mut LayoutContext<Color>) -> R>(
    f: F,
) -> R {
    FONT_CONTEXT.with(|fc| LAYOUT_CONTEXT.with(|lc| f(&mut fc.borrow_mut(), &mut lc.borrow_mut())))
}

/// 创建文本布局
pub fn create_text_layout(
    text: &str,
    font_size: f64,
    color: Color,
    max_width: Option<f64>,
) -> TextLayout {
    with_text_contexts(|fc, lc| {
        let mut builder = lc.ranged_builder(fc, text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(font_size as f32));
        builder.push_default(StyleProperty::Brush(color));
        let mut layout = builder.build(text);
        layout.break_all_lines(max_width.map(|w| w as f32));
        layout.align(Alignment::Start, AlignmentOptions::default());
        layout
    })
}

/// 文本引擎
pub struct TextEngine;
impl TextEngine {
    pub fn measure_text(text: &str, font_size: f64, max_width: Option<f64>) -> (f64, f64) {
        with_text_contexts(|fc, lc| {
            let mut builder = lc.ranged_builder(fc, text, 1.0, true);
            builder.push_default(StyleProperty::FontSize(font_size as f32));
            builder.push_default(StyleProperty::Brush(Color::BLACK));
            let mut layout = builder.build(text);
            layout.break_all_lines(max_width.map(|w| w as f32));
            layout.align(Alignment::Start, AlignmentOptions::default());
            (layout.width() as f64, layout.height() as f64)
        })
    }
}
