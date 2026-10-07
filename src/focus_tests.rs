use super::*;
use crate::track::{Kind, Layer, Visibility};

fn tree() -> (Track, Vec<NodeId>) {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);

    let mut stops = Vec::new();
    for _ in 0..3 {
        let n = t.create(Kind::Box, None);
        t.append_child(root, n);
        t.get_mut(n).unwrap().tab_stop = true;
        stops.push(n);
    }
    (t, stops)
}

#[test]
fn set_focus_clears_the_old_one() {
    let (mut t, stops) = tree();
    let c = set_focus(&mut t, Some(stops[0]), FocusState::Keyboard);
    assert_eq!(c.got, Some(stops[0]));
    assert_eq!(c.lost, None);
    assert_eq!(t.focused, Some(stops[0]));
    assert_eq!(t.get(stops[0]).unwrap().focus_state, FocusState::Keyboard);
    assert!(t.get(stops[0]).unwrap().interaction.focused);

    let c = set_focus(&mut t, Some(stops[1]), FocusState::Pointer);
    assert_eq!(c.lost, Some(stops[0]));
    assert_eq!(c.got, Some(stops[1]));
    assert_eq!(t.get(stops[0]).unwrap().focus_state, FocusState::Unfocused);
    assert!(!t.get(stops[0]).unwrap().interaction.focused);
}

#[test]
fn same_target_only_updates_the_state() {
    let (mut t, stops) = tree();
    set_focus(&mut t, Some(stops[0]), FocusState::Keyboard);
    let c = set_focus(&mut t, Some(stops[0]), FocusState::Pointer);
    assert!(!c.is_change());
    assert_eq!(t.get(stops[0]).unwrap().focus_state, FocusState::Pointer);
}

#[test]
fn clearing_focus() {
    let (mut t, stops) = tree();
    set_focus(&mut t, Some(stops[1]), FocusState::Keyboard);
    let c = set_focus(&mut t, None, FocusState::Unfocused);
    assert_eq!(c.lost, Some(stops[1]));
    assert_eq!(t.focused, None);
}

#[test]
fn dead_target_is_ignored() {
    let (mut t, _) = tree();
    let c = set_focus(&mut t, Some(NodeId::NULL), FocusState::Keyboard);
    assert!(!c.is_change());
    assert_eq!(t.focused, None);
}

#[test]
fn focusable_ancestor_picks_the_innermost() {
    let (mut t, stops) = tree();
    let inner = t.create(Kind::Box, None);
    t.append_child(stops[0], inner);
    assert_eq!(focusable_ancestor(&t, &[stops[0], inner]), Some(stops[0]));
    assert_eq!(focusable_ancestor(&t, &[inner]), None);

    t.get_mut(stops[0]).unwrap().interaction.enabled = false;
    assert_eq!(focusable_ancestor(&t, &[stops[0], inner]), None, "禁用不可聚焦");
}

#[test]
fn tab_order_respects_tab_index() {
    let (mut t, stops) = tree();
    assert_eq!(tab_order(&t), vec![stops[0], stops[1], stops[2]], "默认按树序");

    // tab_index = -1 ⇒ 排到最前（负数优先，符合 WinUI 的 TabIndex 语义）
    t.get_mut(stops[2]).unwrap().tab_index = -1;
    assert_eq!(tab_order(&t), vec![stops[2], stops[0], stops[1]]);

    // tab_index = 5 ⇒ 排到最后
    t.get_mut(stops[2]).unwrap().tab_index = 5;
    assert_eq!(tab_order(&t), vec![stops[0], stops[1], stops[2]]);
}

#[test]
fn tab_order_skips_disabled_and_collapsed() {
    let (mut t, stops) = tree();
    t.get_mut(stops[1]).unwrap().interaction.enabled = false;
    t.get_mut(stops[2]).unwrap().visibility = Visibility::Collapsed;
    assert_eq!(tab_order(&t), vec![stops[0]]);
}

#[test]
fn next_tab_cycles_both_ways() {
    let (t, stops) = tree();
    assert_eq!(next_tab(&t, Some(stops[0]), true), Some(stops[1]));
    assert_eq!(next_tab(&t, Some(stops[1]), true), Some(stops[2]));
    assert_eq!(next_tab(&t, Some(stops[2]), true), Some(stops[0]), "循环");

    assert_eq!(next_tab(&t, Some(stops[0]), false), Some(stops[2]), "反向循环");
    assert_eq!(next_tab(&t, None, true), Some(stops[0]));
    assert_eq!(next_tab(&t, None, false), Some(stops[2]));
}

#[test]
fn empty_order_returns_none() {
    let t = Track::new();
    assert_eq!(next_tab(&t, None, true), None);
}
