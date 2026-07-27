use std::cell::Cell;
use std::rc::Rc;

use crate::event::{Event, EventContext, MouseButton};
use crate::layout::style::FlexStyle;
use crate::state::State;
use crate::theme;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::PaintStyle;
use crate::widget::{BuildContext, Widget};

/// 滚动条方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollOrientation {
    #[default]
    Vertical,
    Horizontal,
}

/// 滚动条（ScrollBar）。
///
/// 一个带拖拽缩放的轻量滚动条。依赖外部提供 `offset`、`viewport` 与 `content`。
///
/// `offset` 使用 `State<(f32, f32)>`，与引擎 `FlexStyle::bind_scroll_state` 兼容：
/// - 垂直滚动条读取 `.1`（Y 轴）
/// - 水平滚动条读取 `.0`（X 轴）
///
/// **与 VirtualList 配合：**
/// ```ignore
/// let scroll = State::new((0.0, 0.0));
/// Row::new()
///     .child(VirtualList::new(360.0, 1000, 34.0, scroll.clone()))
///     .child(ScrollBar::vertical(360.0, scroll.clone(), 360.0, 34000.0))
/// ```
///
/// **与 ScrollView 配合（推荐）：**
/// ```ignore
/// let scroll = State::new((0.0, 0.0));
/// ScrollView::new(200.0)
///     .bind_scroll_state(&scroll)
///     .child(content)
/// ```
#[derive(Clone)]
pub struct ScrollBar {
    orientation: ScrollOrientation,
    length: f32,
    /// 滚动偏移。垂直方向读 `.1`，水平方向读 `.0`。
    offset: State<(f32, f32)>,
    viewport: f32,
    content: f32,
    thickness: f32,
}

impl ScrollBar {
    /// 垂直滚动条（读取 `offset.get().1` 作为 Y 轴偏移）。
    pub fn vertical(length: f32, offset: State<(f32, f32)>, viewport: f32, content: f32) -> Self {
        Self {
            orientation: ScrollOrientation::Vertical,
            length,
            offset,
            viewport,
            content,
            thickness: 8.0,
        }
    }

    /// 水平滚动条（读取 `offset.get().0` 作为 X 轴偏移）。
    pub fn horizontal(length: f32, offset: State<(f32, f32)>, viewport: f32, content: f32) -> Self {
        Self {
            orientation: ScrollOrientation::Horizontal,
            length,
            offset,
            viewport,
            content,
            thickness: 8.0,
        }
    }

    /// 设置滚动条厚度（默认 8px）。
    pub fn thickness(mut self, t: f32) -> Self {
        self.thickness = t;
        self
    }
}

impl Widget for ScrollBar {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        let t = theme::current();
        // 根据方向读取对应轴偏移
        let ofs = if self.orientation == ScrollOrientation::Vertical {
            self.offset.get().1
        } else {
            self.offset.get().0
        };
        let max = (self.content - self.viewport).max(1.0);
        let thumb_ratio = (self.viewport / self.content).clamp(0.02, 1.0);
        let thumb_size = (self.length * thumb_ratio).max(8.0);
        let thumb_offset = (ofs / max) * (self.length - thumb_size);

        let is_vertical = self.orientation == ScrollOrientation::Vertical;

        // ---------- 共享状态 ----------
        let captured = Rc::new(Cell::new(false));
        let last_pos = Rc::new(Cell::new(0.0f32));
        let state = self.offset.clone();
        let len = self.length;
        let ts = thumb_size;
        let view = self.viewport;
        let cont = self.content;

        // 轨道布局
        let track_layout = if is_vertical {
            FlexStyle::default().width(self.thickness).height(self.length)
        } else {
            FlexStyle::default().height(self.thickness).width(self.length)
        };

        // ---------- 拖拽开始 ----------
        let on_thumb_down = Rc::new({
            let captured = captured.clone();
            let last_pos = last_pos.clone();
            move |ctx: &mut EventContext| {
                if let Some(Event::MouseDown { button, .. }) = ctx.event() {
                    if *button != MouseButton::Left {
                        return;
                    }
                    captured.set(true);
                    let pos = ctx
                        .current_rect()
                        .map(|r| if is_vertical { r.y as f32 } else { r.x as f32 });
                    if let Some(p) = pos {
                        last_pos.set(p);
                    }
                }
            }
        });

        // ---------- 拖拽移动 ----------
        let on_move = Rc::new({
            let captured = captured.clone();
            let last_pos = last_pos.clone();
            let state = state.clone();
            move |ctx: &mut EventContext| {
                if !captured.get() {
                    return;
                }
                if let Some(Event::MouseMove { x, y, .. }) = ctx.event() {
                    let cur = if is_vertical { *y } else { *x };
                    let delta_px = cur - last_pos.get();
                    last_pos.set(cur);
                    let max_scroll = (cont - view).max(1.0);
                    let scale = max_scroll / (len - ts).max(1.0);
                    state.update(|pair| {
                        let v = if is_vertical { pair.1 } else { pair.0 };
                        let nv = (v + delta_px * scale).clamp(0.0, max_scroll);
                        if is_vertical { pair.1 = nv } else { pair.0 = nv };
                    });
                }
            }
        });

        // ---------- 拖拽释放 + 轨道跳转 ----------
        let on_track_down = Rc::new({
            let captured = captured.clone();
            let state = state.clone();
            move |ctx: &mut EventContext| {
                if captured.get() {
                    return;
                }
                if let Some(Event::MouseDown { x, y, button, .. }) = ctx.event() {
                    if *button != MouseButton::Left {
                        return;
                    }
                    if let Some(r) = ctx.current_rect() {
                        let pos = if is_vertical { *y - r.y as f32 } else { *x - r.x as f32 };
                        let ratio = (pos / len).clamp(0.0, 1.0);
                        let max_scroll = (cont - view).max(1.0);
                        state.update(|pair| {
                            if is_vertical { pair.1 = ratio * max_scroll } else { pair.0 = ratio * max_scroll };
                        });
                    }
                }
            }
        });

        // ---------- 释放 ----------
        let on_up = Rc::new({
            let captured = captured.clone();
            move |ctx: &mut EventContext| {
                if let Some(Event::MouseUp { button, .. }) = ctx.event() {
                    if *button == MouseButton::Left {
                        captured.set(false);
                    }
                }
            }
        });

        // 滑块
        let thumb = ViewNode::Div {
            layout: if is_vertical {
                FlexStyle::default()
                    .width(self.thickness)
                    .height(thumb_size)
                    .margin_top(thumb_offset)
            } else {
                FlexStyle::default()
                    .height(self.thickness)
                    .width(thumb_size)
                    .margin_left(thumb_offset)
            },
            paint: PaintStyle::new()
                .background(t.text.subtle_default)
                .radius(t.radius.small),
            children: vec![],
            listeners: vec![Listener::on_mouse_down(on_thumb_down)],
            key: None,
        };

        // 轨道
        ViewNode::Div {
            layout: track_layout,
            paint: PaintStyle::new()
                .background(t.background.secondary_default)
                .radius(t.radius.small),
            children: vec![thumb],
            listeners: vec![
                Listener::on_mouse_down(on_track_down),
                Listener::on_mouse_move(on_move),
                Listener::on_mouse_up(on_up),
            ],
            key: None,
        }
    }
}
