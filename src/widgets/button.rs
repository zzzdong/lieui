// src/widgets/button.rs

//! Button Widget
//!
//! 按钮组件，内部使用 Text 渲染文本。

use crate::core::WidgetId;
use crate::event::{Event, EventResult};
use crate::geometry::{Color, Rect, Size};
use crate::layout::{BoxStyle, EdgeInsets, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::{BoxShadow, RenderNode};
use crate::text::TextColor;
use crate::widget::Widget;
use crate::widgets::text::Text;
use vello_cpu::color::AlphaColor;

/// Material Design 3 Contained Button 规范
/// https://m3.material.io/components/buttons/specs
const BUTTON_PAD_X: f32 = 16.0; // 水平内边距
const BUTTON_PAD_Y: f32 = 8.0; // 垂直内边距
const BUTTON_MIN_H: f32 = 36.0; // 最小高度
const BUTTON_RADIUS: f32 = 4.0; // 圆角半径

/// 按钮状态颜色（Material Blue）
const COLOR_DEFAULT: Color = Color(AlphaColor::from_rgb8(25, 118, 210));
const COLOR_HOVERED: Color = Color(AlphaColor::from_rgb8(21, 101, 192));
const COLOR_PRESSED: Color = Color(AlphaColor::from_rgb8(13, 71, 161));
const COLOR_DISABLED: Color = Color(AlphaColor::from_rgb8(189, 189, 189));

/// 点击回调类型 - 只操作 Button 自身
pub type ClickCallback = Box<dyn FnMut(&mut Button)>;

/// 底层事件回调类型 - 统一签名
pub type ButtonEventCallback = Box<dyn FnMut(WidgetId, &Event, &mut ViewContext)>;

pub struct Button {
    bounds: Rect,
    text_widget: Text,
    is_hovered: bool,
    is_pressed: bool,
    is_disabled: bool,
    /// 点击回调（内部包装后注册到 ViewContext）
    on_click: Option<ClickCallback>,
    /// 底层事件回调（直接注册到 ViewContext）
    event_callback: Option<ButtonEventCallback>,
}

impl Button {
    /// 创建新的 Button
    pub fn new(text: impl Into<String>) -> Self {
        let text_widget = Text::new(text).text_color(TextColor::WHITE);

        Self {
            bounds: Rect::zero(),
            text_widget,
            is_hovered: false,
            is_pressed: false,
            is_disabled: false,
            on_click: None,
            event_callback: None,
        }
    }

    /// 设置文本
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text_widget = self.text_widget.content(text);
        self
    }

    /// 获取文本内容
    pub fn text_content(&self) -> &str {
        self.text_widget.text_content()
    }

    /// 设置文本内容（可变）
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text_widget = self.text_widget.clone().content(text);
    }

    /// 设置点击回调（链式调用）- 只操作 Button 自身
    ///
    /// 内部会包装成底层事件回调，只处理 MouseUp 事件（点击）
    pub fn on_click<F>(mut self, mut f: F) -> Self
    where
        F: FnMut(&mut Button) + 'static,
    {
        self.on_click = Some(Box::new(move |btn| f(btn)));
        self
    }

    /// 设置事件回调（链式调用）- 底层签名，可处理任意事件
    pub fn on_event<F>(mut self, f: F) -> Self
    where
        F: FnMut(WidgetId, &Event, &mut ViewContext) + 'static,
    {
        self.event_callback = Some(Box::new(f));
        self
    }

    /// 获取点击回调（供 ViewContext 在创建时注册）
    ///
    /// 返回包装后的底层事件回调，只处理 Click 事件
    pub fn take_click_callback(&mut self) -> Option<ButtonEventCallback> {
        self.on_click.take().map(|mut callback| {
            Box::new(move |id: WidgetId, event: &Event, ctx: &mut ViewContext| {
                // wrap：只处理 Click 事件
                if matches!(event, Event::Click { .. }) {
                    if let Some(mut btn) = ctx.get::<Button>(id) {
                        callback(&mut btn);
                    }
                }
            }) as ButtonEventCallback
        })
    }

    /// 获取底层事件回调
    pub fn take_event_callback(&mut self) -> Option<ButtonEventCallback> {
        self.event_callback.take()
    }

    fn bg_color(&self) -> Color {
        if self.is_disabled {
            COLOR_DISABLED
        } else if self.is_pressed {
            COLOR_PRESSED
        } else if self.is_hovered {
            COLOR_HOVERED
        } else {
            COLOR_DEFAULT
        }
    }

    /// 设置禁用状态
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.is_disabled = disabled;
        self
    }

    /// 设置悬停状态
    pub fn set_hovered(&mut self, hovered: bool) {
        self.is_hovered = hovered;
    }

    /// 设置按下状态
    pub fn set_pressed(&mut self, pressed: bool) {
        self.is_pressed = pressed;
    }

    /// 获取悬停状态
    pub fn is_hovered(&self) -> bool {
        self.is_hovered
    }

    /// 获取按下状态
    pub fn is_pressed(&self) -> bool {
        self.is_pressed
    }

    /// 获取禁用状态
    pub fn is_disabled(&self) -> bool {
        self.is_disabled
    }
}

impl Widget for Button {
    crate::impl_widget_any!(Button);

    fn type_name(&self) -> &'static str {
        "Button"
    }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        // 创建文本子节点 - 使用一个新的子节点ID
        let text_id = WidgetId::new();
        let text_node = self.text_widget.layout(text_id);

        // 按钮使用固定尺寸策略：
        // 尺寸 = 文本尺寸 + 内边距
        let text_size = self.text_widget.measure(None);
        let width = text_size.width + BUTTON_PAD_X * 2.0;
        let height = (text_size.height + BUTTON_PAD_Y * 2.0).max(BUTTON_MIN_H);

        LayoutNode::new(id)
            .with_box_style(BoxStyle {
                padding: EdgeInsets::symmetric(BUTTON_PAD_X, BUTTON_PAD_Y),
                min_size: Size::new(width, height),
                max_size: Size::new(f32::INFINITY, f32::INFINITY),
                ..Default::default()
            })
            .with_fixed_size(Size::new(width, height))
            .add_child(text_node)
    }

    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            let bounds = computed.content_box;

            // 按钮背景
            let mut node = RenderNode::div(bounds)
                .background(self.bg_color())
                .border_radius(BUTTON_RADIUS);

            // 阴影效果（仅在非按下状态）
            if !self.is_pressed && !self.is_disabled {
                node = node.box_shadow(BoxShadow::new(COLOR_DEFAULT));
            }

            // 渲染文本 - 在按钮内容区域居中
            let text_layout = self.text_widget.do_layout(Some(bounds.width));
            let text_size = Size::new(text_layout.width(), text_layout.height());

            // 计算文本居中位置（相对于按钮）
            let text_x = (bounds.width - text_size.width) / 2.0;
            let text_y = (bounds.height - text_size.height) / 2.0;
            let text_bounds = Rect::new(
                bounds.x + text_x,
                bounds.y + text_y,
                text_size.width,
                text_size.height,
            );

            let text_node = RenderNode::text(text_bounds, text_layout);
            node = node.add_child(text_node);

            node
        } else {
            // 如果还没有计算，返回空节点
            RenderNode::div(Rect::zero())
        }
    }

    fn can_focus(&self) -> bool {
        !self.is_disabled
    }

    fn handle_event(&mut self, event: &Event, _ctx: &mut ViewContext) -> (EventResult, bool) {
        match event {
            Event::MouseEnter => {
                if !self.is_disabled && !self.is_hovered {
                    self.is_hovered = true;
                    return (EventResult::Continue, true);
                }
            }
            Event::MouseLeave => {
                if self.is_hovered || self.is_pressed {
                    self.is_hovered = false;
                    self.is_pressed = false;
                    return (EventResult::Continue, true);
                }
            }
            Event::MouseDown { .. } => {
                if !self.is_disabled && !self.is_pressed {
                    self.is_pressed = true;
                    return (EventResult::Continue, true);
                }
            }
            Event::MouseUp { .. } => {
                if self.is_pressed {
                    self.is_pressed = false;
                    return (EventResult::Continue, true);
                }
            }
            _ => {}
        }
        (EventResult::Continue, false)
    }

    fn bounds(&self) -> Option<Rect> {
        Some(self.bounds)
    }
}
