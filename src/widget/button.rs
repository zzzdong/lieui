use crate::layout::box_model::{BoxStyle, EdgeInsets};
use crate::layout::flex::{AlignItems, FlexDirection, FlexStyle, JustifyContent};
use crate::state;
use crate::theme;
use crate::view::node::{ClickCallbackRef, DisplayMode, ViewNode};
use crate::view::View;

pub struct Button {
    label: String,
    callback_id: Option<u64>,
}

impl Button {
    pub fn new(l: impl Into<String>) -> Self {
        Self {
            label: l.into(),
            callback_id: None,
        }
    }

    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.callback_id = Some(state::register_click(Box::new(f)));
        self
    }
}

impl View for Button {
    fn build(&self) -> ViewNode {
        let t = theme::current();
        let label_node = ViewNode::Text {
            content: self.label.clone(),
            font_size: 13.0,
            color: t.text.on_brand_default,
            key: None,
            listener: None,
        };
        let content = ViewNode::Div {
            style: BoxStyle {
                expand: true,
                ..BoxStyle::default()
            },
            flex: FlexStyle {
                direction: FlexDirection::Row,
                justify: JustifyContent::Center,
                align: AlignItems::Center,
                spacing: 0.0,
                expand: true,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                wrap: crate::layout::flex::FlexWrap::NoWrap,
            },
            display: DisplayMode::Flex,
            key: None,
            children: vec![label_node],
            listener: None,
        };
        ViewNode::Div {
            style: BoxStyle {
                padding: EdgeInsets::new(t.spacer.md, t.spacer.md, 6.0, 6.0),
                background_color: Some(t.background.brand_default),
                hover_background: Some(t.background.brand_hover),
                pressed_background: Some(t.background.brand_clicked),
                border_radius: t.radius.small,
                ..BoxStyle::default()
            },
            flex: FlexStyle::default(),
            display: DisplayMode::Block,
            key: None,
            children: vec![content],
            listener: self.callback_id.map(ClickCallbackRef::Simple),
        }
    }
}
