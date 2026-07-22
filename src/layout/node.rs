//! LayoutNode + LayoutContext

use crate::core::ElementId;
use crate::layout::box_model::{BoxStyle, ComputedLayout, IntrinsicSize, LayoutConstraint};
use crate::layout::measurable::{DefaultMeasurer, Measurable};
use crate::runtime::element::ElementTree;

#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub id: ElementId,
    pub style: BoxStyle,
    pub intrinsic: IntrinsicSize,
    pub computed: ComputedLayout,
    pub dirty: bool,
    pub children: Vec<LayoutNode>,
}

impl LayoutNode {
    pub fn new(id: ElementId) -> Self {
        Self { id, style: BoxStyle::new(), intrinsic: IntrinsicSize::zero(),
               computed: ComputedLayout::default(), dirty: true, children: Vec::new() }
    }
    pub fn bounds(&self) -> crate::geometry::Rect { self.computed.rect() }
}

#[derive(Debug, Clone)]
pub struct LayoutContext { pub root: Option<LayoutNode> }

impl LayoutContext {
    pub fn new() -> Self { Self { root: None } }

    pub fn hit_test(&self, point: crate::geometry::Point, _tree: &ElementTree) -> Option<ElementId> {
        self.root.as_ref().and_then(|r| r.hit_test_rec(point.x, point.y))
    }

    pub fn compute(&mut self, viewport: crate::geometry::Size, tree: &ElementTree) {
        let Some(root_id) = tree.root() else { return; };
        let constraint = LayoutConstraint::loose((viewport.width, viewport.height));
        let full = Self::build_tree(root_id, constraint, tree, None);
        self.root = Some(full);
    }

    fn build_tree(id: ElementId, constraint: LayoutConstraint, tree: &ElementTree, _parent_type: Option<&str>) -> LayoutNode {
        let measurer = DefaultMeasurer;
        let intrinsic = tree.props(id)
            .map(|p| measurer.measure(p, &constraint))
            .unwrap_or(IntrinsicSize::zero());
        tree.set_intrinsic(id, intrinsic);

        let type_name = tree.type_name(id).unwrap_or("");

        let mut y_off = 0.0f32;
        let mut x_off = 0.0f32;
        let spacing = 4.0f32;
        let mut children = Vec::new();

        for child_id in tree.children_of(id) {
            let child = Self::build_tree(child_id, constraint, tree, Some(type_name));
            match type_name {
                "column" => {
                    let mut c = child;
                    c.computed.x = 0.0;
                    c.computed.y = y_off;
                    y_off += c.computed.height + spacing;
                    children.push(c);
                }
                "row" => {
                    let mut c = child;
                    c.computed.x = x_off;
                    c.computed.y = 0.0;
                    x_off += c.computed.width + spacing;
                    children.push(c);
                }
                _ => {
                    children.push(child);
                }
            }
        }

        let w = if type_name == "column" {
            // Column 宽度为最大子宽度
            children.iter().map(|c| c.computed.width).fold(0.0f32, f32::max)
        } else {
            constraint.max_width.min(intrinsic.width).max(constraint.min_width)
        };
        let h = match type_name {
            "column" => y_off.max(1.0),
            "row" => children.iter().map(|c| c.computed.height).fold(0.0f32, f32::max),
            _ => constraint.max_height.min(intrinsic.height).max(constraint.min_height),
        };

        let mut node = LayoutNode::new(id);
        node.intrinsic = intrinsic;
        node.computed = ComputedLayout { x: 0.0, y: 0.0, width: w, height: h };
        node.children = children;
        node
    }
}

impl Default for LayoutContext { fn default() -> Self { Self::new() } }

impl LayoutNode {
    pub fn hit_test_rec(&self, px: f32, py: f32) -> Option<ElementId> {
        if !self.computed.contains(px, py) { return None; }
        for child in self.children.iter().rev() {
            if let Some(id) = child.hit_test_rec(px, py) { return Some(id); }
        }
        Some(self.id)
    }
}
