//! Input — 单行文本输入框
//!
//! MVP 实现：基于 `PlainEditor` 管理文本/光标/IME/选区，
//! 在 `build()` 里用 `PlainEditor::cursor_geometry` 计算 caret 坐标，
//! 用 `selection_geometry` 生成选区高亮矩形，均以绝对定位 `Div` 呈现。
//!
//! 鼠标选取：单击定位光标、拖拽选择（鼠标捕获，可拖出控件）、
//! Shift+单击扩选、双击选词、三击选行。

use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::animation::Animation;
use parley::BoundingBox;

use crate::clipboard;
use crate::event::{Event, EventContext, Key, MouseButton};
use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::text::{
    align_to_utf8_boundary, apply_plain_editor_style, create_plain_editor, editor_cursor_geometry,
    with_text_contexts, PlainTextEditor, TextEngine,
};
use crate::theme;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::{PaintStyle, TextStyle};
use crate::widget::{BuildContext, Stateful, Widget};

/// Input 组件的运行时状态。
pub struct InputState {
    pub editor: PlainTextEditor,
    pub focused: bool,
    /// 是否正在鼠标拖拽选择（MouseDown 置位，MouseUp 清除）。
    pub selecting: bool,
    /// 最近一次用户输入 / 聚焦时刻，用于闪烁光标的"常亮 → 开始闪"计时。
    pub last_activity: Option<Instant>,
    /// 闪烁定时器句柄；聚焦期间存在，失焦时置 `None` 自动取消注册。
    anim: Option<Animation>,
}

/// 闪烁光标参数：聚焦后先常亮 `BLINK_SOLID`，随后以 `BLINK_HALF` 为半周期闪烁。
const BLINK_SOLID: Duration = Duration::from_millis(500);
const BLINK_HALF: Duration = Duration::from_millis(530);
/// 动画 tick 间隔，与半周期一致，保证每次翻转都恰好触发一次重建。
const BLINK_INTERVAL: Duration = BLINK_HALF;

impl InputState {
    pub fn new(style: &TextStyle) -> Self {
        Self {
            editor: create_plain_editor(style),
            focused: false,
            selecting: false,
            last_activity: None,
            anim: None,
        }
    }
}

/// PatternFly 文本输入校验状态（validation state）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputStatus {
    /// 普通
    #[default]
    Default,
    /// 校验通过
    Success,
    /// 警告
    Warning,
    /// 错误
    Danger,
}

/// PatternFly 文本输入尺寸。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputSize {
    /// 小
    Sm,
    /// 中（默认）
    #[default]
    Md,
    /// 大
    Lg,
}

pub struct Input {
    placeholder: String,
    width: f32,
    height: Option<f32>,
    text_style: TextStyle,
    paint: PaintStyle,
    padding_h: Option<f32>,
    padding_v: Option<f32>,
    status: InputStatus,
    size: InputSize,
    multiline: bool,
    on_change: Option<Rc<dyn Fn(String)>>,
    on_submit: Option<Rc<dyn Fn(String)>>,
    listeners: Vec<Listener>,
}

impl Input {
    pub fn new(placeholder: impl Into<String>) -> Self {
        let t = theme::current();
        Self {
            placeholder: placeholder.into(),
            width: 200.0,
            height: None,
            text_style: TextStyle {
                font_size: 0.0,
                color: t.text.regular_default,
                wrap: false,
                text_align: crate::view::paint::TextAlign::Start,
                ..TextStyle::default()
            },
            paint: PaintStyle::new()
                .background(t.background.primary_default)
                .border(1.0, t.border.default)
                .radius(t.radius.small),
            padding_h: None,
            padding_v: None,
            status: InputStatus::Default,
            size: InputSize::Md,
            multiline: false,
            on_change: None,
            on_submit: None,
            listeners: Vec::new(),
        }
    }

    pub fn multiline(mut self, v: bool) -> Self {
        self.multiline = v;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = w.max(0.0);
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = Some(h.max(0.0));
        self
    }

    pub fn padding_h(mut self, v: f32) -> Self {
        self.padding_h = Some(v.max(0.0));
        self
    }

    pub fn padding_v(mut self, v: f32) -> Self {
        self.padding_v = Some(v.max(0.0));
        self
    }

    /// PatternFly 校验状态（非 focus 时以此着色边框）。
    pub fn status(mut self, s: InputStatus) -> Self {
        self.status = s;
        self
    }

    /// PatternFly 尺寸（决定默认 padding 与字号）。
    pub fn size(mut self, s: InputSize) -> Self {
        self.size = s;
        self
    }

    pub fn font_size(mut self, s: f64) -> Self {
        self.text_style.font_size = s;
        self
    }

    pub fn color(mut self, c: Color) -> Self {
        self.text_style.color = c;
        self
    }

    pub fn text_style(mut self, s: TextStyle) -> Self {
        self.text_style = s;
        self
    }

    pub fn placeholder_color(mut self, c: Color) -> Self {
        // placeholder 用更淡的文本色，用户可显式覆盖。
        self.text_style.color = c;
        self
    }

    pub fn background(mut self, c: Color) -> Self {
        self.paint.background_color = Some(c);
        self
    }

    pub fn border_color(mut self, c: Color) -> Self {
        self.paint.border_color = Some(c);
        self
    }

    pub fn radius(mut self, r: f32) -> Self {
        self.paint.border_radius = r;
        self
    }

    pub fn on_change<F: Fn(String) + 'static>(mut self, f: F) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    pub fn on_submit<F: Fn(String) + 'static>(mut self, f: F) -> Self {
        self.on_submit = Some(Rc::new(f));
        self
    }

    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click(Rc::new(f)));
        self
    }
    pub fn on_click_with_ctx<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click_with_ctx(Rc::new(f)));
        self
    }

    fn focus_listener(state: &Stateful<InputState>, focused: bool) -> Listener {
        let state = state.clone();
        let event = if focused {
            crate::event::EventType::FocusIn
        } else {
            crate::event::EventType::FocusOut
        };
        Listener {
            event,
            callback: crate::view::node::Callback::WithCtx(Rc::new(
                move |ctx: &mut EventContext| {
                    state.update(|s| {
                        s.focused = focused;
                        let now = Instant::now();
                        if focused {
                            // 聚焦：进入"常亮"相位并启动闪烁定时器。
                            s.last_activity = Some(now);
                            if s.anim.is_none() {
                                s.anim = Some(Animation::new(BLINK_INTERVAL, || {
                                    crate::state::request_rebuild();
                                }));
                            }
                        } else {
                            // 失焦：停止闪烁定时器（Drop 自动取消注册）。
                            s.anim = None;
                            s.last_activity = None;
                        }
                    });
                    ctx.request_render();
                },
            )),
        }
    }

    fn key_listener(&self, state: &Stateful<InputState>) -> Listener {
        let state = state.clone();
        let on_change = self.on_change.clone();
        let on_submit = self.on_submit.clone();
        let multiline = self.multiline;
        Listener::on_key_down(Rc::new(move |ctx: &mut EventContext| {
            let Some(Event::KeyDown { key, modifiers }) = ctx.event() else {
                return;
            };
            let key = *key;
            let modifiers = *modifiers;
            let mut text_changed = false;
            {
                let mut st = state.get_mut();
                st.last_activity = Some(Instant::now());
                let editor = &mut st.editor;
                match key {
                    Key::Backspace => {
                        with_text_contexts(|fc, lc| editor.driver(fc, lc).backdelete());
                        text_changed = true;
                    }
                    Key::Delete => {
                        with_text_contexts(|fc, lc| editor.driver(fc, lc).delete());
                        text_changed = true;
                    }
                    Key::ArrowLeft => {
                        if modifiers.ctrl && modifiers.shift {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).select_word_left());
                        } else if modifiers.ctrl {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).move_word_left());
                        } else if modifiers.shift {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).select_left());
                        } else {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).move_left());
                        }
                    }
                    Key::ArrowRight => {
                        if modifiers.ctrl && modifiers.shift {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).select_word_right());
                        } else if modifiers.ctrl {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).move_word_right());
                        } else if modifiers.shift {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).select_right());
                        } else {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).move_right());
                        }
                    }
                    Key::ArrowUp => {
                        if modifiers.shift {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).select_up());
                        } else {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).move_up());
                        }
                    }
                    Key::ArrowDown => {
                        if modifiers.shift {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).select_down());
                        } else {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).move_down());
                        }
                    }
                    Key::Home => {
                        if modifiers.shift {
                            with_text_contexts(|fc, lc| {
                                editor.driver(fc, lc).select_to_text_start()
                            });
                        } else {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).move_to_text_start());
                        }
                    }
                    Key::End => {
                        if modifiers.shift {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).select_to_text_end());
                        } else {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).move_to_text_end());
                        }
                    }
                    Key::Character(c) => {
                        if modifiers.ctrl && (c == 'a' || c == 'A') {
                            with_text_contexts(|fc, lc| editor.driver(fc, lc).select_all());
                        } else if modifiers.ctrl && (c == 'c' || c == 'C') {
                            if let Some(selected) = editor.selected_text() {
                                clipboard::copy_text(selected);
                            }
                        } else if modifiers.ctrl && (c == 'x' || c == 'X') {
                            if let Some(selected) = editor.selected_text() {
                                clipboard::copy_text(selected);
                                with_text_contexts(|fc, lc| {
                                    editor.driver(fc, lc).delete_selection()
                                });
                                text_changed = true;
                            }
                        } else if modifiers.ctrl && (c == 'v' || c == 'V') {
                            if let Some(text) = clipboard::paste_text() {
                                with_text_contexts(|fc, lc| {
                                    editor.driver(fc, lc).insert_or_replace_selection(&text)
                                });
                                text_changed = true;
                            }
                        } else if !modifiers.ctrl && !modifiers.alt && !modifiers.meta {
                            let s = c.to_string();
                            with_text_contexts(|fc, lc| {
                                editor.driver(fc, lc).insert_or_replace_selection(&s)
                            });
                            text_changed = true;
                        }
                    }
                    Key::Enter => {
                        if multiline {
                            with_text_contexts(|fc, lc| {
                                editor.driver(fc, lc).insert_or_replace_selection("\n")
                            });
                            text_changed = true;
                        } else if let Some(cb) = &on_submit {
                            let text = editor.raw_text().to_string();
                            cb(text);
                        }
                    }
                    Key::Escape => {
                        // 取消选区，光标停在 focus 端。
                        with_text_contexts(|fc, lc| editor.driver(fc, lc).collapse_selection());
                    }
                    _ => {}
                }
            }
            if text_changed {
                if let Some(cb) = &on_change {
                    let text = state.get().editor.raw_text().to_string();
                    cb(text);
                }
            }
            ctx.request_render();
            ctx.request_rebuild();
        }))
    }

    fn ime_preedit_listener(state: &Stateful<InputState>) -> Listener {
        let state = state.clone();
        Listener::on_ime_preedit(Rc::new(move |ctx: &mut EventContext| {
            let Some(Event::ImePreedit {
                text,
                cursor_start,
                cursor_end,
            }) = ctx.event()
            else {
                return;
            };
            let text = text.clone();
            state.update(|s| {
                with_text_contexts(|fc, lc| {
                    let mut driver = s.editor.driver(fc, lc);
                    if text.is_empty() {
                        // 空预编辑文本表示合成结束或取消，清理即可。
                        driver.clear_compose();
                    } else {
                        let start = cursor_start
                            .map(|i| align_to_utf8_boundary(&text, i))
                            .unwrap_or(text.len());
                        let end = cursor_end
                            .map(|i| align_to_utf8_boundary(&text, i))
                            .unwrap_or(start);
                        let start = start.min(end);
                        let end = end.max(start);
                        driver.set_compose(&text, Some((start, end)));
                    }
                });
            });
            ctx.request_render();
            ctx.request_rebuild();
        }))
    }

    fn ime_commit_listener(state: &Stateful<InputState>) -> Listener {
        let state = state.clone();
        Listener::on_ime_commit(Rc::new(move |ctx: &mut EventContext| {
            let Some(Event::ImeCommit { text }) = ctx.event() else {
                return;
            };
            let text = text.clone();
            state.update(|s| {
                with_text_contexts(|fc, lc| {
                    let mut driver = s.editor.driver(fc, lc);
                    driver.clear_compose();
                    driver.insert_or_replace_selection(&text);
                });
            });
            ctx.request_render();
            ctx.request_rebuild();
        }))
    }

    fn ime_disabled_listener(state: &Stateful<InputState>) -> Listener {
        let state = state.clone();
        Listener::on_ime_disabled(Rc::new(move |ctx: &mut EventContext| {
            state.update(|s| {
                with_text_contexts(|fc, lc| {
                    s.editor.driver(fc, lc).clear_compose();
                });
            });
            ctx.request_render();
            ctx.request_rebuild();
        }))
    }

    /// 鼠标按下：定位光标 / Shift+单击扩选 / 双击选词 / 三击选行，
    /// 并请求鼠标捕获以支持拖出控件继续选择。
    fn mouse_down_listener(&self, state: &Stateful<InputState>) -> Listener {
        let state = state.clone();
        let (sz_pv, sz_ph, _sz_font) = input_size_metrics(theme::current(), self.size);
        let (pad_h, pad_v) = (
            self.padding_h.unwrap_or(sz_ph),
            self.padding_v.unwrap_or(sz_pv),
        );
        Listener::on_mouse_down(Rc::new(move |ctx: &mut EventContext| {
            let (x, y, shift, clicks) = match ctx.event() {
                Some(Event::MouseDown {
                    x,
                    y,
                    button: MouseButton::Left,
                    modifiers,
                    click_count,
                }) => (*x, *y, modifiers.shift, *click_count),
                _ => return,
            };
            let Some(rect) = ctx.current_rect() else {
                return;
            };
            // 窗口坐标 → 文本局部坐标（扣除控件位置与内边距）。
            let lx = x - rect.x - pad_h;
            let ly = y - rect.y - pad_v;
            state.update(|s| {
                s.selecting = true;
                s.last_activity = Some(Instant::now());
                with_text_contexts(|fc, lc| {
                    let mut driver = s.editor.driver(fc, lc);
                    match clicks {
                        2 => driver.select_word_at_point(lx, ly),
                        3 => driver.select_line_at_point(lx, ly),
                        _ if shift => driver.shift_click_extension(lx, ly),
                        _ => driver.move_to_point(lx, ly),
                    }
                });
            });
            ctx.capture_mouse();
            ctx.request_render();
            ctx.request_rebuild();
        }))
    }

    /// 鼠标移动：拖拽中持续扩展选区（仅 selecting 期间生效）。
    fn mouse_move_listener(&self, state: &Stateful<InputState>) -> Listener {
        let state = state.clone();
        let (sz_pv, sz_ph, _sz_font) = input_size_metrics(theme::current(), self.size);
        let (pad_h, pad_v) = (
            self.padding_h.unwrap_or(sz_ph),
            self.padding_v.unwrap_or(sz_pv),
        );
        Listener::on_mouse_move(Rc::new(move |ctx: &mut EventContext| {
            if !state.get().selecting {
                return;
            }
            let (x, y) = match ctx.event() {
                Some(Event::MouseMove { x, y }) => (*x, *y),
                _ => return,
            };
            let Some(rect) = ctx.current_rect() else {
                return;
            };
            let lx = x - rect.x - pad_h;
            let ly = y - rect.y - pad_v;
            state.update(|s| {
                s.last_activity = Some(Instant::now());
                with_text_contexts(|fc, lc| {
                    s.editor.driver(fc, lc).extend_selection_to_point(lx, ly);
                });
            });
            ctx.request_render();
            ctx.request_rebuild();
        }))
    }

    /// 鼠标释放：结束拖拽选择（捕获由 EventManager 自动解除）。
    fn mouse_up_listener(state: &Stateful<InputState>) -> Listener {
        let state = state.clone();
        Listener::on_mouse_up(Rc::new(move |_ctx: &mut EventContext| {
            if state.get().selecting {
                state.update(|s| s.selecting = false);
            }
        }))
    }
}

/// 返回 (垂直 padding, 水平 padding, 字号)。
fn input_size_metrics(t: theme::Theme, s: InputSize) -> (f32, f32, f64) {
    match s {
        InputSize::Sm => (4.0, 8.0, t.font.sm),
        InputSize::Md => (6.0, 12.0, t.font.sm),
        InputSize::Lg => (8.0, 16.0, t.font.md),
    }
}

/// 校验状态对应的边框色。
fn input_status_color(t: theme::Theme, s: InputStatus) -> Color {
    match s {
        InputStatus::Default => t.border.default,
        InputStatus::Success => t.status.success,
        InputStatus::Warning => t.status.warning,
        InputStatus::Danger => t.status.danger,
    }
}

impl Widget for Input {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let t = theme::current();

        // 尺寸解析：size 决定默认 padding/字号，可被显式覆盖。
        let (sz_pv, sz_ph, sz_font) = input_size_metrics(t, self.size);
        let pv = self.padding_v.unwrap_or(sz_pv);
        let ph = self.padding_h.unwrap_or(sz_ph);
        let mut text_style = self.text_style.clone();
        if text_style.font_size <= 0.0 {
            text_style.font_size = sz_font;
        }

        let state = ctx.use_state(|| InputState::new(&text_style));

        let content_width = (self.width - ph * 2.0).max(0.0);
        // 单行内容高度以实际文本行高（含 ascent/descent/行距）为准，而非仅 font_size；
        // 否则盒子比文本矮，文本会溢出并贴底。
        let line_h = TextEngine::measure_text("Mg", &text_style).1 as f32;
        let inner_height = self
            .height
            .unwrap_or(line_h + pv * 2.0);

        // 同步样式/宽度，并计算光标与选区几何。
        let (text_content, caret, sel_rects, focused) = {
            let mut st = state.get_mut();
            apply_plain_editor_style(&mut st.editor, &text_style);
            let caret =
                editor_cursor_geometry(&mut st.editor, &text_style, Some(content_width), 1.0);
            // editor_cursor_geometry 已 refresh_layout，选区几何直接可取。
            let sel_rects = st.editor.selection_geometry();
            let text_content = st.editor.raw_text().to_string();
            (text_content, caret, sel_rects, st.focused)
        };

        // 闪烁光标：聚焦后先常亮一段时间，再按半周期翻转显隐。
        let show_caret = if focused {
            match state.get().last_activity {
                Some(t) => {
                    let elapsed = Instant::now().duration_since(t);
                    if elapsed < BLINK_SOLID {
                        true
                    } else {
                        let half = BLINK_HALF.as_millis();
                        ((elapsed - BLINK_SOLID).as_millis() / half) % 2 == 0
                    }
                }
                None => true,
            }
        } else {
            false
        };

        let is_empty = text_content.is_empty();
        let show_placeholder = is_empty && !focused;
        let display_text = if show_placeholder {
            self.placeholder.clone()
        } else {
            text_content
        };
        let display_style = {
            let mut s = if show_placeholder {
                let mut s = text_style.clone();
                s.color = t.text.subtle_default;
                s
            } else {
                text_style.clone()
            };
            s.wrap = self.multiline;
            s
        };

        // 单行时让文本垂直居中：以一致的文本行高（line_h）计算居中偏移，
        // 避免空文本时 measure 返回 0 导致光标错位。多行时无需偏移。
        let content_inner_h = inner_height - pv * 2.0;
        let center_offset = if self.multiline {
            0.0
        } else {
            ((content_inner_h - line_h) / 2.0).max(0.0)
        };

        let text_node = ViewNode::Text {
            content: display_text,
            style: display_style,
            layout: FlexStyle::default().flex_grow(1.0),
            key: None,
            listeners: vec![],
        };

        // 选区高亮：排在文本之前（先绘），品牌色半透明，不遮挡字形。
        let mut children: Vec<ViewNode> = Vec::new();
        if focused && !sel_rects.is_empty() {
            let b = t.text.brand_default;
            let sel_color = Color::rgba(b.r, b.g, b.b, 0x4d);
            for (i, (bb, _line)) in sel_rects.iter().enumerate() {
                let w = (bb.x1 - bb.x0) as f32;
                let h = (bb.y1 - bb.y0) as f32;
                if w <= 0.0 || h <= 0.0 {
                    continue;
                }
                children.push(ViewNode::Div {
                    layout: FlexStyle::default()
                        .width(w)
                        .height(h)
                        .absolute()
                        .position_left(ph + bb.x0 as f32)
                        .position_top(pv + center_offset + bb.y0.max(0.0) as f32),
                    paint: PaintStyle::new().background(sel_color),
                    key: Some(format!("__sel_{}__", i)),
                    children: vec![],
                    listeners: vec![],
                });
            }
        }
        children.push(text_node);

        // 光标：聚焦、几何可用且当前处于"点亮"相位时显示。
        if focused && show_caret {
            if let Some(BoundingBox { x0, y0, y1, .. }) = caret {
                let caret_y = y0.max(0.0);
                // 光标高度取该行块高度（与文本行一致），不再撑满整个输入框。
                let caret_height = (y1 - y0).max(8.0);
                let caret = ViewNode::Div {
                    layout: FlexStyle::default()
                        .width(1.0)
                        .height(caret_height as f32)
                        .absolute()
                        .position_left(ph + x0 as f32)
                        .position_top(pv + center_offset + caret_y as f32),
                    paint: PaintStyle::new().background(text_style.color),
                    key: Some("__ime_caret__".to_string()),
                    children: vec![],
                    listeners: vec![],
                };
                children.push(caret);
            }
        }

        // 焦点时边框用品牌色；否则按校验状态着色（非 Default）。
        let mut paint = self.paint.clone();
        if focused {
            paint.border_color = Some(t.text.brand_default);
        } else if self.status != InputStatus::Default {
            paint.border_color = Some(input_status_color(t, self.status));
        }

        let mut layout = FlexStyle::row()
            .align_items(if self.multiline {
                FlexAlign::Start
            } else {
                FlexAlign::Center
            })
            .width(self.width)
            .height(inner_height)
            .padding_left(ph)
            .padding_right(ph)
            .padding_top(pv)
            .padding_bottom(pv);
        if let Some(h) = self.height {
            layout = layout.height(h);
        }

        let mut listeners = self.listeners.clone();
        listeners.push(Self::focus_listener(&state, true));
        listeners.push(Self::focus_listener(&state, false));
        listeners.push(self.key_listener(&state));
        listeners.push(self.mouse_down_listener(&state));
        listeners.push(self.mouse_move_listener(&state));
        listeners.push(Self::mouse_up_listener(&state));
        listeners.push(Self::ime_preedit_listener(&state));
        listeners.push(Self::ime_commit_listener(&state));
        listeners.push(Self::ime_disabled_listener(&state));

        ViewNode::Div {
            layout,
            paint,
            key: None,
            children,
            listeners,
        }
    }
}
