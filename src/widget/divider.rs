use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::view::node::ViewNode;
use crate::view::paint::PaintStyle;
use crate::widget::layout::LayoutAttr;
use crate::widget::{BuildContext, Widget};

#[derive(Clone)]
pub struct Divider {
    layout: LayoutAttr,
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
            layout: LayoutAttr::new().height(1.0),
            background: None,
        }
    }
    pub fn height(mut self, h: f32) -> Self {
        self.layout = self.layout.height(h);
        self
    }
    /// 用完整布局属性（builder 式）设置本分隔线的布局。
    pub fn layout(mut self, l: LayoutAttr) -> Self {
        self.layout = l;
        self
    }
    pub fn background(mut self, c: Color) -> Self {
        self.background = Some(c);
        self
    }

    /// 设置 flex 收缩因子（默认 1.0）。
    pub fn flex_shrink(mut self, v: f32) -> Self {
        self.layout = self.layout.flex_shrink(v);
        self
    }
}

impl Widget for Divider {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        let t = crate::theme::current();
        let layout = self.layout.apply(FlexStyle::block().flex_grow(1.0));
        ViewNode::Div {
            layout,
            paint: PaintStyle::new().background(self.background.unwrap_or(t.border.default)),
            key: None,
            children: vec![],
            listeners: vec![],
        }
    }
}
