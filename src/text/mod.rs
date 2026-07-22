//! 文本引擎 — 简化版
use crate::geometry::Color;

pub struct ColorBrush(pub Color);

pub struct TextStyle { pub font_size: f64, pub color: Color }
impl TextStyle {
    pub fn new() -> Self { Self { font_size: 16.0, color: Color::BLACK } }
}

pub struct TextEngine;
impl TextEngine {
    pub fn measure_width(text: &str, font_size: f64, _max_width: Option<f64>) -> f64 { text.len() as f64 * font_size * 0.3 }
    pub fn measure_height(_text: &str, font_size: f64, _max_width: Option<f64>) -> f64 { font_size * 1.2 }
}
