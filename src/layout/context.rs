//! LayoutContext — FlexNode 驱动的布局上下文
//!
//! 使用新 FlexNode 引擎（移植自 Taitank）替代旧的简化布局实现。

use crate::core::ElementId;
use crate::geometry::{Point, Size};
use crate::layout::box_model::ComputedLayout;
use crate::layout::flex_node::FlexNode;
use crate::layout::node::LayoutNode;
use crate::layout::style::FlexStyle;
use crate::layout::types::{
    CSSDirection, Dimension, Direction as LayoutDirection, FlexAlign, FlexDirection,
    FlexWrap as NewFlexWrap, PositionType as NewPositionType, VALUE_UNDEFINED,
};
use crate::layout::LayoutConstraint;
use crate::runtime::element::ElementTree;
use crate::view::node::{DisplayMode, LayoutStyle, NodeType};

#[derive(Clone)]
pub struct LayoutContext {
    pub root: Option<LayoutNode>,
}

impl LayoutContext {
    pub fn new() -> Self {
        Self { root: None }
    }

    pub fn collect(&mut self, root_id: ElementId, tree: &ElementTree, prev: Option<&LayoutNode>) {
        // 仅构建 LayoutNode 树结构（不含布局结果）
        // 布局计算在 compute 中通过 FlexNode 引擎完成
        self.root = Some(self.collect_layout_children(root_id, tree, prev));
    }

    fn collect_flex(&self, id: ElementId, tree: &ElementTree) -> FlexNode {
        let st = Self::node_style(tree, id);
        let nt = tree
            .get_node_ref(id)
            .map(|n| n.node_type())
            .unwrap_or(NodeType::Div);

        let is_leaf = tree.children_of(id).is_empty();
        let fs = Self::to_flex_style(&st, nt, is_leaf);

        let mut fn_node = FlexNode::new(id.as_ffi(), fs);

        // 叶子节点: 测量 intrinsic size 并存储
        if is_leaf {
            if let Some(r) = tree.get_node_ref(id) {
                let m = r.measure(&LayoutConstraint::default());
                tree.set_intrinsic(id, m);
                // 记录文本排版内容，供布局时按约束宽度重新测量（支持换行）
                if let crate::view::node::ViewNode::Text {
                    content, font_size, ..
                } = r
                {
                    fn_node.measure_text = Some((content.clone(), *font_size));
                }
            }
            let m = tree.intrinsic(id);
            fn_node.intrinsic_size = Some((m.width, m.height));
        }

        // 递归构建子树
        for cid in tree.children_of(id) {
            let child = self.collect_flex(cid, tree);
            fn_node.children.push(child);
        }

        fn_node
    }

    /// 仅构建 LayoutNode 树结构（不含布局结果），布局计算在 compute 中完成
    fn collect_layout_children(
        &self,
        id: ElementId,
        tree: &ElementTree,
        _prev: Option<&LayoutNode>,
    ) -> LayoutNode {
        let nt = tree
            .get_node_ref(id)
            .map(|n| n.node_type())
            .unwrap_or(NodeType::Div);
        let mut ln = LayoutNode::new(id, nt);
        for cid in tree.children_of(id) {
            ln.children
                .push(self.collect_layout_children(cid, tree, _prev));
        }
        ln
    }

    /// 执行布局计算
    pub fn compute(&mut self, viewport: Size, tree: &ElementTree) {
        if self.root.is_none() {
            return;
        }

        // 第一步：构建 FlexNode 树
        let root_id = self.root.as_ref().unwrap().id;
        let mut flex_root = self.collect_flex(root_id, tree);

        // 第二步：运行 FlexNode layout
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

        // 第三步：从 FlexNode 树构建 LayoutNode 树（含全局坐标转换）
        self.root = Some(Self::flex_to_layout(&flex_root, tree, 0.0, 0.0));
    }

    /// FlexNode 树 → LayoutNode 树（全局坐标 + 写入 ElementTree）
    fn flex_to_layout(
        node: &FlexNode,
        tree: &ElementTree,
        offset_x: f32,
        offset_y: f32,
    ) -> LayoutNode {
        let id = ElementId::from_u64(node.id);
        let nt = tree
            .get_node_ref(id)
            .map(|n| n.node_type())
            .unwrap_or(NodeType::Div);
        let mut ln = LayoutNode::new(id, nt);

        let global_x = offset_x + node.get_left();
        let global_y = offset_y + node.get_top();
        ln.computed = ComputedLayout {
            x: global_x,
            y: global_y,
            width: node.get_width(),
            height: node.get_height(),
        };
        tree.set_layout(id, ln.computed);

        for child in &node.children {
            ln.children
                .push(Self::flex_to_layout(child, tree, global_x, global_y));
        }
        ln
    }

    /// LayoutStyle → FlexStyle 转换
    fn to_flex_style(ls: &LayoutStyle, node_type: NodeType, _is_leaf: bool) -> FlexStyle {
        let flex_grow_from_display = if ls.display == DisplayMode::Flex && ls.flex.expand {
            1.0
        } else {
            0.0
        };
        let flex_grow = if ls.box_style.expand && flex_grow_from_display == 0.0 {
            1.0
        } else {
            flex_grow_from_display
        };

        let mut fs = FlexStyle {
            flex_direction: if ls.display == DisplayMode::Flex {
                match ls.flex.direction {
                    crate::layout::flex::FlexDirection::Row => FlexDirection::Row,
                    crate::layout::flex::FlexDirection::Column => FlexDirection::Column,
                }
            } else {
                FlexDirection::Column
            },
            justify_content: if ls.display == DisplayMode::Flex {
                Self::to_flex_justify(ls.flex.justify)
            } else {
                FlexAlign::Start
            },
            align_items: if ls.display == DisplayMode::Flex {
                Self::to_flex_align(ls.flex.align)
            } else {
                FlexAlign::Stretch
            },
            item_space: if ls.display == DisplayMode::Flex {
                ls.flex.spacing
            } else {
                0.0
            },
            flex_grow,
            flex_wrap: if ls.display == DisplayMode::Flex {
                if ls.flex.wrap == crate::layout::flex::FlexWrap::Wrap {
                    NewFlexWrap::Wrap
                } else {
                    NewFlexWrap::NoWrap
                }
            } else {
                NewFlexWrap::NoWrap
            },
            node_type: if node_type == NodeType::Text {
                crate::layout::types::NodeType::Text
            } else {
                crate::layout::types::NodeType::Default
            },
            position_type: if ls.box_style.position_type
                == crate::layout::box_model::PositionType::Absolute
            {
                NewPositionType::Absolute
            } else {
                NewPositionType::Relative
            },
            ..Default::default()
        };

        fs.dim[Dimension::Width as usize] = ls.box_style.fixed_width.unwrap_or(VALUE_UNDEFINED);
        fs.dim[Dimension::Height as usize] = ls.box_style.fixed_height.unwrap_or(VALUE_UNDEFINED);

        fs.set_padding(CSSDirection::Left, ls.box_style.padding.left);
        fs.set_padding(CSSDirection::Top, ls.box_style.padding.top);
        fs.set_padding(CSSDirection::Right, ls.box_style.padding.right);
        fs.set_padding(CSSDirection::Bottom, ls.box_style.padding.bottom);

        fs.set_margin(CSSDirection::Left, ls.box_style.margin.left);
        fs.set_margin(CSSDirection::Top, ls.box_style.margin.top);
        fs.set_margin(CSSDirection::Right, ls.box_style.margin.right);
        fs.set_margin(CSSDirection::Bottom, ls.box_style.margin.bottom);

        fs
    }

    fn to_flex_align(align: crate::layout::flex::AlignItems) -> FlexAlign {
        match align {
            crate::layout::flex::AlignItems::Start => FlexAlign::Start,
            crate::layout::flex::AlignItems::Center => FlexAlign::Center,
            crate::layout::flex::AlignItems::End => FlexAlign::End,
            crate::layout::flex::AlignItems::Stretch => FlexAlign::Stretch,
        }
    }

    fn to_flex_justify(j: crate::layout::flex::JustifyContent) -> FlexAlign {
        match j {
            crate::layout::flex::JustifyContent::Start => FlexAlign::Start,
            crate::layout::flex::JustifyContent::Center => FlexAlign::Center,
            crate::layout::flex::JustifyContent::End => FlexAlign::End,
            crate::layout::flex::JustifyContent::SpaceBetween => FlexAlign::SpaceBetween,
            crate::layout::flex::JustifyContent::SpaceAround => FlexAlign::SpaceAround,
            crate::layout::flex::JustifyContent::SpaceEvenly => FlexAlign::SpaceEvenly,
        }
    }

    fn node_style(tree: &ElementTree, id: ElementId) -> LayoutStyle {
        tree.get_node_ref(id)
            .map(|n| n.layout_style())
            .unwrap_or_default()
    }

    pub fn hit_test(&self, point: Point, _tree: &ElementTree) -> Option<ElementId> {
        self.root
            .as_ref()
            .and_then(|r| r.hit_test_rec(point.x, point.y))
    }

    pub fn dump_tree(&self) {
        if let Some(ref root) = self.root {
            Self::dump_node(root, 0);
        }
    }

    fn dump_node(node: &LayoutNode, depth: usize) {
        let indent = "  ".repeat(depth);
        eprintln!(
            "{}[{:?}] id={:?} pos=({:.1},{:.1}) size=({:.1},{:.1})",
            indent,
            node.node_type,
            node.id,
            node.computed.x,
            node.computed.y,
            node.computed.width,
            node.computed.height,
        );
        for child in &node.children {
            Self::dump_node(child, depth + 1);
        }
    }
}

impl Default for LayoutContext {
    fn default() -> Self {
        Self::new()
    }
}
