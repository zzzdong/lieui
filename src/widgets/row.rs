// src/widgets/row.rs

//! Row Widget - 水平布局容器

use crate::core::WidgetId;
use crate::geometry::Rect;
use crate::layout::{AlignItems, FlexStyle, JustifyContent, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::RenderNode;
use crate::widget::Widget;

pub struct Row {
    bounds: Rect,
    children: Vec<WidgetId>,
    pub(crate) spacing: f32,
    /// 是否扩展填满父容器
    expand: bool,
}

impl Row {
    pub fn new() -> Self {
        Self {
            bounds: Rect::zero(),
            children: Vec::new(),
            spacing: 0.0,
            expand: false,
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
                .align(AlignItems::Center)
                .justify(JustifyContent::Center)
                .gap(self.spacing),
        );

        // 如果 expand，设置弹性属性
        if self.expand {
            node = node.with_flex_grow(1.0);
        }

        node
    }

    fn render(&mut self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            RenderNode::view(computed.content_box)
        } else {
            RenderNode::view(Rect::zero())
        }
    }

    fn children(&self) -> &[WidgetId] {
        &self.children
    }

    fn add_child(&mut self, child_id: WidgetId) {
        self.children.push(child_id);
    }

    fn remove_child(&mut self, child_id: WidgetId) {
        self.children.retain(|&id| id != child_id);
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
