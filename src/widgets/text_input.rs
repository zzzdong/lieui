// src/widgets/text_input.rs
//! TextInput Widget - 文本输入控件
//!
//! 基于 parley::PlainEditor 实现, 支持键盘输入和 IME。
//! 尺寸由父控件约束或固定宽度决定, 不受内部内容影响。

use crate::core::WidgetId;
use crate::event::{Event, EventContext, EventResult, Key};
use crate::geometry::{Color, Rect, Size};
use crate::layout::{BoxStyle, EdgeInsets, LayoutNode};
use crate::prelude::ViewContext;
use crate::render::RenderNode;
use crate::text::{TextColor, TextStyle};
use crate::widget::Widget;
use clipboard_rs::{Clipboard, ClipboardContext};
use parley::PlainEditor;
use parley::editing::PlainEditorDriver;
use vello_cpu::color::AlphaColor;

// ============================================
// Fluent UI 颜色系统
// ============================================
const NEUTRAL_WHITE: Color = Color(AlphaColor::from_rgb8(255, 255, 255));
const NEUTRAL_100: Color = Color(AlphaColor::from_rgb8(244, 244, 244));
const NEUTRAL_200: Color = Color(AlphaColor::from_rgb8(234, 234, 234));
const NEUTRAL_300: Color = Color(AlphaColor::from_rgb8(200, 200, 200));
const NEUTRAL_400: Color = Color(AlphaColor::from_rgb8(166, 166, 166));
const NEUTRAL_600: Color = Color(AlphaColor::from_rgb8(96, 96, 96));
const NEUTRAL_700: Color = Color(AlphaColor::from_rgb8(51, 51, 51));
const THEME_PRIMARY: Color = Color(AlphaColor::from_rgb8(0, 120, 212));

// ============================================
// 尺寸规范
// ============================================
const INPUT_HEIGHT: f32 = 32.0;
const INPUT_PAD_X: f32 = 12.0;
const INPUT_PAD_Y: f32 = 6.0;
const INPUT_RADIUS: f32 = 4.0;
const CURSOR_WIDTH: f32 = 1.0;

/// 文本输入控件
///
/// 内部使用 [`PlainEditor`] 管理文本缓冲区、光标和选区,
/// 通过 [`PlainEditorDriver`] 执行编辑操作, 自动维护布局刷新。
pub struct TextInput {
    /// parley 纯文本编辑器 (缓冲区 + 光标 + 选区 + 布局)
    editor: PlainEditor<TextColor>,
    /// 占位符文本
    placeholder: String,
    /// 占位符样式
    placeholder_style: TextStyle,
    /// 是否获得焦点
    is_focused: bool,
    /// 是否悬停
    is_hovered: bool,
    /// 是否禁用
    is_disabled: bool,
    /// 脏标记
    dirty: bool,
    /// 固定宽度 (None 表示由父容器约束决定)
    fixed_width: Option<f32>,
    /// 固定高度 (None 表示由父容器约束决定)
    fixed_height: Option<f32>,
    /// 是否为多行模式
    multiline: bool,
    /// 鼠标左键是否按下（用于拖动选区）
    is_mouse_down: bool,
    /// 内容区域边界（在 render 时更新，用于事件处理时的坐标转换）
    content_bounds: Option<Rect>,
}

impl TextInput {
    /// 创建新的 TextInput
    pub fn new() -> Self {
        // PlainEditor::new(font_size) 创建编辑器, 默认字体大小 14.0[reference:1]
        let editor = PlainEditor::new(14.0);

        let mut placeholder_style = TextStyle::default();
        placeholder_style.0.brush = TextColor(NEUTRAL_400.0);
        placeholder_style.0.font_size = 14.0;

        Self {
            editor,
            placeholder: String::new(),
            placeholder_style,
            is_focused: false,
            is_hovered: false,
            is_disabled: false,
            dirty: true,
            fixed_width: None,
            fixed_height: None,
            multiline: false,
            is_mouse_down: false,
            content_bounds: None,
        }
    }

    /// 设置占位符文本
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// 设置固定宽度
    pub fn width(mut self, width: f32) -> Self {
        self.fixed_width = Some(width);
        self
    }

    /// 设置固定高度
    pub fn height(mut self, height: f32) -> Self {
        self.fixed_height = Some(height);
        self
    }

    /// 设置是否为多行模式
    pub fn multiline(mut self, multiline: bool) -> Self {
        self.multiline = multiline;
        self
    }

    /// 设置禁用状态
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.is_disabled = disabled;
        self
    }

    /// 获取当前文本 (不包括 IME 预编辑)[reference:2]
    pub fn get_text(&self) -> String {
        self.editor.text().into_iter().flat_map(|s| s.chars()).collect()
    }

    /// 设置文本内容（链式调用）
    pub fn text(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        if !text.is_empty() {
            self.with_driver(|driver| {
                driver.select_all();
                driver.insert_or_replace_selection(&text);
            });
        }
        self
    }

    /// 获取背景色
    fn bg_color(&self) -> Color {
        if self.is_disabled { NEUTRAL_100 } else { NEUTRAL_WHITE }
    }

    /// 获取边框色
    fn border_color(&self) -> Color {
        if self.is_disabled { NEUTRAL_200 }
        else if self.is_focused { THEME_PRIMARY }
        else if self.is_hovered { NEUTRAL_600 }
        else { NEUTRAL_300 }
    }

    /// 获取边框宽度
    fn border_width(&self) -> f32 {
        if self.is_focused { 2.0 } else { 1.0 }
    }

    /// 获取 PlainEditorDriver 并执行操作
    fn with_driver<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut PlainEditorDriver<TextColor>) -> R,
    {
        crate::text::with_text_contexts(|font_cx, layout_cx| {
            let mut driver = self.editor.driver(font_cx, layout_cx);
            f(&mut driver)
        })
    }

    /// 更新光标闪烁状态
    fn reset_cursor_blink(&mut self) {
        self.dirty = true;
    }

    /// 编辑后的通用处理
    fn after_edit(&mut self) {
        self.dirty = true;
        // 多行模式下需要刷新布局以更新高度
        if self.multiline {
            self.refresh_layout();
        }
    }

    /// 刷新文本布局
    fn refresh_layout(&mut self) {
        self.with_driver(|_driver| {
            // driver 会自动处理布局刷新
        });
    }

    /// 复制选中的文本到剪贴板
    fn copy_selection(&mut self) {
        if let Some(text) = self.editor.selected_text()
            && let Ok(clipboard) = ClipboardContext::new() {
                let _ = clipboard.set_text(text.to_owned());
            }
    }

    /// 剪切选中的文本到剪贴板
    fn cut_selection(&mut self) {
        self.copy_selection();
        self.with_driver(|driver| {
            driver.delete_selection();
        });
        self.after_edit();
    }

    /// 从剪贴板粘贴文本
    fn paste_from_clipboard(&mut self) {
        if let Ok(clipboard) = ClipboardContext::new()
            && let Ok(text) = clipboard.get_text() {
                self.with_driver(|driver| {
                    driver.insert_or_replace_selection(&text);
                });
                self.after_edit();
            }
    }

    /// 将全局坐标转换为本地文本坐标
    fn global_to_local(&self, x: f32, y: f32) -> (f32, f32) {
        if let Some(content_bounds) = self.content_bounds {
            let text_vertical_offset = if let Some(layout_ref) = self.editor.try_layout() {
                let text_height = layout_ref.height();
                if self.multiline {
                    0.0
                } else {
                    (content_bounds.height - text_height) / 2.0
                }
            } else {
                0.0
            };
            let local_x = x - content_bounds.x;
            let local_y = y - content_bounds.y - text_vertical_offset;
            return (local_x, local_y);
        }
        (x, y)
    }
}

impl Widget for TextInput {
    crate::impl_widget_any!(TextInput);

    fn type_name(&self) -> &'static str { "TextInput" }

    fn is_dirty(&self) -> bool { self.dirty }

    fn clear_dirty(&mut self) { self.dirty = false; }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        // 多行模式下，高度根据内容自适应；单行模式下使用固定高度
        let (min_height, max_height) = if self.multiline {
            // 多行：最小高度为单行高度，最大高度无限制或由 fixed_height 决定
            let min_h = self.fixed_height.unwrap_or(INPUT_HEIGHT);
            let max_h = self.fixed_height.unwrap_or(f32::INFINITY);
            (min_h, max_h)
        } else {
            // 单行：固定高度
            let h = self.fixed_height.unwrap_or(INPUT_HEIGHT);
            (h, h)
        };

        // 固定宽度或依赖父容器约束 (最小宽度 120px)
        let min_width = self.fixed_width.unwrap_or(120.0);
        let max_width = self.fixed_width.unwrap_or(f32::INFINITY);

        LayoutNode::new(id)
            .with_box_style(BoxStyle {
                padding: EdgeInsets::symmetric(INPUT_PAD_X, INPUT_PAD_Y),
                min_size: Size::new(min_width, min_height),
                max_size: Size::new(max_width, max_height),
                ..Default::default()
            })
            .with_border_radius(Some(INPUT_RADIUS))
    }

    fn render(&mut self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            let bounds = computed.padding_box;
            // 保存内容区域边界，用于事件处理时的坐标转换
            self.content_bounds = Some(computed.content_box);

            let mut node = RenderNode::div(bounds)
                .background(self.bg_color())
                .border_color(self.border_color())
                .border_width(self.border_width())
                .border_radius(INPUT_RADIUS);

            let content_bounds = computed.content_box;

            // 计算文本的垂直偏移量（用于光标和选区的正确位置）
            let text_vertical_offset = if let Some(layout_ref) = self.editor.try_layout() {
                let text_height = layout_ref.height();
                if self.multiline {
                    0.0 // 多行模式：顶部对齐，无偏移
                } else {
                    // 单行模式：垂直居中的偏移量
                    (content_bounds.height - text_height) / 2.0
                }
            } else {
                0.0
            };

            // 1. 首先渲染选区高亮（在文本下方）
            for (sel_rect, _) in self.editor.selection_geometry() {
                let sel_bounds = Rect::new(
                    content_bounds.x + sel_rect.x0 as f32,
                    content_bounds.y + text_vertical_offset + sel_rect.y0 as f32,
                    sel_rect.width() as f32,
                    sel_rect.height() as f32,
                );
                let sel_node = RenderNode::div(sel_bounds).background(
                    Color(AlphaColor::from_rgba8(0, 120, 212, 128))
                );
                node = node.add_child(sel_node);
            }

            // 2. 然后渲染文本
            if let Some(layout_ref) = self.editor.try_layout() {
                let text_size = Size::new(layout_ref.width(), layout_ref.height());
                // 多行模式：顶部对齐；单行模式：垂直居中
                let text_y = if self.multiline {
                    content_bounds.y
                } else {
                    content_bounds.y + (content_bounds.height - text_size.height) / 2.0
                };
                let text_bounds = Rect::new(
                    content_bounds.x,
                    text_y,
                    text_size.width,
                    text_size.height,
                );

                // 使用 PlainEditor 的 try_layout 获取布局并渲染
                if let Some(layout) = self.editor.try_layout() {
                    let text_node = RenderNode::text(text_bounds, layout.clone());
                    node = node.add_child(text_node);
                }
            } else if self.editor.text().into_iter().flat_map(|s| s.chars()).count() == 0 {
                // 空文本时显示占位符
                let layout = crate::text::TextEngine::layout(
                    &self.placeholder,
                    &self.placeholder_style,
                    1.0,
                    Some(content_bounds.width)
                );
                let text_size = Size::new(layout.width(), layout.height());
                // 多行模式：顶部对齐；单行模式：垂直居中
                let text_y = if self.multiline {
                    content_bounds.y
                } else {
                    content_bounds.y + (content_bounds.height - text_size.height) / 2.0
                };
                let text_bounds = Rect::new(
                    content_bounds.x,
                    text_y,
                    text_size.width,
                    text_size.height,
                );
                let text_node = RenderNode::text(text_bounds, layout);
                node = node.add_child(text_node);
            }

            // 3. 最后渲染光标 (当获得焦点时，在文本上方)
            if self.is_focused
                && let Some(cursor_geometry) = self.editor.cursor_geometry(CURSOR_WIDTH) {
                    let cursor_x = content_bounds.x + cursor_geometry.x0 as f32;
                    // 光标 Y 位置 = 内容区域 Y + 文本垂直偏移 + 光标在文本中的 Y 位置
                    let cursor_y = content_bounds.y + text_vertical_offset + cursor_geometry.y0 as f32;
                    let cursor_height = cursor_geometry.height() as f32;

                    let cursor_bounds = Rect::new(cursor_x, cursor_y, CURSOR_WIDTH, cursor_height);
                    let cursor_node = RenderNode::div(cursor_bounds).background(NEUTRAL_700);
                    node = node.add_child(cursor_node);
                }

            node
        } else {
            RenderNode::div(Rect::zero())
        }
    }

    fn can_focus(&self) -> bool { !self.is_disabled }

    fn handle_event(&mut self, event: &Event, ctx: &EventContext) -> EventResult {
        if self.is_disabled { return EventResult::Continue; }

        match event {
            Event::MouseEnter => { self.is_hovered = true; self.dirty = true; ctx.request_render(); }
            Event::MouseLeave => { self.is_hovered = false; self.dirty = true; ctx.request_render(); }
            Event::FocusIn => { self.is_focused = true; self.reset_cursor_blink(); self.dirty = true; ctx.request_render(); }
            Event::FocusOut => { self.is_focused = false; self.dirty = true; ctx.request_render(); }

            Event::MouseDown { x, y, button, .. } if *button == crate::event::MouseButton::Left => {
                self.is_mouse_down = true;
                // 使用 move_to_point 定位光标[reference:4]
                let (local_x, local_y) = self.global_to_local(*x, *y);
                self.with_driver(|driver| {
                    driver.move_to_point(local_x, local_y);
                });
                self.after_edit();
            }

            Event::MouseUp { button, .. } if *button == crate::event::MouseButton::Left => {
                self.is_mouse_down = false;
            }

            Event::MouseMove { x, y, .. }
                // 如果鼠标左键按下，更新选区
                if self.is_mouse_down => {
                    let (local_x, local_y) = self.global_to_local(*x, *y);
                    self.with_driver(|driver| {
                        driver.extend_selection_to_point(local_x, local_y);
                    });
                    self.after_edit();
                }

            Event::KeyDown { key, modifiers } => {
                self.reset_cursor_blink();
                let select = modifiers.shift;
                let ctrl = modifiers.ctrl;

                // 处理 Ctrl+字符 快捷键 (需要在 Character 匹配之前处理)
                if ctrl {
                    match key {
                        Key::Character('a') | Key::Character('A') => {
                            self.with_driver(|driver| driver.select_all());
                            self.after_edit();
                            return EventResult::Continue;
                        }
                        Key::Character('c') | Key::Character('C') => {
                            self.copy_selection();
                            return EventResult::Continue;
                        }
                        Key::Character('x') | Key::Character('X') => {
                            self.cut_selection();
                            return EventResult::Continue;
                        }
                        Key::Character('v') | Key::Character('V') => {
                            self.paste_from_clipboard();
                            return EventResult::Continue;
                        }
                        _ => {}
                    }
                }

                match key {
                    Key::Character(c) => {
                        self.with_driver(|driver| {
                            driver.insert_or_replace_selection(&c.to_string());
                        });
                        self.after_edit();
                    }
                    Key::Space => {
                        self.with_driver(|driver| {
                            driver.insert_or_replace_selection(" ");
                        });
                        self.after_edit();
                    }
                    Key::Backspace => {
                        self.with_driver(|driver| {
                            driver.backdelete();
                        });
                        self.after_edit();
                    }
                    Key::Delete => {
                        self.with_driver(|driver| {
                            driver.delete();
                        });
                        self.after_edit();
                    }
                    Key::Enter => {
                        self.with_driver(|driver| {
                            driver.insert_or_replace_selection("\n");
                        });
                        self.after_edit();
                    }
                    Key::Home => {
                        self.with_driver(|driver| {
                            if select { driver.select_to_line_start(); }
                            else { driver.move_to_line_start(); }
                        });
                        self.after_edit();
                    }
                    Key::End => {
                        self.with_driver(|driver| {
                            if select { driver.select_to_line_end(); }
                            else { driver.move_to_line_end(); }
                        });
                        self.after_edit();
                    }
                    Key::ArrowLeft => {
                        self.with_driver(|driver| {
                            if select { driver.select_left(); }
                            else { driver.move_left(); }
                        });
                        self.after_edit();
                    }
                    Key::ArrowRight => {
                        self.with_driver(|driver| {
                            if select { driver.select_right(); }
                            else { driver.move_right(); }
                        });
                        self.after_edit();
                    }
                    Key::ArrowUp => {
                        self.with_driver(|driver| {
                            if select { driver.select_up(); }
                            else { driver.move_up(); }
                        });
                        self.after_edit();
                    }
                    Key::ArrowDown => {
                        self.with_driver(|driver| {
                            if select { driver.select_down(); }
                            else { driver.move_down(); }
                        });
                        self.after_edit();
                    }
                    Key::Escape => {
                        self.with_driver(|driver| {
                            driver.collapse_selection();
                        });
                        self.after_edit();
                    }
                    Key::Tab => return EventResult::Continue,
                    _ => {}
                }
            }
            Event::ImePreedit { text, cursor_start, cursor_end } => {
                if text.is_empty() {
                    // 空预编辑文本时清除合成状态
                    self.with_driver(|driver| driver.clear_compose());
                } else {
                    let cursor = cursor_start.map(|s| (s, cursor_end.unwrap_or(s)));
                    self.with_driver(|driver| {
                        driver.set_compose(text.as_str(), cursor);
                    });
                }
                self.dirty = true;
            }
            Event::ImeCommit { text }
                if !text.is_empty() => {
                    self.with_driver(|driver| {
                        driver.finish_compose();
                        driver.insert_or_replace_selection(text.as_str());
                    });
                    self.after_edit();
                }
            Event::ImeDisabled => {
                self.with_driver(|driver| driver.clear_compose());
                self.dirty = true;
            }
            _ => {}
        }

        EventResult::Continue
    }

    fn bounds(&self) -> Option<Rect> { None }
}

impl Default for TextInput {
    fn default() -> Self { Self::new() }
}
