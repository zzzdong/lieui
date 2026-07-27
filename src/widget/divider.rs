use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::view::node::ViewNode;
use crate::view::paint::PaintStyle;
use crate::widget::{BuildContext, Widget};

#[derive(Clone)]
pub struct Divider {
    height: f32,
    background: Option<Color>,
    flex_shrink: f32,
}

impl Default for Divider {
    fn default() -> Self {
        Self::new()
    }
}

impl Divider {
    pub fn new() -> Self {
        Self {
            height: 1.0,
            background: None,
            flex_shrink: 1.0,
        }
    }
    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }
    pub fn background(mut self, c: Color) -> Self {
        self.background = Some(c);
        self
    }

    /// 设置 flex 收缩因子（默认 1.0）。
    pub fn flex_shrink(mut self, v: f32) -> Self {
        self.flex_shrink = v;
        self
    }
}

impl Widget for Divider {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        let t = crate::theme::current();
        let mut layout = FlexStyle::block().height(self.height).flex_grow(1.0);
        if self.flex_shrink != 1.0 {
            layout = layout.flex_shrink(self.flex_shrink);
        }
        ViewNode::Div {
            layout,
            paint: PaintStyle::new().background(self.background.unwrap_or(t.border.default)),
            key: None,
            children: vec![],
            listeners: vec![],
        }
    }
}
