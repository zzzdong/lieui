//! LayoutEngine —— 树驱动的布局入口
//!
//! 与旧实现的差异（M1）：
//! - 布局 pass 以「重排边界」为根，只重算脏子树，不整窗重算；
//! - 每 pass 统计节点数（`LayoutStats`），供测试断言「改单个文本节点不触发整窗重排」；
//! - 文本测量通过 `LayoutTree::measure` 回调，引擎本身不依赖 parley；
//! - 结果同时记录**父内容盒内的局部偏移**，使边界子树能在不重算祖先的前提下重排。

use crate::box_model::ComputedLayout;
use crate::flex_node::FlexNode;
use crate::measure::LayoutTree;
use crate::types::{Direction as LayoutDirection, FlexDirection, NodeType, VALUE_UNDEFINED};

/// 布局统计（累计 + 最近一帧）
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LayoutStats {
    /// 累计 pass 次数
    pub passes: u32,
    /// 最近一次 pass 参与布局的节点数
    pub last_pass_nodes: u32,
    /// 累计参与布局的节点数
    pub total_nodes: u32,
    /// 单 pass 最大节点数（用于判断是否退化成整窗重排）
    pub max_pass_nodes: u32,
}

/// 一个 pass 的输出：深度优先序的 (节点, 布局结果)
#[derive(Debug, Default, Clone)]
pub struct LayoutOutput {
    entries: Vec<(u64, ComputedLayout)>,
    /// 复用的子节点收集缓冲，避免每 pass 分配
    scratch: Vec<u64>,
}

impl LayoutOutput {
    pub fn push(&mut self, node: u64, layout: ComputedLayout) {
        self.entries.push((node, layout));
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn get(&self, node: u64) -> Option<ComputedLayout> {
        self.entries
            .iter()
            .find(|(id, _)| *id == node)
            .map(|(_, l)| *l)
    }
    pub fn iter(&self) -> impl Iterator<Item = (u64, ComputedLayout)> + '_ {
        self.entries.iter().copied()
    }
}

/// 布局引擎。每窗口一份。
#[derive(Default)]
pub struct LayoutEngine {
    stats: LayoutStats,
    output: LayoutOutput,
}

impl LayoutEngine {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn stats(&self) -> LayoutStats {
        self.stats
    }

    pub fn reset_stats(&mut self) {
        self.stats = LayoutStats::default();
    }

    #[inline]
    pub fn output(&self) -> &LayoutOutput {
        &self.output
    }

    /// 以 `root` 为根执行一次布局 pass。
    ///
    /// - `avail` = 子树可用宽高；`VALUE_UNDEFINED`(NaN) 表示不约束，根节点按内容自然尺寸布局
    ///   （Modal / Popup / Tooltip 等弹层）。
    /// - `origin` = root 应放置的坐标系原点（整窗布局为 (0,0)；从重排边界布局时为
    ///   边界父容器的内容盒原点）。
    pub fn layout<T: LayoutTree>(
        &mut self,
        tree: &mut T,
        root: u64,
        avail: (f32, f32),
        origin: (f32, f32),
    ) -> &LayoutOutput {
        self.output.clear();
        let mut count = 0u32;
        let mut flex_root = build_flex(tree, root, &mut self.output.scratch, &mut count);
        flex_root.layout(tree, avail.0, avail.1, LayoutDirection::Ltr);
        write_layout(tree, &flex_root, root, origin.0, origin.1, &mut self.output);

        self.stats.passes += 1;
        self.stats.last_pass_nodes = count;
        self.stats.total_nodes += count;
        self.stats.max_pass_nodes = self.stats.max_pass_nodes.max(count);
        &self.output
    }
}

/// 递归构建 Flex 树。`count` 累计本子树节点数。
fn build_flex<T: LayoutTree>(
    tree: &mut T,
    node: u64,
    scratch: &mut Vec<u64>,
    count: &mut u32,
) -> FlexNode {
    *count += 1;
    let mut style = tree.style_of(node);
    if tree.is_text(node) {
        style.node_type = NodeType::Text;
    }
    let mut fnode = FlexNode::new(node, style);
    fnode.intrinsic_size = tree.measure(node, None);

    let mut kids = std::mem::take(scratch);
    kids.clear();
    tree.collect_children(node, &mut kids);
    for child in kids.drain(..) {
        fnode.children.push(build_flex(tree, child, scratch, count));
    }
    *scratch = kids;

    // 滚动容器：直接子节点须保持自然尺寸，否则内容会被压缩到视口高度而塌陷
    if fnode.style.overflow_scroll {
        for c in fnode.children.iter_mut() {
            c.style.flex_shrink = 0.0;
            c.style.flex_grow = 0.0;
        }
    }
    fnode
}

/// 把 Flex 结果写成绝对坐标。滚动容器额外计算内容尺寸与钳制后的滚动偏移。
fn write_layout<T: LayoutTree>(
    tree: &mut T,
    fnode: &FlexNode,
    node: u64,
    ox: f32,
    oy: f32,
    out: &mut LayoutOutput,
) {
    let local_x = fnode.get_left();
    let local_y = fnode.get_top();
    let gx = ox + local_x;
    let gy = oy + local_y;
    let vw = fnode.get_width();
    let vh = fnode.get_height();

    let (cw, ch, sx, sy, child_ox, child_oy) = if fnode.style.overflow_scroll {
        let (raw_x, raw_y) = tree.scroll_offset(node);
        let cw = fnode.style.content_width.unwrap_or_else(|| {
            fnode.children.iter().fold(0.0f32, |m, c| {
                m.max(c.get_left() + c.get_width() + c.get_layout_end_margin(FlexDirection::Row))
            })
        });
        let ch = fnode.style.content_height.unwrap_or_else(|| {
            fnode.children.iter().fold(0.0f32, |m, c| {
                m.max(c.get_top() + c.get_height() + c.get_layout_end_margin(FlexDirection::Column))
            })
        });
        let sx = raw_x.clamp(0.0, (cw - vw).max(0.0));
        let sy = raw_y.clamp(0.0, (ch - vh).max(0.0));
        (cw, ch, sx, sy, gx - sx, gy - sy)
    } else {
        (vw, vh, 0.0, 0.0, gx, gy)
    };

    out.push(
        node,
        ComputedLayout {
            x: gx,
            y: gy,
            width: vw,
            height: vh,
            content_width: cw,
            content_height: ch,
            scroll_x: sx,
            scroll_y: sy,
            local_x,
            local_y,
            avail_w: fnode.layout_result.avail[0],
            avail_h: fnode.layout_result.avail[1],
            overflow_scroll: fnode.style.overflow_scroll,
        },
    );

    let mut kids = std::mem::take(&mut out.scratch);
    kids.clear();
    tree.collect_children(node, &mut kids);
    for (i, child) in kids.iter().copied().enumerate() {
        if let Some(cf) = fnode.children.get(i) {
            write_layout(tree, cf, child, child_ox, child_oy, out);
        }
    }
    kids.clear();
    out.scratch = kids;
}

/// 窗口根布局的便捷封装：root 填满 viewport。
pub fn viewport_avail(width: f32, height: f32) -> (f32, f32) {
    (
        if width > 0.0 { width } else { VALUE_UNDEFINED },
        if height > 0.0 {
            height
        } else {
            VALUE_UNDEFINED
        },
    )
}
