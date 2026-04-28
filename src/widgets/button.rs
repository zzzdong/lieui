// src/widgets/button.rs

//! Button Widget
//!
//! 按钮组件，内部使用 Text 渲染文本。

use crate::core::WidgetId;
use crate::event::{Event, EventResult, EventType, Propagation};
use crate::geometry::{Color, Rect, Size};
use crate::layout::{BoxStyle, EdgeInsets, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::{BoxShadow, RenderNode};
use crate::text::TextColor;
use crate::widget::Widget;
use crate::widgets::text::Text;
use std::collections::HashMap;
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

/// 用户回调类型 - 简化签名，无需参数
/// 用户通过闭包捕获所需的 WidgetRef 或状态
type UserCallback = Box<dyn FnMut()>;

pub struct Button {
    bounds: Rect,
    text_widget: Text,
    is_hovered: bool,
    is_pressed: bool,
    is_disabled: bool,
    /// 脏标记
    dirty: bool,
    /// 用户回调，按事件类型分组
    callbacks: std::collections::HashMap<EventType, Vec<UserCallback>>,
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
            dirty: false,
            callbacks: HashMap::new(),
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

    /// 注册鼠标按下回调
    pub fn on_mouse_down<F>(mut self, f: F) -> Self
    where
        F: FnMut() + 'static,
    {
        self.callbacks
            .entry(EventType::MouseDown)
            .or_default()
            .push(Box::new(f));
        self
    }

    /// 注册鼠标释放回调
    pub fn on_mouse_up<F>(mut self, f: F) -> Self
    where
        F: FnMut() + 'static,
    {
        self.callbacks
            .entry(EventType::MouseUp)
            .or_default()
            .push(Box::new(f));
        self
    }

    /// 触发指定事件类型的用户回调
    fn fire_callbacks(&mut self, event_type: EventType) {
        if let Some(cbs) = self.callbacks.get_mut(&event_type) {
            for cb in cbs {
                cb();
            }
        }
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

    fn is_dirty(&self) -> bool {
        self.dirty
    }
    fn clear_dirty(&mut self) {
        self.dirty = false;
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
            // 使用 padding_box 作为按钮背景边界，使按钮在布局中正确居中
            let bounds = computed.padding_box;

            // 按钮背景
            let mut node = RenderNode::div(bounds)
                .background(self.bg_color())
                .border_radius(BUTTON_RADIUS);

            // 阴影效果（仅在非按下状态）
            if !self.is_pressed && !self.is_disabled {
                node = node.box_shadow(BoxShadow::new(COLOR_DEFAULT));
            }

            // 渲染文本 - 在按钮内容区域居中
            let content_bounds = computed.content_box;
            let text_layout = self.text_widget.do_layout(Some(content_bounds.width));
            let text_size = Size::new(text_layout.width(), text_layout.height());

            // 计算文本居中位置（相对于内容区域）
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
            // 如果还没有计算，返回空节点
            RenderNode::div(Rect::zero())
        }
    }

    fn can_focus(&self) -> bool {
        !self.is_disabled
    }

    fn handle_event(&mut self, event: &Event, _propagation: &mut Propagation) -> EventResult {
        match event {
            // ===== 内部行为 =====
            Event::MouseEnter => {
                if !self.is_disabled && !self.is_hovered {
                    self.is_hovered = true;
                    self.dirty = true;
                }
                // 触发用户回调
                self.fire_callbacks(EventType::MouseEnter);
            }
            Event::MouseLeave => {
                if self.is_hovered || self.is_pressed {
                    self.is_hovered = false;
                    self.is_pressed = false;
                    self.dirty = true;
                }
                // 触发用户回调
                self.fire_callbacks(EventType::MouseLeave);
            }
            Event::MouseDown { .. } => {
                if !self.is_disabled && !self.is_pressed {
                    self.is_pressed = true;
                    self.dirty = true;
                }
                // 触发用户回调
                self.fire_callbacks(EventType::MouseDown);
            }
            Event::MouseUp { .. } => {
                if self.is_pressed {
                    self.is_pressed = false;
                    self.dirty = true;
                }
                // 触发用户回调
                self.fire_callbacks(EventType::MouseUp);
            }
            Event::Click { .. } => {
                // Click 事件在 MouseUp 中已经处理了 is_pressed
                // 触发用户回调（可能改变其他 Widget 的状态，由 dirty flag 系统自动检测）
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
