use crate::geometry::Color;
use crate::layout::box_model::BoxStyle;
use crate::layout::flex::{AlignItems, FlexDirection, JustifyContent};
use crate::state;
use crate::view::node::ViewNode;
use crate::view::View;

pub struct Checkbox {
    checked: bool,
    label: String,
    callback_id: Option<u64>,
}

impl Checkbox {
    pub fn new(checked: bool) -> Self {
        Self { checked, label: String::new(), callback_id: None }
    }

    pub fn label(mut self, s: impl Into<String>) -> Self {
        self.label = s.into(); self
    }

    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.callback_id = Some(state::register_click(Box::new(f)));
        self
    }
}

impl View for Checkbox {
    fn build(&self) -> ViewNode {
        let check_style = BoxStyle {
            fixed_width: Some(16.0), fixed_height: Some(16.0),
            border_radius: 3.0,
            background_color: Some(
                if self.checked { Color::new(66, 133, 244) } else { Color::new(240, 240, 240) },
            ),
            hover_background: Some(
                if self.checked { Color::new(50, 110, 220) } else { Color::new(220, 220, 220) },
            ),
            ..BoxStyle::default()
        };
        let check_box = ViewNode::Box { style: check_style, key: None, children: vec![] };

        let mut children = vec![check_box];
        if !self.label.is_empty() {
            children.push(ViewNode::Text {
                content: self.label.clone(), font_size: 13.0, color: Color::BLACK, key: None,
            });
        }
        let content = ViewNode::Flex {
            direction: FlexDirection::Row,
            justify: JustifyContent::Start,
            align: AlignItems::Center,
            spacing: 8.0, expand: false,
            flex_grow: 0.0, flex_shrink: 1.0,
            wrap: crate::layout::flex::FlexWrap::NoWrap,
            key: None, children,
        };
        ViewNode::Listener {
            on_click: self.callback_id, key: None, child: Box::new(content),
        }
    }
}
