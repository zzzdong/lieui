//! LayoutContext — 完整 Flex 布局（基于原 v2 已验证实现）

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

    /// Phase 1: 从 ElementTree 构建布局树，读入 flex 属性
    pub fn collect(&mut self, root_id: ElementId, tree: &ElementTree) {
        self.root = Some(self.collect_node(root_id, tree));
    }

    fn collect_node(&self, id: ElementId, tree: &ElementTree) -> LayoutNode {
        let props = tree.props(id);
        let measurer = DefaultMeasurer;
        let intrinsic = props.map(|p| measurer.measure(p, &LayoutConstraint::default())).unwrap_or(IntrinsicSize::zero());
        tree.set_intrinsic(id, intrinsic);

        let type_name = tree.type_name(id).unwrap_or("");
        let mut node = LayoutNode::new(id, type_name);
        node.intrinsic = intrinsic;

        // 读取 flex 属性
        if let Some(p) = props {
            if let Some(fw) = p.get_f32("width") { node.style.fixed_width = Some(fw); }
            if let Some(fh) = p.get_f32("height") { node.style.fixed_height = Some(fh); }
            if let Some(exp) = p.get_bool("expand") { node.style.expand = exp; }
            node.justify = parse_justify(p.get_str("justify").unwrap_or("start"));
            node.align = parse_align(p.get_str("align").unwrap_or("stretch"));
        }

        for child_id in tree.children_of(id) {
            node.children.push(self.collect_node(child_id, tree));
        }
        node
    }

    /// Phase 2: 全量布局计算
    pub fn compute(&mut self, viewport: Size) {
        if let Some(root) = &mut self.root {
            let constraint = LayoutConstraint::tight(viewport.width, viewport.height);
            Self::layout_node(root, constraint, Point::zero());
        }
    }

    /// 命中测试
    pub fn hit_test(&self, point: Point, _tree: &ElementTree) -> Option<ElementId> {
        self.root.as_ref().and_then(|r| r.hit_test_rec(point.x, point.y))
    }

    // ================================================
    // 布局核心算法
    // ================================================

    fn layout_node(node: &mut LayoutNode, constraint: LayoutConstraint, offset: Point) {
        let is_row = node.type_name == "row";
        let justify = node.justify;
        let align = node.align;

        // 1. 递归测量子节点
        for child in &mut node.children {
            let child_main = Self::main_of(Size::new(child.intrinsic.width, child.intrinsic.height), is_row);
            let child_cross = Self::cross_of(Size::new(child.intrinsic.width, child.intrinsic.height), is_row);
            let child_cons = if is_row {
                LayoutConstraint::new(0.0, child_main, 0.0, child_cross)
            } else {
                LayoutConstraint::new(0.0, child_cross, 0.0, child_main)
            };
            Self::layout_node(child, child_cons, Point::zero());
        }

        // 2. 计算容器尺寸
        let total_main: f32 = node.children.iter()
            .map(|c| Self::main_of(Size::new(c.computed.width, c.computed.height), is_row))
            .sum();
        let max_cross: f32 = node.children.iter()
            .map(|c| Self::cross_of(Size::new(c.computed.width, c.computed.height), is_row))
            .fold(0.0f32, f32::max);

        let spacing = 4.0f32;
        let gap_total = spacing * (node.children.len().saturating_sub(1) as f32);
        let children_total = total_main + gap_total;

        // expand 撑满
        let expand = node.style.expand;
        let pw = if expand { constraint.max_width }
                 else { node.style.fixed_width.unwrap_or(match is_row { true => children_total, false => max_cross })
                       .max(constraint.min_width).min(constraint.max_width) };
        let ph = if expand { constraint.max_height }
                 else { node.style.fixed_height.unwrap_or(match is_row { true => max_cross, false => children_total })
                       .max(constraint.min_height).min(constraint.max_height) };
        node.computed.width = pw;
        node.computed.height = ph;

        // 3. 定位子节点
        if node.children.is_empty() { return; }

        let parent_main = Self::main_of(Size::new(pw, ph), is_row);
        let free = (parent_main - children_total).max(0.0);
        let n = node.children.len();
        let gap = Self::gap(justify, free, n);
        let start = Self::start(justify, 0.0, free, n);

        let mut pos = start;
        for (i, child) in node.children.iter_mut().enumerate() {
            let cs = Self::main_of(Size::new(child.computed.width, child.computed.height), is_row);
            let cc = Self::cross_of(Size::new(child.computed.width, child.computed.height), is_row);
            let pc = Self::cross_of(Size::new(pw, ph), is_row);
            let cross = match align {
                AlignItems::Center => (pc - cc) / 2.0,
                AlignItems::End => (pc - cc).max(0.0),
                _ => 0.0,
            };

            if is_row { child.computed.x = offset.x + pos; child.computed.y = offset.y + cross; }
            else { child.computed.x = offset.x + cross; child.computed.y = offset.y + pos; }

            pos += cs;
            if let Some(g) = gap { if i < n - 1 { pos += g; } }
            else if i < n - 1 { pos += spacing; }
        }
    }

    fn main_of(s: Size, r: bool) -> f32 { if r { s.width } else { s.height } }
    fn cross_of(s: Size, r: bool) -> f32 { if r { s.height } else { s.width } }

    fn start(j: JustifyContent, s: f32, f: f32, _n: usize) -> f32 {
        match j { JustifyContent::Start => s, JustifyContent::Center => s + f / 2.0, JustifyContent::End => s + f,
            JustifyContent::SpaceBetween | JustifyContent::SpaceAround | JustifyContent::SpaceEvenly => s, }
    }

    fn gap(j: JustifyContent, f: f32, n: usize) -> Option<f32> {
        match j { JustifyContent::SpaceBetween if n > 1 => Some(f / (n - 1) as f32),
            JustifyContent::SpaceAround => Some(f / n as f32), JustifyContent::SpaceEvenly => Some(f / (n + 1) as f32), _ => None }
    }
}

fn parse_justify(s: &str) -> JustifyContent {
    match s { "center" => JustifyContent::Center, "end" => JustifyContent::End,
        "space-between" => JustifyContent::SpaceBetween, "space-around" => JustifyContent::SpaceAround,
        "space-evenly" => JustifyContent::SpaceEvenly, _ => JustifyContent::Start }
}
fn parse_align(s: &str) -> AlignItems {
    match s { "center" => AlignItems::Center, "end" => AlignItems::End, _ => AlignItems::Start }
}

impl Default for LayoutContext { fn default() -> Self { Self::new() } }
