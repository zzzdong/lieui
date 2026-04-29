// src/layout/context.rs
//! 布局上下文 - 约束收集与位置计算

use crate::core::{ViewContext, WidgetId};
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
    // Phase 1: 收集约束
    // ========================================================================

    /// 从 Widget 树收集布局约束
    pub fn collect(&mut self, root_id: WidgetId, view: &ViewContext) {
        self.root = Some(self.collect_node(root_id, view));
    }

    fn collect_node(&self, id: WidgetId, view: &ViewContext) -> LayoutNode {
        let widget = view.get_widget(id).expect("Widget not found");

        // 获取 widget 的布局节点，传入正确的 id
        let mut node = widget.layout(id);

        // 递归收集子元素
        for child_id in widget.children() {
            let child_node = self.collect_node(*child_id, view);
            node.children.push(child_node);
        }

        node
    }

    // ========================================================================
    // Phase 2: 计算位置
    // ========================================================================

    /// 根据 viewport 计算所有节点的绝对位置
    pub fn compute(&mut self, viewport: Size) {
        if let Some(root) = &mut self.root {
            let constraint = LayoutConstraint::tight(viewport);
            Self::layout_node(root, constraint, Point::ZERO);
        }
    }

    // ========================================================================
    // 只读测量
    // ========================================================================

    /// 只读测量节点，返回 border-box 尺寸（content + padding + border）
    ///
    /// 不产生副作用（不写 computed），用于父容器在布局前评估子节点大小。
    fn measure_node(node: &LayoutNode, constraint: LayoutConstraint) -> Size {
        let max_avail = constraint.max_size();
        let content_avail = node.box_style.content_available(max_avail);

        let content_size = if node.children.is_empty() {
            // 叶子节点：测量固有尺寸
            let max_w = if content_avail.width.is_finite() {
                Some(content_avail.width)
            } else {
                None
            };
            node.intrinsic_size.measure(max_w)
        } else if let Some(flex) = &node.flex_style {
            // Flex 容器
            Self::measure_flex_content(node, flex, content_avail)
        } else {
            // 默认流式容器
            Self::measure_flow_content(node, content_avail)
        };

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

    /// 只读测量 Flex 容器的内容尺寸
    fn measure_flex_content(node: &LayoutNode, flex: &FlexStyle, content_avail: Size) -> Size {
        let is_row = flex.direction == FlexDirection::Row;
        let cross_avail = if is_row {
            content_avail.height
        } else {
            content_avail.width
        };

        // 子节点测量约束：主轴无约束（自然尺寸），交叉轴约束在容器交叉轴可用空间内
        let child_constraint = if is_row {
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
        };

        let mut total_main = flex.gap * (node.children.len().saturating_sub(1)) as f32;
        let mut max_cross = 0.0f32;

        for child in &node.children {
            let child_size = Self::measure_node(child, child_constraint);
            let child_main = if is_row {
                child_size.width
            } else {
                child_size.height
            };
            let child_cross = if is_row {
                child_size.height
            } else {
                child_size.width
            };
            total_main += child_main;
            max_cross = max_cross.max(child_cross);
        }

        // 容器交叉轴大小
        let container_cross = if flex.align_items == AlignItems::Stretch {
            cross_avail
        } else {
            max_cross
        };

        if is_row {
            Size::new(total_main, container_cross)
        } else {
            Size::new(container_cross, total_main)
        }
    }

    /// 只读测量流式容器的内容尺寸（纵向堆叠）
    fn measure_flow_content(node: &LayoutNode, content_avail: Size) -> Size {
        let child_constraint = LayoutConstraint {
            min_width: 0.0,
            max_width: content_avail.width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };

        let mut total_height = 0.0;
        let mut max_width: f32 = 0.0;

        for child in &node.children {
            let child_size = Self::measure_node(child, child_constraint);
            total_height += child_size.height;
            max_width = max_width.max(child_size.width);
        }

        Size::new(max_width, total_height)
    }

    // ========================================================================
    // 布局计算
    // ========================================================================

    /// 递归布局节点，写 computed，返回 margin-box 尺寸
    fn layout_node(node: &mut LayoutNode, constraint: LayoutConstraint, position: Point) -> Size {
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
            // 叶子节点
            let max_w = if content_avail.width.is_finite() {
                Some(content_avail.width)
            } else {
                None
            };
            node.intrinsic_size.measure(max_w)
        } else if let Some(flex) = &node.flex_style.clone() {
            // Flex 容器
            Self::flex_layout(node, flex, content_avail, content_origin)
        } else {
            // 默认流式容器
            Self::flow_layout(node, content_avail, content_origin)
        };

        let final_content = content_size.clamp(node.box_style.min_size, node.box_style.max_size);

        // 计算各个 box 层级
        let border = &node.box_style.border;
        let padding = &node.box_style.padding;
        let margin = &node.box_style.margin;

        let content_box = Rect::new(
            position.x + margin.left + border.left + padding.left,
            position.y + margin.top + border.top + padding.top,
            final_content.width,
            final_content.height,
        );

        let padding_box = Rect::new(
            content_box.x - padding.left,
            content_box.y - padding.top,
            final_content.width + padding.horizontal_sum(),
            final_content.height + padding.vertical_sum(),
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

        let hit_rect = border_box; // 命中测试以 border_box 为界
        let hit_radius = node.border_radius.unwrap_or(0.0);
        let hit_shape = RoundedRect::new(hit_rect, hit_radius);

        node.computed = Some(ComputedLayout {
            margin_box,
            border_box,
            padding_box,
            content_box,
            hit_shape: Some(hit_shape),
        });

        margin_box.size()
    }

    /// 流式布局：纵向堆叠子元素（修正了原实现中所有子元素位置重叠的 bug）
    fn flow_layout(node: &mut LayoutNode, content_avail: Size, content_origin: Point) -> Size {
        let child_constraint = LayoutConstraint::loose(content_avail);

        let mut y_offset = 0.0;
        let mut max_width: f32 = 0.0;

        for child in &mut node.children {
            let child_pos = Point::new(content_origin.x, content_origin.y + y_offset);
            let size = Self::layout_node(child, child_constraint, child_pos);
            y_offset += size.height;
            max_width = max_width.max(size.width);
        }

        Size::new(max_width, y_offset)
    }

    /// Flex 布局：5 阶段算法
    ///
    /// Phase 1 — 测量子元素（使用 measure_node）
    /// Phase 2 — 分配弹性空间（flex-grow / flex-shrink）
    /// Phase 3 — 交叉轴尺寸计算
    /// Phase 4 — 子元素定位
    /// Phase 5 — 返回容器内容尺寸
    fn flex_layout(
        node: &mut LayoutNode,
        flex: &FlexStyle,
        content_avail: Size,
        content_origin: Point,
    ) -> Size {
        let child_count = node.children.len();
        if child_count == 0 {
            return Size::ZERO;
        }

        // ====================================================================
        // Phase 1 — 测量子元素
        // ====================================================================
        let is_row = flex.direction == FlexDirection::Row;
        let cross_avail = if is_row {
            content_avail.height
        } else {
            content_avail.width
        };

        // 子节点测量约束：主轴无约束（自然尺寸），交叉轴约束在容器交叉轴可用空间内
        let child_measure_constraint = if is_row {
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
        };

        let mut child_main_sizes: Vec<f32> = Vec::with_capacity(child_count);
        let mut child_cross_sizes: Vec<f32> = Vec::with_capacity(child_count);

        for child in &node.children {
            let child_size = Self::measure_node(child, child_measure_constraint);
            let child_main = if let Some(basis) = child.flex_basis {
                basis
            } else if is_row {
                child_size.width
            } else {
                child_size.height
            };
            let child_cross = if is_row {
                child_size.height
            } else {
                child_size.width
            };
            child_main_sizes.push(child_main);
            child_cross_sizes.push(child_cross);
        }

        // Total main size consumed by all children (including gaps)
        let total_main: f32 = child_main_sizes.iter().sum::<f32>()
            + flex.gap * (child_count.saturating_sub(1)) as f32;

        // Available space on the main axis (from the parent constraint)
        let available_main = Self::main_size(content_avail, flex.direction);

        // Positive = surplus (distribute via flex-grow)
        // Negative  = deficit (distribute via flex-shrink)
        let free_space = available_main - total_main;

        // ====================================================================
        // Phase 2 — Distribute free space
        // ====================================================================
        if free_space.abs() > 0.001 {
            if free_space > 0.0 {
                // flex-grow: distribute surplus proportionally to each child's flex_grow
                let total_grow: f32 = node.children.iter().map(|c| c.flex_grow).sum();
                if total_grow > 0.0 {
                    for (i, child) in node.children.iter().enumerate() {
                        child_main_sizes[i] += free_space * (child.flex_grow / total_grow);
                    }
                }
            } else {
                // flex-shrink: distribute deficit proportionally to (main_size × flex_shrink)
                let total_weight: f32 = node
                    .children
                    .iter()
                    .enumerate()
                    .map(|(i, c)| c.flex_shrink * child_main_sizes[i])
                    .sum();
                if total_weight > 0.0 {
                    for (i, child) in node.children.iter().enumerate() {
                        let shrink =
                            free_space * (child.flex_shrink * child_main_sizes[i] / total_weight);
                        child_main_sizes[i] = (child_main_sizes[i] + shrink).max(0.0);
                    }
                }
            }
        }

        // Recalculated total after distribution (used for positioning)
        let actual_total_main: f32 = child_main_sizes.iter().sum::<f32>()
            + flex.gap * (child_count.saturating_sub(1)) as f32;

        let final_free_space = available_main - actual_total_main;

        // ====================================================================
        // Phase 3 — Cross-axis sizing
        // ====================================================================
        let available_cross = Self::cross_size(content_avail, flex.direction);

        // Container cross: when fill_available or Stretch, use the full available
        // cross space. Otherwise use the max of children's natural cross sizes.
        let fill_available = node.flex_grow > 0.0;
        let container_cross = if fill_available || flex.align_items == AlignItems::Stretch {
            available_cross
        } else {
            child_cross_sizes.iter().copied().fold(0.0, f32::max)
        };

        // ====================================================================
        // Phase 4 — Position children along main + cross axes
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

            // Natural cross size for non-Stretch alignment
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

            Self::layout_node(child, child_constraint, child_pos);

            let gap = Self::justify_gap(flex.justify_content, final_free_space, child_count)
                .unwrap_or(flex.gap);
            offset += child_main + gap;
        }

        // ====================================================================
        // Phase 5 — Return container's own content size
        // ====================================================================
        if fill_available {
            content_avail
        } else {
            Self::make_size(actual_total_main, container_cross, flex.direction)
        }
    }

    // ========================================================================
    // Flex 轴辅助
    // ========================================================================

    /// Extract the main-axis component from a Size
    fn main_size(size: Size, direction: FlexDirection) -> f32 {
        match direction {
            FlexDirection::Row => size.width,
            FlexDirection::Column => size.height,
        }
    }

    /// Extract the cross-axis component from a Size
    fn cross_size(size: Size, direction: FlexDirection) -> f32 {
        match direction {
            FlexDirection::Row => size.height,
            FlexDirection::Column => size.width,
        }
    }

    /// Extract the main-axis component from a Point
    fn main_component(point: Point, direction: FlexDirection) -> f32 {
        match direction {
            FlexDirection::Row => point.x,
            FlexDirection::Column => point.y,
        }
    }

    /// Build a Size from (main, cross) components
    fn make_size(main: f32, cross: f32, direction: FlexDirection) -> Size {
        match direction {
            FlexDirection::Row => Size::new(main, cross),
            FlexDirection::Column => Size::new(cross, main),
        }
    }

    /// Starting offset along the main axis for a given justify-content value.
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

    /// Gap between items for a given justify-content value.
    /// Returns `None` when the caller should fall back to `flex.gap`.
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

    // ========================================================================
    // 公开查询
    // ========================================================================

    pub fn bounds(&self, id: WidgetId) -> Option<Rect> {
        self.root.as_ref()?.find(id)?.bounds()
    }

    pub fn computed(&self, id: WidgetId) -> Option<&ComputedLayout> {
        self.root.as_ref()?.find(id)?.computed.as_ref()
    }

    pub fn hit_test(&self, point: Point) -> Option<WidgetId> {
        self.root.as_ref()?.hit_test(point)
    }
}

impl Default for LayoutContext {
    fn default() -> Self {
        Self::new()
    }
}
