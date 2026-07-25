//! Image — 映射到 `ViewNode::Image` 的基础组件

use crate::event::EventContext;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::{ImageFit, ImageStyle};
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;

pub struct Image {
    data: std::sync::Arc<Vec<u8>>,
    style: ImageStyle,
    listeners: Vec<Listener>,
}
impl Image {
    pub fn from_rgba(data: Vec<u8>, w: u32, h: u32) -> Self {
        Self {
            data: std::sync::Arc::new(data),
            style: ImageStyle {
                width: w,
                height: h,
                ..ImageStyle::default()
            },
            listeners: Vec::new(),
        }
    }
    pub fn from_arc(data: std::sync::Arc<Vec<u8>>, w: u32, h: u32) -> Self {
        Self {
            data,
            style: ImageStyle {
                width: w,
                height: h,
                ..ImageStyle::default()
            },
            listeners: Vec::new(),
        }
    }
    pub fn opacity(mut self, o: f32) -> Self {
        self.style.opacity = o.clamp(0.0, 1.0);
        self
    }
    pub fn fit(mut self, f: ImageFit) -> Self {
        self.style.fit = f;
        self
    }
    pub fn radius(mut self, r: f32) -> Self {
        self.style.border_radius = r;
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
impl Widget for Image {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        ViewNode::Image {
            data: std::sync::Arc::clone(&self.data),
            style: self.style,
            layout: crate::layout::style::FlexStyle::default(),
            key: None,
            listeners: self.listeners.clone(),
        }
    }
}
