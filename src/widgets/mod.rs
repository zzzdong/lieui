//! 内置组件的**绘制**分派：`Kind` 枚举 → 绘制原语（设计 §3.8 "按 `Kind` 枚举分派"）。
//!
//! 为什么在这里而不是在 `scene.rs`：`scene.rs` 只负责"树 → 列表"的遍历与状态
//! （裁剪/变换/不透明度/子节点顺序），**组件长什么样**属于组件自己的知识。
//! M5 会把行为（`handle`）也放到这个目录，形成"构造 / 行为 / 绘制"三件套。
//!
//! 视图态（hover / pressed / disabled）在这里解析 ⇒ 鼠标划过只需重绘，不必重跑 `view()`。

use lieui_geom::{Color, Point, Rect, Size};
use lieui_text::{TextEngine, TextSpec};

use crate::cmd::CmdBuf;
use crate::event::{Event, EventKind, EventView, KeyCode, NamedKey, PointerButton, Routing};
use crate::render::scene::{Cull, Op, Scene, TextCache};
use crate::track::{FocusState, Kind, KindTag, Node, NodeId, ScrollDrag, Track, clamp_to_char_boundary};
use crate::transform::Affine;

// ───────────────────────── 内置行为（M5）─────────────────────────

/// 沿命中链执行**框架内置行为**。
///
/// 与用户处理器的分工（设计 §3.5 的两段式分发）：
/// - 内置行为需要 `&mut Track`（改视图态、按几何算值），所以它是 `dispatch` 里**独立的先一遍**，
///   而不是塞进 `HandlerSlot` 的闭包（闭包只拿得到 `&mut Ctx`）；
/// - 顺序按路由策略（`Tunnel` 外→内 / `Bubble` 内→外 / `Direct` 只目标），
///   与用户处理器的顺序一致，因此"滑块在自己身上处理按压，父容器再收到冒泡"是自然结果。
pub fn handle_route(track: &mut Track, path: &[NodeId], ev: &Event, cmd: &mut CmdBuf) {
    match ev.kind().routing() {
        Routing::Tunnel => {
            for id in path {
                handle(track, *id, ev, cmd);
            }
        }
        Routing::Bubble => {
            for id in path.iter().rev() {
                handle(track, *id, ev, cmd);
            }
        }
        Routing::Direct => {
            if let Some(t) = path.last() {
                handle(track, *t, ev, cmd);
            }
        }
    }
}

/// 单个节点的内置行为（按 `Kind` 枚举分派）
///
/// 收 `&Event` 而不只是 `&EventView`：IME 预编辑/提交带字符串 payload，
/// 而 `EventView` 是 `Copy` 的定长摘要（放不下字符串）。
pub fn handle(track: &mut Track, id: NodeId, ev: &Event, cmd: &mut CmdBuf) {
    let Some(node) = track.get(id) else {
        return;
    };
    if !node.interaction.enabled {
        return;
    }
    // 滚动条是"虚拟部件"：不占节点，行为挂在滚动容器自身上（先于组件行为）
    if node.layout.overflow_scroll && node.layout.show_scrollbar {
        scroll_handle(track, id, ev, cmd);
        if track.get(id).is_none() {
            return; // 处理中被销毁的极端情况
        }
    }
    let Some(node) = track.get(id) else {
        return;
    };
    if !node.interaction.enabled {
        return;
    }
    match node.kind.tag() {
        KindTag::Slider => slider_handle(track, id, &ev.summary(), cmd),
        KindTag::Checkbox => checkbox_handle(track, id, &ev.summary()),
        KindTag::Switch => switch_handle(track, id, &ev.summary()),
        KindTag::Radio => radio_handle(track, id, &ev.summary()),
        KindTag::Input => input_handle(track, id, ev, cmd),
        // 自定义节点：转交它自己的状态机（不拿 Track；树操作走 cmd）
        KindTag::Custom => {
            if let Some(Kind::Custom(cell)) = track.get(id).map(|n| n.kind.clone())
                && let Some(mut inst) = cell.try_borrow_mut()
            {
                inst.on_event(id, &ev.summary(), cmd);
            }
        }
        // Box / Text / Image / Button / Progress 没有框架内置行为：
        // 点击由用户在 `view()` 里 `.on(EventKind::Tapped, ..)` 决定。
        _ => {}
    }
}

/// 滑块：按下取捕获 + 拖拽改值（值写回绑定 signal）
///
/// **只在有绑定时生效**（与复选框同一条规则）：`slider(0.5)` 是"显示这个值"，
/// `slider_bound(&sig)` 才是"能拖、并且改的是模型"。
/// 不这样规定的话，"拖动一个未绑定的滑块"会得到一个**下次 view() 才回弹**的中间态，
/// 它的可见性取决于"下一次 view() 何时发生"——不可预测，索性不允许。
fn slider_handle(track: &mut Track, id: NodeId, ev: &EventView, cmd: &mut CmdBuf) {
    let bound = track.get(id).map(|n| n.bindings.value.is_some()).unwrap_or(false);
    if !bound {
        return;
    }
    match ev.kind {
        EventKind::PointerPressed => {
            if ev.button != PointerButton::Left {
                return;
            }
            track.set_dragging(id, true);
            // 指针捕获：拖出滑块范围也继续跟随（否则拖快一点就丢）
            cmd.capture(ev.pointer, id);
            track.slider_drag_to(id, ev.pos.x);
        }
        EventKind::PointerMoved => {
            let dragging = matches!(
                track.get(id).map(|n| &n.kind),
                Some(Kind::Slider { dragging: true, .. })
            );
            if dragging {
                track.slider_drag_to(id, ev.pos.x);
            }
        }
        EventKind::PointerReleased | EventKind::PointerCaptureLost | EventKind::PointerCanceled => {
            track.set_dragging(id, false);
            cmd.release(ev.pointer);
        }
        _ => {}
    }
}

/// 复选框：点击翻转（**只在有绑定时**——≈ `v-model` 语义）
fn checkbox_handle(track: &mut Track, id: NodeId, ev: &EventView) {
    if ev.kind != EventKind::Tapped {
        return;
    }
    let bound = track.get(id).map(|n| n.bindings.checked.is_some()).unwrap_or(false);
    if !bound {
        return; // 未绑定 ⇒ 交给用户处理器（模型说了算）
    }
    track.toggle_checked(id);
}

/// 开关：点击翻转（只在有绑定时生效——≈ `v-model` 语义）
fn switch_handle(track: &mut Track, id: NodeId, ev: &EventView) {
    if ev.kind != EventKind::Tapped {
        return;
    }
    let bound = track.get(id).map(|n| n.bindings.checked.is_some()).unwrap_or(false);
    if !bound {
        return;
    }
    track.toggle_switch(id);
}

/// 单选：点击把本项 value 写回组 signal（只在有绑定时生效）
fn radio_handle(track: &mut Track, id: NodeId, ev: &EventView) {
    if ev.kind != EventKind::Tapped {
        return;
    }
    let bound = track.get(id).map(|n| n.bindings.text.is_some()).unwrap_or(false);
    if !bound {
        return;
    }
    track.select_radio(id);
}

// ───────────────────────── 输入框（M5）─────────────────────────

/// 输入框：聚焦 / 点选光标 / 拖选 / 键盘编辑 / IME。
///
/// **编辑只在有绑定时生效**（与滑块、复选框同一条规则）：没有绑定的输入框
/// 是"只显示模型给出的文本"，否则编辑结果会在下次 `view()` 时被描述值覆盖。
fn input_handle(track: &mut Track, id: NodeId, ev: &Event, cmd: &mut CmdBuf) {
    let bound = track.get(id).map(|n| n.bindings.text.is_some()).unwrap_or(false);
    let view = ev.summary();

    match view.kind {
        EventKind::PointerPressed => {
            if view.button != PointerButton::Left {
                return;
            }
            // 点进来即拿焦点（键盘编辑的前提）
            cmd.focus(id, FocusState::Pointer);
            cmd.capture(view.pointer, id);
            if bound {
                let caret = caret_at_x(track, id, view.pos.x);
                track.input_set_caret(id, caret, false);
            }
        }
        EventKind::PointerMoved => {
            if !bound || track.captured_by(view.pointer) != Some(id) {
                return;
            }
            // 按住拖动 ⇒ 扩选（anchor 不动）
            let caret = caret_at_x(track, id, view.pos.x);
            track.input_set_caret(id, caret, true);
        }
        EventKind::PointerReleased | EventKind::PointerCanceled => {
            cmd.release(view.pointer);
        }
        EventKind::LostFocus => {
            // 失焦结束 IME 组合（否则组合串会留在框里）
            track.input_set_preedit(id, String::new());
        }
        EventKind::KeyDown => {
            if bound {
                input_key(track, id, &view);
            }
        }
        EventKind::CharacterReceived => {
            // 字符只进"当前真正持有焦点"的输入框（避免多窗口/多框串字）
            if !bound || !track.input_is_active(id) {
                return;
            }
            let text = match ev {
                Event::Key { text: Some(t), .. } => t.clone(),
                _ => match view.key {
                    Some(KeyCode::Char(c)) => c.to_string(),
                    _ => return,
                },
            };
            track.input_insert(id, &text);
        }
        EventKind::TextCompositionChanged => {
            if !bound {
                return;
            }
            if let Event::ImePreedit { text, .. } = ev {
                track.input_set_preedit(id, text.clone());
            }
        }
        _ => {}
    }
}

// ───────────────────────── 滚动条（M5：thumb 绘制 + 拖动）─────────────────────────

/// 滚动条的粗细（绘制宽 / 命中带宽都用它）
pub const SCROLLBAR_WIDTH: f32 = 8.0;
/// thumb 与视口边缘的间距
const SCROLLBAR_INSET: f32 = 2.0;
/// thumb 的最小长度（再短的视口比例也可抓）
const MIN_THUMB_LEN: f32 = 24.0;
/// thumb 命中带比绘制更宽（触摸容差）
const SCROLLBAR_HIT_INFLATE: f32 = 6.0;

/// 竖直滚动条的 `(track, thumb)`；内容没有竖直溢出 ⇒ `None`
pub(crate) fn vscroll_parts(view: Rect, content: Size, offset: f32) -> Option<(Rect, Rect)> {
    scroll_parts(view, content.height, offset, view.height, true)
}

/// 水平滚动条的 `(track, thumb)`；内容没有水平溢出 ⇒ `None`
pub(crate) fn hscroll_parts(view: Rect, content: Size, offset: f32) -> Option<(Rect, Rect)> {
    scroll_parts(view, content.width, offset, view.width, false)
}

fn scroll_parts(view: Rect, content_len: f32, offset: f32, view_len: f32, vertical: bool) -> Option<(Rect, Rect)> {
    // `partial_cmp != Some(Greater)` 覆盖两种"不画"：没溢出（`Less`/`Equal`），
    // 以及**任一尺寸是 NaN**（`partial_cmp` 返回 `None`）—— NaN 尺寸顺着布局流到几何
    // 计算里只会画出一条 NaN 长的轨道，不如不画。
    if content_len.partial_cmp(&(view_len + 0.5)) != Some(std::cmp::Ordering::Greater) {
        return None; // 没溢出就没滚动条
    }
    let (track_len, base, across) = if vertical {
        (
            view.height - SCROLLBAR_INSET * 2.0,
            view.y + SCROLLBAR_INSET,
            view.right() - SCROLLBAR_WIDTH - SCROLLBAR_INSET,
        )
    } else {
        (
            view.width - SCROLLBAR_INSET * 2.0,
            view.x + SCROLLBAR_INSET,
            view.bottom() - SCROLLBAR_WIDTH - SCROLLBAR_INSET,
        )
    };
    // 轨道长度必须为正。**窗口最小化时客户区是 0×0，布局会把容器高度算成负数**
    // （实测 -26px），那样下面 `clamp(MIN_THUMB_LEN, track_len)` 的下界就大于上界 ——
    // `f32::clamp` 会 panic（`min > max`），这正是"导出 PNG 后最小化窗口就崩"的根因。
    let track_len = track_len.max(0.0);
    if track_len <= 0.0 {
        return None; // 没有可画的轨道
    }
    let max_scroll = (content_len - view_len).max(0.0);
    // 上下界必须有序：轨道比"最小 thumb"还短时（矮容器），把下界收敛到轨道长度 ——
    // 于是 thumb 铺满整条轨道（仍是可见的"可滚动"提示，且 `travel = 0` 拖不动但不会崩）。
    let thumb_len = (track_len * view_len / content_len).clamp(MIN_THUMB_LEN.min(track_len), track_len);
    let travel = (track_len - thumb_len).max(0.0);
    let pos = base
        + travel
            * if max_scroll > 0.0 {
                (offset / max_scroll).clamp(0.0, 1.0)
            } else {
                0.0
            };
    let track_rect = if vertical {
        Rect::new(across, base, SCROLLBAR_WIDTH, track_len)
    } else {
        Rect::new(base, across, track_len, SCROLLBAR_WIDTH)
    };
    let thumb = if vertical {
        Rect::new(across, pos, SCROLLBAR_WIDTH, thumb_len)
    } else {
        Rect::new(pos, across, thumb_len, SCROLLBAR_WIDTH)
    };
    Some((track_rect, thumb))
}

/// 滚动条 thumb 的拖拽行为（按下抓取 / 拖动映射 / 松手释放）
fn scroll_handle(track: &mut Track, id: NodeId, ev: &Event, cmd: &mut CmdBuf) {
    let view = ev.summary();
    // 一次性把需要的几何/状态拷出来（全是 Copy），避免与 get_mut 冲突
    let Some((rect, content, drag, (ox, oy))) = track
        .get(id)
        .map(|n| (n.rect(), n.content_size, n.scroll_drag, n.scroll_offset))
    else {
        return;
    };

    match view.kind {
        EventKind::PointerPressed => {
            if view.button != PointerButton::Left {
                return;
            }
            // 竖直优先（内容通常竖直溢出）；命中带比可见 thumb 更宽
            if let Some((track_r, thumb)) = vscroll_parts(rect, content, oy)
                && view.pos.x >= track_r.x - SCROLLBAR_HIT_INFLATE
                && view.pos.x <= track_r.right() + SCROLLBAR_HIT_INFLATE
                && view.pos.y >= thumb.y - SCROLLBAR_HIT_INFLATE
                && view.pos.y <= thumb.bottom() + SCROLLBAR_HIT_INFLATE
            {
                track.get_mut(id).unwrap().scroll_drag = Some(ScrollDrag {
                    pointer: view.pointer,
                    vertical: true,
                    grab: view.pos.y - thumb.y,
                });
                cmd.capture(view.pointer, id);
                cmd.damage(id);
                return;
            }
            if let Some((track_r, thumb)) = hscroll_parts(rect, content, ox)
                && view.pos.y >= track_r.y - SCROLLBAR_HIT_INFLATE
                && view.pos.y <= track_r.bottom() + SCROLLBAR_HIT_INFLATE
                && view.pos.x >= thumb.x - SCROLLBAR_HIT_INFLATE
                && view.pos.x <= thumb.right() + SCROLLBAR_HIT_INFLATE
            {
                track.get_mut(id).unwrap().scroll_drag = Some(ScrollDrag {
                    pointer: view.pointer,
                    vertical: false,
                    grab: view.pos.x - thumb.x,
                });
                cmd.capture(view.pointer, id);
                cmd.damage(id);
            }
        }
        EventKind::PointerMoved => {
            let Some(d) = drag else { return };
            if d.pointer != view.pointer || track.captured_by(view.pointer) != Some(id) {
                return;
            }
            if d.vertical {
                let Some((track_r, thumb)) = vscroll_parts(rect, content, oy) else {
                    return;
                };
                let max_scroll = (content.height - rect.height).max(0.0);
                let travel = (track_r.height - thumb.height).max(0.0);
                let pos = (view.pos.y - d.grab - track_r.y).clamp(0.0, travel);
                let ny = if travel > 0.0 { pos / travel * max_scroll } else { 0.0 };
                track.set_scroll_offset(id, (ox, ny));
            } else {
                let Some((track_r, thumb)) = hscroll_parts(rect, content, ox) else {
                    return;
                };
                let max_scroll = (content.width - rect.width).max(0.0);
                let travel = (track_r.width - thumb.width).max(0.0);
                let pos = (view.pos.x - d.grab - track_r.x).clamp(0.0, travel);
                let nx = if travel > 0.0 { pos / travel * max_scroll } else { 0.0 };
                track.set_scroll_offset(id, (nx, oy));
            }
        }
        EventKind::PointerReleased | EventKind::PointerCanceled => {
            if let Some(d) = drag
                && d.pointer == view.pointer
            {
                track.get_mut(id).unwrap().scroll_drag = None;
                cmd.release(view.pointer);
                cmd.damage(id);
            }
        }
        _ => {}
    }
}

/// 画滚动条（滚动容器专用；不溢出的轴不画）——由 [`draw_scrollbar_overlay`] 调用
fn draw_scrollbars(out: &mut Scene, cull: &Cull, n: &Node, rect: Rect, transform: Affine, theme: &crate::theme::Theme) {
    if !n.layout.show_scrollbar {
        return;
    }
    let dragging = n.scroll_drag.is_some();
    let color = if dragging {
        theme.scrollbar_thumb_drag
    } else {
        theme.scrollbar_thumb
    };
    if let Some((_, thumb)) = vscroll_parts(rect, n.content_size, n.scroll_offset.1) {
        push(
            out,
            cull,
            transform,
            thumb,
            Op::Rect {
                rect: thumb,
                radius: SCROLLBAR_WIDTH * 0.5,
                color,
                transform,
            },
        );
    }
    if let Some((_, thumb)) = hscroll_parts(rect, n.content_size, n.scroll_offset.0) {
        push(
            out,
            cull,
            transform,
            thumb,
            Op::Rect {
                rect: thumb,
                radius: SCROLLBAR_WIDTH * 0.5,
                color,
                transform,
            },
        );
    }
}

/// 键盘编辑（方向键 / 退格删除 / Home End / Ctrl+A）
fn input_key(track: &mut Track, id: NodeId, view: &EventView) {
    let Some(code) = view.key else {
        return;
    };
    let extend = view.modifiers.shift();
    match code {
        KeyCode::Named(NamedKey::Backspace) => {
            track.input_backspace(id);
        }
        KeyCode::Named(NamedKey::Delete) => {
            track.input_delete(id);
        }
        KeyCode::Named(NamedKey::Left) => {
            track.input_move_caret(id, -1, extend);
        }
        KeyCode::Named(NamedKey::Right) => {
            track.input_move_caret(id, 1, extend);
        }
        KeyCode::Named(NamedKey::Home) => {
            track.input_set_caret(id, 0, extend);
        }
        KeyCode::Named(NamedKey::End) => {
            track.input_set_caret(id, usize::MAX, extend);
        }
        KeyCode::Char('a') | KeyCode::Char('A') if view.modifiers.ctrl() => {
            track.input_select_all(id);
        }
        _ => {}
    }
}

/// 显示串 = `text` 在光标处插入 IME 组合串
fn display_text(text: &str, preedit: &str, caret: usize) -> String {
    if preedit.is_empty() {
        return text.to_string();
    }
    let caret = clamp_to_char_boundary(text, caret);
    format!("{}{}{}", &text[..caret], preedit, &text[caret..])
}

/// 显示串偏移 → 文本偏移（组合串内部的点击夹到光标位）
fn display_to_text(text: &str, preedit: &str, caret: usize, offset: usize) -> usize {
    if preedit.is_empty() {
        return clamp_to_char_boundary(text, offset);
    }
    let caret = clamp_to_char_boundary(text, caret);
    if offset <= caret {
        offset
    } else if offset <= caret + preedit.len() {
        caret
    } else {
        clamp_to_char_boundary(text, offset - preedit.len())
    }
}

/// 点击位置 → 文本偏移（按前缀实测宽度取最近）
fn caret_at_x(track: &Track, id: NodeId, x: f32) -> usize {
    let Some(n) = track.get(id) else {
        return 0;
    };
    let Kind::Input {
        text,
        preedit,
        caret,
        scroll,
        ..
    } = &n.kind
    else {
        return 0;
    };
    let display = display_text(text, preedit, *caret);
    // 补偿水平滚动：点击位置对应"内容坐标" = 屏幕 x - 左内边距 + scroll
    let local = (x - n.rect().x - pad_left(n) + *scroll).max(0.0) as f64;
    let spec = &n.text.spec;

    let mut best = 0usize;
    let mut best_d = f64::MAX;
    let mut probe = |offset: usize| {
        let w = TextEngine::measure_text(&display[..offset], spec).0;
        let d = (w - local).abs();
        if d < best_d {
            best_d = d;
            best = offset;
        }
    };
    for (i, _) in display.char_indices() {
        probe(i);
    }
    probe(display.len());
    display_to_text(text, preedit, *caret, best)
}

/// 节点左内边距（输入框文本/光标的起点）
fn pad_left(n: &Node) -> f32 {
    n.layout.padding[lieui_layout::CSSDirection::Left as usize]
}

/// 节点上内边距
fn pad_top(n: &Node) -> f32 {
    n.layout.padding[lieui_layout::CSSDirection::Top as usize]
}

/// 内容盒：`rect` 去掉四边内边距。
///
/// 文本/墨迹应该落在**内容盒**里而不是 border box —— 否则"文本 + 对称内边距"的盒子
/// （tooltip 就是典型）padding 会被吃掉，视觉上看着偏。
fn content_rect(n: &Node, rect: Rect) -> Rect {
    use lieui_layout::CSSDirection::{Bottom, Left, Right, Top};
    let pad = |d: lieui_layout::CSSDirection| n.layout.padding[d as usize];
    let (l, t, r, b) = (pad(Left), pad(Top), pad(Right), pad(Bottom));
    Rect::new(
        rect.x + l,
        rect.y + t,
        (rect.width - l - r).max(0.0),
        (rect.height - t - b).max(0.0),
    )
}

/// 归一化选区（无选区 ⇒ `None`）
fn selection_of(caret: usize, anchor: usize) -> Option<(usize, usize)> {
    if caret == anchor {
        None
    } else {
        Some((caret.min(anchor), caret.max(anchor)))
    }
}

/// 显示串前缀的实测宽度（字节偏移 → 像素）；走 `lieui-text` 的测度缓存
fn measure(display: &str, spec: &TextSpec, offset: usize) -> f32 {
    let offset = clamp_to_char_boundary(display, offset);
    TextEngine::measure_text(&display[..offset], spec).0 as f32
}

/// 输入框正文（左对齐、给定原点），复用文本排版缓存
#[allow(clippy::too_many_arguments)]
fn push_input_text(
    cache: &mut TextCache,
    out: &mut Scene,
    cull: &Cull,
    text: &str,
    spec: &TextSpec,
    color: Color,
    origin: Point,
    transform: Affine,
) {
    let (layout, hit) = cache.get_or_build_counted(text, spec, color);
    if hit {
        out.stats.text_cache_hits += 1;
    } else {
        out.stats.text_layouts_built += 1;
    }
    let op = Op::Text {
        layout,
        origin,
        color,
        transform,
    };
    let bounds = op.screen_bounds().unwrap_or(Rect::new(origin.x, origin.y, 1.0, 1.0));
    if cull.hit_visible(&bounds) {
        out.push(op);
    } else {
        out.stats.prims_culled += 1;
    }
}

/// 节点自身内容进绘制列表
pub(crate) fn draw(
    cache: &mut TextCache,
    track: &Track,
    id: NodeId,
    transform: Affine,
    cull: &Cull,
    out: &mut Scene,
    theme: &crate::theme::Theme,
) {
    let Some(n) = track.get(id) else {
        return;
    };
    let rect = n.rect();
    if rect.is_empty() {
        return;
    }

    // ── ① 投影（kinds 之前，避免盖住自身内容）──
    if let Some(sh) = n.paint.shadow {
        let r = Rect::new(rect.x + sh.offset_x, rect.y + sh.offset_y, rect.width, rect.height).inflate(sh.spread);
        push(
            out,
            cull,
            transform,
            r,
            Op::Shadow {
                rect: r,
                radius: n.paint.border_radius,
                std_dev: (sh.blur * 0.5).max(0.0),
                color: dim_if_disabled(sh.color, n),
                transform,
            },
        );
    }

    // ── ② 背景（hover / pressed 配色在这里解析）──
    let radius = n.paint.border_radius.min(rect.width * 0.5).min(rect.height * 0.5);
    if let Some(color) = resolve_background(n) {
        push(
            out,
            cull,
            transform,
            rect,
            Op::Rect {
                rect,
                radius,
                color: dim_if_disabled(color, n),
                transform,
            },
        );
    }

    // ── ③ 边框 ──
    if n.paint.border_width > 0.0
        && let Some(color) = n.paint.border_color
    {
        push(
            out,
            cull,
            transform,
            rect,
            Op::Border {
                rect,
                radius,
                width: n.paint.border_width,
                color: dim_if_disabled(color, n),
                transform,
            },
        );
    }

    // ── ④ 组件自身内容 ──
    let text_color = dim_if_disabled(resolve_text_color(n), n);
    match &n.kind {
        // 纯容器；Image 的绘制留到 M5（需要 ImageFit / 圆角裁剪的像素级 blit）
        // 纯容器
        Kind::Box => {}

        // 图片：等比缩放（contain）放进节点 rect；手动 blit（见 raster）
        Kind::Image(img) => {
            let bounds = transform.bounding_box(rect);
            if cull.hit_visible(&bounds) {
                out.push(Op::Image {
                    image: img.clone(),
                    rect,
                    transform,
                });
            } else {
                out.stats.prims_culled += 1;
            }
        }

        // 自定义节点：整块交给用户（只读；坐标已是窗口坐标系）
        Kind::Custom(cell) => {
            let inst = cell.borrow();
            inst.draw(out, rect, transform);
        }

        Kind::Text(_) => {
            if let Kind::Text(s) = &n.kind {
                let spec = draw_spec(n);
                // 光学对齐时按**内容盒**放墨迹：节点高度 = 墨迹高 + padding，
                // 若仍贴 border box 上缘，padding 会被吃掉（文本看着往上顶）。
                // 非光学路径保持原样（行盒自带 leading，既有布局按 border box 定位）。
                let r = if spec.optical_align {
                    content_rect(n, rect)
                } else {
                    rect
                };
                push_text(cache, out, cull, s, &spec, text_color, r, false, transform);
            }
        }

        Kind::Button { label } => {
            let spec = draw_spec(n);
            push_text(cache, out, cull, label, &spec, text_color, rect, true, transform);
        }

        // 输入框：文本（或 placeholder）+ 选区 + 光标 + IME 组合下划线
        Kind::Input {
            text,
            placeholder,
            caret,
            anchor,
            preedit,
            scroll,
        } => {
            // 水平滚动：内容随偏移左移，超出部分由节点的 clip_content 裁掉
            let x0 = rect.x + pad_left(n) - scroll;
            let top = rect.y + pad_top(n);
            let display = display_text(text, preedit, *caret);
            let empty = text.is_empty() && preedit.is_empty();

            // 选区（失焦时不显示，与实际产品一致）
            if let Some((a, b)) = selection_of(*caret, *anchor)
                && n.interaction.focused
            {
                let sx = x0 + measure(&display, &n.text.spec, a);
                let ex = x0 + measure(&display, &n.text.spec, b);
                let line_h = TextEngine::measure_text("x", &n.text.spec).1 as f32;
                let sel = Rect::new(sx, top, (ex - sx).max(1.0), line_h.max(rect.height - pad_top(n) * 2.0));
                push(
                    out,
                    cull,
                    transform,
                    sel,
                    Op::Rect {
                        rect: sel,
                        radius: 2.0,
                        color: theme.selection,
                        transform,
                    },
                );
            }

            // 正文 / 占位提示
            let (shown, color) = if empty {
                (placeholder.as_str(), theme.text_secondary)
            } else {
                (display.as_str(), text_color)
            };
            if !shown.is_empty() {
                let (_, th) = TextEngine::measure_text("x", &n.text.spec);
                let origin = Point::new(x0, rect.y + (rect.height - th as f32) * 0.5);
                push_input_text(cache, out, cull, shown, &n.text.spec, color, origin, transform);
            }

            // IME 组合串下划线
            if !preedit.is_empty() {
                let px = x0 + measure(&display, &n.text.spec, *caret);
                let pw = TextEngine::measure_text(preedit, &n.text.spec).0 as f32;
                let (_, th) = TextEngine::measure_text("x", &n.text.spec);
                let y = rect.y + (rect.height + th as f32) * 0.5;
                let underline = Rect::new(px, y, pw.max(1.0), 1.0);
                push(
                    out,
                    cull,
                    transform,
                    underline,
                    Op::Rect {
                        rect: underline,
                        radius: 0.0,
                        color: text_color,
                        transform,
                    },
                );
            }

            // 光标（键盘焦点 + 闪烁相位才画；相位由帧驱动翻转——聚焦才动，空闲帧零功耗）
            if n.interaction.focused && n.interaction.enabled && track.blink_on {
                let cx = x0 + measure(&display, &n.text.spec, *caret);
                let caret_rect = Rect::new(cx, rect.y + pad_top(n), 1.5, (rect.height - pad_top(n) * 2.0).max(4.0));
                push(
                    out,
                    cull,
                    transform,
                    caret_rect,
                    Op::Rect {
                        rect: caret_rect,
                        radius: 0.0,
                        color: theme.caret,
                        transform,
                    },
                );
            }
        }

        Kind::Checkbox { checked } => {
            let box_size = rect.height.min(18.0);
            let b = Rect::new(rect.x, rect.y + (rect.height - box_size) * 0.5, box_size, box_size);
            let border = n.paint.border_color.unwrap_or(text_color);
            push(
                out,
                cull,
                transform,
                b,
                Op::Border {
                    rect: b,
                    radius: 3.0,
                    width: 1.5,
                    color: border,
                    transform,
                },
            );
            if *checked {
                let inner = b.inflate(-4.0);
                push(
                    out,
                    cull,
                    transform,
                    inner,
                    Op::Rect {
                        rect: inner,
                        radius: 1.5,
                        color: border,
                        transform,
                    },
                );
            }
        }

        Kind::Slider { value, min, max, .. } => {
            let track_h = 4.0;
            let track = Rect::new(rect.x, rect.y + (rect.height - track_h) * 0.5, rect.width, track_h);
            let accent = n.paint.background_color.unwrap_or(theme.accent);
            let knob = 12.0;
            // 归一化到 min..max（0..1 范围外也能画对；max <= min 时退化到 0）
            let t = if *max > *min {
                ((value - *min) / (*max - *min)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let x = rect.x + (rect.width - knob).max(0.0) * t;
            push(
                out,
                cull,
                transform,
                track,
                Op::Rect {
                    rect: track,
                    radius: track_h * 0.5,
                    color: theme.control_border,
                    transform,
                },
            );
            let filled = Rect::new(track.x, track.y, (x + knob * 0.5) - track.x, track_h);
            push(
                out,
                cull,
                transform,
                filled,
                Op::Rect {
                    rect: filled,
                    radius: track_h * 0.5,
                    color: accent,
                    transform,
                },
            );
            let k = Rect::new(x, rect.y + (rect.height - knob) * 0.5, knob, knob);
            push(
                out,
                cull,
                transform,
                k,
                Op::Rect {
                    rect: k,
                    radius: knob * 0.5,
                    color: accent,
                    transform,
                },
            );
        }

        // 开关：药丸轨道 + 圆 thumb（on = accent，off = 边框色）
        Kind::Switch { on } => {
            let track_w = 40.0_f32.min(rect.width);
            let track_h = 20.0_f32.min(rect.height);
            let t = Rect::new(rect.x, rect.y + (rect.height - track_h) * 0.5, track_w, track_h);
            let accent = n.paint.background_color.unwrap_or(theme.accent);
            let (fill, thumb_c) = if *on {
                (accent, Color::WHITE)
            } else {
                (theme.control_border, theme.control)
            };
            push(
                out,
                cull,
                transform,
                t,
                Op::Rect {
                    rect: t,
                    radius: track_h * 0.5,
                    color: fill,
                    transform,
                },
            );
            let r = track_h - 4.0;
            let tx = if *on { t.right() - r - 2.0 } else { t.x + 2.0 };
            let thumb = Rect::new(tx, t.y + 2.0, r, r);
            push(
                out,
                cull,
                transform,
                thumb,
                Op::Rect {
                    rect: thumb,
                    radius: r * 0.5,
                    color: thumb_c,
                    transform,
                },
            );
        }

        // 单选：圆环 + 选中圆点（点 = accent）
        Kind::Radio { selected, .. } => {
            let d = rect.height.min(18.0);
            let b = Rect::new(rect.x, rect.y + (rect.height - d) * 0.5, d, d);
            let border = n.paint.border_color.unwrap_or(theme.control_border);
            let accent = n.paint.background_color.unwrap_or(theme.accent);
            push(
                out,
                cull,
                transform,
                b,
                Op::Border {
                    rect: b,
                    radius: d * 0.5,
                    width: 1.5,
                    color: if *selected { accent } else { border },
                    transform,
                },
            );
            if *selected {
                let inner = b.inflate(-4.0);
                push(
                    out,
                    cull,
                    transform,
                    inner,
                    Op::Rect {
                        rect: inner,
                        radius: inner.width * 0.5,
                        color: accent,
                        transform,
                    },
                );
            }
        }

        Kind::Progress { value } => {
            let filled = Rect::new(rect.x, rect.y, rect.width * value.clamp(0.0, 1.0), rect.height);
            let accent = n.paint.background_color.unwrap_or(theme.accent);
            push(
                out,
                cull,
                transform,
                filled,
                Op::Rect {
                    rect: filled,
                    radius,
                    color: accent,
                    transform,
                },
            );
        }
    }

    // ── ⑤ 滚动条：**不在这里画** ──
    // 它是覆盖层（虚拟部件），必须画在**子项之后**，否则会被列表项/内容子节点盖住。
    // 由 `scene::walk` 在走完 children 之后调用 [`draw_scrollbar_overlay`]。
}

/// 滚动条覆盖层（`scene::walk` 在**子项之后**调用；仍在滚动容器的裁剪内）。
///
/// 为什么单独成一层：滚动条属于滚动容器自身，但视觉上必须在内容之上
/// （旧实现画在容器自己的 draw 里 ⇒ 后画的列表项会把它盖住）。
pub(crate) fn draw_scrollbar_overlay(
    out: &mut Scene,
    cull: &Cull,
    track: &crate::track::Track,
    id: crate::track::NodeId,
    transform: Affine,
    theme: &crate::theme::Theme,
) {
    let Some(n) = track.get(id) else {
        return;
    };
    if !(n.layout.overflow_scroll && n.layout.show_scrollbar) {
        return;
    }
    let rect = n.rect();
    draw_scrollbars(out, cull, n, rect, transform, theme);
}

/// 背景色解析：pressed > hover > 常态
pub fn resolve_background(n: &Node) -> Option<Color> {
    if n.interaction.pressed
        && let Some(c) = n.paint.pressed_background
    {
        return Some(c);
    }
    if n.interaction.pointer_over
        && let Some(c) = n.paint.hover_background
    {
        return Some(c);
    }
    n.paint.background_color
}

/// 文本色解析：pressed > hover > 常态
pub fn resolve_text_color(n: &Node) -> Color {
    if n.interaction.pressed
        && let Some(c) = n.text.pressed_color
    {
        return c;
    }
    if n.interaction.pointer_over
        && let Some(c) = n.text.hover_color
    {
        return c;
    }
    n.text.color
}

/// 绘制用的排版规格：补上**测度时用的换行宽度**。
///
/// 布局引擎按约束宽度测度文本（会换行），绘制若不施加同一约束就会"盒子按两行算、
/// 只画一行"⇒ 文本贴在盒子顶部（看起来顶对齐，且与相邻项无法居中对齐）。
/// 用引擎写回的 `n.text_wrap` 复用同一宽度，测度与绘制逐字一致。
fn draw_spec(n: &Node) -> TextSpec {
    let mut spec = n.text.spec.clone();
    if spec.wrap
        && spec.max_width.is_none()
        && let Some(w) = n.text_wrap
    {
        spec.max_width = Some(f64::from(w));
    }
    spec
}

/// 禁用态：半透明（视觉降级；行为上的禁用由 `hit`/内置行为负责）
fn dim_if_disabled(c: Color, n: &Node) -> Color {
    if n.interaction.enabled {
        c
    } else {
        Color::rgba(c.r, c.g, c.b, c.a / 2)
    }
}

fn push(out: &mut Scene, cull: &Cull, _transform: Affine, bounds: Rect, op: Op) {
    let screen = op.screen_bounds().unwrap_or(bounds);
    if cull.hit_visible(&screen) {
        out.push(op);
    } else {
        out.stats.prims_culled += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn push_text(
    cache: &mut TextCache,
    out: &mut Scene,
    cull: &Cull,
    text: &str,
    spec: &TextSpec,
    color: Color,
    rect: Rect,
    center: bool,
    transform: Affine,
) {
    if text.is_empty() || rect.is_empty() {
        return;
    }
    let (layout, ink, hit) = cache.get_full(text, spec, color);
    if hit {
        out.stats.text_cache_hits += 1;
    } else {
        out.stats.text_layouts_built += 1;
    }

    // 光学对齐（`spec.optical_align`）：按**墨迹**而不是行盒定位。
    // - 节点矩形的高度由测度给出（= 墨迹高）⇒ 非居中场景把墨迹上缘对到矩形上缘；
    // - 居中场景（按钮等）把墨迹盒居中到矩形里 —— 这才是视觉居中。
    let ink = ink.filter(|b| !b.is_empty());
    let origin = match (ink, center) {
        (Some(b), false) => Point::new(rect.x, rect.y - b.top),
        (Some(b), true) => Point::new(
            rect.x + (rect.width - layout.width()) * 0.5,
            rect.y + (rect.height - b.height()) * 0.5 - b.top,
        ),
        (None, true) => Point::new(
            rect.x + (rect.width - layout.width()) * 0.5,
            rect.y + (rect.height - layout.height()) * 0.5,
        ),
        (None, false) => Point::new(rect.x, rect.y),
    };

    let op = Op::Text {
        layout,
        origin,
        color,
        transform,
    };
    let bounds = op.screen_bounds().unwrap_or(rect);
    if cull.hit_visible(&bounds) {
        out.push(op);
    } else {
        out.stats.prims_culled += 1;
    }
}

#[cfg(test)]
#[path = "widgets_tests.rs"]
mod tests;
