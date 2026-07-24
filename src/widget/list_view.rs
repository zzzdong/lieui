use crate::layout::box_model::{BoxStyle, EdgeInsets};
use crate::layout::flex::FlexStyle;
use crate::state::State;
use crate::theme;
use crate::view::node::{DisplayMode, ViewNode};
use crate::view::View;

/// 带滚动样式的高容器
///
/// 内部用负 margin-top 模拟滚动偏移。
pub struct ListView {
    height: Option<f32>,
    expand: bool,
    scroll_y: State<f32>,
    child: Option<Box<dyn View>>,
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

    pub fn child(mut self, v: impl View + 'static) -> Self {
        self.child = Some(Box::new(v));
        self
    }
}

impl View for ListView {
    fn build(&self) -> ViewNode {
        let t = theme::current();
        let inner = match &self.child {
            Some(c) => c.build(),
            None => ViewNode::Div {
                style: BoxStyle::default(),
                flex: FlexStyle::default(),
                display: DisplayMode::Block,
                key: None,
                children: vec![],
                listener: None,
                interactive: false,
            },
        };

        let scroll_y = *self.scroll_y.get();
        // 负 margin-top 将内容上移
        let offset_box = ViewNode::Div {
            style: BoxStyle {
                margin: EdgeInsets::new(0.0, -scroll_y, 0.0, 0.0),
                ..BoxStyle::default()
            },
            flex: FlexStyle::default(),
            display: DisplayMode::Block,
            key: None,
            children: vec![inner],
            listener: None,
            interactive: false,
        };

        let mut style = BoxStyle {
            background_color: Some(t.background.secondary_default),
            ..BoxStyle::default()
        };
        if let Some(h) = self.height {
            style.fixed_height = Some(h);
        } else if self.expand {
            style.expand = true;
        }

        ViewNode::Div {
            style,
            flex: FlexStyle::default(),
            display: DisplayMode::Block,
            key: None,
            children: vec![offset_box],
            listener: None,
            interactive: false,
        }
    }
}
