use std::rc::Rc;

use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::state::State;
use crate::theme::current;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::{PaintStyle, TextStyle};
use crate::widget::{BuildContext, Widget};

/// 单选组。选中索引由外部 `State<usize>` 持有。
///
/// 每个选项是一行（圆点 + 文本），点击设置选中索引；圆点在选中时填充品牌色。
#[derive(Clone)]
pub struct Radio {
    value: State<usize>,
    options: Vec<String>,
    selected_color: Color,
    text_color: Color,
    on_change: Option<Rc<dyn Fn(usize)>>,
}

impl Radio {
    pub fn new(value: State<usize>) -> Self {
        Self {
            value,
            options: Vec::new(),
            selected_color: current().background.brand_default,
            text_color: current().text.regular_default,
            on_change: None,
        }
    }
    pub fn option(mut self, label: impl Into<String>) -> Self {
        self.options.push(label.into());
        self
    }
    pub fn selected_color(mut self, c: Color) -> Self {
        self.selected_color = c;
        self
    }
    pub fn text_color(mut self, c: Color) -> Self {
        self.text_color = c;
        self
    }
    /// 选中变化回调（点击选项切换时触发，参数为选中索引）。
    pub fn on_change<F: Fn(usize) + 'static>(mut self, f: F) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl Widget for Radio {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        let selected = *self.value.get();
        let value = self.value.clone();
        let on_change = self.on_change.clone();

        let mut children = Vec::with_capacity(self.options.len());
        for (i, opt) in self.options.iter().enumerate() {
            let is_sel = i == selected;
            let value_i = value.clone();
            let on_change = on_change.clone();
            let on_click = Rc::new(move || {
                value_i.set(i);
                if let Some(cb) = &on_change {
                    cb(i);
                }
            });

            let dot = ViewNode::Div {
                layout: FlexStyle::default().width(18.0).height(18.0),
                paint: PaintStyle::new()
                    .background(if is_sel {
                        self.selected_color
                    } else {
                        Color::WHITE
                    })
                    .radius(9.0)
                    .border(2.0, self.selected_color),
                children: vec![],
                listeners: vec![],
                key: None,
            };

            let label = ViewNode::Text {
                content: opt.clone(),
                style: TextStyle {
                    font_size: 14.0,
                    color: self.text_color,
                    ..Default::default()
                },
                layout: FlexStyle::default(),
                key: None,
                listeners: vec![],
            };

            let row = FlexStyle::row()
                .align_items(FlexAlign::Center)
                .gap(8.0)
                .padding_all(4.0);

            children.push(ViewNode::Div {
                layout: row,
                paint: PaintStyle::new().hover_background(current().background.secondary_default),
                children: vec![dot, label],
                listeners: vec![Listener::on_click(on_click).builtin()],
                key: Some(format!("opt-{i}")),
            });
        }

        ViewNode::Div {
            layout: FlexStyle::column().gap(4.0),
            paint: PaintStyle::new(),
            children,
            listeners: vec![],
            key: None,
        }
    }
}
