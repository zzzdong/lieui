//! LayoutNode + LayoutContext

use crate::core::ElementId;
use crate::layout::box_model::{BoxStyle, ComputedLayout, IntrinsicSize, LayoutConstraint};
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
    pub fn find(&self, target: ElementId) -> Option<&LayoutNode> {
        if self.id == target { return Some(self); }
        for child in &self.children { if let Some(f) = child.find(target) { return Some(f); } }
        None
    }
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
        let full = Self::build_tree(root_id, constraint, tree);
        self.root = Some(full);
    }

    fn build_tree(id: ElementId, constraint: LayoutConstraint, tree: &ElementTree) -> LayoutNode {
        let intrinsic = tree.intrinsic(id);
        let mut node = LayoutNode::new(id);
        node.intrinsic = intrinsic;
        let w = constraint.max_width.min(intrinsic.width).max(constraint.min_width);
        let h = constraint.max_height.min(intrinsic.height).max(constraint.min_height);
        node.computed = ComputedLayout { x: 0.0, y: 0.0, width: w, height: h };
        for child_id in tree.children_of(id) {
            node.children.push(Self::build_tree(child_id, constraint, tree));
        }
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
