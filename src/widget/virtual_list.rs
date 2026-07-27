use crate::layout::style::FlexStyle;
use crate::state::State;
use crate::theme::current;
use crate::view::node::ViewNode;
use crate::view::paint::PaintStyle;
use crate::widget::{BuildContext, Widget};
use std::sync::Arc;

/// 等高分项的虚拟滚动列表。
///
/// 基于 ViewNode 层 engine 滚动：视口标记 `overflow_scroll` + `content_height`。
/// 只构建视口内可见的窗口 `[first, last)` 子项，配合 engine 裁剪与偏移实现滚动，
/// 避免一次性布局全部数据（万级条目也不卡）。
///
/// 滚动偏移通过 `FlexStyle::bind_scroll_state` 由引擎自动同步写入 `scroll_state`，
/// 消除手动 wheel listener 的偏移漂移风险。
///
/// - `height`：视口高度（固定）。
/// - `item_count` / `item_height`：数据总量与每项高度，决定可滚动总高。
/// - `scroll_state`：引擎自动同步的 `State<(f32, f32)>`，读取 `.1` 获得 Y 偏移。
/// - `item`：按索引生成子控件的工厂。
#[derive(Clone)]
pub struct VirtualList {
    height: f32,
    item_count: usize,
    item_height: f32,
    /// 引擎自动同步的滚动偏移 State（Y 轴通过 `.get().1` 读取）。
    scroll_state: State<(f32, f32)>,
    item: Arc<dyn Fn(usize) -> Box<dyn Widget>>,
    overscan: usize,
}

impl VirtualList {
    /// `scroll_state` 由引擎自动同步，读取时 `state.get().1` 获得 Y 轴偏移。
    pub fn new(
        height: f32,
        item_count: usize,
        item_height: f32,
        scroll_state: State<(f32, f32)>,
    ) -> Self {
        Self {
            height,
            item_count,
            item_height,
            scroll_state,
            item: Arc::new(|_| Box::new(Empty)),
            overscan: 3,
        }
    }
    pub fn item(mut self, f: impl Fn(usize) -> Box<dyn Widget> + 'static) -> Self {
        self.item = Arc::new(f);
        self
    }
    /// 直接设置已有的 `Arc` 工厂（用于 ListView 等组合控件复用现有工厂）。
    pub fn item_arc(mut self, f: Arc<dyn Fn(usize) -> Box<dyn Widget>>) -> Self {
        self.item = f;
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
        let sy = self.scroll_state.get().1; // 读取 Y 轴偏移
        let ih = self.item_height;
        let h = self.height;
        let count = self.item_count;
        let total = count as f32 * ih;

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

        let mut items = Vec::with_capacity(last.saturating_sub(first));
        for i in first..last {
            let row_widget = (*self.item)(i);
            let node = ctx.child(i, &*row_widget);
            items.push(ViewNode::Div {
                layout: FlexStyle::default().height(ih).flex_shrink(0.0),
                paint: PaintStyle::new(),
                children: vec![node],
                listeners: vec![],
                key: Some(format!("row-{i}")),
            });
        }

        let content_box = ViewNode::Div {
            layout: FlexStyle::column().flex_shrink(0.0),
            paint: PaintStyle::new(),
            children: items,
            listeners: vec![],
            key: Some("content".into()),
        };

        // 不再需要手动 wheel listener：引擎 scroll_by() 自动同步 scroll_state，
        // scroll_state 的变化触发 State::set() → request_rebuild() → 重新计算窗口。
        ViewNode::Div {
            layout: FlexStyle::default()
                .height(h)
                .overflow_scroll()
                .content_height(total)
                .bind_scroll_state(&self.scroll_state),
            paint: PaintStyle::new()
                .background(current().background.primary_default)
                .clip(true),
            children: vec![content_box],
            listeners: vec![],
            key: None,
        }
    }

}
