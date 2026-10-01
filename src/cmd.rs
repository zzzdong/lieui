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
use crate::track::{FocusState, KindDesc, Layer, NodeId, RootId, Track, Visibility};

/// 一条延迟写入命令
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    // ── 局部内容刷新（不重跑 `view()`）──
    /// 直接改一个文本节点的内容
    SetText { id: NodeId, text: String },
    /// 覆盖组件的 desc 组字段（state 组保留，见 §3.4.1）
    SetKindDesc { id: NodeId, desc: KindDesc },
    /// 可见性
    SetVisibility { id: NodeId, visibility: Visibility },

    // ── 交互态（框架内置行为使用）──
    SetPointerOver { id: NodeId, over: bool },
    SetPressed { id: NodeId, pressed: bool },
    /// 设置键盘焦点（`FocusState::Unfocused` 表示清除）
    SetFocus { id: NodeId, state: FocusState },

    // ── 指针捕获（多指针）──
    CapturePointer { pointer: PointerId, id: NodeId },
    ReleasePointer { pointer: PointerId },

    // ── 滚动（M2 由布局/滚动容器消费）──
    ScrollTo { id: NodeId, offset: (f32, f32) },
    /// "让我可见"：沿祖先冒泡到最近的滚动容器处理
    BringIntoView { id: NodeId },

    // ── 层 ──
    Mount { layer: Layer, id: NodeId },
    Unmount { root: RootId },

    // ── 重绘 / 失效 ──
    /// 只把这些节点的矩形并入脏区
    DamageNode { id: NodeId },
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
        self.push(Cmd::SetText {
            id,
            text: text.into(),
        });
    }

    pub fn set_kind_desc(&mut self, id: NodeId, desc: KindDesc) {
        self.push(Cmd::SetKindDesc { id, desc });
    }

    pub fn damage(&mut self, id: NodeId) {
        self.push(Cmd::DamageNode { id });
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
                let changed = track
                    .get_mut(*id)
                    .map(|n| desc.apply_to(&mut n.kind))
                    .unwrap_or(false);
                if changed {
                    track.mark_layout_dirty(*id);
                    track.mark_paint_dirty(*id);
                    dirty |= Dirty::LAYOUT | Dirty::PAINT | Dirty::PRESENT;
                }
            }
            Cmd::SetVisibility { id, visibility } => {
                let changed = {
                    match track.get_mut(*id) {
                        Some(n) if n.visibility != *visibility => {
                            n.visibility = *visibility;
                            true
                        }
                        _ => false,
                    }
                };
                if changed {
                    // 收起/显示会改变它在父流里的占位 ⇒ 兄弟要重排
                    track.mark_flow_dirty(*id);
                    track.mark_paint_dirty(*id);
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
                    let is_scroller = track
                        .get(c)
                        .map(|n| n.layout.overflow_scroll)
                        .unwrap_or(false);
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
mod tests {
    use super::*;
    use crate::track::{Kind, Key, Layer, Track};

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
                desc: KindDesc::Slider { value: 0.9, min: 0.0, max: 1.0 },
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
                desc: KindDesc::Slider { value: 0.9, min: 0.0, max: 1.0 },
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
}
