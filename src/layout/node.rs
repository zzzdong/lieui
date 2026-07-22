//! LayoutNode — 布局树节点

use crate::core::ElementId;
use crate::geometry::{Point, Rect};
use crate::layout::box_model::{BoxStyle, ComputedLayout, IntrinsicSize};
use crate::layout::flex::{AlignItems, JustifyContent};

#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub id: ElementId,
    pub style: BoxStyle,
    pub intrinsic: IntrinsicSize,
    pub computed: ComputedLayout,
    pub dirty: bool,
    pub children: Vec<LayoutNode>,
    /// 元素类型（"column"/"row"/"text"/"button" 等）
    pub type_name: &'static str,
    /// Flex 属性
    pub justify: JustifyContent,
    pub align: AlignItems,
}

impl LayoutNode {
    pub fn new(id: ElementId, type_name: &'static str) -> Self {
        Self {
            id,
            style: BoxStyle::new(),
            intrinsic: IntrinsicSize::zero(),
            computed: ComputedLayout::default(),
            dirty: true,
            children: Vec::new(),
            type_name,
            justify: JustifyContent::Start,
            align: AlignItems::Stretch,
        }
    }

    pub fn bounds(&self) -> Rect { self.computed.rect() }

    pub fn contains(&self, p: Point) -> bool { self.computed.contains(p.x, p.y) }

    pub fn hit_test_rec(&self, px: f32, py: f32) -> Option<ElementId> {
        if !self.computed.contains(px, py) { return None; }
        for child in self.children.iter().rev() {
            if let Some(id) = child.hit_test_rec(px, py) { return Some(id); }
        }
        Some(self.id)
    }

    pub fn find_mut(&mut self, id: ElementId) -> Option<&mut LayoutNode> {
        if self.id == id { return Some(self); }
        for child in &mut self.children {
            if let Some(found) = child.find_mut(id) { return Some(found); }
        }
        None
    }

    pub fn mark_dirty(&mut self) { self.dirty = true; }

    pub fn is_dirty(&self) -> bool { self.dirty }

    pub fn has_dirty_child(&self) -> bool {
        self.children.iter().any(|c| c.is_dirty() || c.has_dirty_child())
    }

    pub fn propagate_dirty_down(&mut self) {
        self.dirty = true;
        for child in &mut self.children {
            child.propagate_dirty_down();
        }
    }
}
