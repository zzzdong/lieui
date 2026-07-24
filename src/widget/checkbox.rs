use crate::layout::box_model::BoxStyle;
use crate::layout::flex::{AlignItems, FlexDirection, FlexStyle, JustifyContent};
use crate::state;
use crate::theme;
use crate::view::node::{ClickCallbackRef, DisplayMode, ViewNode};
use crate::view::View;

pub struct Checkbox {
    checked: bool,
    label: String,
    callback_id: Option<u64>,
}

impl Checkbox {
    pub fn new(checked: bool) -> Self {
        Self {
            checked,
            label: String::new(),
            callback_id: None,
        }
    }

    pub fn label(mut self, s: impl Into<String>) -> Self {
        self.label = s.into();
        self
    }

    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.callback_id = Some(state::register_click(Box::new(f)));
        self
    }
}

impl View for Checkbox {
    fn build(&self) -> ViewNode {
        let t = theme::current();
        let check_style = BoxStyle {
            fixed_width: Some(16.0),
            fixed_height: Some(16.0),
            border_radius: t.radius.small,
            background_color: Some(if self.checked {
                t.background.brand_default
            } else {
                t.background.primary_default
            }),
            hover_background: Some(if self.checked {
                t.background.brand_hover
            } else {
                t.background.secondary_default
            }),
            border_color: Some(t.border.strong),
            border_width: 1.0,
            ..BoxStyle::default()
        };
        let check_box = ViewNode::Div {
            style: check_style,
            flex: FlexStyle::default(),
            display: DisplayMode::Block,
            key: None,
            children: vec![],
            listener: None,
        };

        let mut children = vec![check_box];
        if !self.label.is_empty() {
            children.push(ViewNode::Text {
                content: self.label.clone(),
                font_size: 13.0,
                color: t.text.regular_default,
                key: None,
                listener: None,
            });
        }
        ViewNode::Div {
            style: BoxStyle::default(),
            flex: FlexStyle {
                direction: FlexDirection::Row,
                justify: JustifyContent::Start,
                align: AlignItems::Center,
                spacing: t.spacer.sm,
                expand: false,
                flex_grow: 0.0,
                flex_shrink: 1.0,
                wrap: crate::layout::flex::FlexWrap::NoWrap,
            },
            display: DisplayMode::Flex,
            key: None,
            children,
            listener: self.callback_id.map(ClickCallbackRef::Simple),
        }
    }
}
