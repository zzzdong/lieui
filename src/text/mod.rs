// src/text/engine.rs

use parley::style::{FontFamily, FontStyle, FontWeight, LineHeight};
use parley::{FontContext, LayoutContext, RangedBuilder};
use std::cell::RefCell;
use vello_cpu::color::{AlphaColor, Srgb};

thread_local! {
    pub static TEXT_ENGINE: RefCell<TextEngine> = RefCell::new(TextEngine::new());
}

pub struct TextEngine {
    font_cx: FontContext,
    layout_cx: LayoutContext<TextColor>,
}

impl TextEngine {
    pub fn new() -> Self {
        Self {
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
        }
    }

    pub fn with<R, F: FnOnce(&mut TextEngine) -> R>(f: F) -> R {
        TEXT_ENGINE.with(|engine| f(&mut engine.borrow_mut()))
    }

    pub fn layout(
        &mut self,
        text: &str,
        style: &TextStyle,
        scale: f32,
        max_width: Option<f32>,
    ) -> TextLayout {
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, scale, true);

        style.apply(&mut builder);

        let mut layout = builder.build(text);

        layout.break_all_lines(max_width);

        layout.align(
            parley::Alignment::Start,
            parley::AlignmentOptions::default(),
        );

        layout
    }
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ========== TextColor ==========

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextColor(pub AlphaColor<Srgb>);

impl TextColor {
    pub const BLACK: Self = Self(AlphaColor::BLACK);
    pub const WHITE: Self = Self(AlphaColor::WHITE);
    pub const RED: Self = Self(AlphaColor::from_rgb8(255, 0, 0));
    pub const GREEN: Self = Self(AlphaColor::from_rgb8(0, 128, 0));
    pub const BLUE: Self = Self(AlphaColor::from_rgb8(0, 0, 255));

    pub fn from_hex(hex: &str) -> Option<Self> {
        parse_color(hex).map(TextColor)
    }

    pub fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        Self(AlphaColor::from_rgb8(r, g, b))
    }

    pub fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(AlphaColor::from_rgba8(r, g, b, a))
    }

    pub fn inner(&self) -> AlphaColor<Srgb> {
        self.0
    }
}

impl Default for TextColor {
    fn default() -> Self {
        Self(AlphaColor::BLACK)
    }
}

// ========== 颜色解析 ==========

fn parse_color(hex: &str) -> Option<AlphaColor<Srgb>> {
    let hex = hex.trim_start_matches('#');

    // 命名颜色
    match hex.to_lowercase().as_str() {
        "black" => return Some(AlphaColor::from_rgb8(0, 0, 0)),
        "white" => return Some(AlphaColor::from_rgb8(255, 255, 255)),
        "red" => return Some(AlphaColor::from_rgb8(255, 0, 0)),
        "green" => return Some(AlphaColor::from_rgb8(0, 128, 0)),
        "blue" => return Some(AlphaColor::from_rgb8(0, 0, 255)),
        "yellow" => return Some(AlphaColor::from_rgb8(255, 255, 0)),
        "cyan" => return Some(AlphaColor::from_rgb8(0, 255, 255)),
        "magenta" => return Some(AlphaColor::from_rgb8(255, 0, 255)),
        "gray" | "grey" => return Some(AlphaColor::from_rgb8(128, 128, 128)),
        "transparent" => return Some(AlphaColor::from_rgba8(0, 0, 0, 0)),
        _ => {}
    }

    // 十六进制颜色
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(AlphaColor::from_rgb8(r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(AlphaColor::from_rgba8(r, g, b, a))
        }
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
            Some(AlphaColor::from_rgb8(r, g, b))
        }
        _ => None,
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
