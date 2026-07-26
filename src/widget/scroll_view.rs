use std::rc::Rc;

use crate::event::{Event, EventContext};
use crate::layout::style::FlexStyle;
use crate::state::State;
use crate::theme;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::PaintStyle;
use crate::widget::{BuildContext, Widget};

/// 通用滚动容器（ScrollView）。
///
/// 基于 ViewNode 层 engine 滚动：视口标记 `overflow_scroll`，自身按视口尺寸布局，
/// 子节点在内容画布上自然排布；滚动偏移、内容尺寸计算、裁剪与滚轮处理
/// 全部由布局/渲染/事件引擎统一完成。
pub struct ScrollView {
    height: Option<f32>,
    child: Option<Box<dyn Widget>>,
    /// 可选的链接 State。设置后，每次滚轮都会同步更新此 State，
    /// 供 ScrollBar 等外部控件读取滚动偏移。
    linked_scroll_y: Option<State<f32>>,
}

impl ScrollView {
    /// 固定高度视口。
    pub fn new(height: f32) -> Self {
        Self {
            height: Some(height),
            child: None,
            linked_scroll_y: None,
        }
    }

    /// 高度跟随父容器可用空间自适应撑满。
    pub fn expand() -> Self {
        Self {
            height: None,
            child: None,
            linked_scroll_y: None,
        }
    }

    /// 设置要滚动的单个子 widget。
    pub fn child(mut self, v: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(v));
        self
    }

    /// 绑定一个 State，使引擎滚动时同步更新此 State（供 ScrollBar 等使用）。
    pub fn bind_scroll_y(mut self, s: &State<f32>) -> Self {
        self.linked_scroll_y = Some(s.clone());
        self
    }
}

impl Widget for ScrollView {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let t = theme::current();

        let inner = match &self.child {
            Some(c) => ctx.child(0, c.as_ref()),
            None => ViewNode::Div {
                layout: FlexStyle::block(),
                paint: PaintStyle::default(),
                children: vec![],
                listeners: vec![],
                key: None,
            },
        };

        // 可选的监听器：同步更新 linked_scroll_y（供 ScrollBar 等使用）
        let mut listeners: Vec<Listener> = Vec::new();
        if let Some(s) = &self.linked_scroll_y {
            let state = s.clone();
            listeners.push(Listener::on_mouse_wheel(Rc::new(
                move |ctx: &mut EventContext| {
                    if let Some(Event::MouseWheel { delta_y, .. }) = ctx.event() {
                        state.update(|v| *v = (*v + *delta_y).max(0.0));
                    }
                },
            )));
        }

        let viewport = match self.height {
            Some(h) => FlexStyle::block().height(h),
            None => FlexStyle::block().flex_grow(1.0).flex_shrink(1.0),
        }
        .overflow_scroll();

        ViewNode::Div {
            layout: viewport,
            paint: PaintStyle::new()
                .background(t.background.secondary_default)
                .clip(true),
            children: vec![inner],
            listeners,
            key: None,
        }
    }
}
