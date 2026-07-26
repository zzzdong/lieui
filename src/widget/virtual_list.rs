use std::rc::Rc;
use std::sync::Arc;

use crate::event::Event;
use crate::event::EventContext;
use crate::layout::style::FlexStyle;
use crate::state::State;
use crate::theme::current;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::PaintStyle;
use crate::widget::{BuildContext, Widget};

/// 等高分项的虚拟滚动列表。
///
/// 只构建视口内可见的窗口 `[first, last)` 子项，配合 `clip` 视口裁剪与
/// 内层 `margin_top` 偏移实现滚动，避免一次性布局全部数据（万级条目也不卡）。
///
/// - `height`：视口高度（固定）。
/// - `item_count` / `item_height`：数据总量与每项高度，决定可滚动总高。
/// - `scroll_y`：外部 `State<f32>`，由滚轮事件更新并钳制在 `[0, total - height]`。
/// - `item`：按索引生成子控件的工厂。
pub struct VirtualList {
    height: f32,
    item_count: usize,
    item_height: f32,
    scroll_y: State<f32>,
    item: Arc<dyn Fn(usize) -> Box<dyn Widget>>,
    overscan: usize,
}

impl VirtualList {
    pub fn new(
        height: f32,
        item_count: usize,
        item_height: f32,
        scroll_y: State<f32>,
    ) -> Self {
        Self {
            height,
            item_count,
            item_height,
            scroll_y,
            item: Arc::new(|_| Box::new(Empty)),
            overscan: 3,
        }
    }
    pub fn item(mut self, f: impl Fn(usize) -> Box<dyn Widget> + 'static) -> Self {
        self.item = Arc::new(f);
        self
    }
    pub fn overscan(mut self, n: usize) -> Self {
        self.overscan = n;
        self
    }
}

/// 未设置 `item` 工厂时的占位控件。
struct Empty;

impl Widget for Empty {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        ViewNode::Div {
            layout: FlexStyle::default(),
            paint: PaintStyle::new(),
            children: vec![],
            listeners: vec![],
            key: None,
        }
    }

}

impl Widget for VirtualList {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let sy = *self.scroll_y.get();
        let ih = self.item_height;
        let h = self.height;
        let count = self.item_count;
        let total = count as f32 * ih;
        let max_scroll = (total - h).max(0.0);

        let first = if ih > 0.0 {
            (((sy / ih).floor() as usize).saturating_sub(self.overscan)).min(count)
        } else {
            0
        };
        let last = if ih > 0.0 {
            (((sy + h) / ih).ceil() as usize + self.overscan).min(count)
        } else {
            count
        };
        let offset = first as f32 * ih - sy;

        let mut items = Vec::with_capacity(last.saturating_sub(first));
        for i in first..last {
            let row_widget = (*self.item)(i);
            let node = ctx.child(i, &*row_widget);
            items.push(ViewNode::Div {
                layout: FlexStyle::default().height(ih),
                paint: PaintStyle::new(),
                children: vec![node],
                listeners: vec![],
                key: Some(format!("row-{i}")),
            });
        }

        let value = self.scroll_y.clone();
        let on_wheel = Rc::new(move |ctx: &mut EventContext| {
            if let Some(Event::MouseWheel { delta_y, .. }) = ctx.event() {
                let dy = *delta_y;
                value.update(|v| *v = (*v + dy).clamp(0.0, max_scroll));
            }
        });

        ViewNode::Div {
            layout: FlexStyle::default().height(h),
                paint: PaintStyle::new()
                    .background(current().background.primary_default)
                    .clip(true),
            children: vec![ViewNode::Div {
                layout: FlexStyle::column().margin_top(offset),
                paint: PaintStyle::new(),
                children: items,
                listeners: vec![Listener::on_mouse_wheel(on_wheel)],
                key: Some("content".into()),
            }],
            listeners: vec![],
            key: None,
        }
    }


}
