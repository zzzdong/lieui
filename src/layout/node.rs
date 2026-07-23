//! LayoutNode — 布局树节点

use crate::core::ElementId;
use crate::geometry::{Point, Rect};
use crate::layout::box_model::{BoxStyle, ComputedLayout, IntrinsicSize};
use crate::layout::flex::{AlignItems, FlexDirection, JustifyContent};
use crate::view::node::NodeType;

#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub id: ElementId,
    pub style: BoxStyle,
    pub intrinsic: IntrinsicSize,
    pub computed: ComputedLayout,
    pub dirty: bool,
    pub children: Vec<LayoutNode>,
    /// 元素类型
    pub node_type: NodeType,
    /// 是否为 Flex 容器
    pub is_flex: bool,
    /// Flex 方向
    pub direction: FlexDirection,
    /// Flex 主轴对齐
    pub justify: JustifyContent,
    /// Flex 交叉轴对齐
    pub align: AlignItems,
    /// Flex 子节点间距
    pub spacing: f32,
}

impl LayoutNode {
    pub fn new(id: ElementId, node_type: NodeType) -> Self {
        Self {
            id,
            style: BoxStyle::new(),
            intrinsic: IntrinsicSize::zero(),
            computed: ComputedLayout::default(),
            dirty: true,
            children: Vec::new(),
            node_type,
            is_flex: false,
            direction: FlexDirection::Column,
            justify: JustifyContent::Start,
            align: AlignItems::Stretch,
            spacing: 4.0,
        }
    }

    pub fn bounds(&self) -> Rect {
        self.computed.rect()
    }

    pub fn contains(&self, p: Point) -> bool {
        self.computed.contains(p.x, p.y)
    }

    pub fn hit_test_rec(&self, px: f32, py: f32) -> Option<ElementId> {
        if !self.computed.contains(px, py) {
            return None;
        }
        // 优先命中更内层的节点，以支持嵌套监听（例如行可点击，行内的 Checkbox 也可点击）。
        for child in self.children.iter().rev() {
            if let Some(id) = child.hit_test_rec(px, py) {
                return Some(id);
            }
        }
        Some(self.id)
    }

    pub fn find(&self, id: ElementId) -> Option<&LayoutNode> {
        if self.id == id {
            return Some(self);
        }
        for child in &self.children {
            if let Some(found) = child.find(id) {
                return Some(found);
            }
        }
        None
    }

    pub fn find_mut(&mut self, id: ElementId) -> Option<&mut LayoutNode> {
        if self.id == id {
            return Some(self);
        }
        for child in &mut self.children {
            if let Some(found) = child.find_mut(id) {
                return Some(found);
            }
        }
        None
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn has_dirty_child(&self) -> bool {
        self.children
            .iter()
            .any(|c| c.is_dirty() || c.has_dirty_child())
    }

    pub fn propagate_dirty_down(&mut self) {
        self.dirty = true;
        for child in &mut self.children {
            child.propagate_dirty_down();
        }
    }
}
