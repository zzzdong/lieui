// src/layout/node.rs
//! 布局节点 - 约束与计算结果

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

/// 布局节点（约束与计算结果）
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

    /// Phase 2 计算结果
    pub computed: Option<ComputedLayout>,

    /// 是否参与事件交互（默认为 true）
    /// 设为 false 时，命中测试将跳过此节点及其子树
    pub interactive: bool,

    pub border_radius: Option<f32>,
}

impl LayoutNode {
    /// 创建新的布局节点
    pub fn new(id: WidgetId) -> Self {
        Self {
            id,
            box_style: BoxStyle::default(),
            flex_style: None,
            intrinsic_size: IntrinsicSize::Fixed(Size::ZERO),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            flex_basis: None,
            children: Vec::new(),
            computed: None,
            interactive: true,
            border_radius: None,
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

    /// 设置命中测试形状（默认为 border_box）
    pub fn with_border_radius(mut self, radius: Option<f32>) -> Self {
        self.border_radius = radius;
        self
    }

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

    /// 获取 content_box（便捷方法）
    pub fn bounds(&self) -> Option<Rect> {
        self.computed.map(|c| c.content_box)
    }

    /// 命中测试（递归）
    ///
    /// 返回命中的 WidgetId。如果 interactive 为 false，跳过此节点及其子树。
    pub fn hit_test(&self, point: Point) -> Option<WidgetId> {
        if !self.interactive {
            return None;
        }

        if let Some(computed) = &self.computed {
            // 使用带圆角的 border_box 作为命中形状
            let outer_hit = self.border_radius.map_or_else(
                || RoundedRect::new(computed.border_box, 0.0), // 无圆角用普通矩形
                |radius| RoundedRect::new(computed.border_box, radius),
            );

            if !outer_hit.contains(point) {
                return None;
            }

            // 判断是否在 content_box 内（content 区域通常无圆角，可直接用 Rect::contains）
            if computed.content_box.contains(point) {
                // 点在内容区，可能被子元素捕获
                for child in self.children.iter().rev() {
                    if let Some(id) = child.hit_test(point) {
                        return Some(id);
                    }
                }
                // 子元素都没命中，返回自己
                return Some(self.id);
            } else {
                // 点在 padding 区域（外圈），直接返回当前容器
                return Some(self.id);
            }
        }

        None
    }

    /// 输出为 XML（调试用）
    pub fn to_xml(&self, indent: usize) -> String {
        let spaces = "  ".repeat(indent);

        let bounds_str = if let Some(computed) = &self.computed {
            format!(
                " content=\"{:.1}x{:.1}\" margin_box=\"{:.1}x{:.1}\"",
                computed.content_box.width,
                computed.content_box.height,
                computed.margin_box.width,
                computed.margin_box.height
            )
        } else {
            String::new()
        };

        let children_xml = self
            .children
            .iter()
            .map(|c| c.to_xml(indent + 1))
            .collect::<String>();

        format!(
            "{}<LayoutNode id=\"{}\"{}{}>\n{}{}</LayoutNode>\n",
            spaces,
            self.id.0,
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

    #[test]
    fn test_layout_node() {
        let node = LayoutNode::new(WidgetId::new());
        assert_eq!(node.bounds(), None);
    }
}
