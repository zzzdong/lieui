use std::rc::Rc;

use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::state::State;
use crate::theme::current;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::{PaintStyle, TextStyle};
use crate::widget::{BuildContext, Widget};

/// 选项卡（标签页）控件：顶部一排标签头，下方显示当前激活标签的内容。
///
/// 激活索引由 `State<usize>` 驱动，点击标签头即切换。
pub struct Tab {
    active: State<usize>,
    tabs: Vec<(String, Box<dyn Widget>)>,
    header_height: f32,
}

impl Tab {
    /// `active` 为当前选中标签的索引（0 起）。
    pub fn new(active: State<usize>) -> Self {
        Self {
            active,
            tabs: Vec::new(),
            header_height: 36.0,
        }
    }

    /// 追加一个标签。`content` 可以是任意控件，仅在激活时参与布局。
    pub fn tab(mut self, title: impl Into<String>, content: impl Widget + 'static) -> Self {
        self.tabs.push((title.into(), Box::new(content)));
        self
    }

    /// 设置标签头高度（默认 36）。
    pub fn header_height(mut self, h: f32) -> Self {
        self.header_height = h;
        self
    }
}

impl Widget for Tab {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let active = *self.active.get();
        let hh = self.header_height;
        let value = self.active.clone();

        let mut header_children = Vec::with_capacity(self.tabs.len());
        for (i, (title, _)) in self.tabs.iter().enumerate() {
            let is_active = i == active;
            let value_i = value.clone();
            let on_click = Rc::new(move || {
                value_i.set(i);
            });

            header_children.push(ViewNode::Div {
                layout: FlexStyle::default()
                    .height(hh)
                    .padding_left(16.0)
                    .padding_right(16.0)
                    .align_items(FlexAlign::Center)
                    .justify_content(FlexAlign::Center),
                paint: PaintStyle::new().background(if is_active {
                    current().background.brand_default
                } else {
                    Color::TRANSPARENT
                }),
                children: vec![ViewNode::Text {
                    content: title.clone(),
                    style: TextStyle {
                        font_size: 14.0,
                        color: if is_active {
                            Color::WHITE
                        } else {
                            current().text.regular_default
                        },
                        ..Default::default()
                    },
                    layout: FlexStyle::default(),
                    key: None,
                    listeners: vec![],
                }],
                listeners: vec![Listener::on_click(on_click)],
                key: Some(format!("tab-{i}")),
            });
        }

        let header = ViewNode::Div {
            layout: FlexStyle::row().align_items(FlexAlign::Center).height(hh),
            paint: PaintStyle::new(),
            children: header_children,
            listeners: vec![],
            key: Some("header".into()),
        };

        let separator = ViewNode::Div {
            layout: FlexStyle::default().height(1.0),
            paint: PaintStyle::new().background(current().border.default),
            children: vec![],
            listeners: vec![],
            key: Some("sep".into()),
        };

        let content = if let Some((_, widget)) = self.tabs.get(active) {
            ctx.child(active, widget.as_ref())
        } else {
            ViewNode::Div {
                layout: FlexStyle::default(),
                paint: PaintStyle::new(),
                children: vec![],
                listeners: vec![],
                key: None,
            }
        };

        let content_area = ViewNode::Div {
            layout: FlexStyle::default()
                .padding_all(12.0)
                .flex_grow(1.0),
            paint: PaintStyle::new(),
            children: vec![content],
            listeners: vec![],
            key: Some("content".into()),
        };

        ViewNode::Div {
            layout: FlexStyle::column().flex_grow(1.0),
            paint: PaintStyle::new()
                .background(current().background.primary_default)
                .border(1.0, current().border.default)
                .radius(current().radius.medium),
            children: vec![header, separator, content_area],
            listeners: vec![],
            key: None,
        }
    }
}
