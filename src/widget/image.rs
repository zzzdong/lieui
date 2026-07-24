//! Image — 映射到 `ViewNode::Image` 的基础组件

use crate::event::EventContext;
use crate::state;
use crate::view::node::{ClickCallbackRef, ViewNode};
use crate::view::View;

pub struct Image {
    data: std::sync::Arc<Vec<u8>>,
    w: u32,
    h: u32,
    on_click: Option<ClickCallbackRef>,
}
impl Image {
    pub fn from_rgba(data: Vec<u8>, w: u32, h: u32) -> Self {
        Self {
            data: std::sync::Arc::new(data),
            w,
            h,
            on_click: None,
        }
    }
    pub fn from_arc(data: std::sync::Arc<Vec<u8>>, w: u32, h: u32) -> Self {
        Self {
            data,
            w,
            h,
            on_click: None,
        }
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
impl View for Image {
    fn build(&self) -> ViewNode {
        ViewNode::Image {
            data: std::sync::Arc::clone(&self.data),
            w: self.w,
            h: self.h,
            key: None,
            listener: self.on_click,
        }
    }
}
