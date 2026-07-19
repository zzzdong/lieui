//! ProgressBar Widget - 进度条

use crate::core::WidgetId;
use crate::geometry::{Color, Size};
use crate::layout::{BoxStyle, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::visual::{FillStrokeStyle, LayeredElement, VisualElement};
use crate::widget::Widget;
use kurbo::Rect as KurboRect;
use vello_cpu::color::AlphaColor;

const PROGRESS_HEIGHT: f32 = 8.0;
const PROGRESS_RADIUS: f64 = 4.0;

const NEUTRAL_200: Color = Color(AlphaColor::from_rgb8(234, 234, 234));
const THEME_PRIMARY: Color = Color(AlphaColor::from_rgb8(0, 120, 212));

/// 进度条组件
pub struct ProgressBar {
    progress: f32,
    dirty: bool,
}

impl ProgressBar {
    pub fn new() -> Self {
        Self {
            progress: 0.0,
            dirty: false,
        }
    }

    /// 设置进度值，范围 [0.0, 1.0]
    pub fn progress(mut self, value: f32) -> Self {
        self.progress = value.clamp(0.0, 1.0);
        self
    }

    /// 当前进度
    pub fn current_progress(&self) -> f32 {
        self.progress
    }

    /// 设置进度（可变）
    pub fn set_progress(&mut self, value: f32) {
        let new_value = value.clamp(0.0, 1.0);
        if (self.progress - new_value).abs() > f32::EPSILON {
            self.progress = new_value;
            self.dirty = true;
        }
    }
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ProgressBar {
    crate::impl_widget_any!(ProgressBar);

    fn type_name(&self) -> &'static str {
        "ProgressBar"
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        LayoutNode::new(id)
            .with_box_style(BoxStyle {
                min_size: Size::new(60.0, PROGRESS_HEIGHT),
                max_size: Size::new(f32::INFINITY, PROGRESS_HEIGHT),
                ..Default::default()
            })
            .with_fixed_size(Size::new(100.0, PROGRESS_HEIGHT))
    }

    fn render(&mut self, layout: &LayoutNode, _ctx: &ViewContext) -> Vec<LayeredElement> {
        let mut elements = Vec::new();
        let bounds = layout.computed.content_box;

        let track_rect = KurboRect::new(
            bounds.x as f64,
            bounds.y as f64,
            (bounds.x + bounds.width) as f64,
            (bounds.y + bounds.height) as f64,
        );

        // 背景轨道
        elements.push(LayeredElement::default_layer(VisualElement::RoundedRect {
            rect: track_rect,
            radius: PROGRESS_RADIUS,
            style: FillStrokeStyle {
                fill: Some(NEUTRAL_200),
                stroke: None,
            },
        }));

        // 进度填充
        let fill_width = bounds.width as f64 * self.progress as f64;
        if fill_width > 0.0 {
            let fill_rect = KurboRect::new(
                track_rect.x0,
                track_rect.y0,
                track_rect.x0 + fill_width,
                track_rect.y1,
            );
            elements.push(LayeredElement::default_layer(VisualElement::RoundedRect {
                rect: fill_rect,
                radius: PROGRESS_RADIUS,
                style: FillStrokeStyle {
                    fill: Some(THEME_PRIMARY),
                    stroke: None,
                },
            }));
        }

        elements
    }
}
