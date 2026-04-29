// src/widgets/text_input.rs

//! TextInput Widget - 文本输入控件
//!
//! 支持键盘输入和 IME（输入法编辑器）的文本输入控件。
//! 遵循 Fluent UI 设计规范。

use crate::core::WidgetId;
use crate::event::{Event, EventResult, Key, Propagation};
use crate::geometry::{Color, Rect, Size};
use crate::layout::{BoxStyle, EdgeInsets, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::RenderNode;
use crate::text::{TextColor, TextEngine, TextLayout, TextStyle};
use crate::widget::Widget;
use vello_cpu::color::AlphaColor;

// ============================================
// Fluent UI 颜色系统
// ============================================

const NEUTRAL_WHITE: Color = Color(AlphaColor::from_rgb8(255, 255, 255)); // #ffffff
const NEUTRAL_100: Color = Color(AlphaColor::from_rgb8(244, 244, 244)); // #f4f4f4
const NEUTRAL_200: Color = Color(AlphaColor::from_rgb8(234, 234, 234)); // #eaeaea
const NEUTRAL_300: Color = Color(AlphaColor::from_rgb8(200, 200, 200)); // #c8c8c8
const NEUTRAL_400: Color = Color(AlphaColor::from_rgb8(166, 166, 166)); // #a6a6a6
const NEUTRAL_600: Color = Color(AlphaColor::from_rgb8(96, 96, 96)); // #606060
const NEUTRAL_700: Color = Color(AlphaColor::from_rgb8(51, 51, 51)); // #333333
const THEME_PRIMARY: Color = Color(AlphaColor::from_rgb8(0, 120, 212)); // #0078d4

// ============================================
// 尺寸规范
// ============================================

/// 标准输入框高度
const INPUT_HEIGHT: f32 = 32.0;
/// 水平内边距
const INPUT_PAD_X: f32 = 12.0;
/// 垂直内边距
const INPUT_PAD_Y: f32 = 6.0;
/// 圆角半径
const INPUT_RADIUS: f32 = 4.0;
/// 光标宽度
const CURSOR_WIDTH: f32 = 1.0;
/// 光标闪烁周期（毫秒）
const CURSOR_BLINK_MS: u64 = 530;

/// 文本输入控件
pub struct TextInput {
    /// 当前文本内容
    text: String,
    /// 占位符文本
    placeholder: String,
    /// 文本样式
    style: TextStyle,
    /// 占位符样式
    placeholder_style: TextStyle,
    /// 是否获得焦点
    is_focused: bool,
    /// 是否悬停
    is_hovered: bool,
    /// 是否禁用
    is_disabled: bool,
    /// 光标位置（字符索引）
    cursor_pos: usize,
    /// 选中文本的起始位置（如果有）
    selection_start: Option<usize>,
    /// IME 预编辑文本
    ime_preedit: Option<String>,
    /// IME 预编辑光标位置
    ime_cursor: Option<usize>,
    /// 脏标记
    dirty: bool,
    /// 光标可见性（用于闪烁效果）
    cursor_visible: bool,
    /// 上次光标闪烁时间
    last_cursor_blink: std::time::Instant,
    /// 固定宽度（如果为 None，则根据内容自适应）
    fixed_width: Option<f32>,
}

impl TextInput {
    /// 创建新的 TextInput
    pub fn new() -> Self {
        let mut style = TextStyle::default();
        style.0.brush = TextColor(NEUTRAL_700.0);
        style.0.font_size = 14.0;

        let mut placeholder_style = TextStyle::default();
        placeholder_style.0.brush = TextColor(NEUTRAL_400.0);
        placeholder_style.0.font_size = 14.0;

        Self {
            text: String::new(),
            placeholder: String::new(),
            style,
            placeholder_style,
            is_focused: false,
            is_hovered: false,
            is_disabled: false,
            cursor_pos: 0,
            selection_start: None,
            ime_preedit: None,
            ime_cursor: None,
            dirty: false,
            cursor_visible: true,
            last_cursor_blink: std::time::Instant::now(),
            fixed_width: None,
        }
    }

    /// 设置占位符文本
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// 设置初始文本
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self.cursor_pos = self.text.len();
        self
    }

    /// 获取当前文本
    pub fn get_text(&self) -> &str {
        &self.text
    }

    /// 设置文本（可变）
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor_pos = self.cursor_pos.min(self.text.len());
        self.selection_start = None;
        self.dirty = true;
    }

    /// 设置禁用状态
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.is_disabled = disabled;
        self
    }

    /// 设置固定宽度
    pub fn width(mut self, width: f32) -> Self {
        self.fixed_width = Some(width);
        self
    }

    /// 检查是否获得焦点
    pub fn is_focused(&self) -> bool {
        self.is_focused
    }

    /// 获取背景色
    fn bg_color(&self) -> Color {
        if self.is_disabled {
            NEUTRAL_100
        } else {
            NEUTRAL_WHITE
        }
    }

    /// 获取边框色
    fn border_color(&self) -> Color {
        if self.is_disabled {
            NEUTRAL_200
        } else if self.is_focused {
            THEME_PRIMARY
        } else if self.is_hovered {
            NEUTRAL_600
        } else {
            NEUTRAL_300
        }
    }

    /// 获取边框宽度
    fn border_width(&self) -> f32 {
        if self.is_focused { 2.0 } else { 1.0 }
    }

    /// 插入文本（处理 IME 和普通输入）
    fn insert_text(&mut self, text: &str) {
        // 如果有选中文本，先删除
        if let Some(start) = self.selection_start {
            let (left, right) = if start < self.cursor_pos {
                (start, self.cursor_pos)
            } else {
                (self.cursor_pos, start)
            };
            self.text.drain(left..right);
            self.cursor_pos = left;
            self.selection_start = None;
        }

        // 插入新文本
        self.text.insert_str(self.cursor_pos, text);
        self.cursor_pos += text.len();
        self.dirty = true;
    }

    /// 删除光标前的字符
    fn delete_char_before(&mut self) {
        if let Some(start) = self.selection_start {
            // 删除选中的文本
            let (left, right) = if start < self.cursor_pos {
                (start, self.cursor_pos)
            } else {
                (self.cursor_pos, start)
            };
            self.text.drain(left..right);
            self.cursor_pos = left;
            self.selection_start = None;
            self.dirty = true;
        } else if self.cursor_pos > 0 {
            // 删除光标前的一个字符
            let char_start = self.find_char_start(self.cursor_pos);
            self.text.drain(char_start..self.cursor_pos);
            self.cursor_pos = char_start;
            self.dirty = true;
        }
    }

    /// 删除光标后的字符
    fn delete_char_after(&mut self) {
        if let Some(start) = self.selection_start {
            // 删除选中的文本
            let (left, right) = if start < self.cursor_pos {
                (start, self.cursor_pos)
            } else {
                (self.cursor_pos, start)
            };
            self.text.drain(left..right);
            self.cursor_pos = left;
            self.selection_start = None;
            self.dirty = true;
        } else if self.cursor_pos < self.text.len() {
            // 删除光标后的一个字符
            let char_end = self.find_char_end(self.cursor_pos);
            self.text.drain(self.cursor_pos..char_end);
            self.dirty = true;
        }
    }

    /// 移动光标
    fn move_cursor(&mut self, delta: isize, select: bool) {
        if !select {
            self.selection_start = None;
        } else if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_pos);
        }

        let new_pos = if delta < 0 {
            let abs_delta = (-delta) as usize;
            self.cursor_pos.saturating_sub(abs_delta)
        } else {
            let delta = delta as usize;
            (self.cursor_pos + delta).min(self.text.len())
        };

        self.cursor_pos = new_pos;
        self.dirty = true;
    }

    /// 移动光标到行首
    fn move_cursor_home(&mut self, select: bool) {
        if !select {
            self.selection_start = None;
        } else if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_pos);
        }
        self.cursor_pos = 0;
        self.dirty = true;
    }

    /// 移动光标到行尾
    fn move_cursor_end(&mut self, select: bool) {
        if !select {
            self.selection_start = None;
        } else if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_pos);
        }
        self.cursor_pos = self.text.len();
        self.dirty = true;
    }

    /// 全选
    fn select_all(&mut self) {
        self.selection_start = Some(0);
        self.cursor_pos = self.text.len();
        self.dirty = true;
    }

    /// 查找前一个字符的起始位置（处理 UTF-8）
    fn find_char_start(&self, pos: usize) -> usize {
        if pos == 0 {
            return 0;
        }
        // 从 pos-1 开始往回找字符边界
        let mut start = pos - 1;
        while start > 0 && !self.text.is_char_boundary(start) {
            start -= 1;
        }
        start
    }

    /// 查找字符的结束位置（处理 UTF-8）
    fn find_char_end(&self, pos: usize) -> usize {
        let mut end = pos;
        while end < self.text.len() && !self.text.is_char_boundary(end) {
            end += 1;
        }
        end
    }

    /// 更新光标闪烁状态
    fn update_cursor_blink(&mut self) {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_cursor_blink).as_millis() as u64;
        if elapsed >= CURSOR_BLINK_MS {
            self.cursor_visible = !self.cursor_visible;
            self.last_cursor_blink = now;
            self.dirty = true;
        }
    }

    /// 重置光标闪烁
    fn reset_cursor_blink(&mut self) {
        self.cursor_visible = true;
        self.last_cursor_blink = std::time::Instant::now();
        self.dirty = true;
    }

    /// 执行文本布局
    fn do_layout(&self, text: &str, max_width: Option<f32>) -> TextLayout {
        TextEngine::with(|engine| engine.layout(text, &self.style, 1.0, max_width))
    }

    /// 执行占位符布局
    fn do_placeholder_layout(&self, max_width: Option<f32>) -> TextLayout {
        TextEngine::with(|engine| {
            engine.layout(&self.placeholder, &self.placeholder_style, 1.0, max_width)
        })
    }

    /// 测量文本宽度到指定位置
    fn measure_text_width(&self, text: &str, pos: usize) -> f32 {
        if pos == 0 {
            return 0.0;
        }
        let layout = self.do_layout(&text[..pos.min(text.len())], None);
        layout.width()
    }
}

impl Widget for TextInput {
    crate::impl_widget_any!(TextInput);

    fn type_name(&self) -> &'static str {
        "TextInput"
    }

    fn is_dirty(&self) -> bool {
        self.dirty || (self.is_focused && self.cursor_visible)
    }

    fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        let height = INPUT_HEIGHT;

        // 如果有固定宽度，使用固定宽度；否则根据内容自适应
        let (min_width, fixed_width) = if let Some(fixed) = self.fixed_width {
            (fixed, fixed)
        } else {
            // 测量文本尺寸以确定最小宽度
            let text_size = if self.text.is_empty() {
                self.do_placeholder_layout(None)
            } else {
                self.do_layout(&self.text, None)
            };
            let width = text_size.width() + INPUT_PAD_X * 2.0;
            let final_width = width.max(120.0);
            (final_width, final_width)
        };

        LayoutNode::new(id)
            .with_box_style(BoxStyle {
                padding: EdgeInsets::symmetric(INPUT_PAD_X, INPUT_PAD_Y),
                min_size: Size::new(min_width, height),
                max_size: Size::new(f32::INFINITY, height),
                ..Default::default()
            })
            .with_fixed_size(Size::new(fixed_width, height))
    }

    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            let bounds = computed.padding_box;

            // 输入框背景
            let mut node = RenderNode::div(bounds)
                .background(self.bg_color())
                .border_color(self.border_color())
                .border_width(self.border_width())
                .border_radius(INPUT_RADIUS);

            let content_bounds = computed.content_box;

            // 渲染文本或占位符
            let (text_layout, _text_color) = if self.text.is_empty() && !self.is_focused {
                // 显示占位符
                let layout = self.do_placeholder_layout(Some(content_bounds.width));
                (layout, NEUTRAL_400)
            } else {
                // 显示实际文本（包括 IME 预编辑）
                let display_text = if let Some(ref preedit) = self.ime_preedit {
                    let mut text = self.text.clone();
                    text.insert_str(self.cursor_pos, preedit);
                    text
                } else {
                    self.text.clone()
                };
                let layout = self.do_layout(&display_text, Some(content_bounds.width));
                (layout, NEUTRAL_700)
            };

            let text_size = Size::new(text_layout.width(), text_layout.height());
            let text_bounds = Rect::new(
                content_bounds.x,
                content_bounds.y + (content_bounds.height - text_size.height) / 2.0,
                text_size.width,
                text_size.height,
            );

            let text_node = RenderNode::text(text_bounds, text_layout);
            node = node.add_child(text_node);

            // 渲染光标（当获得焦点且光标可见时）
            if self.is_focused && self.cursor_visible {
                let cursor_x = if let Some(ref preedit) = self.ime_preedit {
                    // 如果有 IME 预编辑，光标在预编辑文本中
                    let preedit_pos = self.ime_cursor.unwrap_or(preedit.len());
                    let base_text = &self.text[..self.cursor_pos];
                    let preedit_text = &preedit[..preedit_pos];
                    let full_text = format!("{}{}", base_text, preedit_text);
                    content_bounds.x + self.measure_text_width(&full_text, full_text.len())
                } else {
                    content_bounds.x + self.measure_text_width(&self.text, self.cursor_pos)
                };

                let cursor_y = content_bounds.y + 2.0;
                let cursor_height = content_bounds.height - 4.0;

                let cursor_bounds = Rect::new(cursor_x, cursor_y, CURSOR_WIDTH, cursor_height);

                let cursor_node = RenderNode::div(cursor_bounds).background(NEUTRAL_700);
                node = node.add_child(cursor_node);
            }

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
                self.is_hovered = true;
                self.dirty = true;
            }
            Event::MouseLeave => {
                self.is_hovered = false;
                self.dirty = true;
            }
            Event::FocusIn => {
                self.is_focused = true;
                self.reset_cursor_blink();
                self.dirty = true;
            }
            Event::FocusOut => {
                self.is_focused = false;
                self.ime_preedit = None;
                self.dirty = true;
            }
            Event::KeyDown { key, modifiers } => {
                self.reset_cursor_blink();

                let select = modifiers.shift;

                match key {
                    // 字符输入（非 IME 模式下）
                    Key::Character(c) => {
                        self.insert_text(&c.to_string());
                    }
                    Key::Space => {
                        self.insert_text(" ");
                    }
                    Key::Enter => {
                        // 可以触发 on_submit 回调
                    }
                    Key::Backspace => {
                        self.delete_char_before();
                    }
                    Key::Escape => {
                        self.selection_start = None;
                        self.dirty = true;
                    }
                    Key::Tab => {
                        // Tab 切换到下一个控件
                        return EventResult::Continue;
                    }
                    Key::ArrowLeft => {
                        self.move_cursor(-1, select);
                    }
                    Key::ArrowRight => {
                        self.move_cursor(1, select);
                    }
                    Key::ArrowUp | Key::ArrowDown => {
                        // 单行输入框，上下箭头不移动
                    }
                    _ => {}
                }
            }
            Event::KeyUp { .. } => {}
            Event::ImePreedit {
                text,
                cursor_start,
                cursor_end: _,
            } => {
                // 更新 IME 预编辑状态
                self.ime_preedit = Some(text.clone());
                // 使用 cursor_start 作为光标位置
                self.ime_cursor = *cursor_start;
                self.dirty = true;
            }
            Event::ImeCommit { text } => {
                // IME 提交最终文本
                self.ime_preedit = None;
                self.ime_cursor = None;
                self.insert_text(text);
            }
            Event::ImeDisabled => {
                // IME 被禁用，清除预编辑状态
                self.ime_preedit = None;
                self.ime_cursor = None;
                self.dirty = true;
            }
            _ => {}
        }

        EventResult::Continue
    }

    fn bounds(&self) -> Option<Rect> {
        None
    }
}

impl Default for TextInput {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_input_creation() {
        let input = TextInput::new();
        assert_eq!(input.get_text(), "");
        assert!(!input.is_focused());
    }

    #[test]
    fn test_text_input_with_text() {
        let input = TextInput::new().text("Hello");
        assert_eq!(input.get_text(), "Hello");
        assert_eq!(input.cursor_pos, 5);
    }

    #[test]
    fn test_text_input_insert() {
        let mut input = TextInput::new();
        input.insert_text("Hello");
        assert_eq!(input.get_text(), "Hello");
        assert_eq!(input.cursor_pos, 5);
    }

    #[test]
    fn test_text_input_delete() {
        let mut input = TextInput::new().text("Hello");
        input.delete_char_before();
        assert_eq!(input.get_text(), "Hell");
        assert_eq!(input.cursor_pos, 4);
    }

    #[test]
    fn test_text_input_move_cursor() {
        let mut input = TextInput::new().text("Hello");
        input.move_cursor(-2, false);
        assert_eq!(input.cursor_pos, 3);
        input.move_cursor(1, false);
        assert_eq!(input.cursor_pos, 4);
    }
}
