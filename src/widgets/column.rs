// src/widgets/column.rs

//! Column Widget - 垂直布局容器

use crate::core::WidgetId;
use crate::geometry::Rect;
use crate::layout::{AlignItems, FlexStyle, JustifyContent, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::RenderNode;
use crate::widget::Widget;

pub struct Column {
    bounds: Rect,
    children: Vec<WidgetId>,
    pub(crate) spacing: f32,
    /// 是否扩展填满父容器
    expand: bool,
    /// 主轴对齐方式
    justify: JustifyContent,
}

impl Column {
    pub fn new() -> Self {
        Self {
            bounds: Rect::zero(),
            children: Vec::new(),
            spacing: 0.0,
            expand: false,
            justify: JustifyContent::Center,
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

    /// 设置主轴对齐方式
    pub fn justify(mut self, justify: JustifyContent) -> Self {
        self.justify = justify;
        self
    }
}

impl Widget for Column {
    crate::impl_widget_any!(Column);

    fn type_name(&self) -> &'static str {
        "Column"
    }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        // Column 使用 Flex 布局，垂直方向，子元素居中
        let mut node = LayoutNode::new(id).with_flex(
            FlexStyle::column()
                .align(AlignItems::Center)
                .justify(self.justify)
                .gap(self.spacing),
        );

        // 如果 expand，设置弹性属性
        if self.expand {
            node = node.with_flex_grow(1.0);
        }

        node
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

impl Default for Column {
    fn default() -> Self {
        Self::new()
    }
}
