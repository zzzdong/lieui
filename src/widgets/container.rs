use crate::core::WidgetId;
use crate::geometry::{Color, Rect, Size};
use crate::layout::{BoxStyle, EdgeInsets, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::{BoxShadow, RenderNode};
use crate::widget::Widget;

pub struct Container {
    bounds: Rect,
    children: Vec<WidgetId>,
    background: Option<Color>,
    border_color: Option<Color>,
    border_width: f32,
    border_radius: f32,
    padding: f32,
    box_shadow: Option<BoxShadow>,
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Container {
    pub fn new() -> Self {
        Self {
            bounds: Rect::zero(),
            children: Vec::new(),
            background: None,
            border_color: None,
            border_width: 0.0,
            border_radius: 0.0,
            padding: 0.0,
            box_shadow: None,
        }
    }

    pub fn background(mut self, color: impl Into<Color>) -> Self {
        self.background = Some(color.into());
        self
    }

    pub fn border(mut self, color: impl Into<Color>, width: f32) -> Self {
        self.border_color = Some(color.into());
        self.border_width = width;
        self
    }

    pub fn border_radius(mut self, radius: f32) -> Self {
        self.border_radius = radius;
        self
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding;
        self
    }

    pub fn box_shadow(mut self, shadow: BoxShadow) -> Self {
        self.box_shadow = Some(shadow);
        self
    }
}

impl Widget for Container {
    crate::impl_widget_any!(Container);

    fn type_name(&self) -> &'static str {
        "Container"
    }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        // Container 使用无限尺寸策略，由外部约束决定
        LayoutNode::new(id).with_box_style(BoxStyle {
            margin: EdgeInsets::ZERO,
            padding: EdgeInsets::all(self.padding),
            border: EdgeInsets::all(self.border_width),
            min_size: Size::ZERO,
            max_size: Size::new(f32::INFINITY, f32::INFINITY),
        })
    }

    fn render(&mut self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            let border_box = computed.border_box;
            let mut node = RenderNode::div(border_box);

            if let Some(bg) = &self.background {
                node = node.background(*bg);
            }
            if let Some(bc) = &self.border_color {
                node = node.border_color(*bc).border_width(self.border_width);
            }
            if self.border_radius > 0.0 {
                node = node.border_radius(self.border_radius);
            }
            if let Some(shadow) = &self.box_shadow {
                node = node.box_shadow(shadow.clone());
            }

            node
        } else {
            RenderNode::div(Rect::zero())
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
