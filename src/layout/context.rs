//! LayoutContext — 两遍法 Flex 布局
//!
//! Phase 1: measure — 递归计算所有节点的固有尺寸（仅大小）
//! Phase 2: layout — 递归计算位置（offset 传递累加）
//!
//! 这种设计避免了"子节点先算好位置→父节点再改"的混乱。

use crate::core::ElementId;
use crate::geometry::{Point, Size};
use crate::layout::flex::{AlignItems, JustifyContent};
use crate::layout::measurable::{DefaultMeasurer, Measurable};
use crate::layout::node::LayoutNode;
use crate::layout::{LayoutConstraint, IntrinsicSize};
use crate::runtime::element::ElementTree;

#[derive(Clone)]
pub struct LayoutContext { pub root: Option<LayoutNode> }

impl LayoutContext {
    pub fn new() -> Self { Self { root: None } }

    // ---- Phase 0: 从 ElementTree 构建树 ----

    pub fn collect(&mut self, root_id: ElementId, tree: &ElementTree) {
        self.root = Some(self.collect_node(root_id, tree));
    }

    fn collect_node(&self, id: ElementId, tree: &ElementTree) -> LayoutNode {
        let props = tree.props(id);
        let measurer = DefaultMeasurer;
        let intrinsic = props
            .map(|p| measurer.measure(p, &LayoutConstraint::default()))
            .unwrap_or(IntrinsicSize::zero());
        tree.set_intrinsic(id, intrinsic);

        let type_name = tree.type_name(id).unwrap_or("");
        let mut node = LayoutNode::new(id, type_name);
        node.intrinsic = intrinsic;

        if let Some(p) = props {
            if let Some(fw) = p.get_f32("width") { node.style.fixed_width = Some(fw); }
            if let Some(fh) = p.get_f32("height") { node.style.fixed_height = Some(fh); }
            if let Some(exp) = p.get_bool("expand") { node.style.expand = exp; }
            node.justify = parse_j(p.get_str("justify").unwrap_or("start"));
            node.align = parse_a(p.get_str("align").unwrap_or("stretch"));
        }

        for child_id in tree.children_of(id) {
            node.children.push(self.collect_node(child_id, tree));
        }
        node
    }

    // ---- Phase 1: 测量尺寸 ----

    pub fn compute(&mut self, viewport: Size) {
        if let Some(root) = &mut self.root {
            let constraint = LayoutConstraint::tight(viewport.width, viewport.height);
            Self::measure(root, constraint);
            Self::layout(root, Point::zero());
        }
    }

    /// Phase 1: 递归测量 — 只算大小，不设位置
    fn measure(node: &mut LayoutNode, constraint: LayoutConstraint) {
        let is_row = node.type_name == "row";

        // 叶子节点：直接取 intrinsic
        if node.children.is_empty() {
            node.computed.width = node.intrinsic.width
                .max(constraint.min_width)
                .min(constraint.max_width);
            node.computed.height = node.intrinsic.height
                .max(constraint.min_height)
                .min(constraint.max_height);
            return;
        }

        // 非叶子：先递归测量子节点
        // 对 flex 容器，子节点不应受父级跨轴约束限制
        let spacing = 4.0;
        for child in &mut node.children {
            Self::measure(child, LayoutConstraint::loose((f32::MAX, f32::MAX)));
        }

        // 根据子节点尺寸算自己的尺寸
        let total_m: f32 = node.children.iter()
            .map(|c| Self::main(Size::new(c.computed.width, c.computed.height), is_row))
            .sum();
        let max_c: f32 = node.children.iter()
            .map(|c| Self::cross(Size::new(c.computed.width, c.computed.height), is_row))
            .fold(0.0f32, f32::max);
        let gap = spacing * (node.children.len().saturating_sub(1) as f32);
        let children_m = total_m + gap;

        let expand = node.style.expand;
        let pw = if expand { constraint.max_width }
                 else { node.style.fixed_width.unwrap_or(
                     if is_row { children_m } else { max_c }
                 ).max(constraint.min_width).min(constraint.max_width) };
        let ph = if expand { constraint.max_height }
                 else { node.style.fixed_height.unwrap_or(
                     if is_row { max_c } else { children_m }
                 ).max(constraint.min_height).min(constraint.max_height) };
        node.computed.width = pw;
        node.computed.height = ph;
    }

    /// Phase 2: 布局位置 — 从根到叶子累加 offset
    fn layout(node: &mut LayoutNode, offset: Point) {
        let is_row = node.type_name == "row";
        let justify = node.justify;
        let align = node.align;

        // 设置自身位置
        node.computed.x = offset.x;
        node.computed.y = offset.y;

        let n = node.children.len();
        if n == 0 { return; }

        let spacing = 4.0;
        let total_m: f32 = node.children.iter()
            .map(|c| Self::main(Size::new(c.computed.width, c.computed.height), is_row))
            .sum();
        let gap = spacing * (n.saturating_sub(1) as f32);
        let pm = Self::main(Size::new(node.computed.width, node.computed.height), is_row);
        let free = (pm - total_m - gap).max(0.0);
        let g = Self::gap(justify, free, n);
        let start = Self::start(justify, 0.0, free, n);
        let _ = if is_row { node.computed.width } else { node.computed.height };
        let pc = if is_row { node.computed.height } else { node.computed.width };

        let mut cur = start;
        for child in node.children.iter_mut() {
            let cs = Self::main(Size::new(child.computed.width, child.computed.height), is_row);
            let cc = Self::cross(Size::new(child.computed.width, child.computed.height), is_row);
            let cross = match align {
                AlignItems::Center => (pc - cc) / 2.0,
                AlignItems::End => (pc - cc).max(0.0),
                AlignItems::Stretch => 0.0,
                AlignItems::Start => 0.0,
            };

            // 计算子节点绝对位置 = 父节点 offset + 相对偏移
            let child_off = if is_row {
                Point::new(offset.x + cur, offset.y + cross)
            } else {
                Point::new(offset.x + cross, offset.y + cur)
            };
            Self::layout(child, child_off);

            cur += cs;
            if let Some(gg) = g { cur += gg; }
            else { cur += spacing; }
        }
    }

    fn main(s: Size, r: bool) -> f32 { if r { s.width } else { s.height } }
    fn cross(s: Size, r: bool) -> f32 { if r { s.height } else { s.width } }

    fn start(j: JustifyContent, s: f32, f: f32, _n: usize) -> f32 {
        match j { JustifyContent::Start => s, JustifyContent::Center => s + f / 2.0,
            JustifyContent::End => s + f, _ => s }
    }

    fn gap(j: JustifyContent, f: f32, n: usize) -> Option<f32> {
        match j { JustifyContent::SpaceBetween if n > 1 => Some(f / (n - 1) as f32),
            JustifyContent::SpaceAround => Some(f / n as f32),
            JustifyContent::SpaceEvenly => Some(f / (n + 1) as f32), _ => None }
    }

    pub fn hit_test(&self, point: Point, _tree: &ElementTree) -> Option<ElementId> {
        self.root.as_ref().and_then(|r| r.hit_test_rec(point.x, point.y))
    }
}

fn parse_j(s: &str) -> JustifyContent {
    match s { "center" => JustifyContent::Center, "end" => JustifyContent::End,
        "space-between" => JustifyContent::SpaceBetween, "space-around" => JustifyContent::SpaceAround,
        "space-evenly" => JustifyContent::SpaceEvenly, _ => JustifyContent::Start }
}
fn parse_a(s: &str) -> AlignItems {
    match s { "center" => AlignItems::Center, "end" => AlignItems::End, _ => AlignItems::Start }
}

impl Default for LayoutContext { fn default() -> Self { Self::new() } }
