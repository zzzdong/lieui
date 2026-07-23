//! ViewNode 原语 — 直接映射到 ViewNode 变体的基础构建块

use crate::geometry::Color;
use crate::layout::box_model::BoxStyle;
use crate::layout::flex::{AlignItems, FlexDirection, JustifyContent};
use crate::view::node::ViewNode;
use crate::view::View;

// ===== Text =====
pub struct Text {
    content: String,
    font_size: f64,
    color: Color,
}
impl Text {
    pub fn new(c: impl Into<String>) -> Self { Self { content: c.into(), font_size: 16.0, color: Color::BLACK } }
    pub fn font_size(mut self, s: f64) -> Self { self.font_size = s; self }
    pub fn color(mut self, c: Color) -> Self { self.color = c; self }
}
impl View for Text {
    fn build(&self) -> ViewNode {
        ViewNode::Text { content: self.content.clone(), font_size: self.font_size, color: self.color, key: None }
    }
}

// ===== Image =====
pub struct Image {
    data: Vec<u8>,
    w: u32,
    h: u32,
}
impl Image {
    pub fn from_rgba(data: Vec<u8>, w: u32, h: u32) -> Self { Self { data, w, h } }
}
impl View for Image {
    fn build(&self) -> ViewNode {
        ViewNode::Image { data: self.data.clone(), w: self.w, h: self.h, key: None }
    }
}

// ===== Container (map → ViewNode::Box) =====
pub struct Container {
    expand: bool,
    width: Option<f32>,
    height: Option<f32>,
    background: Option<Color>,
    padding: f32,
    border_radius: f32,
    children: Vec<Box<dyn View>>,
}
impl Container {
    pub fn new() -> Self { Self { expand: false, width: None, height: None, background: None, padding: 0.0, border_radius: 0.0, children: Vec::new() } }
    pub fn width(mut self, v: f32) -> Self { self.width = Some(v); self }
    pub fn height(mut self, v: f32) -> Self { self.height = Some(v); self }
    pub fn child(mut self, c: impl View + 'static) -> Self { self.children.push(Box::new(c)); self }
    pub fn expand(mut self, v: bool) -> Self { self.expand = v; self }
    pub fn background(mut self, color: Color) -> Self { self.background = Some(color); self }
    pub fn padding(mut self, v: f32) -> Self { self.padding = v; self }
    pub fn border_radius(mut self, r: f32) -> Self { self.border_radius = r; self }
}
impl Default for Container { fn default() -> Self { Self::new() } }
impl View for Container {
    fn build(&self) -> ViewNode {
        ViewNode::Box {
            style: BoxStyle {
                expand: self.expand,
                background_color: self.background,
                padding: crate::layout::box_model::EdgeInsets::all(self.padding),
                fixed_width: self.width,
                fixed_height: self.height,
                border_radius: self.border_radius,
                ..BoxStyle::default()
            },
            key: None,
            children: self.children.iter().map(|c| c.build()).collect(),
        }
    }
}

// ===== Column (map → ViewNode::Flex) =====
pub struct Column {
    spacing: f32,
    justify: JustifyContent,
    align: AlignItems,
    expand: bool,
    children: Vec<Box<dyn View>>,
}
impl Column {
    pub fn new() -> Self { Self { spacing: 4.0, justify: JustifyContent::Start, align: AlignItems::Stretch, expand: false, children: Vec::new() } }
    pub fn child(mut self, c: impl View + 'static) -> Self { self.children.push(Box::new(c)); self }
    pub fn spacing(mut self, s: f32) -> Self { self.spacing = s; self }
    pub fn justify_content(mut self, j: JustifyContent) -> Self { self.justify = j; self }
    pub fn align_items(mut self, a: AlignItems) -> Self { self.align = a; self }
    pub fn expand(mut self, v: bool) -> Self { self.expand = v; self }
    pub fn center(mut self) -> Self { self.justify = JustifyContent::Center; self.align = AlignItems::Center; self.expand = true; self }
}
impl Default for Column { fn default() -> Self { Self::new() } }
impl View for Column {
    fn build(&self) -> ViewNode {
        ViewNode::Flex {
            direction: FlexDirection::Column, justify: self.justify, align: self.align,
            spacing: self.spacing, expand: self.expand, flex_grow: 0.0, flex_shrink: 1.0,
            wrap: crate::layout::flex::FlexWrap::NoWrap,
            key: None,
            children: self.children.iter().map(|c| c.build()).collect(),
        }
    }
}

// ===== Row (map → ViewNode::Flex) =====
pub struct Row {
    spacing: f32,
    justify: JustifyContent,
    align: AlignItems,
    expand: bool,
    children: Vec<Box<dyn View>>,
}
impl Row {
    pub fn new() -> Self { Self { spacing: 4.0, justify: JustifyContent::Start, align: AlignItems::Center, expand: false, children: Vec::new() } }
    pub fn child(mut self, c: impl View + 'static) -> Self { self.children.push(Box::new(c)); self }
    pub fn spacing(mut self, s: f32) -> Self { self.spacing = s; self }
    pub fn justify_content(mut self, j: JustifyContent) -> Self { self.justify = j; self }
    pub fn align_items(mut self, a: AlignItems) -> Self { self.align = a; self }
    pub fn expand(mut self, v: bool) -> Self { self.expand = v; self }
    pub fn center(mut self) -> Self { self.justify = JustifyContent::Center; self.align = AlignItems::Center; self.expand = true; self }
}
impl Default for Row { fn default() -> Self { Self::new() } }
impl View for Row {
    fn build(&self) -> ViewNode {
        ViewNode::Flex {
            direction: FlexDirection::Row, justify: self.justify, align: self.align,
            spacing: self.spacing, expand: self.expand, flex_grow: 0.0, flex_shrink: 1.0,
            wrap: crate::layout::flex::FlexWrap::NoWrap,
            key: None,
            children: self.children.iter().map(|c| c.build()).collect(),
        }
    }
}
