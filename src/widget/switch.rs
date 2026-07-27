use std::rc::Rc;

use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::state::State;
use crate::theme::current;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::{PaintStyle, TextStyle};
use crate::widget::{BuildContext, Widget};

/// 开关（Toggle）。选中状态由外部 `State<bool>` 持有，点击切换。
///
/// 容器为相对定位，圆形手柄以绝对定位水平滑动到开启/关闭位置。
#[derive(Clone)]
pub struct Switch {
    value: State<bool>,
    on_color: Color,
    off_color: Color,
    knob_color: Color,
    width: f32,
    height: f32,
    label: Option<String>,
}

impl Switch {
    pub fn new(value: State<bool>) -> Self {
        Self {
            value,
            on_color: current().background.brand_default,
            off_color: current().border.default,
            knob_color: Color::WHITE,
            width: 44.0,
            height: 24.0,
            label: None,
        }
    }
    /// 在开关右侧显示文本标签。
    pub fn label(mut self, s: impl Into<String>) -> Self {
        self.label = Some(s.into());
        self
    }
    pub fn on_color(mut self, c: Color) -> Self {
        self.on_color = c;
        self
    }
    pub fn off_color(mut self, c: Color) -> Self {
        self.off_color = c;
        self
    }
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }
    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }
}

impl Widget for Switch {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        let on = *self.value.get();
        let w = self.width;
        let h = self.height;
        let knob = h - 4.0;
        let knob_left = if on { w - knob - 2.0 } else { 2.0 };

        let value = self.value.clone();
        let on_click = Rc::new(move || {
            value.update(|v| *v = !*v);
        });

        let knob_style = FlexStyle::default()
            .absolute()
            .position_top(2.0)
            .position_left(knob_left)
            .width(knob)
            .height(knob);

        let toggle = ViewNode::Div {
            layout: FlexStyle::default().width(w).height(h),
            paint: PaintStyle::new()
                .background(if on { self.on_color } else { self.off_color })
                .radius(h / 2.0),
            children: vec![ViewNode::Div {
                layout: knob_style,
                paint: PaintStyle::new()
                    .background(self.knob_color)
                    .radius(knob / 2.0),
                children: vec![],
                listeners: vec![],
                key: None,
            }],
            listeners: vec![Listener::on_click(on_click)],
            key: None,
        };

        match &self.label {
            Some(text) => ViewNode::Div {
                layout: FlexStyle::row()
                    .align_items(FlexAlign::Center)
                    .gap(8.0),
                paint: PaintStyle::new(),
                children: vec![
                    toggle,
                    ViewNode::Text {
                        content: text.clone(),
                        style: TextStyle {
                            font_size: 14.0,
                            color: current().text.regular_default,
                            ..Default::default()
                        },
                        layout: FlexStyle::default(),
                        key: None,
                        listeners: vec![],
                    },
                ],
                listeners: vec![],
                key: None,
            },
            None => toggle,
        }
    }
}
