//! Container Widget - 带样式的矩形容器
//!
//! 父子关系由 WidgetTree 集中管理，Container 不再维护 children 列表

use crate::core::WidgetId;
use crate::geometry::{Color, Rect, Size};
use crate::layout::{BoxStyle, EdgeInsets, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::visual::{BoxShadowDef, FillStrokeStyle, LayeredElement, Stroke, VisualElement};
use crate::widget::Widget;
use kurbo::Rect as KurboRect;

pub struct Container {
    bounds: Rect,
    background: Option<Color>,
    border_color: Option<Color>,
    border_width: f32,
    border_radius: f32,
    padding: f32,
    box_shadow: Option<BoxShadowDef>,
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

    pub fn box_shadow(mut self, shadow: BoxShadowDef) -> Self {
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

    fn render(&mut self, layout: &LayoutNode, _ctx: &ViewContext) -> Vec<LayeredElement> {
        let mut elements = Vec::new();

        let border_box = layout.computed.border_box;
        let rect = KurboRect::new(
            border_box.x as f64,
            border_box.y as f64,
            (border_box.x + border_box.width) as f64,
            (border_box.y + border_box.height) as f64,
        );

        // 创建填充/描边样式
        let style = FillStrokeStyle {
            fill: self.background,
            stroke: if self.border_color.is_some() {
                Some(Stroke {
                    color: self.border_color.unwrap(),
                    width: self.border_width as f64,
                })
            } else {
                None
            },
        };

        // 根据是否有边框半径选择矩形类型
        let background_elem = if self.border_radius > 0.0 {
            VisualElement::RoundedRect {
                rect,
                radius: self.border_radius as f64,
                style,
            }
        } else {
            VisualElement::Rect { rect, style }
        };

        elements.push(LayeredElement::default_layer(background_elem));

        // 添加阴影
        if let Some(shadow) = &self.box_shadow {
            let shadow_def = shadow.clone();
            let shadow_elem = VisualElement::BoxShadow {
                rect,
                radius: self.border_radius as f64,
                shadow: shadow_def,
            };
            elements.push(LayeredElement::default_layer(shadow_elem));
        }

        elements
    }

    fn is_container(&self) -> bool {
        true
    }

    fn bounds(&self) -> Option<Rect> {
        Some(self.bounds)
    }
}
