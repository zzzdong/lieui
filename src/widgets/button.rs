// src/widgets/button.rs

//! Button Widget - Fluent UI 风格
//!
//! 按钮组件，内部使用 Text 渲染文本。
//! 遵循 Fluent UI 设计规范：
//! - 圆角：4px
//! - 高度：32px（标准按钮）
//! - 内边距：左右 12px，上下 6px
//! - 颜色：使用 Fluent UI 主题色板

use crate::core::WidgetId;
use crate::event::{Event, EventResult, EventType, Propagation};
use crate::geometry::{Color, Rect, Size};
use crate::layout::{BoxStyle, EdgeInsets, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::RenderNode;
use crate::text::TextColor;
use crate::widget::Widget;
use crate::widgets::text::Text;
use std::collections::HashMap;
use vello_cpu::color::AlphaColor;

// ============================================
// Fluent UI 颜色系统
// ============================================

/// Fluent UI 主题色 - 品牌蓝
const THEME_PRIMARY: Color = Color(AlphaColor::from_rgb8(0, 120, 212)); // #0078d4
const THEME_DARK_ALT: Color = Color(AlphaColor::from_rgb8(16, 110, 190)); // #106ebe
const THEME_DARK: Color = Color(AlphaColor::from_rgb8(0, 90, 158)); // #005a9e

/// Fluent UI 中性色
const NEUTRAL_WHITE: Color = Color(AlphaColor::from_rgb8(255, 255, 255)); // #ffffff
const NEUTRAL_100: Color = Color(AlphaColor::from_rgb8(244, 244, 244)); // #f4f4f4 (buttonBackground)
const NEUTRAL_200: Color = Color(AlphaColor::from_rgb8(234, 234, 234)); // #eaeaea (buttonBackgroundHovered)
const NEUTRAL_300: Color = Color(AlphaColor::from_rgb8(200, 200, 200)); // #c8c8c8 (buttonBackgroundPressed)
const NEUTRAL_400: Color = Color(AlphaColor::from_rgb8(166, 166, 166)); // #a6a6a6 (buttonTextDisabled)
const NEUTRAL_700: Color = Color(AlphaColor::from_rgb8(51, 51, 51)); // #333333 (buttonText)
const NEUTRAL_800: Color = Color(AlphaColor::from_rgb8(33, 33, 33)); // #212121 (buttonTextHovered)

// ============================================
// 按钮尺寸规范
// ============================================

/// 标准按钮高度
const BUTTON_HEIGHT: f32 = 32.0;
/// 水平内边距
const BUTTON_PAD_X: f32 = 12.0;
/// 垂直内边距
const BUTTON_PAD_Y: f32 = 6.0;
/// 圆角半径
const BUTTON_RADIUS: f32 = 4.0;

// ============================================
// 按钮类型
// ============================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// 默认按钮（灰色背景）
    #[default]
    Default,
    /// 主要按钮（品牌蓝背景）
    Primary,
    /// 幽灵按钮（透明背景，有边框）
    Ghost,
}

/// 用户回调类型 - 简化签名，无需参数
type UserCallback = Box<dyn FnMut()>;

pub struct Button {
    bounds: Rect,
    text_widget: Text,
    variant: ButtonVariant,
    is_hovered: bool,
    is_pressed: bool,
    is_disabled: bool,
    /// 脏标记
    dirty: bool,
    /// 用户回调，按事件类型分组
    callbacks: std::collections::HashMap<EventType, Vec<UserCallback>>,
}

impl Button {
    /// 创建新的 Button（默认样式）
    pub fn new(text: impl Into<String>) -> Self {
        let text_widget = Text::new(text).text_color(TextColor(NEUTRAL_700.0));

        Self {
            bounds: Rect::zero(),
            text_widget,
            variant: ButtonVariant::Default,
            is_hovered: false,
            is_pressed: false,
            is_disabled: false,
            dirty: false,
            callbacks: HashMap::new(),
        }
    }

    /// 创建主要按钮
    pub fn primary(text: impl Into<String>) -> Self {
        let mut button = Self::new(text);
        button.variant = ButtonVariant::Primary;
        button.text_widget = button.text_widget.text_color(TextColor(NEUTRAL_WHITE.0));
        button
    }

    /// 创建幽灵按钮
    pub fn ghost(text: impl Into<String>) -> Self {
        let mut button = Self::new(text);
        button.variant = ButtonVariant::Ghost;
        button
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

    /// 注册点击事件回调
    pub fn on_click<F>(mut self, f: F) -> Self
    where
        F: FnMut() + 'static,
    {
        self.callbacks
            .entry(EventType::Click)
            .or_default()
            .push(Box::new(f));
        self
    }

    /// 注册鼠标进入回调
    pub fn on_mouse_enter<F>(mut self, f: F) -> Self
    where
        F: FnMut() + 'static,
    {
        self.callbacks
            .entry(EventType::MouseEnter)
            .or_default()
            .push(Box::new(f));
        self
    }

    /// 注册鼠标离开回调
    pub fn on_mouse_leave<F>(mut self, f: F) -> Self
    where
        F: FnMut() + 'static,
    {
        self.callbacks
            .entry(EventType::MouseLeave)
            .or_default()
            .push(Box::new(f));
        self
    }

    /// 设置禁用状态
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.is_disabled = disabled;
        self
    }

    /// 获取背景色
    fn bg_color(&self) -> Color {
        if self.is_disabled {
            match self.variant {
                ButtonVariant::Default | ButtonVariant::Ghost => NEUTRAL_100,
                ButtonVariant::Primary => NEUTRAL_100,
            }
        } else if self.is_pressed {
            match self.variant {
                ButtonVariant::Default => NEUTRAL_300,
                ButtonVariant::Primary => THEME_DARK,
                ButtonVariant::Ghost => NEUTRAL_200,
            }
        } else if self.is_hovered {
            match self.variant {
                ButtonVariant::Default => NEUTRAL_200,
                ButtonVariant::Primary => THEME_DARK_ALT,
                ButtonVariant::Ghost => NEUTRAL_200,
            }
        } else {
            match self.variant {
                ButtonVariant::Default => NEUTRAL_WHITE,
                ButtonVariant::Primary => THEME_PRIMARY,
                ButtonVariant::Ghost => NEUTRAL_WHITE,
            }
        }
    }

    /// 获取文本色
    fn text_color(&self) -> Color {
        if self.is_disabled {
            NEUTRAL_400
        } else {
            match self.variant {
                ButtonVariant::Default | ButtonVariant::Ghost => {
                    if self.is_hovered || self.is_pressed {
                        NEUTRAL_800
                    } else {
                        NEUTRAL_700
                    }
                }
                ButtonVariant::Primary => NEUTRAL_WHITE,
            }
        }
    }

    /// 触发指定事件类型的用户回调
    fn fire_callbacks(&mut self, event_type: EventType) {
        if let Some(cbs) = self.callbacks.get_mut(&event_type) {
            for cb in cbs {
                cb();
            }
        }
    }
}

impl Widget for Button {
    crate::impl_widget_any!(Button);

    fn type_name(&self) -> &'static str {
        "Button"
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        // 创建文本子节点 - Text 只是视觉内容，不参与事件交互
        let text_id = WidgetId::new();
        let text_node = self.text_widget.layout(text_id).with_interactive(false);

        // 计算按钮尺寸
        let text_size = self.text_widget.measure(None);
        let width = text_size.width + BUTTON_PAD_X * 2.0;
        let height = BUTTON_HEIGHT;

        LayoutNode::new(id)
            .with_box_style(BoxStyle {
                padding: EdgeInsets::symmetric(BUTTON_PAD_X, BUTTON_PAD_Y),
                min_size: Size::new(width, height),
                max_size: Size::new(f32::INFINITY, height),
                ..Default::default()
            })
            .with_fixed_size(Size::new(width, height))
            .add_child(text_node)
    }

    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            // 使用 padding_box 作为按钮背景边界
            let bounds = computed.padding_box;

            // 按钮背景
            let mut node = RenderNode::div(bounds)
                .background(self.bg_color())
                .border_radius(BUTTON_RADIUS);

            // 幽灵按钮添加边框
            if self.variant == ButtonVariant::Ghost && !self.is_disabled {
                node = node.border_color(NEUTRAL_300).border_width(1.0);
            }

            // 渲染文本 - 在按钮内容区域居中
            let content_bounds = computed.content_box;
            let text_layout = self.text_widget.do_layout(Some(content_bounds.width));
            let text_size = Size::new(text_layout.width(), text_layout.height());

            // 计算文本居中位置
            let text_x = (content_bounds.width - text_size.width) / 2.0;
            let text_y = (content_bounds.height - text_size.height) / 2.0;
            let text_bounds = Rect::new(
                content_bounds.x + text_x,
                content_bounds.y + text_y,
                text_size.width,
                text_size.height,
            );

            let text_node = RenderNode::text(text_bounds, text_layout);
            node = node.add_child(text_node);

            node
        } else {
            RenderNode::div(Rect::zero())
        }
    }

    fn can_focus(&self) -> bool {
        !self.is_disabled
    }

    fn handle_event(&mut self, event: &Event, _propagation: &mut Propagation) -> EventResult {
        if self.is_disabled {
            return EventResult::Continue;
        }

        match event {
            Event::MouseEnter => {
                if !self.is_hovered {
                    self.is_hovered = true;
                    self.dirty = true;
                    self.text_widget
                        .set_text_color(crate::text::TextColor(self.text_color().0));
                }
                self.fire_callbacks(EventType::MouseEnter);
            }
            Event::MouseLeave => {
                if self.is_hovered || self.is_pressed {
                    self.is_hovered = false;
                    self.is_pressed = false;
                    self.dirty = true;
                    self.text_widget
                        .set_text_color(crate::text::TextColor(self.text_color().0));
                }
                self.fire_callbacks(EventType::MouseLeave);
            }
            Event::MouseDown { .. } => {
                if !self.is_pressed {
                    self.is_pressed = true;
                    self.dirty = true;
                    self.text_widget
                        .set_text_color(crate::text::TextColor(self.text_color().0));
                }
                self.fire_callbacks(EventType::MouseDown);
            }
            Event::MouseUp { .. } => {
                if self.is_pressed {
                    self.is_pressed = false;
                    self.dirty = true;
                    self.text_widget
                        .set_text_color(crate::text::TextColor(self.text_color().0));
                }
                self.fire_callbacks(EventType::MouseUp);
            }
            Event::Click { .. } => {
                self.fire_callbacks(EventType::Click);
            }
            _ => {}
        }

        EventResult::Continue
    }

    fn bounds(&self) -> Option<Rect> {
        Some(self.bounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_creation() {
        let button = Button::new("Click me");
        assert_eq!(button.text_content(), "Click me");
        assert!(!button.is_disabled);
    }

    #[test]
    fn test_primary_button() {
        let button = Button::primary("Submit");
        assert_eq!(button.variant, ButtonVariant::Primary);
    }

    #[test]
    fn test_ghost_button() {
        let button = Button::ghost("Cancel");
        assert_eq!(button.variant, ButtonVariant::Ghost);
    }
}
