// src/widgets/row.rs

//! Row Widget - 水平布局容器
//!
//! 父子关系由 WidgetTree 集中管理，Row 不再维护 children 列表

use crate::core::WidgetId;
use crate::geometry::Rect;
use crate::layout::{AlignItems, FlexStyle, JustifyContent, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::visual::LayeredElement;
use crate::widget::Widget;

pub struct Row {
    bounds: Rect,
    pub(crate) spacing: f32,
    /// 是否扩展填满父容器
    expand: bool,
    /// 交叉轴对齐方式
    align: AlignItems,
}

impl Row {
    pub fn new() -> Self {
        Self {
            bounds: Rect::zero(),
            spacing: 0.0,
            expand: false,
            align: AlignItems::Center,
        }
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// 设置是否扩展填满父容器
    pub fn expand(mut self, expand: bool) -> Self {
        self.expand = expand;
        self
    }

    /// 设置交叉轴对齐方式
    pub fn align(mut self, align: AlignItems) -> Self {
        self.align = align;
        self
    }
}

impl Widget for Row {
    crate::impl_widget_any!(Row);

    fn type_name(&self) -> &'static str {
        "Row"
    }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        // Row 使用 Flex 布局，水平方向，子元素在主轴和交叉轴都居中
        let mut node = LayoutNode::new(id).with_flex(
            FlexStyle::row()
                .align(self.align)
                .justify(JustifyContent::Center)
                .gap(self.spacing),
        );

        // 如果 expand，设置弹性属性
        if self.expand {
            node = node.with_flex_grow(1.0);
        }

        node
    }

    fn render(&mut self, _layout: &LayoutNode, _ctx: &ViewContext) -> Vec<LayeredElement> {
        // Row 是布局容器，本身不渲染任何内容
        Vec::new()
    }

    fn is_container(&self) -> bool {
        true
    }

    fn bounds(&self) -> Option<Rect> {
        Some(self.bounds)
    }
}

impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}
