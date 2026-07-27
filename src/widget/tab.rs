use std::rc::Rc;

use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::state::State;
use crate::theme::current;
use crate::view::paint::{PaintStyle, TextStyle};
use crate::view::FontWeight;
use crate::view::node::{Listener, ViewNode};
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
        let t = current();
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

            // ── 选中指示器：底部品牌色圆角条 ──
            let children = if is_active {
                vec![
                    // 顶部弹簧：将文字下压到居中
                    ViewNode::Div {
                        layout: FlexStyle::default().flex_grow(1.0),
                        paint: PaintStyle::new(),
                        children: vec![],
                        listeners: vec![],
                        key: None,
                    },
                    ViewNode::Text {
                        content: title.clone(),
                        style: TextStyle {
                            font_size: 13.0,
                            color: t.text.brand_default,
                            font_weight: FontWeight::Medium,
                            ..Default::default()
                        },
                        layout: FlexStyle::default(),
                        key: None,
                        listeners: vec![],
                    },
                    // 底部弹簧：吸取文字下方空间后让指示条贴底
                    ViewNode::Div {
                        layout: FlexStyle::default().flex_grow(1.0),
                        paint: PaintStyle::new(),
                        children: vec![],
                        listeners: vec![],
                        key: None,
                    },
                    // 品牌色指示条（铺满 tab 宽度，底部横线）
                    ViewNode::Div {
                        layout: FlexStyle::default()
                            .height(3.0)
                            .flex_shrink(0.0)
                            .align_self(FlexAlign::Stretch),
                        paint: PaintStyle::new()
                            .background(t.background.brand_default),
                        children: vec![],
                        listeners: vec![],
                        key: Some("__accent__".into()),
                    },
                ]
            } else {
                vec![
                    ViewNode::Div {
                        layout: FlexStyle::default().flex_grow(1.0),
                        paint: PaintStyle::new(),
                        children: vec![],
                        listeners: vec![],
                        key: None,
                    },
                    ViewNode::Text {
                        content: title.clone(),
                        style: TextStyle {
                            font_size: 13.0,
                            color: t.text.regular_default,
                            font_weight: FontWeight::Normal,
                            ..Default::default()
                        },
                        layout: FlexStyle::default(),
                        key: None,
                        listeners: vec![],
                    },
                    ViewNode::Div {
                        layout: FlexStyle::default().flex_grow(1.0),
                        paint: PaintStyle::new(),
                        children: vec![],
                        listeners: vec![],
                        key: None,
                    },
                ]
            };

            header_children.push(ViewNode::Div {
                layout: FlexStyle::column()
                    .height(hh)
                    .padding_left(14.0)
                    .padding_right(14.0)
                    .align_items(FlexAlign::Center),
                paint: PaintStyle::new(),
                children,
                listeners: vec![Listener::on_click(on_click)],
                key: Some(format!("tab-{i}")),
            });
        }

        let header = ViewNode::Div {
            layout: FlexStyle::row()
                .flex_shrink(0.0)
                .height(hh)
                .padding_left(4.0)
                .padding_right(4.0),
            paint: PaintStyle::new(),
            children: header_children,
            listeners: vec![],
            key: Some("header".into()),
        };

        let separator = ViewNode::Div {
            layout: FlexStyle::default().height(1.0),
            paint: PaintStyle::new().background(t.border.default),
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
                .background(t.background.primary_default)
                .border(1.0, t.border.default)
                .radius(t.radius.medium)
                .clip(true),
            children: vec![header, separator, content_area],
            listeners: vec![],
            key: None,
        }
    }
}
