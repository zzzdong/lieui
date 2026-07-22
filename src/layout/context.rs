//! LayoutContext — 两遍法 Flex 布局
//! Phase 1: measure — 递归计算所有节点的固有尺寸
//! Phase 2: layout — 递归计算位置（offset 传递累加）

use crate::core::ElementId;
use crate::geometry::{Point, Size};
use crate::layout::flex::{AlignItems, JustifyContent};
use crate::layout::measurable::{DefaultMeasurer, Measurable};
use crate::layout::node::LayoutNode;
use crate::layout::{LayoutConstraint, IntrinsicSize};
use crate::runtime::element::{ElementTree, ViewNode};

#[derive(Clone)]
pub struct LayoutContext { pub root: Option<LayoutNode> }

impl LayoutContext {
    pub fn new() -> Self { Self { root: None } }

    pub fn collect(&mut self, root_id: ElementId, tree: &ElementTree) {
        self.root = Some(self.collect_node(root_id, tree));
    }

    fn collect_node(&self, id: ElementId, tree: &ElementTree) -> LayoutNode {
        let node = tree.get_node(id);
        let measurer = DefaultMeasurer;
        let intrinsic = measurer.measure(&node, &LayoutConstraint::default());
        tree.set_intrinsic(id, intrinsic);

        let mut lnode = LayoutNode::new(id, node.type_name());
        lnode.intrinsic = intrinsic;

        // 直接从 ViewNode 变体读取类型字段
        match &node {
            ViewNode::Button { .. } => {}
            ViewNode::Text { .. } => {}
            ViewNode::Image { .. } => {}
            ViewNode::Checkbox { .. } => {}
            ViewNode::Divider { .. } => {}
            ViewNode::Column { justify, align, expand, .. } => {
                lnode.justify = *justify;
                lnode.align = *align;
                lnode.style.expand = *expand;
            }
            ViewNode::Row { justify, align, expand, .. } => {
                lnode.justify = *justify;
                lnode.align = *align;
                lnode.style.expand = *expand;
            }
            ViewNode::Container { expand, .. } => {
                lnode.style.expand = *expand;
            }
            ViewNode::Custom { props, .. } => {
                if let Some(exp) = props.get_bool("expand") { lnode.style.expand = exp; }
                if let Some(fw) = props.get_f32("width") { lnode.style.fixed_width = Some(fw); }
                if let Some(fh) = props.get_f32("height") { lnode.style.fixed_height = Some(fh); }
                if let Some(j) = props.get_str("justify") { lnode.justify = parse_j(j); }
                if let Some(a) = props.get_str("align") { lnode.align = parse_a(a); }
            }
        }

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
        let is_row = node.type_name == "row";
        if node.children.is_empty() {
            node.computed.width = node.intrinsic.width.max(constraint.min_width).min(constraint.max_width);
            node.computed.height = node.intrinsic.height.max(constraint.min_height).min(constraint.max_height);
            return;
        }
        let spacing = 4.0;
        for child in &mut node.children {
            Self::measure(child, LayoutConstraint::loose((f32::MAX, f32::MAX)));
        }
        let total_m: f32 = node.children.iter()
            .map(|c| Self::main(Size::new(c.computed.width, c.computed.height), is_row)).sum();
        let max_c: f32 = node.children.iter()
            .map(|c| Self::cross(Size::new(c.computed.width, c.computed.height), is_row))
            .fold(0.0f32, f32::max);
        let gap = spacing * (node.children.len().saturating_sub(1) as f32);
        let children_m = total_m + gap;
        let expand = node.style.expand;
        node.computed.width = if expand { constraint.max_width }
            else { node.style.fixed_width.unwrap_or(if is_row { children_m } else { max_c })
                .max(constraint.min_width).min(constraint.max_width) };
        node.computed.height = if expand { constraint.max_height }
            else { node.style.fixed_height.unwrap_or(if is_row { max_c } else { children_m })
                .max(constraint.min_height).min(constraint.max_height) };
    }

    fn layout(node: &mut LayoutNode, offset: Point) {
        let is_row = node.type_name == "row";
        node.computed.x = offset.x;
        node.computed.y = offset.y;
        let n = node.children.len();
        if n == 0 { return; }
        let spacing = 4.0;
        let total_m: f32 = node.children.iter()
            .map(|c| Self::main(Size::new(c.computed.width, c.computed.height), is_row)).sum();
        let gap = spacing * (n.saturating_sub(1) as f32);
        let pm = Self::main(Size::new(node.computed.width, node.computed.height), is_row);
        let free = (pm - total_m - gap).max(0.0);
        let g = Self::gap(node.justify, free, n);
        let start = Self::start(node.justify, 0.0, free, n);
        let pc = if is_row { node.computed.height } else { node.computed.width };
        let mut cur = start;
        for child in node.children.iter_mut() {
            let cs = Self::main(Size::new(child.computed.width, child.computed.height), is_row);
            let cc = Self::cross(Size::new(child.computed.width, child.computed.height), is_row);
            let cross = match node.align {
                AlignItems::Center => (pc - cc) / 2.0,
                AlignItems::End => (pc - cc).max(0.0),
                _ => 0.0,
            };
            let child_off = if is_row { Point::new(offset.x + cur, offset.y + cross) }
                else { Point::new(offset.x + cross, offset.y + cur) };
            Self::layout(child, child_off);
            cur += cs;
            if let Some(gg) = g { cur += gg; } else { cur += spacing; }
        }
    }

    fn main(s: Size, r: bool) -> f32 { if r { s.width } else { s.height } }
    fn cross(s: Size, r: bool) -> f32 { if r { s.height } else { s.width } }
    fn start(j: JustifyContent, s: f32, f: f32, _n: usize) -> f32 {
        match j { JustifyContent::Center => s+f/2.0, JustifyContent::End => s+f, _ => s }
    }
    fn gap(j: JustifyContent, f: f32, n: usize) -> Option<f32> {
        match j { JustifyContent::SpaceBetween if n>1 => Some(f/(n-1)as f32),
            JustifyContent::SpaceAround => Some(f/n as f32),
            JustifyContent::SpaceEvenly => Some(f/(n+1)as f32), _ => None }
    }

    pub fn hit_test(&self, point: Point, _tree: &ElementTree) -> Option<ElementId> {
        self.root.as_ref().and_then(|r| r.hit_test_rec(point.x, point.y))
    }
}
impl Default for LayoutContext { fn default() -> Self { Self::new() } }

fn parse_j(s: &str) -> JustifyContent {
    match s { "center"=>JustifyContent::Center, "end"=>JustifyContent::End,
        "space-between"=>JustifyContent::SpaceBetween, "space-around"=>JustifyContent::SpaceAround,
        "space-evenly"=>JustifyContent::SpaceEvenly, _=>JustifyContent::Start }
}
fn parse_a(s: &str) -> AlignItems {
    match s { "center"=>AlignItems::Center, "end"=>AlignItems::End, _=>AlignItems::Start }
}
