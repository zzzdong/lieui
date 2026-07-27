use std::sync::Arc;

use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::state::State;
use crate::view::node::ViewNode;
use crate::view::paint::PaintStyle;
use crate::widget::scroll_bar::ScrollBar;
use crate::widget::virtual_list::VirtualList;
use crate::widget::{BuildContext, Stateful, Widget};

/// 开箱即用的滚动列表控件。
///
/// 内部组合了 `VirtualList` + `ScrollBar` + scroll state，提供完整的垂直滚动列表体验。
/// 与 `VirtualList` 一样基于引擎滚动，支持万级条目。
///
/// # 用法
///
/// ```ignore
/// ListView::new(360.0, 10000, 30.0, |i| {
///     Box::new(Text::new(format!("Item #{i}")))
/// })
/// ```
///
/// 每次 rebuild 时滚动位置自动保持（scroll_state 通过 `use_state` 持久化）。
pub struct ListView {
    height: f32,
    item_count: usize,
    item_height: f32,
    item: Arc<dyn Fn(usize) -> Box<dyn Widget>>,
    overscan: usize,
    scrollbar_thickness: Option<f32>,
    flex_shrink: f32,
}

impl ListView {
    /// 固定高度列表。
    ///
    /// - `height` — 视口高度。
    /// - `item_count` — 数据总数。
    /// - `item_height` — 每项固定高度。
    /// - `item` — 按索引生成子控件的工厂。
    pub fn new(
        height: f32,
        item_count: usize,
        item_height: f32,
        item: impl Fn(usize) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self {
            height,
            item_count,
            item_height,
            item: Arc::new(item),
            overscan: 3,
            scrollbar_thickness: Some(8.0),
            flex_shrink: 1.0,
        }
    }

    /// 视口外额外渲染的条目数（默认 3）。
    pub fn overscan(mut self, n: usize) -> Self {
        self.overscan = n;
        self
    }

    /// 设置滚动条厚度。传 `None` 隐藏滚动条（默认 `Some(8.0)`）。
    pub fn scrollbar(mut self, thickness: Option<f32>) -> Self {
        self.scrollbar_thickness = thickness;
        self
    }

    /// 隐藏滚动条。
    pub fn no_scrollbar(mut self) -> Self {
        self.scrollbar_thickness = None;
        self
    }

    /// 设置 flex 收缩因子（默认 1.0）。
    pub fn flex_shrink(mut self, v: f32) -> Self {
        self.flex_shrink = v;
        self
    }
}

impl Widget for ListView {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        // 通过 use_state 持久化 scroll_state，每次 rebuild 复用同一 State。
        let scroll_state: Stateful<Option<State<(f32, f32)>>> = ctx.use_state(|| None);
        // 分两步：先检查 Ref 是否存在（行末释放），再读取或创建
        let has_state = scroll_state.get().is_some();
        let scroll = if has_state {
            (*scroll_state.get()).clone().unwrap()
        } else {
            let s = State::new((0.0, 0.0));
            scroll_state.set(Some(s.clone()));
            s
        };

        let total = self.item_count as f32 * self.item_height;

        let list_widget = VirtualList::new(
            self.height,
            self.item_count,
            self.item_height,
            scroll.clone(),
        )
        .item_arc(self.item.clone())
        .overscan(self.overscan)
        .flex_shrink(self.flex_shrink);

        match self.scrollbar_thickness {
            Some(t) if t > 0.0 => {
                let bar_widget =
                    ScrollBar::vertical(self.height, scroll, self.height, total).thickness(t);

                let list_node = ctx.child(0, &list_widget);
                let bar_node = ctx.child(1, &bar_widget);

                let mut layout = FlexStyle::row().gap(0.0).align_items(FlexAlign::Stretch);
                if self.flex_shrink != 1.0 {
                    layout = layout.flex_shrink(self.flex_shrink);
                }
                ViewNode::Div {
                    layout,
                    paint: PaintStyle::new(),
                    children: vec![list_node, bar_node],
                    listeners: vec![],
                    key: None,
                }
            }
            _ => {
                // 无滚动条：直接返回 VirtualList 的构建结果
                ctx.child(0, &list_widget)
            }
        }
    }
}
