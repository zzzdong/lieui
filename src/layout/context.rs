//! LayoutContext — FlexNode 驱动的布局上下文
//!
//! 使用 Taitank 风格 FlexNode 引擎。
//! ViewNode 直接提供确定好的 Taitank FlexStyle，本模块只做树转换和坐标计算。
//! 布局结果直接写回 ElementTree::ElementEntry::layout，不再维护独立的 LayoutNode 树。

use crate::core::ElementId;
use crate::geometry::Size;
use crate::layout::box_model::ComputedLayout;
use crate::layout::flex_node::FlexNode;
use crate::layout::types::{
    Direction as LayoutDirection, FlexDirection, NodeType as LayoutNodeType, VALUE_UNDEFINED,
};
use crate::layout::LayoutConstraint;
use crate::runtime::element::ElementTree;
use crate::view::node::{NodeType, ViewNode};

#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutContext;

impl LayoutContext {
    /// 直接对 ElementTree 中的指定子树执行 Flexbox 布局，并将全局坐标写回各 ElementEntry。
    pub fn compute(root_id: ElementId, tree: &ElementTree, viewport: Size) {
        let mut flex_root = Self::build_flex(root_id, tree);
        flex_root.layout(
            if viewport.width > 0.0 {
                viewport.width
            } else {
                VALUE_UNDEFINED
            },
            if viewport.height > 0.0 {
                viewport.height
            } else {
                VALUE_UNDEFINED
            },
            LayoutDirection::Ltr,
        );
        Self::write_layout(&flex_root, tree, root_id, 0.0, 0.0);
    }

    fn build_flex(id: ElementId, tree: &ElementTree) -> FlexNode {
        let view = tree.get_node_ref(id).unwrap();
        let mut fs = view.layout().clone();

        // 根据节点类型设置布局引擎内部类型标记
        fs.node_type = match view.node_type() {
            NodeType::Text => LayoutNodeType::Text,
            _ => LayoutNodeType::Default,
        };

        let children = tree.children_ref(id);
        let is_leaf = children.is_empty();
        let mut fn_node = FlexNode::new(0, fs);

        // 叶子节点：测量 intrinsic size 并存储
        if is_leaf {
            // 按需测量：节点未变化（dirty=false）时直接复用上次布局算出的 intrinsic，
            // 避免每次 rebuild 都对全部文本/叶子重新测量（文本整形很贵）；
            // 新节点或内容/样式变化的节点（dirty=true）才真正测量并写入缓存。
            let m = if !tree.is_dirty(id) {
                tree.intrinsic(id)
            } else {
                let m = view.measure(&LayoutConstraint::default());
                tree.set_intrinsic(id, m);
                m
            };

            // 记录文本排版内容，供布局时按约束宽度重新测量（支持换行）
            if let ViewNode::Text { content, style, .. } = view {
                fn_node.measure_text = Some((content.clone(), style.clone()));
            }

            fn_node.intrinsic_size = Some((m.width, m.height));
        }

        // 递归构建子树
        for &cid in children {
            let child_fn = Self::build_flex(cid, tree);
            fn_node.children.push(child_fn);
        }

        // 滚动容器：直接子节点必须保持自然尺寸，禁止被收缩到视口高度，
        // 否则滚动内容会塌陷、看不到滚动。
        if view.layout().overflow_scroll {
            for c in fn_node.children.iter_mut() {
                c.style.flex_shrink = 0.0;
                c.style.flex_grow = 0.0;
            }
        }

        fn_node
    }

    /// 将 FlexNode 计算结果写回 ElementEntry::layout（全局坐标）。
    fn write_layout(
        node: &FlexNode,
        tree: &ElementTree,
        id: ElementId,
        offset_x: f32,
        offset_y: f32,
    ) {
        let global_x = offset_x + node.get_left();
        let global_y = offset_y + node.get_top();
        tree.set_layout(
            id,
            ComputedLayout {
                x: global_x,
                y: global_y,
                width: node.get_width(),
                height: node.get_height(),
                overflow_scroll: node.style.overflow_scroll,
            },
        );

        let children_ids = tree.children_ref(id);

        // 滚动容器：读取滚动偏移，计算内容尺寸并钳制，子节点整体平移 -offset。
        let (child_origin_x, child_origin_y) = if node.style.overflow_scroll {
            let (ox, oy) = tree.scroll_offset(id);
            let cw = node.style.content_width.unwrap_or_else(|| {
                node.children.iter().fold(0.0f32, |m, c| {
                    m.max(
                        c.get_left() + c.get_width() + c.get_layout_end_margin(FlexDirection::Row),
                    )
                })
            });
            let ch = node.style.content_height.unwrap_or_else(|| {
                node.children.iter().fold(0.0f32, |m, c| {
                    m.max(
                        c.get_top()
                            + c.get_height()
                            + c.get_layout_end_margin(FlexDirection::Column),
                    )
                })
            });
            let vw = node.get_width();
            let vh = node.get_height();
            let max_x = (cw - vw).max(0.0);
            let max_y = (ch - vh).max(0.0);
            let nox = ox.clamp(0.0, max_x);
            let noy = oy.clamp(0.0, max_y);
            tree.set_scroll_offset(id, (nox, noy));
            tree.set_content_size(id, (cw, ch));
            (global_x - nox, global_y - noy)
        } else {
            (global_x, global_y)
        };

        for (child_fn, child_id) in node.children.iter().zip(children_ids.iter()) {
            Self::write_layout(child_fn, tree, *child_id, child_origin_x, child_origin_y);
        }
    }
}
