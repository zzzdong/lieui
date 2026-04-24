// src/layout/context.rs
//! 布局上下文 - 约束收集与位置计算

use crate::core::{ViewContext, WidgetId};
use crate::geometry::{Point, Rect, Size};
use crate::layout::box_model::ComputedLayout;
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
            Self::compute_node(root, Point::ZERO, viewport);
        }
    }

    fn compute_node(node: &mut LayoutNode, position: Point, available: Size) -> Size {
        // 1. 计算 margin 后的可用空间
        let outer = node.box_style.margin.deflate(available);

        // 2. 处理子元素（先复制 flex_style 避免借用冲突）
        let flex_style = node.flex_style.clone();
        if !node.children.is_empty() {
            if let Some(flex) = flex_style {
                Self::compute_flex_children(node, &flex, position, outer);
            } else {
                Self::compute_default_children(node, position, outer);
            }
        }

        // 3. 确定内容大小
        let content_size = Self::resolve_content_size(node, outer);

        // 4. 应用约束
        let final_content = content_size.clamp(node.box_style.min_size, node.box_style.max_size);

        // 5. 计算各个 box
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

        node.computed = Some(ComputedLayout {
            margin_box,
            border_box,
            padding_box,
            content_box,
        });

        margin_box.size()
    }

    // ========================================================================
    // 子元素计算
    // ========================================================================

    fn compute_default_children(node: &mut LayoutNode, position: Point, available: Size) {
        let padding = &node.box_style.padding;
        let border = &node.box_style.border;
        let margin = &node.box_style.margin;

        let child_start = Point::new(
            position.x + margin.left + border.left + padding.left,
            position.y + margin.top + border.top + padding.top,
        );

        let child_available = padding.deflate(border.deflate(available));

        for child in &mut node.children {
            Self::compute_node(child, child_start, child_available);
        }
    }

    fn compute_flex_children(
        node: &mut LayoutNode,
        flex: &FlexStyle,
        position: Point,
        available: Size,
    ) {
        use crate::layout::JustifyContent;

        let padding = &node.box_style.padding;
        let border = &node.box_style.border;
        let margin = &node.box_style.margin;

        let child_available = padding.deflate(border.deflate(available));

        // 1. 先计算所有子元素的期望尺寸
        let mut child_sizes: Vec<Size> = node
            .children
            .iter()
            .map(|child| Self::resolve_intrinsic_size(child, child_available))
            .collect();

        // 2. 分配弹性空间
        let (main_axis, cross_axis) = match flex.direction {
            FlexDirection::Column => (child_available.height, child_available.width),
            FlexDirection::Row => (child_available.width, child_available.height),
        };

        let total_main: f32 = match flex.direction {
            FlexDirection::Column => {
                child_sizes.iter().map(|s| s.height).sum::<f32>()
                    + flex.gap * (node.children.len().saturating_sub(1)) as f32
            }
            FlexDirection::Row => {
                child_sizes.iter().map(|s| s.width).sum::<f32>()
                    + flex.gap * (node.children.len().saturating_sub(1)) as f32
            }
        };

        let free_space = main_axis - total_main;

        // 3. 如果有剩余空间，按 flex_grow 分配
        if free_space > 0.0 {
            let total_grow: f32 = node.children.iter().map(|c| c.flex_grow).sum();
            if total_grow > 0.0 {
                for (i, child) in node.children.iter_mut().enumerate() {
                    let ratio = child.flex_grow / total_grow;
                    match flex.direction {
                        FlexDirection::Column => child_sizes[i].height += free_space * ratio,
                        FlexDirection::Row => child_sizes[i].width += free_space * ratio,
                    }
                }
            }
        }

        // 4. 计算主轴起始偏移（根据 justify_content）
        let child_start = Point::new(
            position.x + margin.left + border.left + padding.left,
            position.y + margin.top + border.top + padding.top,
        );

        let (_used_main, final_free_space) = if free_space > 0.0 {
            let used = match flex.direction {
                FlexDirection::Column => {
                    child_sizes.iter().map(|s| s.height).sum::<f32>()
                        + flex.gap * (node.children.len().saturating_sub(1)) as f32
                }
                FlexDirection::Row => {
                    child_sizes.iter().map(|s| s.width).sum::<f32>()
                        + flex.gap * (node.children.len().saturating_sub(1)) as f32
                }
            };
            (used, main_axis - used)
        } else {
            (total_main, 0.0)
        };

        let mut offset = match flex.direction {
            FlexDirection::Column => {
                let base = child_start.y;
                match flex.justify_content {
                    JustifyContent::Start => base,
                    JustifyContent::Center => base + final_free_space / 2.0,
                    JustifyContent::End => base + final_free_space,
                    JustifyContent::SpaceBetween => base,
                    JustifyContent::SpaceAround => {
                        base + final_free_space / node.children.len() as f32 / 2.0
                    }
                    JustifyContent::SpaceEvenly => {
                        base + final_free_space / (node.children.len() + 1) as f32
                    }
                }
            }
            FlexDirection::Row => {
                let base = child_start.x;
                match flex.justify_content {
                    JustifyContent::Start => base,
                    JustifyContent::Center => base + final_free_space / 2.0,
                    JustifyContent::End => base + final_free_space,
                    JustifyContent::SpaceBetween => base,
                    JustifyContent::SpaceAround => {
                        base + final_free_space / node.children.len() as f32 / 2.0
                    }
                    JustifyContent::SpaceEvenly => {
                        base + final_free_space / (node.children.len() + 1) as f32
                    }
                }
            }
        };

        // 5. 排列子元素
        let child_count = node.children.len();
        for (i, child) in node.children.iter_mut().enumerate() {
            let child_pos = match flex.direction {
                FlexDirection::Column => {
                    let x = match flex.align_items {
                        AlignItems::Start => child_start.x,
                        AlignItems::Center => {
                            child_start.x + (cross_axis - child_sizes[i].width) / 2.0
                        }
                        AlignItems::End => child_start.x + cross_axis - child_sizes[i].width,
                        AlignItems::Stretch => child_start.x,
                    };
                    Point::new(x, offset)
                }
                FlexDirection::Row => {
                    let y = match flex.align_items {
                        AlignItems::Start => child_start.y,
                        AlignItems::Center => {
                            child_start.y + (cross_axis - child_sizes[i].height) / 2.0
                        }
                        AlignItems::End => child_start.y + cross_axis - child_sizes[i].height,
                        AlignItems::Stretch => child_start.y,
                    };
                    Point::new(offset, y)
                }
            };

            let child_avail = match flex.direction {
                FlexDirection::Column => Size::new(cross_axis, child_sizes[i].height),
                FlexDirection::Row => Size::new(child_sizes[i].width, cross_axis),
            };

            Self::compute_node(child, child_pos, child_avail);

            // 更新 offset，考虑 justify_content 的间距模式
            let gap = match flex.justify_content {
                JustifyContent::SpaceBetween if child_count > 1 => {
                    final_free_space / (child_count - 1) as f32
                }
                JustifyContent::SpaceAround => final_free_space / child_count as f32,
                JustifyContent::SpaceEvenly => final_free_space / (child_count + 1) as f32,
                _ => flex.gap,
            };

            match flex.direction {
                FlexDirection::Column => offset += child_sizes[i].height + gap,
                FlexDirection::Row => offset += child_sizes[i].width + gap,
            }
        }
    }

    // ========================================================================
    // 尺寸解析
    // ========================================================================

    fn resolve_intrinsic_size(node: &LayoutNode, available: Size) -> Size {
        let max_width = if available.width.is_finite() {
            Some(available.width)
        } else {
            None
        };
        node.intrinsic_size.measure(max_width)
    }

    fn resolve_content_size(node: &LayoutNode, available: Size) -> Size {
        // 对于有固有尺寸的节点（如 Text），使用固有尺寸
        // 对于没有固有尺寸的节点（如 Container），使用可用空间
        let max_width = if available.width.is_finite() {
            Some(available.width)
        } else {
            None
        };
        let measured = node.intrinsic_size.measure(max_width);

        // 如果测量结果为 0，使用可用空间（对于容器类组件）
        let width = if measured.width > 0.0 {
            measured.width
        } else {
            available.width
        };
        let height = if measured.height > 0.0 {
            measured.height
        } else {
            available.height
        };

        Size::new(width, height)
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
