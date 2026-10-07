use super::*;
use crate::track::{Key, Kind, Layer, Track};

fn text_node(t: &mut Track, s: &str) -> NodeId {
    t.create(Kind::Text(s.to_string()), None)
}

#[test]
fn set_text_marks_layout_and_damage() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    let id = text_node(&mut t, "a");
    t.append_child(root, id);
    let _ = t.take_damage();

    let d = apply_cmds(&mut t, &[Cmd::SetText { id, text: "b".into() }]);
    assert!(d.contains(Dirty::LAYOUT));
    assert!(d.contains(Dirty::PAINT));
    assert!(matches!(t.get(id).map(|n| &n.kind), Some(Kind::Text(s)) if s == "b"));
}

#[test]
fn set_text_with_same_value_is_a_no_op() {
    let mut t = Track::new();
    let id = text_node(&mut t, "a");
    t.get_mut(id).unwrap().flags = crate::track::Flags::empty();

    let d = apply_cmds(&mut t, &[Cmd::SetText { id, text: "a".into() }]);
    assert!(d.is_empty(), "同值不应产生脏标志");
    assert!(!t.get(id).unwrap().flags.contains(crate::track::Flags::MEASURE_DIRTY));
}

#[test]
fn set_kind_desc_keeps_state_and_reports_change() {
    let mut t = Track::new();
    let id = t.create(
        Kind::Slider {
            value: 0.1,
            min: 0.0,
            max: 1.0,
            dragging: true,
        },
        None,
    );

    let d = apply_cmds(
        &mut t,
        &[Cmd::SetKindDesc {
            id,
            desc: KindDesc::Slider {
                value: 0.9,
                min: 0.0,
                max: 1.0,
            },
        }],
    );
    assert!(d.contains(Dirty::PAINT));
    match &t.get(id).unwrap().kind {
        Kind::Slider { value, dragging, .. } => {
            assert_eq!(*value, 0.9);
            assert!(*dragging);
        }
        _ => panic!(),
    }

    // 同值再来一次：零操作
    let d = apply_cmds(
        &mut t,
        &[Cmd::SetKindDesc {
            id,
            desc: KindDesc::Slider {
                value: 0.9,
                min: 0.0,
                max: 1.0,
            },
        }],
    );
    assert!(d.is_empty());
}

#[test]
fn focus_moves_and_clears() {
    let mut t = Track::new();
    let a = t.create(Kind::Box, None);
    let b = t.create(Kind::Box, None);

    apply_cmds(
        &mut t,
        &[Cmd::SetFocus {
            id: a,
            state: FocusState::Keyboard,
        }],
    );
    assert_eq!(t.focused, Some(a));
    assert_eq!(t.get(a).unwrap().focus_state, FocusState::Keyboard);

    // 焦点移到 b：a 应自动失焦
    apply_cmds(
        &mut t,
        &[Cmd::SetFocus {
            id: b,
            state: FocusState::Pointer,
        }],
    );
    assert_eq!(t.focused, Some(b));
    assert_eq!(t.get(a).unwrap().focus_state, FocusState::Unfocused);

    apply_cmds(
        &mut t,
        &[Cmd::SetFocus {
            id: b,
            state: FocusState::Unfocused,
        }],
    );
    assert_eq!(t.focused, None);
}

#[test]
fn capture_and_release_pointer_clears_pressed() {
    let mut t = Track::new();
    let id = t.create(Kind::Box, None);
    let p = PointerId(0);

    apply_cmds(
        &mut t,
        &[
            Cmd::SetPressed { id, pressed: true },
            Cmd::CapturePointer { pointer: p, id },
        ],
    );
    assert_eq!(t.captured_by(p), Some(id));
    assert!(t.state(id).pressed);

    apply_cmds(&mut t, &[Cmd::ReleasePointer { pointer: p }]);
    assert_eq!(t.captured_by(p), None);
    assert!(!t.state(id).pressed, "释放捕获应清 pressed");
}

#[test]
fn scroll_is_paint_only() {
    let mut t = Track::new();
    let id = t.create(Kind::Box, None);
    t.get_mut(id).unwrap().flags = crate::track::Flags::empty();

    let d = apply_cmds(
        &mut t,
        &[Cmd::ScrollTo {
            id,
            offset: (0.0, 30.0),
        }],
    );
    assert!(d.contains(Dirty::PAINT));
    assert!(!d.contains(Dirty::LAYOUT), "滚动不触发重排");
}

#[test]
fn bring_into_view_bubbles_to_scroller() {
    let mut t = Track::new();
    let scroller = t.create(Kind::Box, None);
    t.get_mut(scroller).unwrap().layout.overflow_scroll = true;
    let mid = t.create(Kind::Box, None);
    let leaf = t.create(Kind::Text("x".into()), None);
    t.append_child(scroller, mid);
    t.append_child(mid, leaf);
    let _ = t.take_damage();

    let d = apply_cmds(&mut t, &[Cmd::BringIntoView { id: leaf }]);
    assert!(d.contains(Dirty::PAINT));
    assert!(
        t.get(scroller)
            .unwrap()
            .flags
            .contains(crate::track::Flags::PAINT_DIRTY),
        "最近滚动祖先应被标脏"
    );
}

#[test]
fn mount_and_unmount_roots() {
    let mut t = Track::new();
    let content = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, content);

    let modal = t.create(Kind::Box, None);
    let d = apply_cmds(
        &mut t,
        &[Cmd::Mount {
            layer: Layer::Modal,
            id: modal,
        }],
    );
    assert!(d.contains(Dirty::PAINT));
    assert_eq!(t.roots_of(Layer::Modal).count(), 1);

    let rid = t.roots_of(Layer::Modal).next().unwrap().id;
    let d = apply_cmds(&mut t, &[Cmd::Unmount { root: rid }]);
    assert!(d.contains(Dirty::LAYOUT));
    assert_eq!(t.roots_of(Layer::Modal).count(), 0);
    assert!(!t.contains(modal), "卸载应销毁子树");
}

#[test]
fn invalidate_view_sets_view_flag() {
    let mut t = Track::new();
    let d = apply_cmds(&mut t, &[Cmd::InvalidateView]);
    assert_eq!(d, Dirty::VIEW);
}

#[test]
fn cmdbuf_helpers_record_expected_commands() {
    let mut b = CmdBuf::new();
    let id = NodeId::new(0, 0);
    b.set_text(id, "hi");
    b.damage(id);
    b.request_repaint();
    b.invalidate_view();
    assert_eq!(b.len(), 4);
    assert!(matches!(b.as_slice()[0], Cmd::SetText { .. }));
    assert!(matches!(b.as_slice()[3], Cmd::InvalidateView));
    let taken = b.take();
    assert_eq!(taken.len(), 4);
    assert!(b.is_empty());
}

#[test]
fn key_is_not_needed_for_cmds() {
    // 只是确认 Cmd 的 payload 都是纯数据（可 Debug/可比）
    let c = vec![Cmd::SetKindDesc {
        id: NodeId::new(1, 2),
        desc: KindDesc::Checkbox { checked: true },
    }];
    let s = format!("{c:?}");
    assert!(s.contains("Checkbox"));
    let _ = Key::U64(1);
}
