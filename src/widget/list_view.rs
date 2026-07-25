use crate::layout::style::FlexStyle;
use crate::state::State;
use crate::theme;
use crate::view::node::ViewNode;
use crate::view::paint::PaintStyle;
use crate::widget::{BuildContext, Widget};

/// 带滚动样式的高容器
///
/// 内部用负 margin-top 模拟滚动偏移。
pub struct ListView {
    height: Option<f32>,
    expand: bool,
    scroll_y: State<f32>,
    child: Option<Box<dyn Widget>>,
}

impl ListView {
    pub fn new(height: f32, scroll_y: &State<f32>) -> Self {
        Self {
            height: Some(height),
            expand: false,
            scroll_y: scroll_y.clone(),
            child: None,
        }
    }

    /// 让 ListView 高度跟随父容器可用空间自适应撑满。
    pub fn expand(scroll_y: &State<f32>) -> Self {
        Self {
            height: None,
            expand: true,
            scroll_y: scroll_y.clone(),
            child: None,
        }
    }

    pub fn child(mut self, v: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(v));
        self
    }
}

impl Widget for ListView {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let t = theme::current();
        let inner = match &self.child {
            Some(c) => ctx.child(0, c.as_ref()),
            None => ViewNode::Div {
                layout: FlexStyle::block(),
                paint: PaintStyle::default(),
                key: None,
                children: vec![],
                listeners: vec![],
            },
        };

        let scroll_y = *self.scroll_y.get();
        // 负 margin-top 将内容上移
        let offset_box = ViewNode::Div {
            layout: FlexStyle::block().margin_top(-scroll_y),
            paint: PaintStyle::default(),
            key: None,
            children: vec![inner],
            listeners: vec![],
        };

        let mut layout = FlexStyle::block().flex_grow(1.0).flex_shrink(1.0);
        if let Some(h) = self.height {
            layout = layout.height(h);
        } else if self.expand {
            layout = layout.flex_grow(1.0);
        }

        ViewNode::Div {
            layout,
            paint: PaintStyle::new().background(t.background.secondary_default),
            key: None,
            children: vec![offset_box],
            listeners: vec![],
        }
    }
}
