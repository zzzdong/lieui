use crate::layout::box_model::BoxStyle;
use crate::layout::flex::FlexStyle;
use crate::theme;
use crate::view::node::{DisplayMode, ViewNode};
use crate::view::View;

pub struct Divider;

impl View for Divider {
    fn build(&self) -> ViewNode {
        let t = theme::current();
        ViewNode::Div {
            style: BoxStyle {
                fixed_height: Some(1.0),
                expand: true,
                background_color: Some(t.border.default),
                ..BoxStyle::default()
            },
            flex: FlexStyle::default(),
            display: DisplayMode::Block,
            key: None,
            children: vec![],
            listener: None,
            interactive: false,
        }
    }
}
