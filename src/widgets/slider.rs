//! Slider Widget - 滑动条

use crate::core::WidgetId;
use crate::event::{Event, EventContext, EventResult, MouseButton};
use crate::geometry::{Color, Rect, Size};
use crate::layout::{BoxStyle, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::visual::{FillStrokeStyle, LayeredElement, Stroke, VisualElement};
use crate::widget::Widget;
use kurbo::Rect as KurboRect;
use vello_cpu::color::AlphaColor;

const SLIDER_HEIGHT: f32 = 20.0;
const TRACK_HEIGHT: f64 = 4.0;
const THUMB_SIZE: f64 = 16.0;
const THUMB_RADIUS: f64 = 8.0;

const NEUTRAL_200: Color = Color(AlphaColor::from_rgb8(234, 234, 234));
const NEUTRAL_400: Color = Color(AlphaColor::from_rgb8(166, 166, 166));
const THEME_PRIMARY: Color = Color(AlphaColor::from_rgb8(0, 120, 212));
const WHITE: Color = Color(AlphaColor::from_rgb8(255, 255, 255));

type ValueCallback = Box<dyn FnMut(f32)>;

/// 滑动条组件
pub struct Slider {
    value: f32,
    min: f32,
    max: f32,
    step: Option<f32>,
    dirty: bool,
    bounds: Rect,
    dragging: bool,
    value_callbacks: Vec<ValueCallback>,
}

impl Slider {
    pub fn new() -> Self {
        Self {
            value: 0.0,
            min: 0.0,
            max: 100.0,
            step: None,
            dirty: false,
            bounds: Rect::zero(),
            dragging: false,
            value_callbacks: Vec::new(),
        }
    }

    /// 设置取值范围
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max;
        self.value = self.value.clamp(min, max);
        self
    }

    /// 设置初始值
    pub fn value(mut self, value: f32) -> Self {
        self.value = value.clamp(self.min, self.max);
        self.dirty = true;
        self
    }

    /// 设置步长
    pub fn step(mut self, step: f32) -> Self {
        self.step = Some(step.max(0.0));
        self
    }

    /// 当前值
    pub fn current_value(&self) -> f32 {
        self.value
    }

    /// 设置当前值（可变）
    pub fn set_value(&mut self, value: f32) {
        let new_value = self.snap(value.clamp(self.min, self.max));
        if (self.value - new_value).abs() > f32::EPSILON {
            self.value = new_value;
            self.dirty = true;
            self.fire_value_callbacks();
        }
    }

    /// 注册数值变化回调
    pub fn on_value_changed<F>(mut self, f: F) -> Self
    where
        F: FnMut(f32) + 'static,
    {
        self.value_callbacks.push(Box::new(f));
        self
    }

    fn snap(&self, value: f32) -> f32 {
        match self.step {
            Some(step) if step > 0.0 => {
                let steps = (value / step).round();
                steps * step
            }
            _ => value,
        }
    }

    fn value_from_x(&self, x: f32) -> f32 {
        if self.bounds.width <= 0.0 {
            return self.min;
        }
        let ratio = ((x - self.bounds.x) / self.bounds.width).clamp(0.0, 1.0);
        self.snap(self.min + ratio * (self.max - self.min))
    }

    fn fire_value_callbacks(&mut self) {
        let value = self.value;
        for cb in &mut self.value_callbacks {
            cb(value);
        }
    }

    fn update_from_event(&mut self, x: f32) {
        let new_value = self.value_from_x(x);
        if (self.value - new_value).abs() > f32::EPSILON {
            self.value = new_value;
            self.dirty = true;
            self.fire_value_callbacks();
        }
    }
}

impl Default for Slider {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Slider {
    crate::impl_widget_any!(Slider);

    fn type_name(&self) -> &'static str {
        "Slider"
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        LayoutNode::new(id).with_box_style(BoxStyle {
            min_size: Size::new(60.0, SLIDER_HEIGHT),
            max_size: Size::new(f32::INFINITY, SLIDER_HEIGHT),
            ..Default::default()
        })
    }

    fn render(&mut self, layout: &LayoutNode, _ctx: &ViewContext) -> Vec<LayeredElement> {
        self.bounds = layout.computed.content_box;
        let mut elements = Vec::new();
        let bounds = self.bounds;

        let track_y = bounds.y + (bounds.height - TRACK_HEIGHT as f32) / 2.0;
        let track_rect = KurboRect::new(
            bounds.x as f64,
            track_y as f64,
            (bounds.x + bounds.width) as f64,
            (track_y + TRACK_HEIGHT as f32) as f64,
        );

        // 未填充轨道
        elements.push(LayeredElement::default_layer(VisualElement::RoundedRect {
            rect: track_rect,
            radius: TRACK_HEIGHT / 2.0,
            style: FillStrokeStyle {
                fill: Some(NEUTRAL_200),
                stroke: None,
            },
        }));

        // 已填充部分
        let ratio = if self.max > self.min {
            ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0) as f64
        } else {
            0.0
        };
        let fill_width = bounds.width as f64 * ratio;
        if fill_width > 0.0 {
            let fill_rect = KurboRect::new(
                track_rect.x0,
                track_rect.y0,
                track_rect.x0 + fill_width,
                track_rect.y1,
            );
            elements.push(LayeredElement::default_layer(VisualElement::RoundedRect {
                rect: fill_rect,
                radius: TRACK_HEIGHT / 2.0,
                style: FillStrokeStyle {
                    fill: Some(THEME_PRIMARY),
                    stroke: None,
                },
            }));
        }

        // 滑块
        let thumb_center_x = bounds.x as f64 + bounds.width as f64 * ratio;
        let thumb_center_y = bounds.y as f64 + bounds.height as f64 / 2.0;
        let thumb_rect = KurboRect::new(
            thumb_center_x - THUMB_SIZE / 2.0,
            thumb_center_y - THUMB_SIZE / 2.0,
            thumb_center_x + THUMB_SIZE / 2.0,
            thumb_center_y + THUMB_SIZE / 2.0,
        );

        elements.push(LayeredElement::default_layer(VisualElement::RoundedRect {
            rect: thumb_rect,
            radius: THUMB_RADIUS,
            style: FillStrokeStyle {
                fill: Some(WHITE),
                stroke: Some(Stroke {
                    color: if self.dragging {
                        THEME_PRIMARY
                    } else {
                        NEUTRAL_400
                    },
                    width: if self.dragging { 2.0 } else { 1.0 },
                }),
            },
        }));

        elements
    }

    fn handle_event(&mut self, event: &Event, ctx: &EventContext) -> EventResult {
        match event {
            Event::MouseDown {
                button: MouseButton::Left,
                x,
                ..
            } => {
                self.dragging = true;
                self.update_from_event(*x);
                ctx.request_render();
                ctx.stop_propagation();
                EventResult::Stop
            }
            Event::MouseMove { x, .. } => {
                if self.dragging {
                    self.update_from_event(*x);
                    ctx.request_render();
                }
                EventResult::Continue
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                if self.dragging {
                    self.dragging = false;
                    ctx.request_render();
                }
                EventResult::Continue
            }
            _ => EventResult::Continue,
        }
    }

    fn can_focus(&self) -> bool {
        true
    }
}
