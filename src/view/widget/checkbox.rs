use crate::layout::box_model::BoxStyle;
use crate::layout::flex::{AlignItems, FlexDirection, FlexStyle, JustifyContent};
use crate::state;
use crate::view::design_tokens::semantic as t;
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
        let check_style = BoxStyle {
            fixed_width: Some(16.0),
            fixed_height: Some(16.0),
            border_radius: t::BORDER_RADIUS_SMALL,
            background_color: Some(if self.checked {
                t::BACKGROUND_BRAND_DEFAULT
            } else {
                t::BACKGROUND_PRIMARY_DEFAULT
            }),
            hover_background: Some(if self.checked {
                t::BACKGROUND_BRAND_HOVER
            } else {
                t::BACKGROUND_SECONDARY_DEFAULT
            }),
            // PatternFly checkbox 默认有 1px 边框
            border_color: Some(t::BORDER_STRONG),
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
                color: t::TEXT_REGULAR_DEFAULT,
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
                spacing: t::SPACER_SM,
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
