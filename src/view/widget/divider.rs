use crate::layout::box_model::BoxStyle;
use crate::layout::flex::FlexStyle;
use crate::view::design_tokens::semantic as t;
use crate::view::node::{DisplayMode, ViewNode};
use crate::view::View;

pub struct Divider;

impl View for Divider {
    fn build(&self) -> ViewNode {
        ViewNode::Div {
            style: BoxStyle {
                fixed_height: Some(1.0),
                expand: true,
                background_color: Some(t::BORDER_DEFAULT),
                ..BoxStyle::default()
            },
            flex: FlexStyle::default(),
            display: DisplayMode::Block,
            key: None,
            children: vec![],
            listener: None,
        }
    }
}
