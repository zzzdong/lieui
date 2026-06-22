// src/layout/context.rs
//! 布局上下文 - 约束收集与位置计算
//!
//! API 设计：
//! - 对外暴露 collect、compute、compute_incremental、mark_dirty、clear_dirty
//! - 内部使用两阶段布局：measure（不写状态）→ layout（写状态）
//! - 支持增量布局：只计算 dirty 子树

use crate::core::WidgetId;
use crate::core::layers::Layers;
use crate::geometry::types::RoundedRect;
use crate::geometry::{Point, Rect, Size};
use crate::layout::JustifyContent;
use crate::layout::box_model::ComputedLayout;
use crate::layout::constraint::LayoutConstraint;
use crate::layout::flex::{AlignItems, FlexDirection, FlexStyle};
use crate::layout::node::LayoutNode;

/// 布局上下文
pub struct LayoutContext {
    /// 布局树根节点
    pub root: Option<LayoutNode>,
}

impl LayoutContext {
    /// 创建新的布局上下文
    pub fn new() -> Self {
        Self { root: None }
    }

    // ========================================================================
    // 公开 API
    // ========================================================================

    /// Phase 1: 从 Widget 树收集布局约束
    pub fn collect(&mut self, root_id: WidgetId, layers: &Layers) {
        self.root = Some(self.collect_node(root_id, layers));
    }

    /// Phase 2: 全量布局计算（忽略 dirty 标记）
    pub fn compute(&mut self, viewport: Size) {
        if let Some(root) = &mut self.root {
            let constraint = LayoutConstraint::tight(viewport);
            Self::layout_node_full(root, constraint, Point::ZERO);
        }
    }

    /// Phase 2: 增量布局计算（只计算 dirty 子树）
    ///
    /// 返回是否需要渲染（有任何节点被重新布局）
    pub fn compute_incremental(&mut self, viewport: Size) -> bool {
        if let Some(root) = &mut self.root {
            if !root.is_dirty() && !root.has_dirty_child() {
                return false; // 无需重新布局
            }
            let constraint = LayoutConstraint::tight(viewport);
            Self::layout_node_incremental(root, constraint, Point::ZERO);
            return true;
        }
        false
    }

    /// 标记指定节点为 dirty
    pub fn mark_dirty(&mut self, id: WidgetId) {
        if let Some(root) = &mut self.root
            && let Some(node) = root.find_mut(id)
        {
            node.mark_dirty();
        }
    }

    /// 标记指定节点及其所有子节点为 dirty
    pub fn mark_dirty_with_children(&mut self, id: WidgetId) {
        if let Some(root) = &mut self.root
            && let Some(node) = root.find_mut(id)
        {
            node.propagate_dirty_down();
        }
    }

    /// 清除所有节点的 dirty 标记
    pub fn clear_all_dirty(&mut self) {
        if let Some(root) = &mut self.root {
            Self::clear_dirty_recursive(root);
        }
    }

    /// 检查是否有任何 dirty 节点
    pub fn has_dirty(&self) -> bool {
        self.root
            .as_ref()
            .is_some_and(|r| r.is_dirty() || r.has_dirty_child())
    }

    /// 收集所有 dirty 节点的 ID
    pub fn dirty_ids(&self) -> Vec<WidgetId> {
        self.root
            .as_ref()
            .map_or(Vec::new(), |r| r.collect_dirty_ids())
    }

    /// 查询节点边界
    pub fn bounds(&self, id: WidgetId) -> Option<Rect> {
        Some(self.root.as_ref()?.find(id)?.bounds())
    }

    /// 查询节点计算结果
    pub fn computed(&self, id: WidgetId) -> Option<&ComputedLayout> {
        self.root.as_ref()?.find(id).map(|n| &n.computed)
    }

    /// 命中测试
    pub fn hit_test(&self, point: Point) -> Option<WidgetId> {
        self.root.as_ref()?.hit_test(point)
    }

    // ========================================================================
    // 内部：约束收集
    // ========================================================================

    fn collect_node(&self, id: WidgetId, layers: &Layers) -> LayoutNode {
        let widget = layers.tree.get_widget_immut(id).expect("Widget not found");
        // 从 Widget 的 dirty 状态同步到 LayoutNode
        let mut node = widget.layout(id).with_dirty(widget.is_dirty());
        let children = layers.tree.children_of(id);
        drop(widget);

        for child_id in children {
            node.children.push(self.collect_node(child_id, layers));
        }
        node
    }

    // ========================================================================
    // 内部：dirty 清除
    // ========================================================================

    fn clear_dirty_recursive(node: &mut LayoutNode) {
        node.clear_dirty();
        for child in &mut node.children {
            Self::clear_dirty_recursive(child);
        }
    }

    // ========================================================================
    // 内部：测量（不写状态）
    // ========================================================================

    /// 测量节点的内容尺寸（不写 computed）
    ///
    /// 返回 content-box 尺寸，用于父容器评估子节点大小。
    fn measure_content(node: &LayoutNode, content_avail: Size) -> Size {
        if node.children.is_empty() {
            // 叶子节点：测量固有尺寸
            Self::measure_leaf(node, content_avail)
        } else if let Some(flex) = &node.flex_style {
            // Flex 容器
            Self::measure_flex(node, flex, content_avail)
        } else {
            // 流式容器
            Self::measure_flow(node, content_avail)
        }
    }

    /// 测量叶子节点的固有尺寸
    fn measure_leaf(node: &LayoutNode, content_avail: Size) -> Size {
        let max_w = if content_avail.width.is_finite() {
            Some(content_avail.width)
        } else {
            None
        };
        node.intrinsic_size.measure(max_w)
    }

    /// 测量 Flex 容器的内容尺寸
    fn measure_flex(node: &LayoutNode, flex: &FlexStyle, content_avail: Size) -> Size {
        let is_row = flex.direction == FlexDirection::Row;
        let cross_avail = if is_row {
            content_avail.height
        } else {
            content_avail.width
        };

        // 子节点测量约束：主轴无约束，交叉轴约束在可用空间内
        let child_constraint = Self::flex_measure_constraint(is_row, cross_avail);

        let mut total_main = flex.gap * (node.children.len().saturating_sub(1)) as f32;
        let mut max_cross = 0.0f32;

        for child in &node.children {
            let child_size = Self::measure_node(child, child_constraint);
            total_main += Self::main_of(child_size, is_row);
            max_cross = max_cross.max(Self::cross_of(child_size, is_row));
        }

        let container_cross = if flex.align_items == AlignItems::Stretch {
            cross_avail
        } else {
            max_cross
        };

        Self::make_size(total_main, container_cross, flex.direction)
    }

    /// 测量流式容器的内容尺寸（纵向堆叠）
    fn measure_flow(node: &LayoutNode, content_avail: Size) -> Size {
        let child_constraint = LayoutConstraint::loose(content_avail);

        let mut total_height = 0.0;
        let mut max_width: f32 = 0.0;

        for child in &node.children {
            let child_size = Self::measure_node(child, child_constraint);
            total_height += child_size.height;
            max_width = max_width.max(child_size.width);
        }

        Size::new(max_width, total_height)
    }

    /// 测量节点，返回 border-box 尺寸（content + padding + border）
    fn measure_node(node: &LayoutNode, constraint: LayoutConstraint) -> Size {
        let max_avail = constraint.max_size();
        let content_avail = node.box_style.content_available(max_avail);

        let content_size = Self::measure_content(node, content_avail);
        let clamped = content_size.clamp(node.box_style.min_size, node.box_style.max_size);

        // Border-box: content + padding + border
        Size::new(
            clamped.width
                + node.box_style.padding.horizontal_sum()
                + node.box_style.border.horizontal_sum(),
            clamped.height
                + node.box_style.padding.vertical_sum()
                + node.box_style.border.vertical_sum(),
        )
    }

    // ========================================================================
    // 内部：布局（写状态）
    // ========================================================================

    /// 全量布局：忽略 dirty 标记，强制计算所有节点
    fn layout_node_full(
        node: &mut LayoutNode,
        constraint: LayoutConstraint,
        position: Point,
    ) -> Size {
        Self::layout_node_impl(node, constraint, position, true)
    }

    /// 增量布局：只计算 dirty 节点，跳过 clean 节点
    fn layout_node_incremental(
        node: &mut LayoutNode,
        constraint: LayoutConstraint,
        position: Point,
    ) -> Size {
        Self::layout_node_impl(node, constraint, position, false)
    }

    /// 布局实现
    ///
    /// 参数：
    /// - force: 是否强制计算（忽略 dirty）
    fn layout_node_impl(
        node: &mut LayoutNode,
        constraint: LayoutConstraint,
        position: Point,
        force: bool,
    ) -> Size {
        // 如果不强制且节点 clean，跳过计算
        if !force && !node.is_dirty() {
            // 但仍需检查子节点是否有 dirty
            if !node.has_dirty_child() {
                return node.computed.margin_box.size(); // 完全跳过
            }
            // 子节点有 dirty，需要递归处理子节点
            // 但当前节点的 computed 已有效，只需更新子节点位置
            return Self::layout_children_incremental(node, constraint, position);
        }

        // 需要重新计算
        let max_avail = constraint.max_size();
        let content_avail = node.box_style.content_available(max_avail);

        let content_origin = Point::new(
            position.x
                + node.box_style.margin.left
                + node.box_style.border.left
                + node.box_style.padding.left,
            position.y
                + node.box_style.margin.top
                + node.box_style.border.top
                + node.box_style.padding.top,
        );

        // 根据节点类型计算内容尺寸
        let content_size = if node.children.is_empty() {
            Self::measure_leaf(node, content_avail)
        } else if let Some(flex) = &node.flex_style.clone() {
            Self::flex_layout(node, flex, content_avail, content_origin, force)
        } else {
            Self::flow_layout(node, content_avail, content_origin, force)
        };

        // 对 tight 约束维度：强制扩展到约束大小
        // 这确保根容器（收到 tight 约束）能完全覆盖视口
        let final_content = Size::new(
            if constraint.min_width == constraint.max_width && constraint.min_width > 0.0 {
                content_avail.width.max(content_size.width)
            } else {
                content_size.width
            },
            if constraint.min_height == constraint.max_height && constraint.min_height > 0.0 {
                content_avail.height.max(content_size.height)
            } else {
                content_size.height
            },
        )
        .clamp(node.box_style.min_size, node.box_style.max_size);
        Self::compute_boxes(node, position, final_content);
        node.clear_dirty(); // 计算完成后清除 dirty

        node.computed.margin_box.size()
    }

    /// 增量布局子节点（当前节点 clean，子节点有 dirty）
    fn layout_children_incremental(
        node: &mut LayoutNode,
        constraint: LayoutConstraint,
        position: Point,
    ) -> Size {
        let content_avail = node.box_style.content_available(constraint.max_size());
        let content_origin = Point::new(
            position.x
                + node.box_style.margin.left
                + node.box_style.border.left
                + node.box_style.padding.left,
            position.y
                + node.box_style.margin.top
                + node.box_style.border.top
                + node.box_style.padding.top,
        );

        // 根据容器类型递归布局 dirty 子节点
        if let Some(flex) = &node.flex_style.clone() {
            Self::flex_layout_incremental_children(node, flex, content_avail, content_origin);
        } else {
            Self::flow_layout_incremental_children(node, content_avail, content_origin);
        }

        node.computed.margin_box.size()
    }

    /// 计算 box 层级并写入 computed
    fn compute_boxes(node: &mut LayoutNode, position: Point, content_size: Size) {
        let border = &node.box_style.border;
        let padding = &node.box_style.padding;
        let margin = &node.box_style.margin;

        let content_box = Rect::new(
            position.x + margin.left + border.left + padding.left,
            position.y + margin.top + border.top + padding.top,
            content_size.width,
            content_size.height,
        );

        let padding_box = Rect::new(
            content_box.x - padding.left,
            content_box.y - padding.top,
            content_size.width + padding.horizontal_sum(),
            content_size.height + padding.vertical_sum(),
        );

        let border_box = Rect::new(
            padding_box.x - border.left,
            padding_box.y - border.top,
            padding_box.width + border.horizontal_sum(),
            padding_box.height + border.vertical_sum(),
        );

        let margin_box = Rect::new(
            border_box.x - margin.left,
            border_box.y - margin.top,
            border_box.width + margin.horizontal_sum(),
            border_box.height + margin.vertical_sum(),
        );

        let hit_radius = node.border_radius.unwrap_or(0.0);
        let hit_shape = RoundedRect::new(border_box, hit_radius);

        node.computed = ComputedLayout {
            margin_box,
            border_box,
            padding_box,
            content_box,
            hit_shape: Some(hit_shape),
        };
    }

    /// 流式布局：纵向堆叠子元素
    ///
    /// 流容器（类似 CSS `display: block`）的行为：
    /// - 宽度：始终扩展到可用宽度（100% 行为）
    /// - 高度：由内容决定（tight 约束的强制扩展在 layout_node_impl 中统一处理）
    fn flow_layout(
        node: &mut LayoutNode,
        content_avail: Size,
        content_origin: Point,
        force: bool,
    ) -> Size {
        let child_constraint = LayoutConstraint::loose(content_avail);

        let mut y_offset = 0.0;
        let mut max_width: f32 = 0.0;

        for child in &mut node.children {
            let child_pos = Point::new(content_origin.x, content_origin.y + y_offset);
            let size = if force {
                Self::layout_node_full(child, child_constraint, child_pos)
            } else {
                Self::layout_node_incremental(child, child_constraint, child_pos)
            };
            y_offset += size.height;
            max_width = max_width.max(size.width);
        }

        // 流容器宽度行为：扩展到可用空间（CSS block 元素 width: 100%）
        let width = if content_avail.width.is_finite() {
            content_avail.width.max(max_width)
        } else {
            max_width
        };

        Size::new(width, y_offset)
    }

    /// 流式布局：增量处理 dirty 子节点
    fn flow_layout_incremental_children(
        node: &mut LayoutNode,
        content_avail: Size,
        content_origin: Point,
    ) {
        let child_constraint = LayoutConstraint::loose(content_avail);
        let mut y_offset = 0.0;

        // 使用已缓存的子节点高度来计算位置
        for child in &mut node.children {
            let child_pos = Point::new(content_origin.x, content_origin.y + y_offset);
            if child.is_dirty() || child.has_dirty_child() {
                Self::layout_node_incremental(child, child_constraint, child_pos);
            }
            y_offset += child.computed.margin_box.height;
        }
    }

    /// Flex 布局：5 阶段算法
    ///
    /// Phase 1 — 测量子元素（使用 measure_node，不写 computed）
    /// Phase 2 — 分配弹性空间（flex-grow / flex-shrink）
    /// Phase 3 — 交叉轴尺寸计算
    /// Phase 4 — 子元素定位（调用 layout_node，写 computed）
    /// Phase 5 — 返回容器内容尺寸
    fn flex_layout(
        node: &mut LayoutNode,
        flex: &FlexStyle,
        content_avail: Size,
        content_origin: Point,
        force: bool,
    ) -> Size {
        let child_count = node.children.len();
        if child_count == 0 {
            return Size::ZERO;
        }

        let is_row = flex.direction == FlexDirection::Row;
        let cross_avail = Self::cross_size(content_avail, flex.direction);
        let available_main = Self::main_size(content_avail, flex.direction);

        // ====================================================================
        // Phase 1 — 测量子元素
        // ====================================================================
        let child_measure_constraint = Self::flex_measure_constraint(is_row, cross_avail);

        let mut child_main_sizes: Vec<f32> = Vec::with_capacity(child_count);
        let mut child_cross_sizes: Vec<f32> = Vec::with_capacity(child_count);

        for child in &node.children {
            let child_size = Self::measure_node(child, child_measure_constraint);
            let child_main = child
                .flex_basis
                .unwrap_or_else(|| Self::main_of(child_size, is_row));
            let child_cross = Self::cross_of(child_size, is_row);
            child_main_sizes.push(child_main);
            child_cross_sizes.push(child_cross);
        }

        let total_main: f32 = child_main_sizes.iter().sum::<f32>()
            + flex.gap * (child_count.saturating_sub(1)) as f32;
        let free_space = available_main - total_main;

        // ====================================================================
        // Phase 2 — 分配弹性空间
        // ====================================================================
        Self::distribute_flex_space(node, &mut child_main_sizes, free_space);

        let actual_total_main: f32 = child_main_sizes.iter().sum::<f32>()
            + flex.gap * (child_count.saturating_sub(1)) as f32;
        let final_free_space = available_main - actual_total_main;

        // ====================================================================
        // Phase 3 — 交叉轴尺寸
        // ====================================================================
        let fill_available = node.flex_grow > 0.0;
        let container_cross = if fill_available || flex.align_items == AlignItems::Stretch {
            cross_avail
        } else {
            child_cross_sizes.iter().copied().fold(0.0, f32::max)
        };

        // ====================================================================
        // Phase 4 — 定位子元素
        // ====================================================================
        let main_start = Self::main_component(content_origin, flex.direction);
        let mut offset = Self::justify_offset(
            flex.justify_content,
            main_start,
            final_free_space,
            child_count,
        );

        for (i, child) in node.children.iter_mut().enumerate() {
            let child_main = child_main_sizes[i];
            let natural_cross = child_cross_sizes[i];

            let (child_cross, cross_offset) = match flex.align_items {
                AlignItems::Stretch => (container_cross, 0.0),
                AlignItems::Start => (natural_cross, 0.0),
                AlignItems::Center => (natural_cross, (container_cross - natural_cross) / 2.0),
                AlignItems::End => (natural_cross, container_cross - natural_cross),
            };

            let (child_pos, child_constraint) = match flex.direction {
                FlexDirection::Row => (
                    Point::new(offset, content_origin.y + cross_offset),
                    LayoutConstraint::tight(Size::new(child_main, child_cross)),
                ),
                FlexDirection::Column => (
                    Point::new(content_origin.x + cross_offset, offset),
                    LayoutConstraint::tight(Size::new(child_cross, child_main)),
                ),
            };

            if force {
                Self::layout_node_full(child, child_constraint, child_pos);
            } else {
                Self::layout_node_incremental(child, child_constraint, child_pos);
            }

            let gap = Self::justify_gap(flex.justify_content, final_free_space, child_count)
                .unwrap_or(flex.gap);
            offset += child_main + gap;
        }

        // ====================================================================
        // Phase 5 — 返回容器内容尺寸
        // ====================================================================
        if fill_available {
            content_avail
        } else {
            Self::make_size(actual_total_main, container_cross, flex.direction)
        }
    }

    /// Flex 布局：增量处理 dirty 子节点
    fn flex_layout_incremental_children(
        node: &mut LayoutNode,
        flex: &FlexStyle,
        content_avail: Size,
        content_origin: Point,
    ) {
        let child_count = node.children.len();
        if child_count == 0 {
            return;
        }

        let is_row = flex.direction == FlexDirection::Row;
        let cross_avail = Self::cross_size(content_avail, flex.direction);
        let available_main = Self::main_size(content_avail, flex.direction);

        // 使用已缓存的子节点尺寸
        let mut child_main_sizes: Vec<f32> = Vec::with_capacity(child_count);
        for child in &node.children {
            child_main_sizes.push(Self::main_of(child.computed.margin_box.size(), is_row));
        }

        let total_main: f32 = child_main_sizes.iter().sum::<f32>()
            + flex.gap * (child_count.saturating_sub(1)) as f32;
        let final_free_space = available_main - total_main;

        let container_cross = cross_avail;

        // 定位 dirty 子节点
        let main_start = Self::main_component(content_origin, flex.direction);
        let mut offset = Self::justify_offset(
            flex.justify_content,
            main_start,
            final_free_space,
            child_count,
        );

        for (i, child) in node.children.iter_mut().enumerate() {
            let child_main = child_main_sizes[i];
            let child_cross = Self::cross_of(child.computed.margin_box.size(), is_row);
            let cross_offset = match flex.align_items {
                AlignItems::Stretch => 0.0,
                AlignItems::Start => 0.0,
                AlignItems::Center => (container_cross - child_cross) / 2.0,
                AlignItems::End => container_cross - child_cross,
            };

            let child_pos = match flex.direction {
                FlexDirection::Row => Point::new(offset, content_origin.y + cross_offset),
                FlexDirection::Column => Point::new(content_origin.x + cross_offset, offset),
            };

            if child.is_dirty() || child.has_dirty_child() {
                let child_constraint = LayoutConstraint::tight(child.computed.margin_box.size());
                Self::layout_node_incremental(child, child_constraint, child_pos);
            }

            let gap = Self::justify_gap(flex.justify_content, final_free_space, child_count)
                .unwrap_or(flex.gap);
            offset += child_main + gap;
        }
    }

    /// 分配弹性空间（flex-grow / flex-shrink）
    fn distribute_flex_space(node: &LayoutNode, sizes: &mut [f32], free_space: f32) {
        if free_space.abs() < 0.001 {
            return;
        }

        if free_space > 0.0 {
            // flex-grow
            let total_grow: f32 = node.children.iter().map(|c| c.flex_grow).sum();
            if total_grow > 0.0 {
                for (i, child) in node.children.iter().enumerate() {
                    sizes[i] += free_space * (child.flex_grow / total_grow);
                }
            }
        } else {
            // flex-shrink
            let total_weight: f32 = node
                .children
                .iter()
                .enumerate()
                .map(|(i, c)| c.flex_shrink * sizes[i])
                .sum();
            if total_weight > 0.0 {
                for (i, child) in node.children.iter().enumerate() {
                    let shrink = free_space * (child.flex_shrink * sizes[i] / total_weight);
                    sizes[i] = (sizes[i] + shrink).max(0.0);
                }
            }
        }
    }

    // ========================================================================
    // Flex 辅助方法
    // ========================================================================

    /// 构建 Flex 测量约束
    fn flex_measure_constraint(is_row: bool, cross_avail: f32) -> LayoutConstraint {
        if is_row {
            LayoutConstraint {
                min_width: 0.0,
                max_width: f32::INFINITY,
                min_height: 0.0,
                max_height: cross_avail,
            }
        } else {
            LayoutConstraint {
                min_width: 0.0,
                max_width: cross_avail,
                min_height: 0.0,
                max_height: f32::INFINITY,
            }
        }
    }

    fn main_size(size: Size, direction: FlexDirection) -> f32 {
        match direction {
            FlexDirection::Row => size.width,
            FlexDirection::Column => size.height,
        }
    }

    fn cross_size(size: Size, direction: FlexDirection) -> f32 {
        match direction {
            FlexDirection::Row => size.height,
            FlexDirection::Column => size.width,
        }
    }

    fn main_component(point: Point, direction: FlexDirection) -> f32 {
        match direction {
            FlexDirection::Row => point.x,
            FlexDirection::Column => point.y,
        }
    }

    fn make_size(main: f32, cross: f32, direction: FlexDirection) -> Size {
        match direction {
            FlexDirection::Row => Size::new(main, cross),
            FlexDirection::Column => Size::new(cross, main),
        }
    }

    fn main_of(size: Size, is_row: bool) -> f32 {
        if is_row { size.width } else { size.height }
    }

    fn cross_of(size: Size, is_row: bool) -> f32 {
        if is_row { size.height } else { size.width }
    }

    fn justify_offset(
        justify: JustifyContent,
        start: f32,
        free_space: f32,
        child_count: usize,
    ) -> f32 {
        match justify {
            JustifyContent::Start => start,
            JustifyContent::Center => start + free_space / 2.0,
            JustifyContent::End => start + free_space,
            JustifyContent::SpaceBetween => start,
            JustifyContent::SpaceAround => start + free_space / child_count as f32 / 2.0,
            JustifyContent::SpaceEvenly => start + free_space / (child_count + 1) as f32,
        }
    }

    fn justify_gap(justify: JustifyContent, free_space: f32, child_count: usize) -> Option<f32> {
        match justify {
            JustifyContent::SpaceBetween if child_count > 1 => {
                Some(free_space / (child_count - 1) as f32)
            }
            JustifyContent::SpaceAround => Some(free_space / child_count as f32),
            JustifyContent::SpaceEvenly => Some(free_space / (child_count + 1) as f32),
            _ => None,
        }
    }
}

impl Default for LayoutContext {
    fn default() -> Self {
        Self::new()
    }
}
