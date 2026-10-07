use super::*;
use crate::cmd::{Cmd, apply_cmds};
use crate::layout::{layout, rect_of};
use crate::track::{Kind, Layer};
use lieui_geom::Size;

const WINDOW: Size = Size::new(300.0, 100.0);
const P: PointerId = PointerId(0);

/// 右键合成 `RightTapped`（而非 `Tapped`）：现有左键行为不该被右键触发
#[test]
fn right_button_synthesizes_right_tapped_instead_of_tapped() {
    let (mut t, _root, kids) = setup();
    let target = kids[0];
    let pos = rect_of(&t, target).center();

    let down = step(
        &mut t,
        InputEvent::Down {
            pointer: P,
            pos,
            button: PointerButton::Right,
        },
    );
    let up = step(
        &mut t,
        InputEvent::Up {
            pointer: P,
            pos,
            button: PointerButton::Right,
        },
    );
    let right: Vec<EventKind> = down
        .events
        .iter()
        .chain(up.events.iter())
        .map(|(_, e)| e.kind())
        .collect();
    assert!(right.contains(&EventKind::RightTapped), "右键 ⇒ RightTapped：{right:?}");
    assert!(!right.contains(&EventKind::Tapped), "右键不该合成 Tapped：{right:?}");

    let down = step(
        &mut t,
        InputEvent::Down {
            pointer: P,
            pos,
            button: PointerButton::Left,
        },
    );
    let up = step(
        &mut t,
        InputEvent::Up {
            pointer: P,
            pos,
            button: PointerButton::Left,
        },
    );
    let left: Vec<EventKind> = down
        .events
        .iter()
        .chain(up.events.iter())
        .map(|(_, e)| e.kind())
        .collect();
    assert!(left.contains(&EventKind::Tapped), "左键 ⇒ Tapped：{left:?}");
    assert!(!left.contains(&EventKind::RightTapped));
}

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
    step(
        &mut t,
        InputEvent::Move {
            pointer: P,
            pos: Point::new(50.0, 50.0),
        },
    );
    assert!(t.state(kids[0]).pointer_over);

    let s = step(
        &mut t,
        InputEvent::Move {
            pointer: P,
            pos: Point::new(150.0, 50.0),
        },
    );
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
    step(
        &mut t,
        InputEvent::Move {
            pointer: P,
            pos: Point::new(50.0, 50.0),
        },
    );
    let s = step(&mut t, InputEvent::Leave);
    assert_eq!(t.hover, None);
    assert!(t.hover_path.is_empty());
    assert!(!t.state(root).pointer_over && !t.state(kids[0]).pointer_over);
    assert_eq!(kinds(&s), vec![EventKind::PointerExited, EventKind::PointerExited]);
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
    assert_eq!(t.pressed.map(|p| p.node), Some(kids[1]));
    assert_eq!(t.pressed.map(|p| p.button), Some(PointerButton::Left));
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

/// 回归（D1）：`Tapped` 必须校验**按下与抬起是同一按键**。
///
/// bug 表现：点击合成只判"按下目标是否仍在释放链上"，不比对按下时的按键。
/// 于是 **右键按下 + 左键抬起落在同一节点 ⇒ 合成 `Tapped`** ⇒ 顺带触发
/// 勾选 / 提交 / 删除 —— 这是数据破坏级缺陷（用户只是想开右键菜单）。
#[test]
fn cross_button_release_does_not_synthesize_tapped() {
    let (mut t, _, kids) = setup();
    layout(&mut t, WINDOW);

    // 右键按下
    step(
        &mut t,
        InputEvent::Down {
            pointer: P,
            pos: Point::new(150.0, 50.0),
            button: PointerButton::Right,
        },
    );
    assert_eq!(t.pressed.map(|p| p.button), Some(PointerButton::Right));

    // 左键抬起，位置仍在同一目标上
    let up = step(
        &mut t,
        InputEvent::Up {
            pointer: P,
            pos: Point::new(150.0, 50.0),
            button: PointerButton::Left,
        },
    );
    assert!(
        up.tapped.is_none(),
        "按键不配对时不得合成点击（否则右键按下会被当成左键点击）"
    );
    assert!(
        !kinds(&up).contains(&EventKind::Tapped),
        "不得派发 Tapped：{kinds:?}",
        kinds = kinds(&up)
    );
    // 右键路径本身不该被左键抬起冒充
    assert!(!kinds(&up).contains(&EventKind::RightTapped));
    // 按下态照常清理
    assert_eq!(t.pressed, None);
    assert!(!t.state(kids[1]).pressed);
}

/// 回归（D1 反向）：按键配对通过时，右键按下 → 右键抬起仍要合成 `RightTapped`。
/// 配对校验不能把右键菜单功能一起堵掉。
#[test]
fn right_button_press_and_release_still_taps() {
    let (mut t, _, kids) = setup();
    layout(&mut t, WINDOW);

    step(
        &mut t,
        InputEvent::Down {
            pointer: P,
            pos: Point::new(150.0, 50.0),
            button: PointerButton::Right,
        },
    );
    let up = step(
        &mut t,
        InputEvent::Up {
            pointer: P,
            pos: Point::new(150.0, 50.0),
            button: PointerButton::Right,
        },
    );
    assert_eq!(up.tapped, Some(kids[1]));
    assert!(kinds(&up).contains(&EventKind::RightTapped));
    assert!(!kinds(&up).contains(&EventKind::Tapped));
}

/// 回归（D10）：二次按下时，上一条按下链必须被清干净。
///
/// bug 表现：`Down` 直接覆盖 `pressed_path`，而后续 `Up` / `Cancel` 只清最新那条链
/// ⇒ 旧链上的节点**永久残留 pressed 视觉**（捕获 / 多指 / 中键+左键同时按下的场景）。
#[test]
fn second_down_clears_previous_pressed_chain() {
    let (mut t, _, kids) = setup();
    let other = fixed(&mut t, 60.0, 60.0);
    t.append_child(kids[0], other);
    layout(&mut t, WINDOW);

    // 在 kids[1] 上按下
    step(
        &mut t,
        InputEvent::Down {
            pointer: P,
            pos: Point::new(150.0, 50.0),
            button: PointerButton::Left,
        },
    );
    assert!(t.state(kids[1]).pressed);

    // 在别的节点上再次按下（不抬起上一次）
    step(
        &mut t,
        InputEvent::Down {
            pointer: P,
            pos: Point::new(30.0, 30.0),
            button: PointerButton::Left,
        },
    );
    assert!(!t.state(kids[1]).pressed, "上一次按下的节点不应残留 pressed 视觉");

    // 收尾：最后一次按下的节点仍应正常清理
    let up = step(
        &mut t,
        InputEvent::Up {
            pointer: P,
            pos: Point::new(30.0, 30.0),
            button: PointerButton::Left,
        },
    );
    assert_eq!(t.pressed, None);
    assert!(up.tapped.is_some());
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
    step(
        &mut t,
        InputEvent::Move {
            pointer: P,
            pos: Point::new(50.0, 50.0),
        },
    );

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

/// 节点中心（命中测试用点）。pp.rs 里有同名helper，但那边不是 pub。
fn center_of(t: &Track, id: NodeId) -> Point {
    let r = crate::layout::rect_of(t, id);
    Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
}

// ─────────────────── A7 回归（D61） ───────────────────

/// 回归（D61）：滚轮事件也必须走**指针捕获**。
///
/// bug 表现：`InputEvent::Wheel` 分支写的是 `let _ = pointer; hit::hit_path(...)`
/// —— **绕过了 `hit_path_for` 的捕获优先逻辑**。于是拖拽过程中（某节点已
/// `capture_pointer`）滚轮会按命中链重新路由，而不是送给捕获者，
/// 表现为"拖拽对滚轮无反应 / 滚轮滚到了别的容器"。
#[test]
fn wheel_is_routed_through_pointer_capture() {
    let (mut t, _, kids) = setup();
    let inner = fixed(&mut t, 40.0, 40.0);
    t.append_child(kids[0], inner);
    layout(&mut t, WINDOW);

    // 在 kids[1] 上按下并捕获（坐标先算出来：`step` 要 `&mut t`）
    let p1 = center_of(&t, kids[1]);
    step(
        &mut t,
        InputEvent::Down {
            pointer: P,
            pos: p1,
            button: PointerButton::Left,
        },
    );
    t.capture_pointer(P, kids[1]);

    // 滚轮落在 kids[0]（**不是**捕获者）上
    let p0 = center_of(&t, kids[0]);
    let out = step(
        &mut t,
        InputEvent::Wheel {
            pointer: P,
            pos: p0,
            delta: (0.0, 10.0),
        },
    );

    let wheel = out
        .events
        .iter()
        .find(|(_, e)| matches!(e.kind(), EventKind::PointerWheelChanged))
        .expect("应产生滚轮事件");
    assert_eq!(
        wheel.0.last().copied(),
        Some(kids[1]),
        "D61：滚轮应路由给捕获者 kids[1]，实际 {:?}",
        wheel.0
    );
}

/// 未捕获时滚轮仍按命中链路由（确认上一条不是"总是发给捕获者"）。
#[test]
fn wheel_without_capture_follows_hit_chain() {
    let (mut t, _, kids) = setup();
    layout(&mut t, WINDOW);

    let p0 = center_of(&t, kids[0]);
    let out = step(
        &mut t,
        InputEvent::Wheel {
            pointer: P,
            pos: p0,
            delta: (0.0, 10.0),
        },
    );
    let wheel = out
        .events
        .iter()
        .find(|(_, e)| matches!(e.kind(), EventKind::PointerWheelChanged))
        .expect("应产生滚轮事件");
    assert_eq!(wheel.0.last().copied(), Some(kids[0]));
}
