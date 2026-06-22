// src/layout/node.rs
//! 布局节点 - 约束与计算结果
//!
//! 设计改进：
//! - computed 字段改为必需，消除 Option 检查
//! - dirty 标记支持增量布局
//! - 提供 mark_dirty() 和 propagate_dirty() 方法

use crate::core::WidgetId;
use crate::geometry::types::RoundedRect;
use crate::geometry::{Point, Rect, Size};
use crate::layout::box_model::{BoxStyle, ComputedLayout};
use crate::layout::flex::FlexStyle;
use crate::layout::measurable::Measurable;

/// 固有尺寸（叶子节点使用，决定内容尺寸的方式）
#[derive(Clone)]
pub enum IntrinsicSize {
    /// 固定尺寸
    Fixed(Size),
    /// 可测量尺寸（Text 等）
    Measurable(Box<dyn Measurable>),
}

impl IntrinsicSize {
    /// 测量尺寸
    pub fn measure(&self, max_width: Option<f32>) -> Size {
        match self {
            Self::Fixed(size) => *size,
            Self::Measurable(measurable) => measurable.measure(max_width),
        }
    }
}

impl std::fmt::Debug for IntrinsicSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fixed(size) => f.debug_tuple("Fixed").field(size).finish(),
            Self::Measurable(_) => f.debug_tuple("Measurable").finish(),
        }
    }
}

impl Default for IntrinsicSize {
    fn default() -> Self {
        Self::Fixed(Size::ZERO)
    }
}

/// 布局节点
///
/// 改进：
/// - computed 字段为必需，消除 Option 检查开销
/// - dirty 标记支持增量布局
#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub id: WidgetId,

    /// 盒子模型约束
    pub box_style: BoxStyle,

    /// Flex 属性（容器使用）
    pub flex_style: Option<FlexStyle>,

    /// 固有尺寸（叶子节点使用）
    pub intrinsic_size: IntrinsicSize,

    /// Flex 弹性属性（作为子元素时使用）
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Option<f32>,

    /// 子节点
    pub children: Vec<LayoutNode>,

    /// 计算结果（必需字段）
    pub computed: ComputedLayout,

    /// 是否参与事件交互
    pub interactive: bool,

    /// 命中测试圆角
    pub border_radius: Option<f32>,

    /// 增量布局标记
    /// - true: 需要重新计算布局
    /// - false: 可以跳过计算（使用缓存的 computed）
    pub dirty: bool,
}

impl LayoutNode {
    /// 创建新的布局节点（带默认 computed）
    pub fn new(id: WidgetId) -> Self {
        Self {
            id,
            box_style: BoxStyle::default(),
            flex_style: None,
            intrinsic_size: IntrinsicSize::default(),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            flex_basis: None,
            children: Vec::new(),
            computed: ComputedLayout::default(),
            interactive: true,
            border_radius: None,
            dirty: true, // 新节点默认需要布局
        }
    }

    /// 设置盒子模型约束
    pub fn with_box_style(mut self, style: BoxStyle) -> Self {
        self.box_style = style;
        self
    }

    /// 设置 Flex 配置
    pub fn with_flex(mut self, flex: FlexStyle) -> Self {
        self.flex_style = Some(flex);
        self
    }

    /// 设置固有尺寸
    pub fn with_intrinsic_size(mut self, size: IntrinsicSize) -> Self {
        self.intrinsic_size = size;
        self
    }

    /// 设置固定尺寸
    pub fn with_fixed_size(self, size: Size) -> Self {
        self.with_intrinsic_size(IntrinsicSize::Fixed(size))
    }

    /// 设置 flex_grow
    pub fn with_flex_grow(mut self, grow: f32) -> Self {
        self.flex_grow = grow;
        self
    }

    /// 设置 flex_shrink
    pub fn with_flex_shrink(mut self, shrink: f32) -> Self {
        self.flex_shrink = shrink;
        self
    }

    /// 设置 flex_basis
    pub fn with_flex_basis(mut self, basis: f32) -> Self {
        self.flex_basis = Some(basis);
        self
    }

    /// 添加子节点
    pub fn add_child(mut self, child: LayoutNode) -> Self {
        self.children.push(child);
        self
    }

    /// 设置是否参与事件交互
    pub fn with_interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    /// 设置命中测试圆角
    pub fn with_border_radius(mut self, radius: Option<f32>) -> Self {
        self.border_radius = radius;
        self
    }

    /// 设置计算结果（Phase 2 使用）
    pub fn with_computed(mut self, computed: ComputedLayout) -> Self {
        self.computed = computed;
        self
    }

    /// 设置 dirty 标记
    pub fn with_dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }

    // ========================================================================
    // 增量布局方法
    // ========================================================================

    /// 标记节点为 dirty
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// 清除 dirty 标记
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// 检查是否 dirty
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 向下传播 dirty：将所有子节点标记为 dirty
    ///
    /// 当父节点布局变化时，子节点需要重新计算位置
    pub fn propagate_dirty_down(&mut self) {
        self.dirty = true;
        for child in &mut self.children {
            child.propagate_dirty_down();
        }
    }

    /// 检查是否有任何子节点是 dirty
    pub fn has_dirty_child(&self) -> bool {
        self.children.iter().any(|c| c.dirty || c.has_dirty_child())
    }

    /// 收集所有 dirty 节点的 ID
    pub fn collect_dirty_ids(&self) -> Vec<WidgetId> {
        let mut ids = Vec::new();
        self.collect_dirty_ids_into(&mut ids);
        ids
    }

    fn collect_dirty_ids_into(&self, ids: &mut Vec<WidgetId>) {
        if self.dirty {
            ids.push(self.id);
        }
        for child in &self.children {
            child.collect_dirty_ids_into(ids);
        }
    }

    // ========================================================================
    // 查询方法
    // ========================================================================

    /// 查找子节点
    pub fn find(&self, id: WidgetId) -> Option<&LayoutNode> {
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

    /// 查找子节点（可变）
    pub fn find_mut(&mut self, id: WidgetId) -> Option<&mut LayoutNode> {
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

    /// 获取 content_box
    pub fn bounds(&self) -> Rect {
        self.computed.content_box
    }

    /// 命中测试（递归）
    pub fn hit_test(&self, point: Point) -> Option<WidgetId> {
        if !self.interactive {
            return None;
        }

        let outer_hit = self.border_radius.map_or_else(
            || RoundedRect::new(self.computed.border_box, 0.0),
            |radius| RoundedRect::new(self.computed.border_box, radius),
        );

        if !outer_hit.contains(point) {
            return None;
        }

        if self.computed.content_box.contains(point) {
            for child in self.children.iter().rev() {
                if let Some(id) = child.hit_test(point) {
                    return Some(id);
                }
            }
            Some(self.id)
        } else {
            Some(self.id)
        }
    }

    /// 输出为 XML（调试用）
    pub fn to_xml(&self, indent: usize) -> String {
        let spaces = "  ".repeat(indent);

        let bounds_str = format!(
            " content=\"{:.1}x{:.1}\" marginBox=\"{:.1}x{:.1}\" dirty=\"{}\"",
            self.computed.content_box.width,
            self.computed.content_box.height,
            self.computed.margin_box.width,
            self.computed.margin_box.height,
            self.dirty
        );

        let children_xml = self
            .children
            .iter()
            .map(|c| c.to_xml(indent + 1))
            .collect::<String>();

        format!(
            "{}<LayoutNode id=\"{:?}\"{}{}>\n{}{}</LayoutNode>\n",
            spaces,
            self.id,
            if self.flex_style.is_some() {
                " flex=\"true\""
            } else {
                ""
            },
            bounds_str,
            children_xml,
            spaces,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    #[test]
    fn test_layout_node() {
        let mut map: SlotMap<WidgetId, ()> = SlotMap::with_key();
        let id = map.insert(());
        let node = LayoutNode::new(id);
        assert_eq!(node.bounds(), Rect::zero());
        assert!(node.is_dirty()); // 新节点默认 dirty
    }

    #[test]
    fn test_dirty_propagation() {
        let mut map: SlotMap<WidgetId, ()> = SlotMap::with_key();
        let parent_id = map.insert(());
        let child_id = map.insert(());

        let child = LayoutNode::new(child_id).with_dirty(false);
        let mut parent = LayoutNode::new(parent_id).add_child(child);

        // 清除 parent 的 dirty
        parent.clear_dirty();
        assert!(!parent.is_dirty());
        assert!(!parent.children[0].is_dirty());

        // 标记 parent dirty 并向下传播
        parent.propagate_dirty_down();
        assert!(parent.is_dirty());
        assert!(parent.children[0].is_dirty());
    }

    #[test]
    fn test_collect_dirty_ids() {
        let mut map: SlotMap<WidgetId, ()> = SlotMap::with_key();
        let id1 = map.insert(());
        let id2 = map.insert(());
        let id3 = map.insert(());

        let node1 = LayoutNode::new(id1).with_dirty(true);
        let node2 = LayoutNode::new(id2).with_dirty(false);
        let node3 = LayoutNode::new(id3).with_dirty(true);

        let root = node1.add_child(node2).add_child(node3);
        let dirty_ids = root.collect_dirty_ids();

        assert_eq!(dirty_ids.len(), 2);
        assert!(dirty_ids.contains(&id1));
        assert!(dirty_ids.contains(&id3));
    }
}
