//! SharedSurfaceView — 把 `SharedSurface` 承载到 widget 树的组件
//!
//! 高频组件（terminal）自持一块 `SharedSurface`，通过本 widget 挂进 widget 树。
//! 渲染阶段由 compositor 按脏区合屏，不经 vello 全量光栅化。

use crate::event::EventContext;
use crate::layout::style::FlexStyle;
use crate::view::node::{Listener, ViewNode};
use crate::widget::layout::LayoutAttr;
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;

/// 承载一块共享像素表面的 widget。
///
/// 布局尺寸默认等于 surface 尺寸，也可通过 `width`/`height` 覆盖显示尺寸。
#[derive(Clone)]
pub struct SharedSurfaceView {
    surface: Rc<crate::render::surface::SharedSurface>,
    layout: LayoutAttr,
    listeners: Vec<Listener>,
}

impl SharedSurfaceView {
    /// 用一个共享表面创建视图。
    pub fn new(surface: Rc<crate::render::surface::SharedSurface>) -> Self {
        Self {
            surface,
            layout: LayoutAttr::new(),
            listeners: Vec::new(),
        }
    }

    /// 显示宽度（默认 surface 自身宽度）。
    pub fn width(mut self, w: f32) -> Self {
        self.layout = self.layout.width(w);
        self
    }

    /// 显示高度（默认 surface 自身高度）。
    pub fn height(mut self, h: f32) -> Self {
        self.layout = self.layout.height(h);
        self
    }

    /// 用完整布局属性设置本视图的布局。
    pub fn layout(mut self, l: LayoutAttr) -> Self {
        self.layout = l;
        self
    }

    pub fn on_click_with_ctx<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click_with_ctx(Rc::new(f)));
        self
    }
    pub fn on_key_down<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_key_down(Rc::new(f)));
        self
    }
    pub fn on_ime_commit<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_ime_commit(Rc::new(f)));
        self
    }
    pub fn on_mouse_wheel<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_mouse_wheel(Rc::new(f)));
        self
    }
    pub fn on_mouse_down<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_mouse_down(Rc::new(f)));
        self
    }
    pub fn on_mouse_up<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_mouse_up(Rc::new(f)));
        self
    }
    pub fn on_mouse_move<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_mouse_move(Rc::new(f)));
        self
    }
    pub fn on_drag_move<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_drag_move(Rc::new(f)));
        self
    }
}

impl Widget for SharedSurfaceView {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        let layout = self.layout.apply(FlexStyle::default());
        ViewNode::SharedSurface {
            surface: Rc::clone(&self.surface),
            layout,
            key: None,
            listeners: self.listeners.clone(),
        }
    }
}
