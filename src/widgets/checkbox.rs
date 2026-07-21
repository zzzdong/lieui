//! Checkbox Widget - 复选框

use crate::core::WidgetId;
use crate::event::{Event, EventContext, EventResult};
use crate::geometry::{Color, Size};
use crate::layout::{BoxStyle, LayoutNode, Measurable, TextMeasure};
use crate::prelude::ViewContext;
use crate::render::visual::{FillStrokeStyle, LayeredElement, Stroke, VisualElement};
use crate::text::TextLayout;
use crate::widget::Widget;
use kurbo::Rect as KurboRect;
use vello_cpu::color::AlphaColor;

const CHECKBOX_SIZE: f32 = 18.0;
const CHECKBOX_LABEL_GAP: f32 = 8.0;
const CHECKBOX_RADIUS: f64 = 3.0;

const NEUTRAL_500: Color = Color(AlphaColor::from_rgb8(144, 144, 144));
const NEUTRAL_700: Color = Color(AlphaColor::from_rgb8(51, 51, 51));
const THEME_PRIMARY: Color = Color(AlphaColor::from_rgb8(0, 120, 212));
const WHITE: Color = Color(AlphaColor::from_rgb8(255, 255, 255));

type ValueCallback = Box<dyn FnMut(bool)>;

/// 复选框组件
pub struct Checkbox {
    label: String,
    label_layout: TextLayout,
    checked: bool,
    dirty: bool,
    value_callbacks: Vec<ValueCallback>,
    visible: bool,
}

impl Checkbox {
    pub fn new(label: impl Into<String>) -> Self {
        let label = label.into();
        let label_layout =
            TextMeasure::new(&label, crate::text::TextStyle::default()).create_layout(None);

        Self {
            label,
            label_layout,
            checked: false,
            dirty: false,
            value_callbacks: Vec::new(),
            visible: true,
        }
    }

    /// 设置初始选中状态
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// 是否已选中
    pub fn is_checked(&self) -> bool {
        self.checked
    }

    /// 设置可见性（不可见时不参与布局/渲染/命中测试）
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// 设置可见性（构造后修改）
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible != visible {
            self.visible = visible;
            self.dirty = true;
        }
    }

    /// 设置选中状态（可变）
    pub fn set_checked(&mut self, checked: bool) {
        if self.checked != checked {
            self.checked = checked;
            self.dirty = true;
        }
    }

    /// 注册状态变化回调，参数为新的选中值
    pub fn on_changed<F>(mut self, f: F) -> Self
    where
        F: FnMut(bool) + 'static,
    {
        self.value_callbacks.push(Box::new(f));
        self
    }

    fn fire_value_callbacks(&mut self) {
        let value = self.checked;
        for cb in &mut self.value_callbacks {
            cb(value);
        }
    }
}

impl Widget for Checkbox {
    crate::impl_widget_any!(Checkbox);

    fn type_name(&self) -> &'static str {
        "Checkbox"
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        if !self.visible {
            return LayoutNode::new(id).with_fixed_size(Size::new(0.0, 0.0));
        }
        let measure = TextMeasure::new(&self.label, crate::text::TextStyle::default());
        let label_size = measure.measure(None);
        let width = CHECKBOX_SIZE + CHECKBOX_LABEL_GAP + label_size.width;
        let height = CHECKBOX_SIZE.max(label_size.height);

        LayoutNode::new(id)
            .with_box_style(BoxStyle {
                min_size: Size::new(width, height),
                max_size: Size::new(width, height),
                ..Default::default()
            })
            .with_fixed_size(Size::new(width, height))
    }

    fn render(&mut self, layout: &LayoutNode, _ctx: &ViewContext) -> Vec<LayeredElement> {
        if !self.visible {
            return Vec::new();
        }
        let mut elements = Vec::new();
        let bounds = layout.computed.content_box;

        // 复选框外框
        let checkbox_rect = KurboRect::new(
            bounds.x as f64,
            bounds.y as f64,
            (bounds.x + CHECKBOX_SIZE) as f64,
            (bounds.y + CHECKBOX_SIZE) as f64,
        );

        let box_style = FillStrokeStyle {
            fill: if self.checked {
                Some(THEME_PRIMARY)
            } else {
                Some(WHITE)
            },
            stroke: Some(Stroke {
                color: if self.checked {
                    THEME_PRIMARY
                } else {
                    NEUTRAL_500
                },
                width: 1.5,
            }),
        };

        elements.push(LayeredElement::default_layer(VisualElement::RoundedRect {
            rect: checkbox_rect,
            radius: CHECKBOX_RADIUS,
            style: box_style,
        }));

        // 选中勾号
        if self.checked {
            let pad = CHECKBOX_SIZE as f64 * 0.22;
            let x1 = checkbox_rect.x0 + pad;
            let y1 = checkbox_rect.y0 + CHECKBOX_SIZE as f64 * 0.55;
            let x2 = checkbox_rect.x0 + CHECKBOX_SIZE as f64 * 0.42;
            let y2 = checkbox_rect.y1 - pad;
            let x3 = checkbox_rect.x1 - pad;
            let y3 = checkbox_rect.y0 + CHECKBOX_SIZE as f64 * 0.32;

            let stroke = Stroke {
                color: WHITE,
                width: 2.0,
            };
            elements.push(LayeredElement::default_layer(VisualElement::Line {
                start: kurbo::Point::new(x1, y1),
                end: kurbo::Point::new(x2, y2),
                style: stroke.clone(),
            }));
            elements.push(LayeredElement::default_layer(VisualElement::Line {
                start: kurbo::Point::new(x2, y2),
                end: kurbo::Point::new(x3, y3),
                style: stroke,
            }));
        }

        // 标签文本
        let label_x = bounds.x + CHECKBOX_SIZE + CHECKBOX_LABEL_GAP;
        let label_y = bounds.y + (bounds.height - 14.0) / 2.0;
        elements.push(LayeredElement::default_layer(VisualElement::TextRun {
            text: self.label.clone(),
            position: kurbo::Point::new(label_x as f64, label_y as f64),
            color: NEUTRAL_700,
            font_size: 14.0,
            font_family: "sans-serif".to_string(),
            rotation: 0.0,
            max_width: None,
            layout: Some(Box::new(self.label_layout.clone())),
        }));

        elements
    }

    fn handle_event(&mut self, event: &Event, ctx: &EventContext) -> EventResult {
        if !self.visible {
            return EventResult::Continue;
        }
        match event {
            Event::Click { .. } => {
                self.set_checked(!self.checked);
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
