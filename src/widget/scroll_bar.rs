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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollOrientation {
    Vertical,
    Horizontal,
}

/// 滚动条（ScrollBar）。
///
/// 一个带拖拽缩放的轻量滚动条。依赖外部提供 `offset`、`viewport` 与 `content`。
///
/// **与 VirtualList 配合：**
/// ```ignore
/// Row::new()
///     .child(VirtualList::new(360.0, 1000, 34.0, scroll_y.clone()))
///     .child(ScrollBar::vertical(360.0, scroll_y.clone(), 360.0, 34000.0))
/// ```
///
/// **与 ScrollView 配合**（先 `bind_scroll_y` 获得 State）：
/// ```ignore
/// let scroll_ofs = State::new(0.0);
/// ScrollView::new(200.0)
///     .bind_scroll_y(&scroll_ofs)
///     .child(content)
/// ```
pub struct ScrollBar {
    orientation: ScrollOrientation,
    length: f32,
    offset: State<f32>,
    viewport: f32,
    content: f32,
    thickness: f32,
}

impl ScrollBar {
    /// 垂直滚动条。
    pub fn vertical(length: f32, offset: State<f32>, viewport: f32, content: f32) -> Self {
        Self {
            orientation: ScrollOrientation::Vertical,
            length,
            offset,
            viewport,
            content,
            thickness: 8.0,
        }
    }

    /// 水平滚动条。
    pub fn horizontal(length: f32, offset: State<f32>, viewport: f32, content: f32) -> Self {
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
        let max = (self.content - self.viewport).max(1.0);
        let thumb_ratio = (self.viewport / self.content).clamp(0.02, 1.0);
        let thumb_size = (self.length * thumb_ratio).max(8.0);
        let thumb_offset = (*self.offset.get() / max) * (self.length - thumb_size);

        let is_vertical = self.orientation == ScrollOrientation::Vertical;

        // ---------- 共享状态 ----------
        let captured = Rc::new(Cell::new(false));
        let last_pos = Rc::new(Cell::new(0.0f32));
        let state = self.offset.clone();
        let _max_c = max;
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
                    // 记录初始鼠标位置（视口内的本地坐标）
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
                    // 像素偏移 → 内容偏移
                    let max_scroll = (cont - view).max(1.0);
                    let scale = max_scroll / (len - ts).max(1.0);
                    state.update(|v| {
                        *v = (*v + delta_px * scale).clamp(0.0, max_scroll)
                    });
                }
            }
        });

        // ---------- 拖拽释放 + 轨道跳转 ----------
        let on_track_down = Rc::new({
            let captured = captured.clone();
            let state = state.clone();
            move |ctx: &mut EventContext| {
                // 如果正在拖拽，不处理跳转
                if captured.get() {
                    return;
                }
                if let Some(Event::MouseDown { x, y, button, .. }) = ctx.event() {
                    if *button != MouseButton::Left {
                        return;
                    }
                    // 点击轨道空白处：跳转到对应比例
                    if let Some(r) = ctx.current_rect() {
                        let pos = if is_vertical { *y - r.y as f32 } else { *x - r.x as f32 };
                        let ratio = (pos / len).clamp(0.0, 1.0);
                        let max_scroll = (cont - view).max(1.0);
                        state.set(ratio * max_scroll);
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
