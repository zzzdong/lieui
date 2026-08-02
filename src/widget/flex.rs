//! Column / Row — 映射到 `ViewNode::Div`（Flex 模式）的布局型组件
//!
//! 主轴/交叉轴对齐、间距、伸缩等通用布局属性统一由 [`LayoutAttr`] 承载，
//! 通过 builder 方法设置（`spacing` 对应 `gap`，`justify_content` /
//! `align_items` / `expand` / `flex_shrink` 一一对应）。

use crate::event::EventContext;
use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::{FlexAlign, FlexWrap};
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::PaintStyle;
use crate::widget::layout::LayoutAttr;
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;

// ===== Column (map → ViewNode::Flex, direction = Column) =====
pub struct Column {
    layout: LayoutAttr,
    children: Vec<Box<dyn Widget>>,
    listeners: Vec<Listener>,
    /// 视觉样式自定义。
    paint: Option<PaintStyle>,
}
impl Column {
    pub fn new() -> Self {
        Self {
            layout: LayoutAttr::new().gap(4.0).align_items(FlexAlign::Stretch),
            children: Vec::new(),
            listeners: Vec::new(),
            paint: None,
        }
    }
    pub fn child(mut self, c: impl Widget + 'static) -> Self {
        self.children.push(Box::new(c));
        self
    }
    /// 子项间距。
    pub fn spacing(mut self, s: f32) -> Self {
        self.layout = self.layout.gap(s);
        self
    }
    /// 用完整布局属性（builder 式）设置本 widget 的布局。
    pub fn layout(mut self, l: LayoutAttr) -> Self {
        self.layout = l;
        self
    }
    pub fn width(mut self, v: f32) -> Self {
        self.layout = self.layout.width(v);
        self
    }
    pub fn height(mut self, v: f32) -> Self {
        self.layout = self.layout.height(v);
        self
    }
    pub fn justify_content(mut self, j: FlexAlign) -> Self {
        self.layout = self.layout.justify_content(j);
        self
    }
    pub fn align_items(mut self, a: FlexAlign) -> Self {
        self.layout = self.layout.align_items(a);
        self
    }
    pub fn expand(mut self, v: bool) -> Self {
        self.layout = self.layout.expand(v);
        self
    }
    pub fn flex_shrink(mut self, v: f32) -> Self {
        self.layout = self.layout.flex_shrink(v);
        self
    }
    pub fn margin(mut self, v: f32) -> Self {
        self.layout = self.layout.margin(v);
        self
    }
    pub fn center(mut self) -> Self {
        self.layout = self
            .layout
            .justify_content(FlexAlign::Center)
            .align_items(FlexAlign::Center)
            .expand(true);
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

    /// 条件添加子节点。当 `c` 为 `Some` 时将其加入子节点列表。
    pub fn maybe_child<W: Widget + 'static>(mut self, c: Option<W>) -> Self {
        if let Some(c) = c {
            self.children.push(Box::new(c));
        }
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
            .justify_content(self.layout.justify_content.unwrap_or(FlexAlign::Start))
            .align_items(self.layout.align_items.unwrap_or(FlexAlign::Stretch))
            .gap(self.layout.gap)
            .wrap(FlexWrap::NoWrap);
        layout = self.layout.apply(layout);
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
    layout: LayoutAttr,
    children: Vec<Box<dyn Widget>>,
    listeners: Vec<Listener>,
    /// 视觉样式自定义。
    paint: Option<PaintStyle>,
}
impl Row {
    pub fn new() -> Self {
        Self {
            layout: LayoutAttr::new().gap(4.0).align_items(FlexAlign::Center),
            children: Vec::new(),
            listeners: Vec::new(),
            paint: None,
        }
    }
    pub fn child(mut self, c: impl Widget + 'static) -> Self {
        self.children.push(Box::new(c));
        self
    }
    /// 子项间距。
    pub fn spacing(mut self, s: f32) -> Self {
        self.layout = self.layout.gap(s);
        self
    }
    /// 用完整布局属性（builder 式）设置本 widget 的布局。
    pub fn layout(mut self, l: LayoutAttr) -> Self {
        self.layout = l;
        self
    }
    pub fn width(mut self, v: f32) -> Self {
        self.layout = self.layout.width(v);
        self
    }
    pub fn height(mut self, v: f32) -> Self {
        self.layout = self.layout.height(v);
        self
    }
    pub fn justify_content(mut self, j: FlexAlign) -> Self {
        self.layout = self.layout.justify_content(j);
        self
    }
    pub fn align_items(mut self, a: FlexAlign) -> Self {
        self.layout = self.layout.align_items(a);
        self
    }
    pub fn expand(mut self, v: bool) -> Self {
        self.layout = self.layout.expand(v);
        self
    }
    pub fn flex_shrink(mut self, v: f32) -> Self {
        self.layout = self.layout.flex_shrink(v);
        self
    }
    pub fn margin(mut self, v: f32) -> Self {
        self.layout = self.layout.margin(v);
        self
    }
    pub fn center(mut self) -> Self {
        self.layout = self
            .layout
            .justify_content(FlexAlign::Center)
            .align_items(FlexAlign::Center)
            .expand(true);
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

    /// 条件添加子节点。当 `c` 为 `Some` 时将其加入子节点列表。
    pub fn maybe_child<W: Widget + 'static>(mut self, c: Option<W>) -> Self {
        if let Some(c) = c {
            self.children.push(Box::new(c));
        }
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
            .justify_content(self.layout.justify_content.unwrap_or(FlexAlign::Start))
            .align_items(self.layout.align_items.unwrap_or(FlexAlign::Center))
            .gap(self.layout.gap)
            .wrap(FlexWrap::NoWrap);
        layout = self.layout.apply(layout);
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
