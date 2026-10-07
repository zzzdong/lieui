//! `Cmd` 命令缓冲：事件阶段**唯一**的延迟写入通道。
//!
//! 为什么必须有它（`docs/architecture-v3.md` §3.5）：
//! 分发期间我们既要持有节点的可变借用（连 handler 槽），又要发起树操作
//! （聚焦、捕获、挂载层、滚动……）。Rust 不允许两者同时存在，所以把"想做但还不能做"的事
//! 记成纯数据，等借用释放后统一 [`apply_cmds`] 落树。
//!
//! 它同时取代了旧实现里散落的 5 个通道：
//! `EventContext.effects` 标志 + `capture_mouse` 请求 + `begin_drag` 请求 +
//! `PENDING_LAYER` + `PENDING_POPUP_HIDE`。
//!
//! **`Cmd` 里只有数据**（不存闭包）—— 这让它可 `Debug`、可断言、可录制。

use crate::event::PointerId;
use crate::reactive::Dirty;
use crate::track::{FocusState, Key, KindDesc, Layer, NodeId, RootId, Track, Visibility};

/// 一条延迟写入命令
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    // ── 局部内容刷新（不重跑 `view()`）──
    /// 直接改一个文本节点的内容
    SetText {
        id: NodeId,
        text: String,
    },
    /// 覆盖组件的 desc 组字段（state 组保留，见 §3.4.1）
    SetKindDesc {
        id: NodeId,
        desc: KindDesc,
    },
    /// 可见性
    SetVisibility {
        id: NodeId,
        visibility: Visibility,
    },

    // ── 交互态（框架内置行为使用）──
    SetPointerOver {
        id: NodeId,
        over: bool,
    },
    SetPressed {
        id: NodeId,
        pressed: bool,
    },
    /// 设置键盘焦点（`FocusState::Unfocused` 表示清除）
    SetFocus {
        id: NodeId,
        state: FocusState,
    },

    // ── 指针捕获（多指针）──
    CapturePointer {
        pointer: PointerId,
        id: NodeId,
    },
    ReleasePointer {
        pointer: PointerId,
    },

    // ── 滚动（M2 由布局/滚动容器消费）──
    ScrollTo {
        id: NodeId,
        offset: (f32, f32),
    },
    /// "让我可见"：沿祖先冒泡到最近的滚动容器处理
    BringIntoView {
        id: NodeId,
    },

    // ── 层 ──
    Mount {
        layer: Layer,
        id: NodeId,
    },
    Unmount {
        root: RootId,
    },

    // ── 重绘 / 失效 ──
    /// 只把这些节点的矩形并入脏区
    DamageNode {
        id: NodeId,
    },
    /// 按 [`Key`] 标脏（落树时解析成 `NodeId`）。
    ///
    /// ## 为什么需要它（`DamageNode` 不够）
    ///
    /// `NodeId` 只在"手里真有一个节点"时才有 —— 事件处理器里能从 handler 拿到，
    /// 但 [`crate::ViewModel::on_animation`] / [`crate::ViewModel::on_tick`] 里
    /// **拿不到**：那时只有一个 `&mut Ctx`，而 `Ctx` 不暴露 `Track`
    /// （`Track::find_by_key` 是公开的，却够不着）。
    ///
    /// 于是应用声明的**自绘节点**（[`crate::CustomNode`]）想每帧重绘自己时，
    /// 只剩 `DamageAll` 一条路 ⇒ **每帧整窗重绘**。本命令把"按 Key 标脏"
    /// 补成一条通用路径，与应用侧动画解耦。
    DamageKey {
        key: Key,
    },
    /// 整窗脏
    DamageAll,
    /// 需要重排（外部改了布局相关的东西）
    InvalidateLayout,
    /// 下次帧重跑 `view()`（非 `Signal` 路径的状态变更）
    InvalidateView,
}

/// 命令缓冲（用户态与框架内部都用它攒命令）
#[derive(Default, Clone, Debug)]
pub struct CmdBuf {
    cmds: Vec<Cmd>,
}

impl CmdBuf {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, c: Cmd) {
        self.cmds.push(c);
    }

    pub fn extend(&mut self, other: impl IntoIterator<Item = Cmd>) {
        self.cmds.extend(other);
    }

    pub fn as_slice(&self) -> &[Cmd] {
        &self.cmds
    }

    pub fn len(&self) -> usize {
        self.cmds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cmds.is_empty()
    }

    pub fn clear(&mut self) {
        self.cmds.clear();
    }

    pub fn take(&mut self) -> Vec<Cmd> {
        std::mem::take(&mut self.cmds)
    }

    // ── 便捷构造（内置行为与 `Ctx` 用）──

    pub fn set_text(&mut self, id: NodeId, text: impl Into<String>) {
        self.push(Cmd::SetText { id, text: text.into() });
    }

    pub fn set_kind_desc(&mut self, id: NodeId, desc: KindDesc) {
        self.push(Cmd::SetKindDesc { id, desc });
    }

    pub fn damage(&mut self, id: NodeId) {
        self.push(Cmd::DamageNode { id });
    }

    /// 按 `Key` 标脏（`on_animation` / `on_tick` 里驱动自绘节点用；见 [`Cmd::DamageKey`]）
    pub fn damage_key(&mut self, key: impl Into<Key>) {
        self.push(Cmd::DamageKey { key: key.into() });
    }

    pub fn damage_all(&mut self) {
        self.push(Cmd::DamageAll);
    }

    pub fn focus(&mut self, id: NodeId, state: FocusState) {
        self.push(Cmd::SetFocus { id, state });
    }

    pub fn capture(&mut self, pointer: PointerId, id: NodeId) {
        self.push(Cmd::CapturePointer { pointer, id });
    }

    pub fn release(&mut self, pointer: PointerId) {
        self.push(Cmd::ReleasePointer { pointer });
    }

    pub fn scroll_to(&mut self, id: NodeId, offset: (f32, f32)) {
        self.push(Cmd::ScrollTo { id, offset });
    }

    pub fn request_repaint(&mut self) {
        self.push(Cmd::DamageAll);
    }

    pub fn invalidate_view(&mut self) {
        self.push(Cmd::InvalidateView);
    }
}

/// 落树：把缓冲里的命令按序应用到保留树，返回本窗口需要置位的脏标志。
///
/// 调用时机：**分发结束、借用释放之后**（`WindowCtx::frame` 或事件入口处）。
pub fn apply_cmds(track: &mut Track, cmds: &[Cmd]) -> Dirty {
    let mut dirty = Dirty::empty();

    for c in cmds {
        match c {
            Cmd::SetText { id, text } => {
                if let Some(n) = track.get_mut(*id)
                    && let crate::track::Kind::Text(t) = &mut n.kind
                    && t != text
                {
                    t.clone_from(text);
                    track.mark_layout_dirty(*id);
                    dirty |= Dirty::LAYOUT | Dirty::PAINT | Dirty::PRESENT;
                }
            }
            Cmd::SetKindDesc { id, desc } => {
                let changed = track.get_mut(*id).map(|n| desc.apply_to(&mut n.kind)).unwrap_or(false);
                if changed {
                    track.mark_layout_dirty(*id);
                    track.mark_paint_dirty(*id);
                    dirty |= Dirty::LAYOUT | Dirty::PAINT | Dirty::PRESENT;
                }
            }
            Cmd::SetVisibility { id, visibility } => {
                // ★ 走 `Track::set_visibility`：它把"flow + paint 双标脏"这套
                //   规则收敛在一处（原先这段逻辑内联在cmd 里，若再加 setter
                //   就会有两份实现，容易只改一处）。
                if track.set_visibility(*id, *visibility) {
                    dirty |= Dirty::LAYOUT | Dirty::PAINT | Dirty::PRESENT;
                }
            }

            Cmd::SetPointerOver { id, over } => {
                track.set_pointer_over(*id, *over);
                dirty |= Dirty::PAINT | Dirty::PRESENT;
            }
            Cmd::SetPressed { id, pressed } => {
                track.set_pressed(*id, *pressed);
                dirty |= Dirty::PAINT | Dirty::PRESENT;
            }
            Cmd::SetFocus { id, state } => {
                if *state == FocusState::Unfocused {
                    if track.focused == Some(*id) {
                        track.focused = None;
                    }
                    track.set_focused(*id, FocusState::Unfocused);
                } else {
                    // 旧焦点失焦
                    if let Some(old) = track.focused
                        && old != *id
                    {
                        track.set_focused(old, FocusState::Unfocused);
                    }
                    track.focused = Some(*id);
                    track.set_focused(*id, *state);
                }
                dirty |= Dirty::PAINT | Dirty::PRESENT;
            }

            Cmd::CapturePointer { pointer, id } => {
                track.capture_pointer(*pointer, *id);
            }
            Cmd::ReleasePointer { pointer } => {
                if let Some(id) = track.release_pointer(*pointer) {
                    track.set_pressed(id, false);
                    dirty |= Dirty::PAINT | Dirty::PRESENT;
                }
            }

            Cmd::ScrollTo { id, offset } => {
                // M2 会按内容尺寸钳制；M1 直接写入并标脏（滚动不重排）
                if track.set_scroll_offset(*id, *offset) {
                    dirty |= Dirty::PAINT | Dirty::PRESENT;
                }
            }
            Cmd::BringIntoView { id } => {
                // 沿祖先找最近的滚动容器（M2 接上真实视口与内容尺寸后生效）
                let mut cur = track.parent_of(*id);
                while let Some(c) = cur {
                    let is_scroller = track.get(c).map(|n| n.layout.overflow_scroll).unwrap_or(false);
                    if is_scroller {
                        track.mark_paint_dirty(c);
                        dirty |= Dirty::PAINT | Dirty::PRESENT;
                        break;
                    }
                    cur = track.parent_of(c);
                }
            }

            Cmd::Mount { layer, id } => {
                track.add_root(*layer, None, *id);
                dirty |= Dirty::LAYOUT | Dirty::PAINT | Dirty::PRESENT;
            }
            Cmd::Unmount { root } => {
                if track.remove_root(*root) > 0 {
                    dirty |= Dirty::LAYOUT | Dirty::PAINT | Dirty::PRESENT;
                }
            }

            Cmd::DamageNode { id } => {
                track.mark_paint_dirty(*id);
                dirty |= Dirty::PAINT | Dirty::PRESENT;
            }
            Cmd::DamageKey { key } => {
                // 落树时才解析：`Key` → `NodeId` 需要 `Track`（`Ctx` 里没有）。
                // 查不到（节点还没对齐出来 / 已被销毁）⇒ 静默跳过：动画节点
                // 通常下一帧就能查到，不值得为它整窗脏。
                if let Some(id) = track.find_by_key(key) {
                    track.mark_paint_dirty(id);
                    dirty |= Dirty::PAINT | Dirty::PRESENT;
                }
            }
            Cmd::DamageAll => {
                track.damage_whole_window();
                dirty |= Dirty::PAINT | Dirty::PRESENT;
            }
            Cmd::InvalidateLayout => {
                // 让"运行时标志"与"节点标志"保持一致（布局只看节点标志）
                track.mark_all_layout_dirty();
                dirty |= Dirty::LAYOUT | Dirty::PAINT | Dirty::PRESENT;
            }
            Cmd::InvalidateView => dirty |= Dirty::VIEW,
        }
    }

    dirty
}

#[cfg(test)]
#[path = "cmd_tests.rs"]
mod tests;
