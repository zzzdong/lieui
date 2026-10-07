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
#[path = "focus_tests.rs"]
mod tests;
