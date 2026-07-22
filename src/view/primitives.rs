//! View 基本类型实现（v2）— 返回类型安全的 ViewNode 变体

use crate::geometry::Color;
use crate::state;
use crate::view::node::{PropMap, PropValue, ViewNode};
use crate::view::View;
use crate::layout::flex::{JustifyContent, AlignItems};

// ===== Text =====
pub struct Text { content: String, font_size: f64, color: Color }
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

// ===== Button =====
pub struct Button { label: String, callback_id: Option<u64> }
impl Button {
    pub fn new(l: impl Into<String>) -> Self { Self { label: l.into(), callback_id: None } }
    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.callback_id = Some(state::register_click(Box::new(f)));
        self
    }
}
impl View for Button {
    fn build(&self) -> ViewNode {
        ViewNode::Button { label: self.label.clone(), on_click: self.callback_id, key: None }
    }
}

// ===== Image =====
pub struct Image { data: Vec<u8>, w: u32, h: u32 }
impl Image {
    pub fn from_rgba(data: Vec<u8>, w: u32, h: u32) -> Self { Self { data, w, h } }
}
impl View for Image {
    fn build(&self) -> ViewNode {
        ViewNode::Image { data: self.data.clone(), w: self.w, h: self.h, key: None }
    }
}

// ===== Checkbox =====
pub struct Checkbox { checked: bool, label: String, callback_id: Option<u64> }
impl Checkbox {
    pub fn new(checked: bool) -> Self { Self { checked, label: String::new(), callback_id: None } }
    pub fn label(mut self, s: impl Into<String>) -> Self { self.label = s.into(); self }
    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.callback_id = Some(state::register_click(Box::new(f)));
        self
    }
}
impl View for Checkbox {
    fn build(&self) -> ViewNode {
        ViewNode::Checkbox { checked: self.checked, label: self.label.clone(), on_click: self.callback_id, key: None }
    }
}

// ===== Container =====
pub struct Container { expand: bool, children: Vec<Box<dyn View>> }
impl Container {
    pub fn new() -> Self { Self { expand: false, children: Vec::new() } }
    pub fn child(mut self, c: impl View + 'static) -> Self { self.children.push(Box::new(c)); self }
    pub fn expand(mut self, v: bool) -> Self { self.expand = v; self }
}
impl View for Container {
    fn build(&self) -> ViewNode {
        ViewNode::Container { expand: self.expand, key: None, children: self.children.iter().map(|c| c.build()).collect() }
    }
}

// ===== Column =====
pub struct Column { spacing: f32, justify: JustifyContent, align: AlignItems, expand: bool, children: Vec<Box<dyn View>> }
impl Column {
    pub fn new() -> Self { Self { spacing: 4.0, justify: JustifyContent::Start, align: AlignItems::Start, expand: false, children: Vec::new() } }
    pub fn child(mut self, c: impl View + 'static) -> Self { self.children.push(Box::new(c)); self }
    pub fn spacing(mut self, s: f32) -> Self { self.spacing = s; self }
    pub fn justify_content(mut self, j: JustifyContent) -> Self { self.justify = j; self }
    pub fn align_items(mut self, a: AlignItems) -> Self { self.align = a; self }
    pub fn expand(mut self, v: bool) -> Self { self.expand = v; self }
    pub fn center(mut self) -> Self { self.justify = JustifyContent::Center; self.align = AlignItems::Center; self.expand = true; self }
}
impl View for Column {
    fn build(&self) -> ViewNode {
        ViewNode::Column {
            justify: self.justify, align: self.align, spacing: self.spacing,
            expand: self.expand, key: None,
            children: self.children.iter().map(|c| c.build()).collect(),
        }
    }
}

// ===== Row =====
pub struct Row { spacing: f32, justify: JustifyContent, align: AlignItems, expand: bool, children: Vec<Box<dyn View>> }
impl Row {
    pub fn new() -> Self { Self { spacing: 4.0, justify: JustifyContent::Start, align: AlignItems::Center, expand: false, children: Vec::new() } }
    pub fn child(mut self, c: impl View + 'static) -> Self { self.children.push(Box::new(c)); self }
    pub fn spacing(mut self, s: f32) -> Self { self.spacing = s; self }
    pub fn justify_content(mut self, j: JustifyContent) -> Self { self.justify = j; self }
    pub fn align_items(mut self, a: AlignItems) -> Self { self.align = a; self }
    pub fn expand(mut self, v: bool) -> Self { self.expand = v; self }
    pub fn center(mut self) -> Self { self.justify = JustifyContent::Center; self.align = AlignItems::Center; self.expand = true; self }
}
impl View for Row {
    fn build(&self) -> ViewNode {
        ViewNode::Row {
            justify: self.justify, align: self.align, spacing: self.spacing,
            expand: self.expand, key: None,
            children: self.children.iter().map(|c| c.build()).collect(),
        }
    }
}

// ===== Divider =====
pub struct Divider;
impl View for Divider {
    fn build(&self) -> ViewNode { ViewNode::Divider { key: None } }
}
