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
use crate::track::{FocusState, Layer, NodeId, Track};

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
            push(&mut out, pointer, pos, EventKind::PointerMoved, PointerButton::Left, track);
        }

        InputEvent::Down {
            pointer,
            pos,
            button,
        } => {
            out.hover_changed = update_hover(track, pos, pointer, &mut out.events);
            let path = hit::hit_path_for(track, pointer, pos);
            if let Some(target) = path.last().copied() {
                track.pressed = Some(target);
                track.pressed_path = path.clone();
                track.set_pressed(target, true);

                // 指针按下取焦点（≈ WinUI：点击让最近的可聚焦祖先获得焦点，来源 = Pointer）
                if let Some(f) = focus::focusable_ancestor(track, &path) {
                    let change = focus::set_focus(track, Some(f), FocusState::Pointer);
                    if let Some(lost) = change.lost {
                        out.events.push((
                            hit::path_to(track, lost),
                            Event::simple(EventKind::LostFocus),
                        ));
                    }
                    if let Some(got) = change.got {
                        out.events.push((
                            hit::path_to(track, got),
                            Event::simple(EventKind::GotFocus),
                        ));
                    }
                }
            }
            if !path.is_empty() {
                out.events.push((
                    path,
                    Event::pointer(EventKind::PointerPressed, pointer, pos, button),
                ));
            }
        }

        InputEvent::Up {
            pointer,
            pos,
            button,
        } => {
            let path = hit::hit_path_for(track, pointer, pos);
            if !path.is_empty() {
                out.events.push((
                    path.clone(),
                    Event::pointer(EventKind::PointerReleased, pointer, pos, button),
                ));
            }

            // 点击合成：按下时的目标仍在释放链上（含自身）⇒ Tapped 发给它
            if let Some(pressed) = track.pressed
                && path.contains(&pressed)
            {
                out.tapped = Some(pressed);
                out.events.push((
                    hit::path_to(track, pressed),
                    Event::pointer(EventKind::Tapped, pointer, pos, button),
                ));
            }

            // 轻关闭（light dismiss，≈ WinUI `IsLightDismissEnabled`）：
            // 点击落在"可关闭弹层"子树之外 ⇒ 给该层根发 `Dismissed`（Direct）。
            // 层是声明式的，框架不能替用户删层——由层根的 Dismissed 处理器翻自己的状态。
            // **排在 Tapped 之后**：锚点按钮的 toggle（关）与 dismiss（关）幂等一致，
            // 不会出现"先关又被锚点重新打开"的次序问题。
            out.events.extend(dismiss_outside_popups(track, &path));

            // 清按下态（整条按下链都清，避免捕获导致漏清）
            let pressed_path = std::mem::take(&mut track.pressed_path);
            for id in pressed_path {
                track.set_pressed(id, false);
            }
            track.pressed = None;

            out.hover_changed |= update_hover(track, pos, pointer, &mut out.events);
        }

        InputEvent::Wheel {
            pointer,
            pos,
            delta,
        } => {
            let _ = pointer;
            let path = hit::hit_path(track, pos);
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
                    Event::pointer(
                        EventKind::PointerCanceled,
                        pointer,
                        Point::zero(),
                        PointerButton::Left,
                    ),
                ));
            }
            let pressed_path = std::mem::take(&mut track.pressed_path);
            for id in pressed_path {
                track.set_pressed(id, false);
            }
            track.pressed = None;
        }

        InputEvent::Leave => {
            out.hover_changed = update_hover_path(track, &[], PointerId(0), Point::zero(), &mut out.events);
        }
    }

    out
}

fn push(
    out: &mut InputStep,
    pointer: PointerId,
    pos: Point,
    kind: EventKind,
    button: PointerButton,
    track: &Track,
) {
    let path = hit::hit_path_for(track, pointer, pos);
    if !path.is_empty() {
        out.events
            .push((path, Event::pointer(kind, pointer, pos, button)));
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
fn update_hover(
    track: &mut Track,
    pos: Point,
    pointer: PointerId,
    events: &mut Vec<(Vec<NodeId>, Event)>,
) -> bool {
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
mod tests {
    use super::*;
    use crate::cmd::{Cmd, apply_cmds};
    use crate::layout::{layout, rect_of};
    use crate::track::{Kind, Layer};
    use lieui_geom::Size;

    const WINDOW: Size = Size::new(300.0, 100.0);
    const P: PointerId = PointerId(0);

    fn fixed(t: &mut Track, w: f32, h: f32) -> NodeId {
        let id = t.create(Kind::Box, None);
        t.get_mut(id).unwrap().layout.dim = [w, h];
        id
    }

    /// 内容根 + 3 个 100×100 子节点（row）
    fn setup() -> (Track, NodeId, Vec<NodeId>) {
        let mut t = Track::new();
        let root = t.create(Kind::Box, None);
        t.get_mut(root).unwrap().layout.flex_direction = lieui_layout::FlexDirection::Row;
        t.add_root(Layer::Content, None, root);
        let kids: Vec<NodeId> = (0..3).map(|_| fixed(&mut t, 100.0, 100.0)).collect();
        for k in &kids {
            t.append_child(root, *k);
        }
        layout(&mut t, WINDOW);
        (t, root, kids)
    }

    fn kinds(step: &InputStep) -> Vec<EventKind> {
        step.events.iter().map(|(_, e)| e.kind()).collect()
    }

    #[test]
    fn move_updates_hover_along_the_chain() {
        let (mut t, root, kids) = setup();
        let s = step(
            &mut t,
            InputEvent::Move {
                pointer: P,
                pos: Point::new(150.0, 50.0),
            },
        );

        assert!(s.hover_changed);
        assert_eq!(t.hover, Some(kids[1]));
        assert_eq!(t.hover_path, vec![root, kids[1]]);
        // 命中链上的每个节点都置 pointer_over（容器也跟着亮）
        assert!(t.state(root).pointer_over);
        assert!(t.state(kids[1]).pointer_over);
        assert!(!t.state(kids[0]).pointer_over);

        assert_eq!(
            kinds(&s),
            vec![
                EventKind::PointerEntered, // root
                EventKind::PointerEntered, // kids[1]
                EventKind::PointerMoved,
            ]
        );
    }

    #[test]
    fn hover_moves_clear_the_old_chain() {
        let (mut t, root, kids) = setup();
        step(&mut t, InputEvent::Move { pointer: P, pos: Point::new(50.0, 50.0) });
        assert!(t.state(kids[0]).pointer_over);

        let s = step(&mut t, InputEvent::Move { pointer: P, pos: Point::new(150.0, 50.0) });
        assert!(!t.state(kids[0]).pointer_over, "旧目标已离开");
        assert!(t.state(kids[1]).pointer_over);
        assert!(t.state(root).pointer_over, "公共祖先保持不变");
        assert_eq!(
            kinds(&s),
            vec![
                EventKind::PointerExited,  // kids[0]
                EventKind::PointerEntered, // kids[1]
                EventKind::PointerMoved,
            ]
        );
    }

    #[test]
    fn leave_clears_everything() {
        let (mut t, root, kids) = setup();
        step(&mut t, InputEvent::Move { pointer: P, pos: Point::new(50.0, 50.0) });
        let s = step(&mut t, InputEvent::Leave);
        assert_eq!(t.hover, None);
        assert!(t.hover_path.is_empty());
        assert!(!t.state(root).pointer_over && !t.state(kids[0]).pointer_over);
        assert_eq!(
            kinds(&s),
            vec![EventKind::PointerExited, EventKind::PointerExited]
        );
    }

    #[test]
    fn down_then_up_on_the_same_node_synthesizes_tapped() {
        let (mut t, _, kids) = setup();
        let down = step(
            &mut t,
            InputEvent::Down {
                pointer: P,
                pos: Point::new(150.0, 50.0),
                button: PointerButton::Left,
            },
        );
        assert_eq!(t.pressed, Some(kids[1]));
        assert!(t.state(kids[1]).pressed);
        assert!(kinds(&down).contains(&EventKind::PointerPressed));

        let up = step(
            &mut t,
            InputEvent::Up {
                pointer: P,
                pos: Point::new(150.0, 50.0),
                button: PointerButton::Left,
            },
        );
        assert_eq!(up.tapped, Some(kids[1]));
        assert!(kinds(&up).contains(&EventKind::Tapped));
        assert_eq!(t.pressed, None);
        assert!(!t.state(kids[1]).pressed, "按下态已清理");
    }

    #[test]
    fn up_on_a_child_of_the_pressed_node_still_taps() {
        let (mut t, _, kids) = setup();
        let inner = fixed(&mut t, 40.0, 40.0);
        t.append_child(kids[1], inner);
        layout(&mut t, WINDOW);

        step(
            &mut t,
            InputEvent::Down {
                pointer: P,
                pos: Point::new(150.0, 50.0),
                button: PointerButton::Left,
            },
        );
        // 释放点落在按下目标的子节点上 ⇒ 仍算点击按下目标
        let up = step(
            &mut t,
            InputEvent::Up {
                pointer: P,
                pos: Point::new(105.0, 5.0),
                button: PointerButton::Left,
            },
        );
        assert_eq!(up.tapped, Some(kids[1]));
    }

    #[test]
    fn up_elsewhere_does_not_tap() {
        let (mut t, _, _) = setup();
        step(
            &mut t,
            InputEvent::Down {
                pointer: P,
                pos: Point::new(50.0, 50.0),
                button: PointerButton::Left,
            },
        );
        let up = step(
            &mut t,
            InputEvent::Up {
                pointer: P,
                pos: Point::new(250.0, 50.0),
                button: PointerButton::Left,
            },
        );
        assert_eq!(up.tapped, None);
        assert!(!kinds(&up).contains(&EventKind::Tapped));
    }

    #[test]
    fn down_grabs_focus_for_the_nearest_tab_stop() {
        let (mut t, root, kids) = setup();
        t.get_mut(kids[1]).unwrap().tab_stop = true;

        step(
            &mut t,
            InputEvent::Down {
                pointer: P,
                pos: Point::new(150.0, 50.0),
                button: PointerButton::Left,
            },
        );
        assert_eq!(t.focused, Some(kids[1]));
        assert_eq!(
            t.get(kids[1]).unwrap().focus_state,
            FocusState::Pointer,
            "焦点来源 = Pointer（键盘聚焦才画 ring）"
        );
        assert!(root.is_null() || t.get(root).is_some());
    }

    #[test]
    fn capture_keeps_events_on_the_captured_chain() {
        let (mut t, _, kids) = setup();
        apply_cmds(
            &mut t,
            &[Cmd::CapturePointer {
                pointer: P,
                id: kids[0],
            }],
        );

        // 指针已经移到别处，但事件仍回到被捕获的链上
        let up = step(
            &mut t,
            InputEvent::Up {
                pointer: P,
                pos: Point::new(250.0, 50.0),
                button: PointerButton::Left,
            },
        );
        let released_on = up
            .events
            .iter()
            .find(|(_, e)| e.kind() == EventKind::PointerReleased)
            .map(|(path, _)| *path.last().unwrap());
        assert_eq!(released_on, Some(kids[0]));
    }

    #[test]
    fn cancel_emits_capture_lost_and_clears_state() {
        let (mut t, _, kids) = setup();
        apply_cmds(
            &mut t,
            &[
                Cmd::CapturePointer {
                    pointer: P,
                    id: kids[0],
                },
                Cmd::SetPressed {
                    id: kids[0],
                    pressed: true,
                },
            ],
        );
        step(&mut t, InputEvent::Move { pointer: P, pos: Point::new(50.0, 50.0) });

        let s = step(&mut t, InputEvent::Cancel { pointer: P });
        assert!(kinds(&s).contains(&EventKind::PointerCaptureLost));
        assert_eq!(t.captured_by(P), None);
        assert!(!t.state(kids[0]).pressed);
        assert_eq!(t.hover, None);
    }

    #[test]
    fn wheel_goes_to_the_hovered_chain_and_default_scrolls_the_container() {
        let mut t = Track::new();
        let root = t.create(Kind::Box, None);
        t.get_mut(root).unwrap().layout.dim = [100.0, 100.0];
        t.get_mut(root).unwrap().layout.overflow_scroll = true;
        t.add_root(Layer::Content, None, root);

        let child = fixed(&mut t, 100.0, 400.0);
        t.append_child(root, child);
        layout(&mut t, WINDOW);
        assert_eq!(t.get(root).unwrap().content_size.height, 400.0);
        assert_eq!(rect_of(&t, child).height, 400.0);

        let s = step(
            &mut t,
            InputEvent::Wheel {
                pointer: P,
                pos: Point::new(50.0, 50.0),
                delta: (0.0, 30.0),
            },
        );
        let path = s.events[0].0.clone();
        assert_eq!(s.events[0].1.kind(), EventKind::PointerWheelChanged);

        // 调用方在"未被处理"后执行默认行为（正 delta = 向上 ⇒ 顶部滚不动）
        assert!(!default_wheel_scroll(&mut t, &path, (0.0, 30.0)), "已在顶部");
        assert_eq!(t.scroll_offset(root), (0.0, 0.0));

        // 向下滚（负 delta）⇒ offset 增大
        assert!(default_wheel_scroll(&mut t, &path, (0.0, -30.0)));
        assert_eq!(t.scroll_offset(root), (0.0, 30.0));

        // 到底部后不再滚
        assert!(default_wheel_scroll(&mut t, &path, (0.0, -1000.0)));
        assert_eq!(t.scroll_offset(root), (0.0, 300.0));
        assert!(!default_wheel_scroll(&mut t, &path, (0.0, -10.0)), "已在底部");
    }

    #[test]
    fn wheel_ignores_non_scrollable_ancestors() {
        let (mut t, _, kids) = setup();
        let s = step(
            &mut t,
            InputEvent::Wheel {
                pointer: P,
                pos: Point::new(50.0, 50.0),
                delta: (0.0, 30.0),
            },
        );
        let path = s.events[0].0.clone();
        assert!(!default_wheel_scroll(&mut t, &path, (0.0, 30.0)));
        assert_eq!(t.scroll_offset(kids[0]), (0.0, 0.0));
    }

    #[test]
    fn input_event_accessors() {
        let e = InputEvent::Move {
            pointer: PointerId(2),
            pos: Point::new(1.0, 2.0),
        };
        assert_eq!(e.pointer(), PointerId(2));
        assert_eq!(e.pos(), Some(Point::new(1.0, 2.0)));
        assert_eq!(InputEvent::Leave.pos(), None);
    }
}
