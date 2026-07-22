//! View 基本类型实现（v2）

use crate::geometry::Color;
use crate::state;
use crate::view::node::{PropMap, PropValue, ViewNode};
use crate::view::View;

// ===== Text =====
pub struct Text { content: String, font_size: f64, color: Color }
impl Text {
    pub fn new(c: impl Into<String>) -> Self { Self { content: c.into(), font_size: 16.0, color: Color::BLACK } }
    pub fn font_size(mut self, s: f64) -> Self { self.font_size = s; self }
    pub fn color(mut self, c: Color) -> Self { self.color = c; self }
}
impl View for Text {
    fn build(&self) -> ViewNode {
        ViewNode { type_name: "text", key: None, props: PropMap::from_array([("content", PropValue::Str(self.content.clone())), ("font_size", PropValue::F64(self.font_size)), ("color", PropValue::Color(self.color))]), children: Vec::new() }
    }
}

// ===== Button（带点击回调全局注册）=====
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
        let mut p = PropMap::new();
        p.set("label", PropValue::Str(self.label.clone()));
        if let Some(id) = self.callback_id { p.set("on_click", PropValue::U32(id as u32)); }
        ViewNode { type_name: "button", key: None, props: p, children: Vec::new() }
    }
}

// ===== Image =====
pub struct Image { data: Vec<u8>, w: u32, h: u32 }
impl Image {
    pub fn from_rgba(data: Vec<u8>, w: u32, h: u32) -> Self { Self { data, w, h } }
}
impl View for Image {
    fn build(&self) -> ViewNode {
        ViewNode { type_name: "image", key: None, props: PropMap::from_array([("data", PropValue::Bytes(self.data.clone())), ("img_w", PropValue::U32(self.w)), ("img_h", PropValue::U32(self.h))]), children: Vec::new() }
    }
}

// ===== Checkbox =====
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

// ===== Container =====
pub struct Container { children: Vec<Box<dyn View>> }
impl Container {
    pub fn new() -> Self { Self { children: Vec::new() } }
    pub fn child(mut self, c: impl View + 'static) -> Self { self.children.push(Box::new(c)); self }
}
impl View for Container {
    fn build(&self) -> ViewNode { ViewNode { type_name: "container", key: None, props: PropMap::new(), children: self.children.iter().map(|c| c.build()).collect() } }
}

// ===== Column =====
pub struct Column { children: Vec<Box<dyn View>> }
impl Column {
    pub fn new() -> Self { Self { children: Vec::new() } }
    pub fn child(mut self, c: impl View + 'static) -> Self { self.children.push(Box::new(c)); self }
}
impl View for Column {
    fn build(&self) -> ViewNode { ViewNode { type_name: "column", key: None, props: PropMap::new(), children: self.children.iter().map(|c| c.build()).collect() } }
}

// ===== Row =====
pub struct Row { children: Vec<Box<dyn View>> }
impl Row {
    pub fn new() -> Self { Self { children: Vec::new() } }
    pub fn child(mut self, c: impl View + 'static) -> Self { self.children.push(Box::new(c)); self }
}
impl View for Row {
    fn build(&self) -> ViewNode { ViewNode { type_name: "row", key: None, props: PropMap::new(), children: self.children.iter().map(|c| c.build()).collect() } }
}

// ===== Divider =====
pub struct Divider;
impl View for Divider {
    fn build(&self) -> ViewNode { ViewNode { type_name: "divider", key: None, props: PropMap::new(), children: Vec::new() } }
}
