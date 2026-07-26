//! Container — 映射到 `ViewNode::Div`（Block 模式）的布局型组件

use crate::event::EventContext;
use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::PaintStyle;
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;

pub struct Container {
    expand: bool,
    width: Option<f32>,
    height: Option<f32>,
    padding: f32,
    clip_content: bool,
    children: Vec<Box<dyn Widget>>,
    listeners: Vec<Listener>,
    /// 视觉样式自定义（背景色、悬停色、按下色、圆角、边框等）。
    paint: Option<PaintStyle>,
}
impl Container {
    pub fn new() -> Self {
        Self {
            expand: false,
            width: None,
            height: None,
            padding: 0.0,
            clip_content: false,
            children: Vec::new(),
            listeners: Vec::new(),
            paint: None,
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
    pub fn child(mut self, c: impl Widget + 'static) -> Self {
        self.children.push(Box::new(c));
        self
    }
    pub fn expand(mut self, v: bool) -> Self {
        self.expand = v;
        self
    }
    pub fn padding(mut self, v: f32) -> Self {
        self.padding = v;
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
        let mut layout = FlexStyle::block().padding_all(self.padding);
        if self.expand {
            layout = layout.flex_grow(1.0);
        }
        if let Some(w) = self.width {
            layout = layout.width(w);
        }
        if let Some(h) = self.height {
            layout = layout.height(h);
        }

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
            if let Some(bc) = p.border_color {
                if p.border_width > 0.0 {
                    paint = paint.border(p.border_width, bc);
                }
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
