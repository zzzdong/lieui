use crate::geometry::Color;
use crate::layout::box_model::{BoxStyle, EdgeInsets};
use crate::state::State;
use crate::view::node::ViewNode;
use crate::view::View;

/// 带滚动样式的高容器
///
/// 内部用负 margin-top 模拟滚动偏移。
pub struct ListView {
    height: f32,
    scroll_y: f32,
    child: Option<Box<dyn View>>,
}

impl ListView {
    pub fn new(height: f32, scroll_y: &State<f32>) -> Self {
        Self { height, scroll_y: *scroll_y.get(), child: None }
    }

    pub fn child(mut self, v: impl View + 'static) -> Self {
        self.child = Some(Box::new(v));
        self
    }
}

impl View for ListView {
    fn build(&self) -> ViewNode {
        let inner = match &self.child {
            Some(c) => c.build(),
            None => ViewNode::Box {
                style: BoxStyle::default(), key: None, children: vec![],
            },
        };

        // 负 margin-top 将内容上移
        let offset_box = ViewNode::Box {
            style: BoxStyle {
                margin: EdgeInsets::new(0.0, -self.scroll_y, 0.0, 0.0),
                ..BoxStyle::default()
            },
            key: None,
            children: vec![inner],
        };

        ViewNode::Box {
            style: BoxStyle {
                fixed_height: Some(self.height),
                background_color: Some(Color::new(248, 248, 248)),
                ..BoxStyle::default()
            },
            key: None,
            children: vec![offset_box],
        }
    }
}
