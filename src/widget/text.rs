//! Text — 映射到 `ViewNode::Text` 的基础组件

use crate::event::EventContext;
use crate::geometry::Color;
use crate::state;
use crate::view::node::{ClickCallbackRef, ViewNode};
use crate::view::View;

pub struct Text {
    content: String,
    font_size: f64,
    color: Color,
    on_click: Option<ClickCallbackRef>,
}
impl Text {
    pub fn new(c: impl Into<String>) -> Self {
        Self {
            content: c.into(),
            font_size: 16.0,
            color: crate::theme::current().text.regular_default,
            on_click: None,
        }
    }
    pub fn font_size(mut self, s: f64) -> Self {
        self.font_size = s;
        self
    }
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }
    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.on_click = Some(ClickCallbackRef::Simple(state::register_click(Box::new(f))));
        self
    }
    pub fn on_click_with_ctx<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.on_click = Some(ClickCallbackRef::WithCtx(state::register_click_with_ctx(
            Box::new(f),
        )));
        self
    }
}
impl View for Text {
    fn build(&self) -> ViewNode {
        ViewNode::Text {
            content: self.content.clone(),
            font_size: self.font_size,
            color: self.color,
            key: None,
            listener: self.on_click,
        }
    }
}
