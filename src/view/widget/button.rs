use crate::geometry::Color;
use crate::layout::box_model::BoxStyle;
use crate::layout::flex::{AlignItems, FlexDirection, JustifyContent};
use crate::state;
use crate::view::node::ViewNode;
use crate::view::View;

pub struct Button {
    label: String,
    callback_id: Option<u64>,
}

impl Button {
    pub fn new(l: impl Into<String>) -> Self {
        Self { label: l.into(), callback_id: None }
    }

    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.callback_id = Some(state::register_click(Box::new(f)));
        self
    }
}

impl View for Button {
    fn build(&self) -> ViewNode {
        let label_node = ViewNode::Text {
            content: self.label.clone(), font_size: 13.0, color: Color::WHITE, key: None,
        };
        let content = ViewNode::Flex {
            direction: FlexDirection::Row,
            justify: JustifyContent::Center,
            align: AlignItems::Center,
            spacing: 0.0, expand: false,
            flex_grow: 0.0, flex_shrink: 1.0,
            wrap: crate::layout::flex::FlexWrap::NoWrap,
            key: None,
            children: vec![label_node],
        };
        let box_node = ViewNode::Box {
            style: BoxStyle {
                padding: crate::layout::box_model::EdgeInsets::new(6.0, 6.0, 4.0, 4.0),
                background_color: Some(Color::new(66, 133, 244)),
                hover_background: Some(Color::new(50, 110, 220)),
                pressed_background: Some(Color::new(40, 90, 190)),
                border_radius: 6.0,
                ..BoxStyle::default()
            },
            key: None,
            children: vec![content],
        };
        ViewNode::Listener {
            on_click: self.callback_id,
            key: None,
            child: Box::new(box_node),
        }
    }
}
