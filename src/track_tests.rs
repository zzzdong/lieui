// 本文件由 `#[path]` 从 `track.rs` 挂入，充当其 `mod tests`。
//
// 下面这个附带 mod 原本是 `track.rs` 的顶层 mod。外移后它的
// `use super::xxx` 指向本模块（而非 `track`），而本模块顶层原有的
// `use super::*;`（来自 `mod tests`）正好把父模块的项引进来 ——
// 所以**无需额外导入**，多加一行反而触发 unused 警告。
//   附带的 mod: component_behavior

use super::*;

fn slider(value: f32) -> Kind {
    Kind::Slider {
        value,
        min: 0.0,
        max: 1.0,
        dragging: false,
    }
}

#[test]
fn alloc_free_reuses_index_but_bumps_generation() {
    let mut t = Track::new();
    let a = t.create(Kind::Box, None);
    assert_eq!(a.index(), 0);
    assert_eq!(a.generation(), 0);

    assert_eq!(t.destroy(a), 1);
    assert!(!t.contains(a), "旧句柄必须失效");

    let b = t.create(Kind::Box, None);
    assert_eq!(b.index(), 0, "index 复用");
    assert_ne!(b.generation(), a.generation(), "generation 必须变");
}

#[test]
fn destroy_subtree_frees_all_and_detaches() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    let mid = t.create(Kind::Box, None);
    let leaf = t.create(Kind::Text("x".into()), None);
    t.append_child(root, mid);
    t.append_child(mid, leaf);
    assert_eq!(t.len(), 3);

    assert_eq!(t.destroy(mid), 2);
    assert_eq!(t.len(), 1);
    assert!(t.children(root).is_empty());
    assert!(!t.contains(leaf));
}

#[test]
fn descendants_and_ancestors() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    let a = t.create(Kind::Box, None);
    let b = t.create(Kind::Text("b".into()), None);
    t.append_child(root, a);
    t.append_child(a, b);

    assert_eq!(t.descendants(root), vec![root, a, b]);
    assert_eq!(t.ancestors(b).collect::<Vec<_>>(), vec![a, root]);
}

#[test]
fn kind_desc_applies_without_touching_state() {
    let mut t = Track::new();
    let id = t.create(slider(0.5), None);
    t.get_mut(id).unwrap().kind = Kind::Slider {
        value: 0.5,
        min: 0.0,
        max: 1.0,
        dragging: true,
    };

    let changed = t
        .get_mut(id)
        .map(|n| {
            KindDesc::Slider {
                value: 0.8,
                min: 0.0,
                max: 1.0,
            }
            .apply_to(&mut n.kind)
        })
        .unwrap();
    assert!(changed);
    match &t.get(id).unwrap().kind {
        Kind::Slider { value, dragging, .. } => {
            assert_eq!(*value, 0.8);
            assert!(*dragging, "state 组必须保留");
        }
        _ => panic!(),
    }

    // 同值再应用一次 → 零变化
    let changed_again = t
        .get_mut(id)
        .map(|n| {
            KindDesc::Slider {
                value: 0.8,
                min: 0.0,
                max: 1.0,
            }
            .apply_to(&mut n.kind)
        })
        .unwrap();
    assert!(!changed_again);
}

#[test]
fn kind_desc_tag_mismatch_reports_changed() {
    let mut t = Track::new();
    let id = t.create(Kind::Text("a".into()), None);
    let mut kind = Kind::Text("a".into());
    assert_eq!(KindDesc::Text("a".into()).tag(), kind.tag());
    assert!(KindDesc::Box.apply_to(&mut kind), "标签不同应报 changed");
    let _ = t.destroy(id);
}

#[test]
fn roots_are_nested_and_removal_cascades() {
    let mut t = Track::new();
    let content = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, content);

    let modal_node = t.create(Kind::Box, None);
    let modal = t.add_root(Layer::Modal, None, modal_node);

    let popup_node = t.create(Kind::Box, None);
    let popup = t.add_root(Layer::Popup, Some(modal), popup_node);

    assert_eq!(t.roots_of(Layer::Popup).count(), 1);
    assert_eq!(t.root(popup).unwrap().owner, Some(modal));

    // 父层消失 → 嵌套子层一起走（返回值 = 一并移除的层根数）
    assert_eq!(t.remove_root(modal), 2);
    assert!(t.root(popup).is_none());
    assert!(!t.contains(popup_node));
    assert!(t.root(modal).is_none());
    // Content 不受影响
    assert!(t.content_root().is_some());
}

#[test]
fn layer_defaults_follow_the_layer() {
    assert!(!LayerOpts::for_layer(Layer::Overlay).hit_test_visible);
    assert!(LayerOpts::for_layer(Layer::Modal).blocks_below);
    assert!(LayerOpts::for_layer(Layer::Modal).backdrop.is_some());
    assert!(LayerOpts::for_layer(Layer::Popup).dismiss_on_outside_click);
    assert!(!LayerOpts::for_layer(Layer::DragPreview).hit_test_visible);
}

#[test]
fn paint_dirty_registers_node_rect_as_damage() {
    let mut t = Track::new();
    let id = t.create(Kind::Box, None);
    // 布局结果（M2 里由引擎写回；这里手工给）
    t.get_mut(id).unwrap().computed = ComputedLayout {
        x: 10.0,
        y: 20.0,
        width: 30.0,
        height: 40.0,
        overflow_scroll: false,
    };
    t.mark_paint_dirty(id);

    let (rects, all) = t.take_damage();
    assert!(!all);
    assert_eq!(rects, vec![Rect::new(10.0, 20.0, 30.0, 40.0)]);
    assert!(t.get(id).unwrap().flags.contains(Flags::PAINT_DIRTY));

    // 尺寸为 0 的节点不产生脏矩形（首帧还没布局）
    let fresh = t.create(Kind::Box, None);
    t.mark_paint_dirty(fresh);
    let (rects, _) = t.take_damage();
    assert!(rects.is_empty());
}

#[test]
fn layout_dirty_bubbles_to_ancestors() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    let mid = t.create(Kind::Box, None);
    let leaf = t.create(Kind::Text("x".into()), None);
    t.append_child(root, mid);
    t.append_child(mid, leaf);
    t.get_mut(root).unwrap().flags = Flags::empty();
    t.get_mut(mid).unwrap().flags = Flags::empty();
    t.get_mut(leaf).unwrap().flags = Flags::empty();

    t.mark_layout_dirty(leaf);

    for id in [leaf, mid, root] {
        assert!(
            t.get(id).unwrap().flags.contains(Flags::MEASURE_DIRTY),
            "ancestor {id:?} 应被冒泡置脏"
        );
    }
}

#[test]
fn interaction_state_transitions_mark_paint_dirty() {
    let mut t = Track::new();
    let id = t.create(Kind::Box, None);
    let _ = t.take_damage();

    t.set_pointer_over(id, true);
    t.set_pressed(id, true);
    t.set_focused(id, FocusState::Keyboard);

    let st = t.state(id);
    assert!(st.pointer_over && st.pressed && st.focused);
    assert_eq!(t.get(id).unwrap().focus_state, FocusState::Keyboard);

    // 重复置同值不再标脏
    t.get_mut(id).unwrap().flags.remove(Flags::PAINT_DIRTY);
    t.set_pointer_over(id, true);
    assert!(!t.get(id).unwrap().flags.contains(Flags::PAINT_DIRTY));
}

#[test]
fn pointer_capture_is_per_pointer() {
    let mut t = Track::new();
    let a = t.create(Kind::Box, None);
    let b = t.create(Kind::Box, None);
    let mouse = PointerId(0);
    let touch = PointerId(1);

    t.capture_pointer(mouse, a);
    t.capture_pointer(touch, b);
    assert_eq!(t.captured_by(mouse), Some(a));
    assert_eq!(t.captured_by(touch), Some(b));

    // 同一指针重复捕获：只留最后一次
    t.capture_pointer(mouse, b);
    assert_eq!(t.captured_by(mouse), Some(b));
    assert_eq!(t.captures.len(), 2);

    assert_eq!(t.release_pointer(mouse), Some(b));
    assert_eq!(t.captured_by(mouse), None);
    assert_eq!(t.captured_by(touch), Some(b));
}

#[test]
fn scroll_offset_change_marks_layout_and_paint() {
    let mut t = Track::new();
    let id = t.create(Kind::Box, None);
    t.get_mut(id).unwrap().flags = Flags::empty();

    assert!(t.set_scroll_offset(id, (0.0, 24.0)));
    assert!(!t.set_scroll_offset(id, (0.0, 24.0)), "同值不算变化");

    let f = t.get(id).unwrap().flags;
    assert!(f.contains(Flags::PAINT_DIRTY));
    // 偏移平移是布局时烘焙的 ⇒ 滚动必须标重排（子树按新偏移平移）
    assert!(f.contains(Flags::MEASURE_DIRTY), "滚动触发边界重排");
}

#[test]
fn set_children_reorders_without_destroying() {
    let mut t = Track::new();
    let p = t.create(Kind::Box, None);
    let a = t.create(Kind::Text("a".into()), None);
    let b = t.create(Kind::Text("b".into()), None);
    t.append_child(p, a);
    t.append_child(p, b);

    t.set_children(p, &[b, a]);
    assert_eq!(t.children(p), &[b, a]);
    assert_eq!(t.parent_of(a), Some(p));
}

#[test]
fn image_desc_uses_arc_identity() {
    let data = Arc::new(ImageData {
        width: 1,
        height: 1,
        rgba: vec![0, 0, 0, 255],
    });
    let mut kind = Kind::Image(Arc::clone(&data));
    // 同一份 Arc → 未变化
    assert!(!KindDesc::Image(Arc::clone(&data)).apply_to(&mut kind));
}

use crate::style::ShadowSpec;
// ─────────────────── A7 回归（D33 / D58） ───────────────────

/// 回归（D33）：带阴影的节点，`paint_bounds` 必须**覆盖阴影的可见范围**。
///
/// bug 表现：绘制时阴影画在 `rect + offset` 再 `inflate(spread)`，并带高斯模糊
/// （`std_dev = blur * 0.5`）；而脏区按 `paint_bounds` 取 —— 此前它只返回节点矩形
/// ⇒ **光晕外圈不在脏区内** ⇒ 改阴影相关属性后，那圈残影不会被重画。
#[test]
fn paint_bounds_covers_shadow_blur_reach() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    {
        let n = t.get_mut(root).unwrap();
        n.layout.dim = [100.0, 40.0];
        n.paint.shadow = Some(ShadowSpec {
            blur: 8.0,
            spread: 2.0,
            offset_x: 0.0,
            offset_y: 4.0,
            color: Color::new(0, 0, 0),
        });
    }
    // 布局结果写回（paint_bounds 依赖 `rect()`）
    t.get_mut(root).unwrap().computed = lieui_layout::ComputedLayout {
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 40.0,
        overflow_scroll: false,
    };

    let pb = t.get(root).unwrap().paint_bounds();
    let r = t.get(root).unwrap().rect();

    // 高斯 3σ ≈ blur * 1.5 = 12，再加 spread 2 ⇒ 至少外扩 14
    let reach = 2.0 + 8.0 * 1.5;
    assert!(
        pb.width >= r.width + 2.0 * reach - 0.5 && pb.height >= r.height + 2.0 * reach - 0.5,
        "D33：paint_bounds {pb:?} 未覆盖阴影外扩（rect {r:?}, reach {reach}）"
    );
    // 四边保守外扩（偏移方向未知，宁可多标）
    assert!(pb.x <= r.x - reach + 0.5, "左侧应外扩");
    assert!(pb.y <= r.y - reach + 0.5, "上侧应外扩");
}

/// 无阴影时 `paint_bounds` 不应被扩张（保持"精确脏区"的收益）。
#[test]
fn paint_bounds_is_not_inflated_without_shadow() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.get_mut(root).unwrap().layout.dim = [100.0, 40.0];
    t.get_mut(root).unwrap().computed = lieui_layout::ComputedLayout {
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 40.0,
        overflow_scroll: false,
    };
    let pb = t.get(root).unwrap().paint_bounds();
    let r = t.get(root).unwrap().rect();
    assert_eq!((pb.x, pb.y, pb.width, pb.height), (r.x, r.y, r.width, r.height));
}

/// 回归（D58）：**本轮布局期间**新提的脏标不能被 `clear_layout_flags` 清掉。
///
/// bug 表现：`clear_layout_flags` 无条件清全树 MEASURE/ARRANGE。而布局过程中
/// （`write_back` 等）可能再次 `mark_layout_dirty`，那些标记属于**下一轮**的活儿，
/// 一起被清 ⇒ 漏失效 ⇒ 画面停在旧布局，且后续帧也不再重排（`has_layout_dirty` 恒假）。
#[test]
fn flags_raised_during_the_layout_round_survive_clear() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.get_mut(root).unwrap().layout.dim = [100.0, 40.0];
    let child = t.create(Kind::Box, None);
    t.append_child(root, child);

    // 第一轮：把已有脏标消费掉
    t.mark_layout_dirty(child);
    t.begin_layout_epoch();
    t.clear_layout_flags();
    assert!(!t.has_layout_dirty(), "第一轮之后应当是干净的");

    // 第二轮：**布局过程中**（模拟 write_back 触发的）又标脏
    t.begin_layout_epoch();
    t.mark_layout_dirty(child);
    t.clear_layout_flags();
    assert!(
        t.has_layout_dirty(),
        "D58：本轮期间提的脏标被 clear_layout_flags 清掉了 ⇒ 漏失效"
    );

    // 第三轮：它属于"本轮之前" ⇒ 应被正常消费
    t.begin_layout_epoch();
    t.clear_layout_flags();
    assert!(!t.has_layout_dirty(), "下一轮应把它消费掉");
}

/// 组件行为 API 的测试护栏（P3-a）。
///
/// ## 为什么补这一批
///
/// D52（组件行为归位）要搬走 `slider_drag_to` / `toggle_*` / `select_radio` /
/// `input_*` 共 15 个方法。搬移是**纯机械**的，**测试是唯一的正确性保证** ——
/// 而实测这 15 个方法此前**几乎零覆盖**（只有 `input_set_preedit` 有 1 处引用，
/// 其余全是定义处本身）。"先补护栏再搬移"是 D52 重做指南的第 1 条。
///
/// ## 重点覆盖三类易错点
///
/// 1. **UTF-8 char 边界**：`caret` 是**字节偏移**，而 `input_move_caret(delta)`
///    以**字符**为单位、`input_backspace` 删**整个字符**。中文/emoji 下按字节
///    算就会切出半个字符（Rust 会 panic：字节索引不是 char 边界）。
/// 2. **反向选区**：`caret` 与 `anchor` 谁大谁小都可能，`input_selection`
///    必须归一化成 `a <= b`。
/// 3. **写回绑定**：所有组件方法都要 `sig.set(..)`，否则模型与视图脱节。
mod component_behavior {
    use super::{Flags, Kind, NodeId, Track};
    use crate::reactive::{Runtime, Signal};

    fn input_node(t: &mut Track, text: &str, caret: usize) -> NodeId {
        t.create(
            Kind::Input {
                text: text.to_string(),
                placeholder: String::new(),
                caret,
                anchor: caret,
                preedit: String::new(),
                scroll: 0.0,
            },
            None,
        )
    }

    fn spanned_input(t: &mut Track, text: &str, caret: usize, anchor: usize) -> NodeId {
        t.create(
            Kind::Input {
                text: text.to_string(),
                placeholder: String::new(),
                caret,
                anchor,
                preedit: String::new(),
                scroll: 0.0,
            },
            None,
        )
    }

    fn input_text(t: &Track, id: NodeId) -> String {
        match t.get(id).map(|n| &n.kind) {
            Some(Kind::Input { text, .. }) => text.clone(),
            _ => panic!("不是 Input 节点"),
        }
    }

    fn caret_of(t: &Track, id: NodeId) -> (usize, usize) {
        match t.get(id).map(|n| &n.kind) {
            Some(Kind::Input { caret, anchor, .. }) => (*caret, *anchor),
            _ => panic!("不是 Input 节点"),
        }
    }

    // ── 1. UTF-8 边界 ────────────────────────────────────────────────

    /// ★ `input_move_caret` 以**字符**为单位移动，而 `caret` 是**字节偏移**。
    ///
    /// "中文字"共 3 个字符 = 9 字节。从字节 0 前移 1 次应到**字节 3**（"中"之后），
    /// 而不是字节 1（那会切在"中"的三字节中间）。
    #[test]
    fn move_caret_steps_by_char_not_by_byte() {
        let mut t = Track::new();
        let id = input_node(&mut t, "中文字", 0);

        assert!(t.input_move_caret(id, 1, false));
        assert_eq!(caret_of(&t, id).0, 3, "前移一个字符应到字节 3（'中'之后），不是 1");

        assert!(t.input_move_caret(id, 1, false));
        assert_eq!(caret_of(&t, id).0, 6, "再前移一个字符应到字节 6");

        assert!(t.input_move_caret(id, -1, false));
        assert_eq!(caret_of(&t, id).0, 3);
    }

    /// ★ 退格删**整个字符**（3 字节），不是 1 字节。
    #[test]
    fn backspace_removes_whole_char_not_one_byte() {
        let mut t = Track::new();
        let id = input_node(&mut t, "中文字", 9);

        assert!(t.input_backspace(id));
        assert_eq!(input_text(&t, id), "中文", "应删掉整个 '字'");

        assert!(t.input_backspace(id));
        assert_eq!(input_text(&t, id), "中");
    }

    /// ★ emoji 是 **4 字节**。退格删整个 emoji；光标在开头时退格返回 false
    /// 且**不得改动文本**。
    #[test]
    fn backspace_handles_four_byte_emoji() {
        let mut t = Track::new();
        let id = input_node(&mut t, "a😀b", 6); // a=1 + 😀=4 + b=1 => 6 字节

        assert!(t.input_backspace(id));
        assert_eq!(input_text(&t, id), "a😀");

        assert!(t.input_move_caret(id, -2, false));
        assert_eq!(caret_of(&t, id).0, 0, "应退到字节 0（'a' 之前）");
        assert!(!t.input_backspace(id), "开头处退格应返回 false");
        assert_eq!(input_text(&t, id), "a😀", "失败的退格不得改动文本");
    }

    /// `input_move_caret` 越过两端必须**夹住**而不是越界。
    #[test]
    fn move_caret_clamps_at_both_ends() {
        let mut t = Track::new();
        let id = input_node(&mut t, "abc", 0);

        assert!(!t.input_move_caret(id, -5, false), "已在开头，报告未变化");
        assert_eq!(caret_of(&t, id).0, 0, "不得为负");

        assert!(t.input_set_caret(id, 3, false));
        assert!(!t.input_move_caret(id, 99, false), "已在末尾，报告未变化");
        assert_eq!(caret_of(&t, id).0, 3, "不得越过末尾");
    }

    // ── 2. 反向选区 ─────────────────────────────────────────────────

    /// ★ `caret > anchor`（反向拖选）必须归一化成 `a <= b`。
    #[test]
    fn selection_is_normalized_when_caret_after_anchor() {
        let mut t = Track::new();
        let id = spanned_input(&mut t, "abcdef", 5, 1);
        assert_eq!(t.input_selection(id), Some((1, 5)), "反向选区应归一化");
        assert_eq!(
            t.input_selected_text(id).as_deref(),
            Some("bcde"),
            "取出的应是规范化后的区间"
        );
    }

    /// 无选区（`caret == anchor`）⇒ `None`，而不是 `(n, n)`。
    #[test]
    fn selection_is_none_when_caret_equals_anchor() {
        let mut t = Track::new();
        let id = input_node(&mut t, "abc", 2);
        assert_eq!(t.input_selection(id), None);
        assert_eq!(t.input_selected_text(id), None);
    }

    /// ★ `input_select_all` 应把整段文本圈成选区。
    #[test]
    fn select_all_covers_whole_text() {
        let mut t = Track::new();
        let id = input_node(&mut t, "中文abc", 0);
        assert!(t.input_select_all(id));
        assert_eq!(t.input_selection(id), Some((0, 9)), "应选中全部 9 字节");
        assert_eq!(t.input_selected_text(id).as_deref(), Some("中文abc"));
    }

    /// ★ 插入**替换选区**，且插入后选区收拢（anchor 也 = caret）。
    #[test]
    fn insert_replaces_selection() {
        let mut t = Track::new();
        let id = spanned_input(&mut t, "hello world", 11, 6);
        assert!(t.input_insert(id, "X"));
        assert_eq!(input_text(&t, id), "hello X");
        assert_eq!(caret_of(&t, id), (7, 7), "插入后选区应收拢（anchor 也 = caret）");
    }

    /// ★ 删除键删**光标后**一个字符（与退格相反方向）。
    #[test]
    fn delete_removes_char_after_caret() {
        let mut t = Track::new();
        let id = input_node(&mut t, "中文", 0);

        assert!(t.input_delete(id));
        // ★ "中文" 只有 2 个字符，删掉开头的 '中' 之后剩 "文"（不是 "文字"）。
        //   第一版我把期望写成 "文字"，测试失败才发现自己数错了字符。
        assert_eq!(input_text(&t, id), "文", "应删掉开头的 '中'");
        assert_eq!(caret_of(&t, id).0, 0, "删除不移动光标");
    }

    /// 末尾处删除 ⇒ 返回 false 且不改文本。
    #[test]
    fn delete_at_end_reports_no_change() {
        let mut t = Track::new();
        let id = input_node(&mut t, "中文", 6);
        assert!(!t.input_delete(id));
        assert_eq!(input_text(&t, id), "中文");
    }

    // ── 3. 绑定写回 ─────────────────────────────────────────────────

    /// ★ `toggle_checked` 必须写回 `checked` 绑定 —— 否则模型与视图脱节
    /// （下次 `view()` 会用模型值覆盖，用户点了没反应）。
    #[test]
    fn toggle_checked_writes_back_binding() {
        let mut t = Track::new();
        let rt = Runtime::new();
        let sig = Signal::new(&rt, false);
        let id = t.create(Kind::Checkbox { checked: false }, None);
        t.get_mut(id).unwrap().bindings.checked = Some(sig.clone());

        assert_eq!(t.toggle_checked(id), Some(true));
        sig.with(|v| assert!(*v, "signal 必须被写回为 true"));

        assert_eq!(t.toggle_checked(id), Some(false));
        sig.with(|v| assert!(!*v, "再次翻转应写回 false"));
    }

    /// `toggle_switch` 与 checkbox 同构，只是字段名不同（`on` vs `checked`）。
    #[test]
    fn toggle_switch_writes_back_binding() {
        let mut t = Track::new();
        let rt = Runtime::new();
        let sig = Signal::new(&rt, false);
        let id = t.create(Kind::Switch { on: false }, None);
        t.get_mut(id).unwrap().bindings.checked = Some(sig.clone());

        assert_eq!(t.toggle_switch(id), Some(true));
        sig.with(|v| assert!(*v));
    }

    /// ★ 类型不匹配时必须返回 `None` 且**不改状态**。
    #[test]
    fn toggle_on_wrong_kind_is_noop() {
        let mut t = Track::new();
        let id = t.create(Kind::Box, None);
        assert_eq!(t.toggle_checked(id), None, "Box 不是 Checkbox");
        assert_eq!(t.toggle_switch(id), None);
        assert_eq!(t.select_radio(id), None);
    }

    /// ★ `select_radio` 把本项 `value` 写回 `text` 绑定。
    ///
    /// 同组互斥**不由它做**（见其文档：信号变化 ⇒ `view()` 重跑 ⇒ 其它项
    /// 自然更新），所以这里只验证"写回本项 value"。
    #[test]
    fn select_radio_writes_back_its_value() {
        let mut t = Track::new();
        let rt = Runtime::new();
        let sig = Signal::new(&rt, String::from("none"));
        let id = t.create(
            Kind::Radio {
                selected: false,
                value: "wifi".into(),
            },
            None,
        );
        t.get_mut(id).unwrap().bindings.text = Some(sig.clone());

        assert_eq!(t.select_radio(id).as_deref(), Some("wifi"));
        sig.with(|v| assert_eq!(v.as_str(), "wifi"));
    }

    /// ★ `slider_drag_to`：x 映射到 `[min,max]`，**值没变时返回 false**。
    #[test]
    fn slider_drag_to_maps_x_and_reports_no_change() {
        let mut t = Track::new();
        let rt = Runtime::new();
        let sig = Signal::new(&rt, 0.0f32);
        let id = t.create(
            Kind::Slider {
                value: 0.0,
                min: 0.0,
                max: 100.0,
                dragging: false,
            },
            None,
        );
        t.get_mut(id).unwrap().bindings.value = Some(sig.clone());
        // ★ `Node::rect()` 读的是 `computed`，**不是** `layout.dim` ——
        //   只设 `layout.dim` 时宽度仍是 0，`t` 恒为 0，永远报告"未变化"。
        t.get_mut(id).unwrap().computed.width = 100.0;

        assert!(t.slider_drag_to(id, 50.0), "值应发生变化");
        sig.with(|v| assert_eq!(*v, 50.0));

        assert!(!t.slider_drag_to(id, 50.0), "同样的 x 应报告未变化");
    }

    /// 滑块 x 超出 `[0,width]` 必须**夹住**而不是越界。
    ///
    /// ★ 注意第一版把 `assert!(slider_drag_to(id, -50.0))` 写在值还是初始
    ///   `0.0` 的时候 —— x=-50 夹到 `t=0` ⇒ 值仍是 0 ⇒ **正确地**报告"未变化"，
    ///   测试却断言它会变。**必须先移到中间**，再测夹回边界。
    #[test]
    fn slider_drag_to_clamps_out_of_range_x() {
        let mut t = Track::new();
        let id = t.create(
            Kind::Slider {
                value: 0.0,
                min: 0.0,
                max: 100.0,
                dragging: false,
            },
            None,
        );
        t.get_mut(id).unwrap().computed.width = 100.0;

        // 先移到中间
        assert!(t.slider_drag_to(id, 50.0));

        // 越界向左 ⇒ 夹到最小值 0（**确实变化**：50 → 0）
        assert!(t.slider_drag_to(id, -50.0), "越界应夹到最小值");
        // 再一次同样的越界 ⇒ 已在最小值，报告未变化
        assert!(!t.slider_drag_to(id, -50.0), "已在最小值，应未变化");

        // 越界向右 ⇒ 夹到最大值 100
        assert!(t.slider_drag_to(id, 9999.0), "越界应夹到最大值");
        assert!(!t.slider_drag_to(id, 9999.0), "已在最大值，应未变化");
    }

    // ── 4. IME preedit ──────────────────────────────────────────────

    /// ★ `input_set_preedit` 存组合串；**提交首个字符时组合串被清空**
    /// （见 `input_insert`：组合串还没进 text，不能重复计入）。
    #[test]
    fn preedit_is_cleared_when_first_char_commits() {
        let mut t = Track::new();
        let id = input_node(&mut t, "", 0);

        assert!(t.input_set_preedit(id, "ni".into()));
        assert_eq!(t.input_preedit(id), Some("ni"), "组合串应暂存");

        assert!(t.input_insert(id, "你"));
        // ★ 实际契约：`input_insert` 里执行的是 `preedit.clear()`，所以这里
        //   得到 `Some("")` 而**不是** `None`。
        //   语义上等效（渲染侧判空即可），但与"从未设置过 preedit ⇒ None"
        //   **不一致** —— 调用方必须同时处理两者。这是实现与直觉的一处偏差，
        //   测试必须按**实际行为**写，否则就会逼着人去改一个并不存在的 bug。
        assert_eq!(
            t.input_preedit(id),
            Some(""),
            "提交后组合串必须被清空（清空后是 Some(\"\") 而非 None）"
        );
        assert_eq!(input_text(&t, id), "你");
    }

    /// 非 Input 节点上这些方法都必须安全返回（而不是 panic）。
    #[test]
    fn input_api_on_non_input_node_is_safe() {
        let mut t = Track::new();
        let id = t.create(Kind::Box, None);
        assert_eq!(t.input_selection(id), None);
        assert_eq!(t.input_selected_text(id), None);
        assert_eq!(t.input_preedit(id), None);
        assert!(!t.input_is_active(id));
        assert!(!t.input_insert(id, "x"));
        assert!(!t.input_backspace(id));
        assert!(!t.input_delete(id));
        assert!(!t.input_select_all(id));
        assert!(!t.input_move_caret(id, 1, false));
    }

    // ── 5. setter 的"标脏"契约（P3-b #1）────────────────────────────────

    /// 建一棵 `root -> (mid -> leaf, sibling)` 的树，三者都设好尺寸。
    fn dirty_tree() -> (Track, NodeId, NodeId, NodeId) {
        let mut t = Track::new();
        let root = t.create(Kind::Box, None);
        t.get_mut(root).unwrap().layout.dim = [200.0, 100.0];
        let mid = t.create(Kind::Box, None);
        t.get_mut(mid).unwrap().layout.dim = [100.0, 40.0];
        let leaf = t.create(Kind::Box, None);
        t.get_mut(leaf).unwrap().layout.dim = [50.0, 20.0];
        let sib = t.create(Kind::Box, None);
        t.get_mut(sib).unwrap().layout.dim = [50.0, 20.0];
        t.append_child(root, mid);
        t.append_child(mid, leaf);
        t.append_child(root, sib);
        t.get_mut(root).unwrap().flags = Flags::empty();
        t.get_mut(mid).unwrap().flags = Flags::empty();
        t.get_mut(leaf).unwrap().flags = Flags::empty();
        t.get_mut(sib).unwrap().flags = Flags::empty();
        (t, root, leaf, sib)
    }

    /// ★★ `set_visibility` 必须同时标 **flow（自己 + 父）与 paint**。
    ///
    /// 漏掉任何一半都会产生"看起来坏了"的症状：
    /// - 漏 flow ⇒ 兄弟不重排（`Collapsed` 让节点退出父流，占位要消失）
    /// - 漏 paint ⇒ 屏幕上留旧像素残影
    #[test]
    fn set_visibility_marks_flow_and_paint() {
        use crate::track::Visibility;
        let (mut t, _root, leaf, _sib) = dirty_tree();

        assert!(t.set_visibility(leaf, Visibility::Collapsed));

        let f = t.get(leaf).unwrap().flags;
        assert!(
            f.contains(Flags::MEASURE_DIRTY | Flags::ARRANGE_DIRTY),
            "自身必须标脏（退出父流）"
        );
        assert!(f.contains(Flags::PAINT_DIRTY), "必须标脏（旧像素要重画）");
        assert!(t.has_layout_dirty(), "Track 必须报告有待布局");
    }

    /// 父节点也必须被标脏 —— 否则兄弟不知道要重排。
    #[test]
    fn set_visibility_marks_parent_too() {
        use crate::track::Visibility;
        let (mut t, _root, leaf, _sib) = dirty_tree();
        assert!(t.set_visibility(leaf, Visibility::Collapsed));
        let parent = t.parent_of(leaf).unwrap();
        assert!(
            t.get(parent)
                .unwrap()
                .flags
                .contains(Flags::MEASURE_DIRTY | Flags::ARRANGE_DIRTY),
            "★ 父节点必须一起标脏，否则兄弟不重排"
        );
    }

    /// 值没变时**不标脏**（否则无谓的全窗重绘会打爆脏区优化）。
    #[test]
    fn set_visibility_no_change_marks_nothing() {
        use crate::track::Visibility;
        let (mut t, _root, leaf, _sib) = dirty_tree();
        assert!(
            !t.set_visibility(leaf, Visibility::Visible),
            "默认就是 Visible，不算变化"
        );
        assert!(
            !t.get(leaf).unwrap().flags.contains(Flags::PAINT_DIRTY),
            "未变化不得标脏"
        );
    }

    /// 不存在的节点 ⇒ 返回 false 且**不 panic**。
    #[test]
    fn set_visibility_on_missing_node_is_safe() {
        let mut t = Track::new();
        assert!(!t.set_visibility(crate::track::NodeId(9999), crate::track::Visibility::Hidden));
        assert!(!t.set_interaction_enabled(crate::track::NodeId(9999), false));
    }

    /// ★ `set_interaction_enabled` 必须标 paint（绘制侧有 4 处读它）。
    #[test]
    fn set_interaction_enabled_marks_paint() {
        let (mut t, _root, leaf, _sib) = dirty_tree();
        assert!(t.set_interaction_enabled(leaf, false));
        assert!(
            t.get(leaf).unwrap().flags.contains(Flags::PAINT_DIRTY),
            "★ 禁用态要立刻重绘（禁用前景色 / 光标闪烁都读它）"
        );
    }

    /// 值没变时不标脏。
    #[test]
    fn set_interaction_enabled_no_change_marks_nothing() {
        let (mut t, _root, leaf, _sib) = dirty_tree();
        assert!(!t.set_interaction_enabled(leaf, true), "默认就是 true");
        assert!(!t.get(leaf).unwrap().flags.contains(Flags::PAINT_DIRTY));
    }

    /// ★★ 交叉验证：setter 确实改了字段（否则上面几条全都不成立）。
    #[test]
    fn setters_actually_change_the_fields() {
        use crate::track::Visibility;
        let (mut t, _root, leaf, _sib) = dirty_tree();
        t.set_visibility(leaf, Visibility::Hidden);
        assert_eq!(t.get(leaf).unwrap().visibility, Visibility::Hidden);
        t.set_interaction_enabled(leaf, false);
        assert!(!t.get(leaf).unwrap().interaction.enabled);
    }
}
