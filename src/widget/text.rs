//! Text — 映射到 `ViewNode::Text` 的基础组件

use crate::event::EventContext;
use crate::geometry::Color;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::{FontWeight, TextAlign, TextStyle};
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;

#[derive(Clone)]
pub struct Text {
    content: String,
    style: TextStyle,
    listeners: Vec<Listener>,
}
impl Text {
    pub fn new(c: impl Into<String>) -> Self {
        Self {
            content: c.into(),
            style: TextStyle {
                font_size: 16.0,
                color: crate::theme::current().text.regular_default,
                ..TextStyle::default()
            },
            listeners: Vec::new(),
        }
    }
    pub fn font_size(mut self, s: f64) -> Self {
        self.style.font_size = s;
        self
    }
    pub fn color(mut self, c: Color) -> Self {
        self.style.color = c;
        self
    }
    pub fn font_family(mut self, f: impl Into<String>) -> Self {
        self.style.font_family = f.into();
        self
    }
    pub fn font_weight(mut self, w: impl Into<FontWeight>) -> Self {
        self.style.font_weight = w.into();
        self
    }
    pub fn line_height(mut self, h: f64) -> Self {
        self.style.line_height = Some(h);
        self
    }
    pub fn max_width(mut self, w: f64) -> Self {
        self.style.max_width = Some(w);
        self
    }
    pub fn text_align(mut self, a: TextAlign) -> Self {
        self.style.text_align = a;
        self
    }
    pub fn wrap(mut self, v: bool) -> Self {
        self.style.wrap = v;
        self
    }
    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click(Rc::new(f)));
        self
    }
    pub fn on_click_with_ctx<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click_with_ctx(Rc::new(f)));
        self
    }
}
impl Widget for Text {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        ViewNode::Text {
            content: self.content.clone(),
            style: self.style.clone(),
            layout: crate::layout::style::FlexStyle::default(),
            key: None,
            listeners: self.listeners.clone(),
        }
    }
}
