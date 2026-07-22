//! LayoutContext — 两遍法 Flex / Box 布局
//! Phase 1: measure — 递归计算所有节点的固有尺寸
//! Phase 2: layout — 递归计算位置（offset 传递累加）

use crate::core::ElementId;
use crate::geometry::{Point, Size};
use crate::layout::flex::{AlignItems, FlexDirection, JustifyContent};
use crate::layout::node::LayoutNode;
use crate::layout::LayoutConstraint;
use crate::runtime::element::ElementTree;

#[derive(Clone)]
pub struct LayoutContext {
    pub root: Option<LayoutNode>,
}

impl LayoutContext {
    pub fn new() -> Self {
        Self { root: None }
    }

    pub fn collect(&mut self, root_id: ElementId, tree: &ElementTree) {
        self.root = Some(self.collect_node(root_id, tree));
    }

    fn collect_node(&self, id: ElementId, tree: &ElementTree) -> LayoutNode {
        let node = tree.get_node(id);
        let intrinsic = node.measure(&LayoutConstraint::default());
        tree.set_intrinsic(id, intrinsic);

        let style = node.layout_style();
        let mut lnode = LayoutNode::new(id, style.node_type);
        lnode.intrinsic = intrinsic;
        lnode.style = style.box_style;
        lnode.is_flex = style.node_type == crate::view::node::NodeType::Flex;
        lnode.direction = style.flex.direction;
        lnode.justify = style.flex.justify;
        lnode.align = style.flex.align;
        lnode.spacing = style.flex.spacing;

        for child_id in tree.children_of(id) {
            lnode.children.push(self.collect_node(child_id, tree));
        }
        lnode
    }

    pub fn compute(&mut self, viewport: Size) {
        if let Some(root) = &mut self.root {
            let constraint = LayoutConstraint::tight(viewport.width, viewport.height);
            Self::measure(root, constraint);
            Self::layout(root, Point::zero());
        }
    }

    fn measure(node: &mut LayoutNode, constraint: LayoutConstraint) {
        if node.children.is_empty() {
            node.computed.width = node
                .intrinsic
                .width
                .max(constraint.min_width)
                .min(constraint.max_width);
            node.computed.height = node
                .intrinsic
                .height
                .max(constraint.min_height)
                .min(constraint.max_height);
            return;
        }

        if node.is_flex {
            Self::measure_flex(node, constraint);
        } else {
            Self::measure_box(node, constraint);
        }
    }

    fn measure_box(node: &mut LayoutNode, constraint: LayoutConstraint) {
        let pad = node.style.padding;
        let inner_constraint = LayoutConstraint::loose((
            (constraint.max_width - pad.horizontal()).max(0.0),
            (constraint.max_height - pad.vertical()).max(0.0),
        ));
        for child in &mut node.children {
            Self::measure(child, inner_constraint);
        }

        let max_child_w = node
            .children
            .iter()
            .map(|c| c.computed.width)
            .fold(0.0f32, f32::max);
        let total_child_h: f32 = node.children.iter().map(|c| c.computed.height).sum();

        let expand_self = node.style.expand;
        node.computed.width = if expand_self {
            constraint.max_width
        } else {
            node.style
                .fixed_width
                .unwrap_or(max_child_w + pad.horizontal())
                .max(constraint.min_width)
                .min(constraint.max_width)
        };
        node.computed.height = if expand_self {
            constraint.max_height
        } else {
            node.style
                .fixed_height
                .unwrap_or(total_child_h + pad.vertical())
                .max(constraint.min_height)
                .min(constraint.max_height)
        };
    }

    fn measure_flex(node: &mut LayoutNode, constraint: LayoutConstraint) {
        let is_row = node.direction == FlexDirection::Row;
        let spacing = node.spacing;
        let pad = node.style.padding;
        let pad_main = if is_row {
            pad.horizontal()
        } else {
            pad.vertical()
        };
        let pad_cross = if is_row {
            pad.vertical()
        } else {
            pad.horizontal()
        };

        // 1. 先按宽松约束测量所有子节点（留出 padding 后的可用空间）
        let child_loose = LayoutConstraint::loose((
            (constraint.max_width - pad.horizontal()).max(0.0),
            (constraint.max_height - pad.vertical()).max(0.0),
        ));
        for child in &mut node.children {
            Self::measure(child, child_loose);
        }

        // 2. 计算容器自身尺寸
        let non_expand_m: f32 = node
            .children
            .iter()
            .filter(|c| !c.style.expand)
            .map(|c| Self::main(Size::new(c.computed.width, c.computed.height), is_row))
            .sum();
        let max_c: f32 = node
            .children
            .iter()
            .map(|c| Self::cross(Size::new(c.computed.width, c.computed.height), is_row))
            .fold(0.0f32, f32::max);
        let gap = spacing * (node.children.len().saturating_sub(1) as f32);
        let children_m = non_expand_m + gap + pad_main;
        let max_c = max_c + pad_cross;
        let expand_self = node.style.expand;
        node.computed.width = if expand_self {
            constraint.max_width
        } else {
            node.style
                .fixed_width
                .unwrap_or(if is_row { children_m } else { max_c })
                .max(constraint.min_width)
                .min(constraint.max_width)
        };
        node.computed.height = if expand_self {
            constraint.max_height
        } else {
            node.style
                .fixed_height
                .unwrap_or(if is_row { max_c } else { children_m })
                .max(constraint.min_height)
                .min(constraint.max_height)
        };

        // 3. 重新测量 expand 子节点
        let expand_count = node.children.iter().filter(|c| c.style.expand).count();
        if expand_count > 0 {
            if is_row {
                let available_m =
                    Self::main(Size::new(node.computed.width, node.computed.height), is_row)
                        - non_expand_m
                        - gap
                        - pad_main;
                let expand_m = available_m.max(0.0) / expand_count as f32;
                let cross_size =
                    Self::cross(Size::new(node.computed.width, node.computed.height), is_row)
                        - pad_cross;
                for child in node.children.iter_mut() {
                    if child.style.expand {
                        let w = child.style.fixed_width.unwrap_or(expand_m);
                        let child_constraint = LayoutConstraint::new(w, w, 0.0, cross_size);
                        Self::measure(child, child_constraint);
                    }
                }
            } else {
                let inner_w = node.computed.width - pad.horizontal();
                let inner_h = node.computed.height - pad.vertical();
                for child in node.children.iter_mut() {
                    if child.style.expand {
                        let h = child.style.fixed_height.unwrap_or(inner_h);
                        let child_constraint = LayoutConstraint::new(0.0, inner_w.max(0.0), h, h);
                        Self::measure(child, child_constraint);
                    }
                }
            }
        }
    }

    fn layout(node: &mut LayoutNode, offset: Point) {
        node.computed.x = offset.x;
        node.computed.y = offset.y;
        if node.children.is_empty() {
            return;
        }
        if node.is_flex {
            Self::layout_flex(node, offset);
        } else {
            Self::layout_box(node, offset);
        }
    }

    fn layout_box(node: &mut LayoutNode, offset: Point) {
        let pad = node.style.padding;
        let inner_off = Point::new(offset.x + pad.left, offset.y + pad.top);
        let mut cur_y = inner_off.y;
        for child in node.children.iter_mut() {
            Self::layout(child, Point::new(inner_off.x, cur_y));
            cur_y += child.computed.height;
        }
    }

    fn layout_flex(node: &mut LayoutNode, offset: Point) {
        let is_row = node.direction == FlexDirection::Row;
        let spacing = node.spacing;
        let pad = node.style.padding;
        let pad_main = if is_row {
            pad.horizontal()
        } else {
            pad.vertical()
        };
        let inner_off = Point::new(offset.x + pad.left, offset.y + pad.top);
        let n = node.children.len();
        let total_m: f32 = node
            .children
            .iter()
            .map(|c| Self::main(Size::new(c.computed.width, c.computed.height), is_row))
            .sum();
        let gap = spacing * (n.saturating_sub(1) as f32);
        let pm =
            Self::main(Size::new(node.computed.width, node.computed.height), is_row) - pad_main;
        let free = (pm - total_m - gap).max(0.0);
        let g = Self::gap(node.justify, free, n);
        let start = Self::start(node.justify, 0.0, free, n);
        let pc = if is_row {
            node.computed.height - pad.vertical()
        } else {
            node.computed.width - pad.horizontal()
        };
        let mut cur = start;
        for child in node.children.iter_mut() {
            let cs = Self::main(
                Size::new(child.computed.width, child.computed.height),
                is_row,
            );
            let cc = Self::cross(
                Size::new(child.computed.width, child.computed.height),
                is_row,
            );
            let cross = match node.align {
                AlignItems::Center => (pc - cc) / 2.0,
                AlignItems::End => (pc - cc).max(0.0),
                _ => 0.0,
            };
            let child_off = if is_row {
                Point::new(inner_off.x + cur, inner_off.y + cross)
            } else {
                Point::new(inner_off.x + cross, inner_off.y + cur)
            };
            Self::layout(child, child_off);
            cur += cs;
            if let Some(gg) = g {
                cur += gg;
            } else {
                cur += spacing;
            }
        }
    }

    fn main(s: Size, r: bool) -> f32 {
        if r {
            s.width
        } else {
            s.height
        }
    }
    fn cross(s: Size, r: bool) -> f32 {
        if r {
            s.height
        } else {
            s.width
        }
    }
    fn start(j: JustifyContent, s: f32, f: f32, _n: usize) -> f32 {
        match j {
            JustifyContent::Center => s + f / 2.0,
            JustifyContent::End => s + f,
            _ => s,
        }
    }
    fn gap(j: JustifyContent, f: f32, n: usize) -> Option<f32> {
        match j {
            JustifyContent::SpaceBetween if n > 1 => Some(f / (n - 1) as f32),
            JustifyContent::SpaceAround => Some(f / n as f32),
            JustifyContent::SpaceEvenly => Some(f / (n + 1) as f32),
            _ => None,
        }
    }

    pub fn hit_test(&self, point: Point, _tree: &ElementTree) -> Option<ElementId> {
        self.root
            .as_ref()
            .and_then(|r| r.hit_test_rec(point.x, point.y))
    }
}
impl Default for LayoutContext {
    fn default() -> Self {
        Self::new()
    }
}
