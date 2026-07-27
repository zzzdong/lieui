//! Column / Row — 映射到 `ViewNode::Div`（Flex 模式）的布局型组件

use crate::event::EventContext;
use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::{FlexAlign, FlexWrap};
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::PaintStyle;
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;

// ===== Column (map → ViewNode::Flex, direction = Column) =====
pub struct Column {
    spacing: f32,
    justify: FlexAlign,
    align: FlexAlign,
    expand: bool,
    flex_shrink: f32,
    children: Vec<Box<dyn Widget>>,
    listeners: Vec<Listener>,
    /// 视觉样式自定义。
    paint: Option<PaintStyle>,
}
impl Column {
    pub fn new() -> Self {
        Self {
            spacing: 4.0,
            justify: FlexAlign::Start,
            align: FlexAlign::Stretch,
            expand: false,
            flex_shrink: 1.0,
            children: Vec::new(),
            listeners: Vec::new(),
            paint: None,
        }
    }
    pub fn child(mut self, c: impl Widget + 'static) -> Self {
        self.children.push(Box::new(c));
        self
    }
    pub fn spacing(mut self, s: f32) -> Self {
        self.spacing = s;
        self
    }
    pub fn justify_content(mut self, j: FlexAlign) -> Self {
        self.justify = j;
        self
    }
    pub fn align_items(mut self, a: FlexAlign) -> Self {
        self.align = a;
        self
    }
    pub fn expand(mut self, v: bool) -> Self {
        self.expand = v;
        self
    }
    pub fn flex_shrink(mut self, v: f32) -> Self {
        self.flex_shrink = v;
        self
    }
    pub fn center(mut self) -> Self {
        self.justify = FlexAlign::Center;
        self.align = FlexAlign::Center;
        self.expand = true;
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

    pub fn paint_style(mut self, p: PaintStyle) -> Self {
        self.paint = Some(p);
        self
    }
    pub fn background(mut self, c: Color) -> Self {
        self.paint
            .get_or_insert_with(PaintStyle::new)
            .background_color = Some(c);
        self
    }
    pub fn hover_background(mut self, c: Color) -> Self {
        self.paint
            .get_or_insert_with(PaintStyle::new)
            .hover_background = Some(c);
        self
    }
    pub fn pressed_background(mut self, c: Color) -> Self {
        self.paint
            .get_or_insert_with(PaintStyle::new)
            .pressed_background = Some(c);
        self
    }
    pub fn border_radius(mut self, r: f32) -> Self {
        self.paint.get_or_insert_with(PaintStyle::new).border_radius = r;
        self
    }
    pub fn border(mut self, width: f32, color: Color) -> Self {
        let p = self.paint.get_or_insert_with(PaintStyle::new);
        p.border_width = width;
        p.border_color = Some(color);
        self
    }
    pub fn opacity(mut self, o: f32) -> Self {
        self.paint.get_or_insert_with(PaintStyle::new).opacity = o.clamp(0.0, 1.0);
        self
    }
}
impl Default for Column {
    fn default() -> Self {
        Self::new()
    }
}
impl Widget for Column {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let mut layout = FlexStyle::column()
            .justify_content(self.justify)
            .align_items(self.align)
            .gap(self.spacing)
            .wrap(FlexWrap::NoWrap);
        if self.expand {
            layout = layout.flex_grow(1.0);
        }
        ViewNode::Div {
            layout,
            paint: self.paint.clone().unwrap_or_default(),
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

// ===== Row (map → ViewNode::Flex, direction = Row) =====
pub struct Row {
    spacing: f32,
    justify: FlexAlign,
    align: FlexAlign,
    expand: bool,
    flex_shrink: f32,
    children: Vec<Box<dyn Widget>>,
    listeners: Vec<Listener>,
    /// 视觉样式自定义。
    paint: Option<PaintStyle>,
}
impl Row {
    pub fn new() -> Self {
        Self {
            spacing: 4.0,
            justify: FlexAlign::Start,
            align: FlexAlign::Center,
            expand: false,
            flex_shrink: 1.0,
            children: Vec::new(),
            listeners: Vec::new(),
            paint: None,
        }
    }
    pub fn child(mut self, c: impl Widget + 'static) -> Self {
        self.children.push(Box::new(c));
        self
    }
    pub fn spacing(mut self, s: f32) -> Self {
        self.spacing = s;
        self
    }
    pub fn justify_content(mut self, j: FlexAlign) -> Self {
        self.justify = j;
        self
    }
    pub fn align_items(mut self, a: FlexAlign) -> Self {
        self.align = a;
        self
    }
    pub fn expand(mut self, v: bool) -> Self {
        self.expand = v;
        self
    }
    pub fn flex_shrink(mut self, v: f32) -> Self {
        self.flex_shrink = v;
        self
    }
    pub fn center(mut self) -> Self {
        self.justify = FlexAlign::Center;
        self.align = FlexAlign::Center;
        self.expand = true;
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

    pub fn paint_style(mut self, p: PaintStyle) -> Self {
        self.paint = Some(p);
        self
    }
    pub fn background(mut self, c: Color) -> Self {
        self.paint
            .get_or_insert_with(PaintStyle::new)
            .background_color = Some(c);
        self
    }
    pub fn hover_background(mut self, c: Color) -> Self {
        self.paint
            .get_or_insert_with(PaintStyle::new)
            .hover_background = Some(c);
        self
    }
    pub fn pressed_background(mut self, c: Color) -> Self {
        self.paint
            .get_or_insert_with(PaintStyle::new)
            .pressed_background = Some(c);
        self
    }
    pub fn border_radius(mut self, r: f32) -> Self {
        self.paint.get_or_insert_with(PaintStyle::new).border_radius = r;
        self
    }
    pub fn border(mut self, width: f32, color: Color) -> Self {
        let p = self.paint.get_or_insert_with(PaintStyle::new);
        p.border_width = width;
        p.border_color = Some(color);
        self
    }
    pub fn opacity(mut self, o: f32) -> Self {
        self.paint.get_or_insert_with(PaintStyle::new).opacity = o.clamp(0.0, 1.0);
        self
    }
}
impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}
impl Widget for Row {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let mut layout = FlexStyle::row()
            .justify_content(self.justify)
            .align_items(self.align)
            .gap(self.spacing)
            .wrap(FlexWrap::NoWrap);
        if self.expand {
            layout = layout.flex_grow(1.0);
        }
        if self.flex_shrink != 1.0 {
            layout = layout.flex_shrink(self.flex_shrink);
        }
        ViewNode::Div {
            layout,
            paint: self.paint.clone().unwrap_or_default(),
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
