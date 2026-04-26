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
}

impl Row {
    pub fn new() -> Self {
        Self {
            bounds: Rect::zero(),
            children: Vec::new(),
            spacing: 0.0,
        }
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
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
        LayoutNode::new(id).with_flex(
            FlexStyle::row()
                .align(AlignItems::Center)
                .justify(JustifyContent::Center)
                .gap(self.spacing),
        )
    }

    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            RenderNode::view(computed.content_box)
        } else {
            RenderNode::view(Rect::zero())
        }
    }

    fn children(&self) -> &[WidgetId] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<WidgetId> {
        &mut self.children
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
