// 本文件由 `#[path]` 从 `event.rs` 挂入，充当其 `mod tests`。
//
// 下面这些附带 mod 原本是 `event.rs` 的顶层 mod。外移后它们的
// `use super::xxx` 指向本模块（而非 `event`），而本模块顶层原有的
// `use super::*;`（来自 `mod tests`）正好把父模块的项引进来 ——
// 所以**无需额外导入**，多加一行反而触发 unused 警告。
//   附带的 mod: emit_tests

mod emit_tests {
    use super::*;
    use crate::app::ExternalData;
    use crate::reactive::Runtime;
    use crate::window::WindowId;

    fn win() -> WindowId {
        WindowId::new(1)
    }

    #[test]
    fn emit_targets_one_window_and_lands_in_the_local_queue() {
        let rt = Runtime::new();
        rt.register_window(win());
        assert!(rt.emit(win(), 7u32));

        let queued = rt.take_pending_external();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, win());
        assert_eq!(queued[0].1.downcast_ref::<u32>(), Some(&7));
    }

    #[test]
    fn emit_global_broadcasts_to_every_window() {
        let rt = Runtime::new();
        let a = WindowId::new(1);
        let b = WindowId::new(2);
        rt.register_window(a);
        rt.register_window(b);

        assert!(rt.emit_global(std::sync::Arc::new("refresh".to_string())));
        let queued = rt.take_pending_external();
        assert_eq!(queued.len(), 2, "每个窗口一条");
        for (w, data) in queued {
            assert!(w == a || w == b);
            let payload = data.downcast::<std::sync::Arc<String>>().expect("是 Arc<String>");
            assert_eq!(payload.as_str(), "refresh");
        }
    }

    #[test]
    fn emitter_is_send_sync_and_carries_its_own_channel() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Emitter<u32>>();

        let rt = Runtime::new();
        rt.register_window(win());
        let tx = Emitter::<&'static str>::new(&rt, win());
        let tx2 = tx.clone();
        assert_eq!(tx2.window(), win());

        // 模拟"业务层在别的线程发事件"：并发发两条，都能落到队列
        let handles: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|msg| {
                let tx = tx.clone();
                std::thread::spawn(move || tx.emit(msg))
            })
            .collect();
        for h in handles {
            assert!(h.join().unwrap());
        }
        let queued = rt.take_pending_external();
        assert_eq!(queued.len(), 2, "跨线程投递同样进队列");
        let mut got: Vec<&'static str> = queued
            .into_iter()
            .map(|(_, d)| d.downcast::<&'static str>().unwrap())
            .collect();
        got.sort_unstable();
        assert_eq!(got, vec!["a", "b"]);
    }

    #[test]
    fn request_stays_the_framework_control_channel() {
        // `request` 走 RequestQueue（开窗/关窗），`emit` 走 Waker 管道 —— 互不干扰
        let rt = Runtime::new();
        rt.register_window(win());
        let cx = Ctx::new(&rt, win(), EventView::tick());
        cx.request(42u8);
        cx.emit("event");
        assert_eq!(rt.requests().len(), 1, "控制消息在 RequestQueue");
        assert_eq!(rt.take_pending_external().len(), 1, "业务事件在 Waker 管道");
    }

    #[test]
    fn external_data_is_the_single_payload_type() {
        // 说明性断言：两条管道共用 `ExternalData` 作为载荷载体
        let d = ExternalData::new(String::from("x"));
        assert_eq!(d.downcast::<String>().as_deref(), Some("x"));
    }
}

use super::*;
use crate::cmd::apply_cmds;
use crate::track::Kind;

fn slot(kind: EventKind, too: bool, f: impl Fn(&mut Ctx) + 'static) -> HandlerSlot {
    HandlerSlot {
        kind,
        handler: Rc::new(f),
        handled_events_too: too,
    }
}

fn chain(depth: usize) -> (Track, Vec<NodeId>) {
    let mut t = Track::new();
    let mut path = Vec::new();
    let root = t.create(Kind::Box, None);
    path.push(root);
    let mut cur = root;
    for _ in 1..depth {
        let c = t.create(Kind::Box, None);
        t.append_child(cur, c);
        cur = c;
        path.push(c);
    }
    (t, path)
}

#[test]
fn routing_is_grouped_as_designed() {
    assert_eq!(EventKind::PreviewKeyDown.routing(), Routing::Tunnel);
    assert_eq!(EventKind::KeyDown.routing(), Routing::Bubble);
    assert_eq!(EventKind::Tapped.routing(), Routing::Bubble);
    assert_eq!(EventKind::PointerCaptureLost.routing(), Routing::Direct);
    assert_eq!(EventKind::Loaded.routing(), Routing::Direct);
    assert_eq!(EventKind::Dismissed.routing(), Routing::Direct);
    assert_eq!(EventKind::Tapped.name(), "Tapped");
}

#[test]
fn pointer_kinds_are_flagged() {
    assert!(EventKind::PointerMoved.is_pointer());
    assert!(EventKind::Tapped.is_pointer());
    assert!(!EventKind::KeyDown.is_pointer());
    assert!(!EventKind::Loaded.is_pointer());
}

#[test]
fn ctx_marks_the_own_window_only() {
    let rt = Runtime::new();
    let a = WindowId::new(1);
    let b = WindowId::new(2);
    rt.register_window(a);
    rt.register_window(b);

    let ctx = Ctx::new(&rt, a, EventView::empty(EventKind::Tapped));
    ctx.invalidate();
    assert!(rt.take_dirty(a).contains(Dirty::VIEW));
    assert!(rt.take_dirty(b).is_empty(), "不应影响其他窗口");

    ctx.request_repaint();
    let d = rt.take_dirty(a);
    assert!(d.contains(Dirty::PAINT) && d.contains(Dirty::PRESENT));
    assert!(!d.contains(Dirty::VIEW), "只重绘不应触发 view()");
}

#[test]
fn bubble_goes_inner_to_outer() {
    let (mut t, path) = chain(3);
    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    for (i, id) in path.iter().enumerate() {
        let log = Rc::clone(&log);
        t.get_mut(*id)
            .unwrap()
            .handlers
            .push(slot(EventKind::Tapped, false, move |_| log.borrow_mut().push(i)));
    }

    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);
    let mut cmds = CmdBuf::new();
    let out = dispatch(
        &rt,
        w,
        &t,
        &path,
        &Event::Simple {
            kind: EventKind::Tapped,
        },
        &mut cmds,
    );

    assert_eq!(out.invoked, 3);
    // 内 → 外：2, 1, 0
    assert_eq!(*log.borrow(), vec![2, 1, 0]);
}

/// 禁用节点**不进路由**（对齐 WinUI `IsEnabled=false`）：它自己的处理器不跑，
/// 但**祖先**的处理器照跑（菜单里"禁用项"仍要能被父容器的轻关闭/取消逻辑看到）。
///
/// 这条曾经是 bug：`enabled` 只挡了内置行为（`widgets::handle`）与焦点，
/// 用户 `on_tap` 照跑 ⇒ "剪切此页（禁用）"看着是灰的、点下去真的执行。
#[test]
fn disabled_nodes_are_skipped_but_ancestors_still_run() {
    let (mut t, path) = chain(3);
    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    for (i, id) in path.iter().enumerate() {
        let log = Rc::clone(&log);
        t.get_mut(*id)
            .unwrap()
            .handlers
            .push(slot(EventKind::Tapped, false, move |_| log.borrow_mut().push(i)));
    }
    // 最内层（2）禁用
    t.get_mut(path[2]).unwrap().interaction.enabled = false;

    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);
    let mut cmds = CmdBuf::new();
    let out = dispatch(
        &rt,
        w,
        &t,
        &path,
        &Event::Simple {
            kind: EventKind::Tapped,
        },
        &mut cmds,
    );

    assert_eq!(*log.borrow(), vec![1, 0], "禁用节点自己没跑，祖先照跑");
    assert_eq!(out.invoked, 2);
}

#[test]
fn tunnel_goes_outer_to_inner() {
    let (mut t, path) = chain(3);
    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    for (i, id) in path.iter().enumerate() {
        let log = Rc::clone(&log);
        t.get_mut(*id)
            .unwrap()
            .handlers
            .push(slot(EventKind::PreviewKeyDown, false, move |_| {
                log.borrow_mut().push(i)
            }));
    }

    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);
    let mut cmds = CmdBuf::new();
    dispatch(
        &rt,
        w,
        &t,
        &path,
        &Event::Simple {
            kind: EventKind::PreviewKeyDown,
        },
        &mut cmds,
    );

    assert_eq!(*log.borrow(), vec![0, 1, 2], "隧道：外 → 内");
}

#[test]
fn direct_only_reaches_the_target() {
    let (mut t, path) = chain(3);
    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    for (i, id) in path.iter().enumerate() {
        let log = Rc::clone(&log);
        t.get_mut(*id)
            .unwrap()
            .handlers
            .push(slot(EventKind::Loaded, false, move |_| log.borrow_mut().push(i)));
    }

    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);
    let mut cmds = CmdBuf::new();
    dispatch(
        &rt,
        w,
        &t,
        &path,
        &Event::Simple {
            kind: EventKind::Loaded,
        },
        &mut cmds,
    );

    assert_eq!(*log.borrow(), vec![2], "只有目标");
}

#[test]
fn handled_stops_the_bubble_but_not_handled_events_too() {
    let (mut t, path) = chain(3);
    let log = Rc::new(std::cell::RefCell::new(Vec::new()));

    // 目标（最内层）：标记 handled
    {
        let log = Rc::clone(&log);
        t.get_mut(path[2])
            .unwrap()
            .handlers
            .push(slot(EventKind::Tapped, false, move |cx| {
                log.borrow_mut().push("target");
                cx.mark_handled();
            }));
    }
    // 中间层：普通（应被跳过）
    {
        let log = Rc::clone(&log);
        t.get_mut(path[1])
            .unwrap()
            .handlers
            .push(slot(EventKind::Tapped, false, move |_| log.borrow_mut().push("mid")));
    }
    // 最外层：handledEventsToo（应仍然执行 —— 内置行为的语义）
    {
        let log = Rc::clone(&log);
        t.get_mut(path[0])
            .unwrap()
            .handlers
            .push(slot(EventKind::Tapped, true, move |_| {
                log.borrow_mut().push("root-builtin")
            }));
    }

    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);
    let mut cmds = CmdBuf::new();
    let out = dispatch(
        &rt,
        w,
        &t,
        &path,
        &Event::Simple {
            kind: EventKind::Tapped,
        },
        &mut cmds,
    );

    assert!(out.handled);
    assert_eq!(out.invoked, 2);
    assert_eq!(out.skipped, 1);
    assert_eq!(*log.borrow(), vec!["target", "root-builtin"]);
}

#[test]
fn handlers_can_queue_commands_that_apply_after_dispatch() {
    let (mut t, path) = chain(2);
    let root = path[0];

    t.get_mut(path[1])
        .unwrap()
        .handlers
        .push(slot(EventKind::Tapped, false, move |cx| {
            cx.damage(root);
            cx.focus(root);
        }));

    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);
    let mut cmds = CmdBuf::new();
    dispatch(
        &rt,
        w,
        &t,
        &path,
        &Event::Simple {
            kind: EventKind::Tapped,
        },
        &mut cmds,
    );

    assert_eq!(cmds.len(), 2);
    // 分发结束、借用释放后才落树
    let d = apply_cmds(&mut t, cmds.as_slice());
    assert!(d.contains(Dirty::PAINT));
    assert_eq!(t.focused, Some(root));
}

#[test]
fn empty_path_is_a_no_op() {
    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);
    let t = Track::new();
    let mut cmds = CmdBuf::new();
    let out = dispatch(
        &rt,
        w,
        &t,
        &[],
        &Event::Simple {
            kind: EventKind::Tapped,
        },
        &mut cmds,
    );
    assert_eq!(out, DispatchOutcome::default());
}

#[test]
fn event_summary_exposes_payload() {
    let ev = Event::Pointer {
        kind: EventKind::PointerPressed,
        pointer: PointerId(3),
        pos: Point::new(10.0, 20.0),
        button: PointerButton::Right,
    };
    let s = ev.summary();
    assert_eq!(s.kind, EventKind::PointerPressed);
    assert_eq!(s.pointer, PointerId(3));
    assert_eq!(s.pos, Point::new(10.0, 20.0));
    assert_eq!(s.button, PointerButton::Right);
    assert_eq!(ev.pointer_pos(), Some(Point::new(10.0, 20.0)));

    let k = Event::Key {
        kind: EventKind::KeyDown,
        code: KeyCode::Named(NamedKey::Enter),
        text: None,
        repeat: false,
        modifiers: Modifiers::SHIFT,
    };
    assert_eq!(k.summary().key, Some(KeyCode::Named(NamedKey::Enter)));
    assert!(k.summary().modifiers.shift());
    assert!(!k.summary().modifiers.ctrl());
    assert!(!Modifiers::CTRL.shift());
    assert!((Modifiers::CTRL | Modifiers::SHIFT).ctrl());
    assert_eq!((Modifiers::CTRL | Modifiers::SHIFT).bits(), 0b011);
    assert!(Modifiers::EMPTY.is_empty());
}
