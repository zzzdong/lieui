//! Container — 映射到 `ViewNode::Div`（Block 模式）的布局型组件

use crate::event::EventContext;
use crate::geometry::Color;
use crate::layout::box_model::{BoxStyle, EdgeInsets};
use crate::layout::flex::FlexStyle;
use crate::state;
use crate::view::node::{ClickCallbackRef, DisplayMode, ViewNode};
use crate::view::View;

pub struct Container {
    expand: bool,
    width: Option<f32>,
    height: Option<f32>,
    background: Option<Color>,
    padding: f32,
    border_radius: f32,
    children: Vec<Box<dyn View>>,
    on_click: Option<ClickCallbackRef>,
}
impl Container {
    pub fn new() -> Self {
        Self {
            expand: false,
            width: None,
            height: None,
            background: None,
            padding: 0.0,
            border_radius: 0.0,
            children: Vec::new(),
            on_click: None,
        }
    }
    pub fn width(mut self, v: f32) -> Self {
        self.width = Some(v);
        self
    }
    pub fn height(mut self, v: f32) -> Self {
        self.height = Some(v);
        self
    }
    pub fn child(mut self, c: impl View + 'static) -> Self {
        self.children.push(Box::new(c));
        self
    }
    pub fn expand(mut self, v: bool) -> Self {
        self.expand = v;
        self
    }
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }
    pub fn padding(mut self, v: f32) -> Self {
        self.padding = v;
        self
    }
    pub fn border_radius(mut self, r: f32) -> Self {
        self.border_radius = r;
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
impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}
impl View for Container {
    fn build(&self) -> ViewNode {
        ViewNode::Div {
            style: BoxStyle {
                expand: self.expand,
                background_color: self.background,
                padding: EdgeInsets::all(self.padding),
                fixed_width: self.width,
                fixed_height: self.height,
                border_radius: self.border_radius,
                ..BoxStyle::default()
            },
            flex: FlexStyle::default(),
            display: DisplayMode::Block,
            key: None,
            children: self.children.iter().map(|c| c.build()).collect(),
            listener: self.on_click,
        }
    }
}
