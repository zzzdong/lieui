//! Container — 映射到 `ViewNode::Div`（Block 模式）的布局型组件
//!
//! 通用语义化容器。布局属性通过 [`LayoutAttr`] 承载（见 `layout_methods!` 生成的
//! `width` / `height` / `margin` / `padding` / `align` / `gap` 等方法），视觉装饰
//! （背景 / 边框 / 圆角 / 阴影）由 `PaintStyle` 提供。

use crate::event::EventContext;
use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::layout_methods;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::PaintStyle;
use crate::widget::layout::LayoutAttr;
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;

pub struct Container {
    layout: LayoutAttr,
    clip_content: bool,
    children: Vec<Box<dyn Widget>>,
    listeners: Vec<Listener>,
    /// 视觉样式自定义（背景色、悬停色、按下色、圆角、边框等）。
    paint: Option<PaintStyle>,
}
impl Container {
    pub fn new() -> Self {
        Self {
            layout: LayoutAttr::new(),
            clip_content: false,
            children: Vec::new(),
            listeners: Vec::new(),
            paint: None,
        }
    }
    layout_methods!(for Container);

    pub fn child(mut self, c: impl Widget + 'static) -> Self {
        self.children.push(Box::new(c));
        self
    }
    pub fn clip(mut self, v: bool) -> Self {
        self.clip_content = v;
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
    /// 鼠标按下（可按 `EventContext::event()` 判断按键）。
    pub fn on_mouse_down<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_mouse_down(Rc::new(f)));
        self
    }
    /// 鼠标释放（可按 `EventContext::event()` 判断按键）。
    pub fn on_mouse_up<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_mouse_up(Rc::new(f)));
        self
    }
    /// 鼠标进入节点区域。
    pub fn on_mouse_enter<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_mouse_enter(Rc::new(f)));
        self
    }
    /// 鼠标离开节点区域。
    pub fn on_mouse_leave<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_mouse_leave(Rc::new(f)));
        self
    }

    // ── 视觉样式 ──

    /// 完整指定视觉样式。
    pub fn paint_style(mut self, p: PaintStyle) -> Self {
        self.paint = Some(p);
        self
    }
    /// 背景色
    pub fn background(mut self, c: Color) -> Self {
        self.paint
            .get_or_insert_with(PaintStyle::new)
            .background_color = Some(c);
        self
    }
    /// 悬停背景色
    pub fn hover_background(mut self, c: Color) -> Self {
        self.paint
            .get_or_insert_with(PaintStyle::new)
            .hover_background = Some(c);
        self
    }
    /// 按下背景色
    pub fn pressed_background(mut self, c: Color) -> Self {
        self.paint
            .get_or_insert_with(PaintStyle::new)
            .pressed_background = Some(c);
        self
    }
    /// 圆角半径
    pub fn border_radius(mut self, r: f32) -> Self {
        self.paint.get_or_insert_with(PaintStyle::new).border_radius = r;
        self
    }
    /// 边框（宽度 + 颜色）
    pub fn border(mut self, width: f32, color: Color) -> Self {
        let p = self.paint.get_or_insert_with(PaintStyle::new);
        p.border_width = width;
        p.border_color = Some(color);
        self
    }
    /// 不透明度
    pub fn opacity(mut self, o: f32) -> Self {
        self.paint.get_or_insert_with(PaintStyle::new).opacity = o.clamp(0.0, 1.0);
        self
    }
    /// 投影（PatternFly 的 box-shadow token）
    pub fn shadow(mut self, s: crate::view::paint::ShadowSpec) -> Self {
        self.paint.get_or_insert_with(PaintStyle::new).shadow = Some(s);
        self
    }
}
impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}
impl Widget for Container {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let mut layout = FlexStyle::block().padding_all(self.layout.padding);
        // 方向性 padding 覆盖统一值。
        if let Some(v) = self.layout.padding_l {
            layout = layout.padding_left(v);
        }
        if let Some(v) = self.layout.padding_t {
            layout = layout.padding_top(v);
        }
        if let Some(v) = self.layout.padding_r {
            layout = layout.padding_right(v);
        }
        if let Some(v) = self.layout.padding_b {
            layout = layout.padding_bottom(v);
        }
        // 其余布局属性统一应用。
        layout = self.layout.apply(layout);

        let mut paint = PaintStyle::default();
        if let Some(p) = &self.paint {
            if let Some(bg) = p.background_color {
                paint = paint.background(bg);
            }
            if let Some(hbg) = p.hover_background {
                paint = paint.hover_background(hbg);
            }
            if let Some(pbg) = p.pressed_background {
                paint = paint.pressed_background(pbg);
            }
            if p.border_radius > 0.0 {
                paint = paint.radius(p.border_radius);
            }
            if let Some(bc) = p.border_color
                && p.border_width > 0.0
            {
                paint = paint.border(p.border_width, bc);
            }
            if p.opacity != 1.0 {
                paint = paint.opacity(p.opacity);
            }
            if let Some(sh) = p.shadow {
                paint = paint.shadow(sh);
            }
        }
        if self.clip_content {
            paint = paint.clip(true);
        }

        ViewNode::Div {
            layout,
            paint,
            key: None,
            children: self
                .children
                .iter()
                .enumerate()
                .map(|(i, c)| ctx.child(i, c.as_ref()))
                .collect(),
            listeners: self.listeners.clone(),
        }
    }
}
