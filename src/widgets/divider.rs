//! Divider Widget - 分隔线

use crate::core::WidgetId;
use crate::geometry::{Color, Size};
use crate::layout::{BoxStyle, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::visual::{LayeredElement, Stroke, VisualElement};
use crate::widget::Widget;
use kurbo::Point;
use vello_cpu::color::AlphaColor;

const DIVIDER_HEIGHT: f32 = 1.0;
const DEFAULT_COLOR: Color = Color(AlphaColor::from_rgb8(220, 220, 220));

/// 水平分隔线组件
pub struct Divider {
    color: Color,
    thickness: f32,
}

impl Divider {
    pub fn new() -> Self {
        Self {
            color: DEFAULT_COLOR,
            thickness: DIVIDER_HEIGHT,
        }
    }

    /// 设置分隔线颜色
    pub fn color(mut self, color: impl Into<Color>) -> Self {
        self.color = color.into();
        self
    }

    /// 设置分隔线粗细
    pub fn thickness(mut self, thickness: f32) -> Self {
        self.thickness = thickness.max(0.0);
        self
    }
}

impl Default for Divider {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Divider {
    crate::impl_widget_any!(Divider);

    fn type_name(&self) -> &'static str {
        "Divider"
    }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        LayoutNode::new(id)
            .with_box_style(BoxStyle {
                min_size: Size::new(0.0, self.thickness),
                max_size: Size::new(f32::INFINITY, self.thickness),
                ..Default::default()
            })
            .with_fixed_size(Size::new(0.0, self.thickness))
    }

    fn render(&mut self, layout: &LayoutNode, _ctx: &ViewContext) -> Vec<LayeredElement> {
        let bounds = layout.computed.content_box;
        let y = bounds.y + bounds.height / 2.0;

        vec![LayeredElement::default_layer(VisualElement::Line {
            start: Point::new(bounds.x as f64, y as f64),
            end: Point::new((bounds.x + bounds.width) as f64, y as f64),
            style: Stroke {
                color: self.color,
                width: self.thickness as f64,
            },
        })]
    }
}
