//! 焦点管理（≈ WinUI `FocusManager` + `IsTabStop` / `TabIndex`）。
//!
//! 现状缺口：旧实现没有 Tab 链（只有鼠标点击设置的焦点），也没有 `FocusState`，
//! 所以"键盘聚焦才画 focus ring"做不到。这里补齐：
//! - [`set_focus`]：单一入口，保证"旧焦点必被清掉"（不会出现两个 `focused`）；
//! - [`focusable_ancestor`]：指针按下时沿命中链找最近的 `tab_stop`（≈ 点击获得焦点）；
//! - [`tab_order`] / [`next_tab`]：Tab / Shift+Tab 顺序，按 `(tab_index, 树序)` 稳定排序。
//!
//! 事件派发（`GettingFocus` / `GotFocus` / `LosingFocus` / `LostFocus`）由调用方
//! 根据 [`FocusChange`] 的返回值发出——**不用闭包**，可取消（`GettingFocus` 的返回风格）。

use crate::track::{FocusState, NodeId, Track};

/// 一次焦点变化的结果
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct FocusChange {
    pub lost: Option<NodeId>,
    pub got: Option<NodeId>,
}

impl FocusChange {
    pub fn is_change(&self) -> bool {
        self.lost.is_some() || self.got.is_some()
    }
}

/// 设置焦点（`target = None` 表示清除焦点）。
///
/// 会先把旧焦点置为 [`FocusState::Unfocused`]，再写新焦点（两个 `set_focused` 各自标脏）。
pub fn set_focus(track: &mut Track, target: Option<NodeId>, state: FocusState) -> FocusChange {
    let target = target.filter(|id| track.contains(*id));
    let old = track.focused;

    if old == target {
        // 同一节点：只更新来源（例如已聚焦的输入框被点击 ⇒ Keyboard → Pointer）
        if let Some(id) = target {
            track.set_focused(id, state);
        }
        return FocusChange::default();
    }

    let mut change = FocusChange::default();
    if let Some(o) = old {
        track.set_focused(o, FocusState::Unfocused);
        change.lost = Some(o);
    }
    match target {
        Some(id) => {
            track.focused = Some(id);
            track.set_focused(id, state);
            change.got = Some(id);
        }
        None => track.focused = None,
    }
    change
}

/// 沿命中链（外 → 内）找**最近的可聚焦节点**：`tab_stop` 且 `enabled`
pub fn focusable_ancestor(track: &Track, path: &[NodeId]) -> Option<NodeId> {
    path.iter().rev().copied().find(|id| {
        track
            .get(*id)
            .map(|n| n.tab_stop && n.interaction.enabled)
            .unwrap_or(false)
    })
}

/// 当前 Tab 顺序：所有层根子树里的可聚焦节点，按 `(tab_index, 树序)` 稳定排序。
///
/// `tab_index` 为 0 的按树序排在前面（与 WinUI 的默认行为一致：0 = 未指定）。
pub fn tab_order(track: &Track) -> Vec<NodeId> {
    let mut nodes: Vec<(i32, usize, NodeId)> = Vec::new();
    let mut seq = 0usize;
    for root in track.roots() {
        for id in track.descendants(root.node) {
            if let Some(n) = track.get(id)
                && n.tab_stop
                && n.interaction.enabled
                && n.visibility == crate::track::Visibility::Visible
            {
                nodes.push((n.tab_index, seq, id));
            }
            seq += 1;
        }
    }
    nodes.sort_by_key(|(idx, seq, _)| (*idx, *seq));
    nodes.into_iter().map(|(_, _, id)| id).collect()
}

/// Tab / Shift+Tab（循环）。`current` 不在链上时：向前取第一个、向后取最后一个。
pub fn next_tab(track: &Track, current: Option<NodeId>, forward: bool) -> Option<NodeId> {
    let order = tab_order(track);
    if order.is_empty() {
        return None;
    }
    let pos = current.and_then(|c| order.iter().position(|id| *id == c));
    match pos {
        Some(i) if forward => Some(order[(i + 1) % order.len()]),
        Some(i) => Some(order[(i + order.len() - 1) % order.len()]),
        None if forward => Some(order[0]),
        None => Some(order[order.len() - 1]),
    }
}

#[cfg(test)]
mod tests {
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
}
