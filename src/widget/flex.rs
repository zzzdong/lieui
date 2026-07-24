//! Column / Row — 映射到 `ViewNode::Div`（Flex 模式）的布局型组件

use crate::event::EventContext;
use crate::layout::box_model::BoxStyle;
use crate::layout::flex::{AlignItems, FlexDirection, FlexStyle, FlexWrap, JustifyContent};
use crate::state;
use crate::view::node::{ClickCallbackRef, DisplayMode, ViewNode};
use crate::view::View;

// ===== Column (map → ViewNode::Flex, direction = Column) =====
pub struct Column {
    spacing: f32,
    justify: JustifyContent,
    align: AlignItems,
    expand: bool,
    children: Vec<Box<dyn View>>,
    on_click: Option<ClickCallbackRef>,
}
impl Column {
    pub fn new() -> Self {
        Self {
            spacing: 4.0,
            justify: JustifyContent::Start,
            align: AlignItems::Stretch,
            expand: false,
            children: Vec::new(),
            on_click: None,
        }
    }
    pub fn child(mut self, c: impl View + 'static) -> Self {
        self.children.push(Box::new(c));
        self
    }
    pub fn spacing(mut self, s: f32) -> Self {
        self.spacing = s;
        self
    }
    pub fn justify_content(mut self, j: JustifyContent) -> Self {
        self.justify = j;
        self
    }
    pub fn align_items(mut self, a: AlignItems) -> Self {
        self.align = a;
        self
    }
    pub fn expand(mut self, v: bool) -> Self {
        self.expand = v;
        self
    }
    pub fn center(mut self) -> Self {
        self.justify = JustifyContent::Center;
        self.align = AlignItems::Center;
        self.expand = true;
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
impl Default for Column {
    fn default() -> Self {
        Self::new()
    }
}
impl View for Column {
    fn build(&self) -> ViewNode {
        ViewNode::Div {
            style: BoxStyle {
                expand: self.expand,
                ..BoxStyle::default()
            },
            flex: FlexStyle {
                direction: FlexDirection::Column,
                justify: self.justify,
                align: self.align,
                spacing: self.spacing,
                expand: self.expand,
                flex_grow: 0.0,
                flex_shrink: 1.0,
                wrap: FlexWrap::NoWrap,
            },
            display: DisplayMode::Flex,
            key: None,
            children: self.children.iter().map(|c| c.build()).collect(),
            listener: self.on_click,
        }
    }
}

// ===== Row (map → ViewNode::Flex, direction = Row) =====
pub struct Row {
    spacing: f32,
    justify: JustifyContent,
    align: AlignItems,
    expand: bool,
    children: Vec<Box<dyn View>>,
    on_click: Option<ClickCallbackRef>,
}
impl Row {
    pub fn new() -> Self {
        Self {
            spacing: 4.0,
            justify: JustifyContent::Start,
            align: AlignItems::Center,
            expand: false,
            children: Vec::new(),
            on_click: None,
        }
    }
    pub fn child(mut self, c: impl View + 'static) -> Self {
        self.children.push(Box::new(c));
        self
    }
    pub fn spacing(mut self, s: f32) -> Self {
        self.spacing = s;
        self
    }
    pub fn justify_content(mut self, j: JustifyContent) -> Self {
        self.justify = j;
        self
    }
    pub fn align_items(mut self, a: AlignItems) -> Self {
        self.align = a;
        self
    }
    pub fn expand(mut self, v: bool) -> Self {
        self.expand = v;
        self
    }
    pub fn center(mut self) -> Self {
        self.justify = JustifyContent::Center;
        self.align = AlignItems::Center;
        self.expand = true;
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
impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}
impl View for Row {
    fn build(&self) -> ViewNode {
        ViewNode::Div {
            style: BoxStyle {
                expand: self.expand,
                ..BoxStyle::default()
            },
            flex: FlexStyle {
                direction: FlexDirection::Row,
                justify: self.justify,
                align: self.align,
                spacing: self.spacing,
                expand: self.expand,
                flex_grow: 0.0,
                flex_shrink: 1.0,
                wrap: FlexWrap::NoWrap,
            },
            display: DisplayMode::Flex,
            key: None,
            children: self.children.iter().map(|c| c.build()).collect(),
            listener: self.on_click,
        }
    }
}
