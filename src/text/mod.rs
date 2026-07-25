//! 文本引擎 — 基于 parley 0.11.0 排版

use std::cell::RefCell;

use parley::{
    style::{FontFamily, FontWeight as ParleyFontWeight, StyleProperty},
    Alignment, AlignmentOptions, FontContext, LayoutContext,
};

use crate::geometry::Color;
use crate::view::paint::{FontWeight, TextAlign, TextStyle};

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

fn apply_text_style(builder: &mut parley::RangedBuilder<Color>, style: &TextStyle) {
    builder.push_default(StyleProperty::FontSize(style.font_size as f32));
    builder.push_default(StyleProperty::Brush(style.color));
    builder.push_default(StyleProperty::FontFamily(FontFamily::named(
        &style.font_family,
    )));

    let pw = match &style.font_weight {
        FontWeight::Normal => ParleyFontWeight::NORMAL,
        FontWeight::Medium => ParleyFontWeight::MEDIUM,
        FontWeight::Bold => ParleyFontWeight::BOLD,
        FontWeight::Weight(w) => ParleyFontWeight::new(*w as f32),
    };
    builder.push_default(StyleProperty::FontWeight(pw));

    // line_height 暂由上层通过额外行间距实现，parley 0.11.0 无直接 StyleProperty。
}

fn map_text_align(a: TextAlign) -> Alignment {
    match a {
        TextAlign::Start => Alignment::Start,
        TextAlign::Center => Alignment::Center,
        TextAlign::End => Alignment::End,
        TextAlign::Justify => Alignment::Justify,
    }
}

/// 创建文本布局
pub fn create_text_layout(text: &str, style: &TextStyle) -> TextLayout {
    with_text_contexts(|fc, lc| {
        let mut builder = lc.ranged_builder(fc, text, 1.0, true);
        apply_text_style(&mut builder, style);
        let mut layout = builder.build(text);
        layout.break_all_lines(style.max_width.map(|w| w as f32));
        layout.align(
            map_text_align(style.text_align),
            AlignmentOptions::default(),
        );
        layout
    })
}

/// 文本引擎
pub struct TextEngine;
impl TextEngine {
    pub fn measure_text(text: &str, style: &TextStyle) -> (f64, f64) {
        let layout = create_text_layout(text, style);
        (layout.width() as f64, layout.height() as f64)
    }
}
