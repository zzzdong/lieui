//! 指针输入状态机：把"低层输入"翻译成"框架状态更新 + 待分发的路由事件"。
//!
//! 归属：这一层**不属于 winit**（winit 只做字段搬运，见 M4），所以它可以无窗口单测。
//! 它负责旧实现里散落的三件事：
//! 1. **hover 传播**：命中链变化时把旧链 ∪ 新链的 `pointer_over` 更新并标脏
//!    （取代 `set_hovered_state` + `hovered_listeners` 路径集合）；
//! 2. **点击合成**：`Tapped` = 按下目标仍在释放链上（含自身）——取代旧实现的 `Click` 特例；
//! 3. **捕获路由**：指针被捕获时事件发给捕获节点的祖先链（滑块拖拽不被中途换目标打断）。
//!
//! **不代表框架默认行为的地方**：滚轮滚动由调用方在"事件未被处理"后调用
//! [`default_wheel_scroll`]（保持"默认行为 vs 路由"的边界清晰）。

use lieui_geom::Point;

use crate::event::{Event, EventKind, PointerButton, PointerId};
use crate::focus;
use crate::hit;
use crate::track::{FocusState, Layer, NodeId, PressState, Track};

/// 低层输入
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    Move {
        pointer: PointerId,
        pos: Point,
    },
    Down {
        pointer: PointerId,
        pos: Point,
        button: PointerButton,
    },
    Up {
        pointer: PointerId,
        pos: Point,
        button: PointerButton,
    },
    Wheel {
        pointer: PointerId,
        pos: Point,
        delta: (f32, f32),
    },
    /// 指针被系统取消（窗口失焦 / 触控被接管）
    Cancel {
        pointer: PointerId,
    },
    /// 指针离开窗口
    Leave,
}

impl InputEvent {
    pub fn pointer(&self) -> PointerId {
        match self {
            InputEvent::Move { pointer, .. }
            | InputEvent::Down { pointer, .. }
            | InputEvent::Up { pointer, .. }
            | InputEvent::Wheel { pointer, .. }
            | InputEvent::Cancel { pointer } => *pointer,
            InputEvent::Leave => PointerId(0),
        }
    }

    pub fn pos(&self) -> Option<Point> {
        match self {
            InputEvent::Move { pos, .. }
            | InputEvent::Down { pos, .. }
            | InputEvent::Up { pos, .. }
            | InputEvent::Wheel { pos, .. } => Some(*pos),
            _ => None,
        }
    }
}

/// 一步输入处理的结果
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputStep {
    /// 待分发的事件：`(命中链, 事件)`，按顺序分发
    pub events: Vec<(Vec<NodeId>, Event)>,
    /// 合成的点击目标（测试/调试观测点）
    pub tapped: Option<NodeId>,
    /// 本次是否刷新了 hover 链
    pub hover_changed: bool,
}

/// 处理一步输入：**只改框架状态 + 产出待分发事件**，不调用任何用户闭包
pub fn step(track: &mut Track, ev: InputEvent) -> InputStep {
    let mut out = InputStep::default();

    match ev {
        InputEvent::Move { pointer, pos } => {
            out.hover_changed = update_hover(track, pos, pointer, &mut out.events);
            push(
                &mut out,
                pointer,
                pos,
                EventKind::PointerMoved,
                PointerButton::Left,
                track,
            );
        }

        InputEvent::Down { pointer, pos, button } => {
            out.hover_changed = update_hover(track, pos, pointer, &mut out.events);
            let path = hit::hit_path_for(track, pointer, pos);

            // 二次按下先清掉上一条按下链（D10）：否则 `pressed_path` 被直接覆盖，
            // 旧链上的节点**永久残留 pressed 视觉**（后续的Up / Cancel 只清最新那条链）。
            clear_pressed(track);

            if let Some(target) = path.last().copied() {
                track.pressed = Some(PressState {
                    node: target,
                    pointer,
                    button,
                    pos,
                });
                track.pressed_path = path.clone();
                track.set_pressed(target, true);

                // 指针按下取焦点（≈ WinUI：点击让最近的可聚焦祖先获得焦点，来源 = Pointer）
                if let Some(f) = focus::focusable_ancestor(track, &path) {
                    let change = focus::set_focus(track, Some(f), FocusState::Pointer);
                    if let Some(lost) = change.lost {
                        out.events
                            .push((hit::path_to(track, lost), Event::simple(EventKind::LostFocus)));
                    }
                    if let Some(got) = change.got {
                        out.events
                            .push((hit::path_to(track, got), Event::simple(EventKind::GotFocus)));
                    }
                }
            }
            if !path.is_empty() {
                out.events
                    .push((path, Event::pointer(EventKind::PointerPressed, pointer, pos, button)));
            }
        }

        InputEvent::Up { pointer, pos, button } => {
            let path = hit::hit_path_for(track, pointer, pos);
            if !path.is_empty() {
                out.events.push((
                    path.clone(),
                    Event::pointer(EventKind::PointerReleased, pointer, pos, button),
                ));
            }

            // 点击合成：按下时的目标仍在释放链上（含自身）**且抬起的是同一按键** ⇒ 点击事件发给它。
            //
            // **按键配对是必需的**（D1）：只判"按下节点是否在释放链上"的话，
            // **右键按下 + 左键抬起落在同一节点会被判成 `Tapped`** ⇒ 触发勾选 / 提交 / 删除。
            //
            // **右键合成 `RightTapped`**（不是 `Tapped`）：配对通过后，右键按下→右键抬起
            // 得到 `RightTapped`，不会顺带触发所有左键行为。需要上下文菜单的节点监听
            // `RightTapped`，其余组件完全不受右键影响。
            if let Some(press) = track.pressed
                && press.button == button
                && path.contains(&press.node)
            {
                out.tapped = Some(press.node);
                let kind = if button == PointerButton::Right {
                    EventKind::RightTapped
                } else {
                    EventKind::Tapped
                };
                out.events.push((
                    hit::path_to(track, press.node),
                    Event::pointer(kind, pointer, pos, button),
                ));
            }

            // 轻关闭（light dismiss，≈ WinUI `IsLightDismissEnabled`）：
            // 点击落在"可关闭弹层"子树之外 ⇒ 给该层根发 `Dismissed`（Direct）。
            // 层是声明式的，框架不能替用户删层——由层根的 Dismissed 处理器翻自己的状态。
            // **排在 Tapped 之后**：锚点按钮的 toggle（关）与 dismiss（关）幂等一致，
            // 不会出现"先关又被锚点重新打开"的次序问题。
            out.events.extend(dismiss_outside_popups(track, &path));

            // 清按下态（整条按下链都清，避免捕获导致漏清）
            clear_pressed(track);

            out.hover_changed |= update_hover(track, pos, pointer, &mut out.events);
        }

        InputEvent::Wheel { pointer, pos, delta } => {
            // ★ 走 `hit_path_for`（D61）：此前用 `hit_path` + `let _ = pointer;`
            //   **绕过了指针捕获** ⇒ 拖拽过程中（某节点已 `capture_pointer`）滚轮事件
            //   会按命中链重新路由，而不是送给捕获者 —— 与其它指针事件语义不一致，
            //   表现为"拖拽时滚轮 scrolls 别的容器 / 拖拽对滚轮无反应"。
            let path = hit::hit_path_for(track, pointer, pos);
            if !path.is_empty() {
                out.events.push((path, Event::wheel(pos, delta)));
            }
        }

        InputEvent::Cancel { pointer } => {
            // 捕获丢失必发（现状缺口：widget 无法可靠清理拖拽态）
            if let Some(captured) = track.captured_by(pointer) {
                track.release_pointer(pointer);
                track.set_pressed(captured, false);
                out.events.push((
                    hit::path_to(track, captured),
                    Event::pointer(
                        EventKind::PointerCaptureLost,
                        pointer,
                        Point::zero(),
                        PointerButton::Left,
                    ),
                ));
            }
            let hover_path = std::mem::take(&mut track.hover_path);
            track.hover = None;
            for id in hover_path.iter() {
                track.set_pointer_over(*id, false);
            }
            if let Some(target) = hover_path.last().copied() {
                out.events.push((
                    hit::path_to(track, target),
                    Event::pointer(EventKind::PointerCanceled, pointer, Point::zero(), PointerButton::Left),
                ));
            }
            clear_pressed(track);
        }

        InputEvent::Leave => {
            out.hover_changed = update_hover_path(track, &[], PointerId(0), Point::zero(), &mut out.events);
        }
    }

    out
}

/// 清掉整条按下链的 `pressed` 视图态 + 窗口级按下态。
///
/// **必须清整条链**，不只是按下目标：`pressed_path` 上的每个节点都被标了 pressed
/// （用于父容器联动高亮），只清目标会让祖先的按下态残留。
///
/// `Down` / `Up` / `Cancel` 三处共用，避免"清法不一致"导致按���态泄漏（D10）。
fn clear_pressed(track: &mut Track) {
    let pressed_path = std::mem::take(&mut track.pressed_path);
    for id in pressed_path {
        track.set_pressed(id, false);
    }
    track.pressed = None;
}

fn push(out: &mut InputStep, pointer: PointerId, pos: Point, kind: EventKind, button: PointerButton, track: &Track) {
    let path = hit::hit_path_for(track, pointer, pos);
    if !path.is_empty() {
        out.events.push((path, Event::pointer(kind, pointer, pos, button)));
    }
}

/// 轻关闭：对每个开启了 `dismiss_on_outside_click` 的弹层根（Popup / Tooltip），
/// 若本次点击不在其子树内，产出一条 `(层根路径, Dismissed)`（Direct 路由，只发层根）。
fn dismiss_outside_popups(track: &Track, tap_path: &[NodeId]) -> Vec<(Vec<NodeId>, Event)> {
    let mut out = Vec::new();
    for root in track.roots() {
        if root.layer != Layer::Popup && root.layer != Layer::Tooltip {
            continue;
        }
        if !root.opts.dismiss_on_outside_click {
            continue;
        }
        // 点击在弹层子树内（路径含层根）⇒ 不关闭
        if tap_path.contains(&root.node) {
            continue;
        }
        out.push((hit::path_to(track, root.node), Event::simple(EventKind::Dismissed)));
    }
    out
}

/// 按真实位置刷新 hover 链
fn update_hover(track: &mut Track, pos: Point, pointer: PointerId, events: &mut Vec<(Vec<NodeId>, Event)>) -> bool {
    let new_path = hit::hit_path(track, pos);
    update_hover_path(track, &new_path, pointer, pos, events)
}

/// 把 hover 链更新到 `new_path`，产出 Entered / Exited 事件（外→内进入，内→外离开）
fn update_hover_path(
    track: &mut Track,
    new_path: &[NodeId],
    pointer: PointerId,
    pos: Point,
    events: &mut Vec<(Vec<NodeId>, Event)>,
) -> bool {
    if track.hover_path == new_path {
        return false;
    }
    let old_path = std::mem::take(&mut track.hover_path);

    // 离开：自内向外
    for id in old_path.iter().rev() {
        if !new_path.contains(id) {
            track.set_pointer_over(*id, false);
            events.push((
                vec![*id],
                Event::pointer(EventKind::PointerExited, pointer, pos, PointerButton::Left),
            ));
        }
    }
    // 进入：自外向内
    for id in new_path {
        if !old_path.contains(id) {
            track.set_pointer_over(*id, true);
            events.push((
                vec![*id],
                Event::pointer(EventKind::PointerEntered, pointer, pos, PointerButton::Left),
            ));
        }
    }

    track.hover_path = new_path.to_vec();
    track.hover = new_path.last().copied();
    true
}

/// 滚轮的**框架默认行为**：沿命中链找最近的（可继续滚动的）滚动容器。
///
/// `delta` 约定与 winit 一致：**正 y = 滚轮向上推 / 触控板向**上**滑**（Windows
/// `WM_MOUSEWHEEL` 正值；macOS 的"自然滚动"方向由系统换算后同样落在此约定）。
/// 向上 ⇒ 视图向内容**开头**走 ⇒ **offset 减小**，所以这里做的是 `offset -= delta`。
///
/// 返回是否真的滚了；到边界时继续向上找（嵌套滚动链），找不到返回 `false`
/// ——调用方（`WindowCtx::pointer`）只在"事件未被处理"时调用它。
pub fn default_wheel_scroll(track: &mut Track, path: &[NodeId], delta: (f32, f32)) -> bool {
    for id in path.iter().rev().copied() {
        let Some(n) = track.get(id) else { continue };
        if !n.layout.overflow_scroll {
            continue;
        }
        let view = n.rect();
        let content = n.content_size;
        let max_x = (content.width - view.width).max(0.0);
        let max_y = (content.height - view.height).max(0.0);
        if max_x <= 0.0 && max_y <= 0.0 {
            continue;
        }
        let (ox, oy) = track.scroll_offset(id);
        let nx = (ox - delta.0).clamp(0.0, max_x);
        let ny = (oy - delta.1).clamp(0.0, max_y);
        if (nx - ox).abs() > 1e-4 || (ny - oy).abs() > 1e-4 {
            track.set_scroll_offset(id, (nx, ny));
            return true;
        }
        // 已到边界：交给上层容器（滚动链）
    }
    false
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
