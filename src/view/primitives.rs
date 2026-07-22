//! View 基本类型实现

use crate::geometry::Color;
use crate::view::node::{PropMap, PropValue, ViewNode};
use crate::view::View;

// ============================================================================
// Text
// ============================================================================
pub struct Text { content: String, font_size: f64, color: Color }
impl Text {
    pub fn new(content: impl Into<String>) -> Self { Self { content: content.into(), font_size: 16.0, color: Color::BLACK } }
    pub fn font_size(mut self, size: f64) -> Self { self.font_size = size; self }
    pub fn color(mut self, color: Color) -> Self { self.color = color; self }
    pub fn content(&self) -> &str { &self.content }
}
impl View for Text {
    fn build(&self) -> ViewNode {
        ViewNode { type_name: "text", key: None, props: PropMap::from_array([("content", PropValue::Str(self.content.clone())), ("font_size", PropValue::F64(self.font_size)), ("color", PropValue::Color(self.color))]), children: Vec::new() }
    }
}

// ============================================================================
// Button
// ============================================================================
pub struct Button { label: String, has_on_click: bool }
impl Button {
    pub fn new(label: impl Into<String>) -> Self { Self { label: label.into(), has_on_click: false } }
    pub fn on_click(mut self) -> Self { self.has_on_click = true; self }
}
impl View for Button {
    fn build(&self) -> ViewNode {
        ViewNode { type_name: "button", key: None, props: PropMap::from_array([("label", PropValue::Str(self.label.clone()))]), children: Vec::new() }
    }
}

// ============================================================================
// Image — 存储像素数据
// ============================================================================
pub struct Image { data: Vec<u8>, width: u32, height: u32 }
impl Image {
    pub fn new(data: Vec<u8>, width: u32, height: u32) -> Self { Self { data, width, height } }
    pub fn from_rgba(data: Vec<u8>, width: u32, height: u32) -> Self { Self { data, width, height } }
}
impl View for Image {
    fn build(&self) -> ViewNode {
        ViewNode { type_name: "image", key: None, props: PropMap::from_array([("data", PropValue::Bytes(self.data.clone())), ("img_w", PropValue::U32(self.width)), ("img_h", PropValue::U32(self.height))]), children: Vec::new() }
    }
}

// ============================================================================
// Checkbox
// ============================================================================
pub struct Checkbox { checked: bool, label: String }
impl Checkbox {
    pub fn new(checked: bool) -> Self { Self { checked, label: String::new() } }
    pub fn label(mut self, s: impl Into<String>) -> Self { self.label = s.into(); self }
}
impl View for Checkbox {
    fn build(&self) -> ViewNode {
        ViewNode { type_name: "checkbox", key: None, props: PropMap::from_array([("checked", PropValue::Bool(self.checked)), ("label", PropValue::Str(self.label.clone()))]), children: Vec::new() }
    }
}

// ============================================================================
// Container
// ============================================================================
pub struct Container { children: Vec<Box<dyn View>> }
impl Container {
    pub fn new() -> Self { Self { children: Vec::new() } }
    pub fn child(mut self, child: impl View + 'static) -> Self { self.children.push(Box::new(child)); self }
}
impl View for Container {
    fn build(&self) -> ViewNode { ViewNode { type_name: "container", key: None, props: PropMap::new(), children: self.children.iter().map(|c| c.build()).collect() } }
}

// ============================================================================
// Column
// ============================================================================
pub struct Column { children: Vec<Box<dyn View>> }
impl Column {
    pub fn new() -> Self { Self { children: Vec::new() } }
    pub fn child(mut self, child: impl View + 'static) -> Self { self.children.push(Box::new(child)); self }
}
impl View for Column {
    fn build(&self) -> ViewNode { ViewNode { type_name: "column", key: None, props: PropMap::new(), children: self.children.iter().map(|c| c.build()).collect() } }
}

// ============================================================================
// Row
// ============================================================================
pub struct Row { children: Vec<Box<dyn View>> }
impl Row {
    pub fn new() -> Self { Self { children: Vec::new() } }
    pub fn child(mut self, child: impl View + 'static) -> Self { self.children.push(Box::new(child)); self }
}
impl View for Row {
    fn build(&self) -> ViewNode { ViewNode { type_name: "row", key: None, props: PropMap::new(), children: self.children.iter().map(|c| c.build()).collect() } }
}

// ============================================================================
// Divider
// ============================================================================
pub struct Divider;
impl View for Divider {
    fn build(&self) -> ViewNode { ViewNode { type_name: "divider", key: None, props: PropMap::new(), children: Vec::new() } }
}
