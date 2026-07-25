use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::view::node::ViewNode;
use crate::view::paint::PaintStyle;
use crate::widget::{BuildContext, Widget};

pub struct Divider {
    height: f32,
    background: Option<Color>,
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
}

impl Widget for Divider {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        let t = crate::theme::current();
        ViewNode::Div {
            layout: FlexStyle::block().height(self.height).flex_grow(1.0),
            paint: PaintStyle::new().background(self.background.unwrap_or(t.border.default)),
            key: None,
            children: vec![],
            listeners: vec![],
        }
    }
}
