use std::{borrow::Cow, cell::RefCell, thread_local};

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontStack, FontWeight, GenericFamily,
    Glyph, Layout, LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty, TextStyle,
};
use vello_cpu::{RenderContext, peniko::Brush, peniko::Color};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextColor(Color);

impl Default for TextColor {
    fn default() -> Self {
        Self(Color::BLACK)
    }
}

impl From<Color> for TextColor {
    fn from(color: Color) -> Self {
        Self(color)
    }
}


pub struct TextEngine {
    font_cx: FontContext,
    layout_cx: LayoutContext<TextColor>,
}

thread_local! {
    static TEXT_ENGINE: RefCell<TextEngine> = RefCell::new(TextEngine::new());
}

impl TextEngine {
    fn new() -> Self {
        Self {
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
        }
    }

    pub fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&mut TextEngine) -> R,
    {
        TEXT_ENGINE.with(|engine| f(&mut engine.borrow_mut()))
    }

    pub fn layout_text(
        text: &str,
        style: &TextStyle<TextColor>,
        max_width: Option<f32>,
        max_height: Option<f32>,
    ) -> Layout<TextColor> {
        Self::with(|engine| engine.layout_glyph_run(text, style, max_width, max_height))
    }

    pub fn paint_text(cx: &mut RenderContext, x: f32, y: f32, layout: &Layout<TextColor>) {
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(run) = item {
                    let brush = run.style().brush;

                    cx.set_paint(brush.0);

                    let glyphs = run.positioned_glyphs().map(|g| {
                        vello_cpu::Glyph {
                        id: g.id,
                        x: x + g.x,
                        y: y + g.y,
                    }});

                    cx.glyph_run(run.run().font())
                        .font_size(run.run().font_size())
                        .fill_glyphs(glyphs);
                }
            }
        }
    }

    // ---------- 内部：全属性推入布局器 ----------
    fn layout_glyph_run<'a>(
        &'a mut self,
        text: &str,
        style: &TextStyle<TextColor>,
        max_width: Option<f32>,
        max_height: Option<f32>,
    ) -> Layout<TextColor> {
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, true);

        // 1. 字体族
        builder.push_default(StyleProperty::FontStack(style.font_stack.clone()));

        // 2. 字号
        builder.push_default(StyleProperty::FontSize(style.font_size));

        // 3. 字重
        builder.push_default(StyleProperty::FontWeight(style.font_weight));

        // 4. 行高

        builder.push_default(StyleProperty::LineHeight(style.line_height));

        // 5. 字间距
        builder.push_default(StyleProperty::LetterSpacing(style.letter_spacing));

        // 6. 颜色（Brush 内放 Color）
        builder.push_default(StyleProperty::<TextColor>::Brush(style.brush));

        let mut layout = builder.build(text);

        // 断行 & 对齐
        match (max_width, max_height) {
            (Some(w), Some(h)) => {
                let mut breaker = layout.break_lines();
                let mut used_h = 0.0;
                while let Some((_, line_h)) = breaker.break_next(w) {
                    if used_h + line_h > h {
                        breaker.revert();
                        break;
                    }
                    used_h += line_h;
                }
                breaker.finish();
            }
            (Some(w), None) => layout.break_all_lines(Some(w)),
            _ => layout.break_all_lines(None),
        }

        layout.align(max_width, Alignment::Start, AlignmentOptions::default());

        layout
    }
}
