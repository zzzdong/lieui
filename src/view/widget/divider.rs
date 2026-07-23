use crate::geometry::Color;
use crate::layout::box_model::BoxStyle;
use crate::view::node::ViewNode;
use crate::view::View;

pub struct Divider;

impl View for Divider {
    fn build(&self) -> ViewNode {
        ViewNode::Box {
            style: BoxStyle {
                fixed_height: Some(1.0), expand: true,
                background_color: Some(Color::new(200, 200, 200)),
                ..BoxStyle::default()
            },
            key: None, children: vec![],
        }
    }
}
