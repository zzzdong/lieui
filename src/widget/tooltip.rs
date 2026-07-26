use std::rc::Rc;

use crate::event::EventContext;
use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::{PaintStyle, TextStyle};
use crate::widget::{BuildContext, Widget};

/// 工具提示。包裹一个子控件，鼠标移入时在其上方浮出提示气泡。
///
/// 容器为相对定位，提示气泡以绝对定位浮于子控件上方；作为最后一个子节点绘制，
/// 自然覆盖在子控件之上。鼠标进入/离开通过 `use_state` 记录的悬停态驱动重建。
pub struct Tooltip {
    child: Box<dyn Widget>,
    tip: String,
}

impl Tooltip {
    pub fn new(child: Box<dyn Widget>, tip: impl Into<String>) -> Self {
        Self {
            child,
            tip: tip.into(),
        }
    }
}

impl Widget for Tooltip {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let hovered = ctx.use_state::<bool>(|| false);
        let h = hovered.clone();
        let h2 = hovered.clone();

        let on_enter = Rc::new(move |_ctx: &mut EventContext| {
            h.set(true);
        });
        let on_leave = Rc::new(move |_ctx: &mut EventContext| {
            h2.set(false);
        });

        let child_node = ctx.child(0, &*self.child);
        let mut children = vec![child_node];

        if *hovered.get() {
            let tip_node = ViewNode::Div {
                layout: FlexStyle::default()
                    .absolute()
                    .position_top(-30.0)
                    .position_left(0.0)
                    .padding_all(6.0)
                    .align_items(FlexAlign::Center)
                    .justify_content(FlexAlign::Center),
                paint: PaintStyle::new()
                    .background(Color::from_hex("#222222"))
                    .radius(crate::theme::current().radius.small),
                children: vec![ViewNode::Text {
                    content: self.tip.clone(),
                    style: TextStyle {
                        font_size: 12.0,
                        color: Color::WHITE,
                        ..Default::default()
                    },
                    layout: FlexStyle::default(),
                    key: None,
                    listeners: vec![],
                }],
                listeners: vec![],
                key: Some("tip".into()),
            };
            children.push(tip_node);
        }

        ViewNode::Div {
            layout: FlexStyle::default(),
            paint: PaintStyle::new(),
            children,
            listeners: vec![Listener::on_mouse_enter(on_enter), Listener::on_mouse_leave(on_leave)],
            key: None,
        }
    }
}
