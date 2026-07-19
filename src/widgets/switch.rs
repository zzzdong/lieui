//! Switch Widget - 开关

use crate::core::WidgetId;
use crate::event::{Event, EventContext, EventResult};
use crate::geometry::{Color, Size};
use crate::layout::{BoxStyle, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::visual::{FillStrokeStyle, LayeredElement, VisualElement};
use crate::widget::Widget;
use kurbo::Rect as KurboRect;
use vello_cpu::color::AlphaColor;

const SWITCH_WIDTH: f32 = 44.0;
const SWITCH_HEIGHT: f32 = 24.0;
const THUMB_MARGIN: f64 = 2.0;
const SWITCH_RADIUS: f64 = 12.0;

const NEUTRAL_300: Color = Color(AlphaColor::from_rgb8(200, 200, 200));
const NEUTRAL_400: Color = Color(AlphaColor::from_rgb8(166, 166, 166));
const THEME_PRIMARY: Color = Color(AlphaColor::from_rgb8(0, 120, 212));
const WHITE: Color = Color(AlphaColor::from_rgb8(255, 255, 255));

type ValueCallback = Box<dyn FnMut(bool)>;

/// 开关组件
pub struct Switch {
    on: bool,
    dirty: bool,
    value_callbacks: Vec<ValueCallback>,
}

impl Switch {
    pub fn new() -> Self {
        Self {
            on: false,
            dirty: false,
            value_callbacks: Vec::new(),
        }
    }

    /// 设置初始开关状态
    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    /// 是否开启
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// 设置开关状态（可变）
    pub fn set_on(&mut self, on: bool) {
        if self.on != on {
            self.on = on;
            self.dirty = true;
        }
    }

    /// 注册状态变化回调，参数为新的开关值
    pub fn on_changed<F>(mut self, f: F) -> Self
    where
        F: FnMut(bool) + 'static,
    {
        self.value_callbacks.push(Box::new(f));
        self
    }

    fn fire_value_callbacks(&mut self) {
        let value = self.on;
        for cb in &mut self.value_callbacks {
            cb(value);
        }
    }
}

impl Default for Switch {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Switch {
    crate::impl_widget_any!(Switch);

    fn type_name(&self) -> &'static str {
        "Switch"
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
                min_size: Size::new(SWITCH_WIDTH, SWITCH_HEIGHT),
                max_size: Size::new(SWITCH_WIDTH, SWITCH_HEIGHT),
                ..Default::default()
            })
            .with_fixed_size(Size::new(SWITCH_WIDTH, SWITCH_HEIGHT))
    }

    fn render(&mut self, layout: &LayoutNode, _ctx: &ViewContext) -> Vec<LayeredElement> {
        let mut elements = Vec::new();
        let bounds = layout.computed.content_box;

        let track_rect = KurboRect::new(
            bounds.x as f64,
            bounds.y as f64,
            (bounds.x + SWITCH_WIDTH) as f64,
            (bounds.y + SWITCH_HEIGHT) as f64,
        );

        let track_style = FillStrokeStyle {
            fill: Some(if self.on { THEME_PRIMARY } else { NEUTRAL_300 }),
            stroke: None,
        };

        elements.push(LayeredElement::default_layer(VisualElement::RoundedRect {
            rect: track_rect,
            radius: SWITCH_RADIUS,
            style: track_style,
        }));

        // 滑块位置
        let thumb_diameter = SWITCH_HEIGHT as f64 - THUMB_MARGIN * 2.0;
        let thumb_x = if self.on {
            track_rect.x1 - THUMB_MARGIN - thumb_diameter
        } else {
            track_rect.x0 + THUMB_MARGIN
        };
        let thumb_y = track_rect.y0 + THUMB_MARGIN;

        let thumb_rect = KurboRect::new(
            thumb_x,
            thumb_y,
            thumb_x + thumb_diameter,
            thumb_y + thumb_diameter,
        );

        let thumb_style = FillStrokeStyle {
            fill: Some(WHITE),
            stroke: Some(crate::render::visual::Stroke {
                color: NEUTRAL_400,
                width: 0.5,
            }),
        };

        elements.push(LayeredElement::default_layer(VisualElement::RoundedRect {
            rect: thumb_rect,
            radius: thumb_diameter / 2.0,
            style: thumb_style,
        }));

        elements
    }

    fn handle_event(&mut self, event: &Event, ctx: &EventContext) -> EventResult {
        match event {
            Event::Click { .. } => {
                self.set_on(!self.on);
                self.fire_value_callbacks();
                ctx.request_render();
                ctx.stop_propagation();
                EventResult::Stop
            }
            _ => EventResult::Continue,
        }
    }

    fn can_focus(&self) -> bool {
        true
    }
}
