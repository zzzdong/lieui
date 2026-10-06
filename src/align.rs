//! 对齐：`描述 → 保留树`（位置 + 一层 key），并**由 patch 精确登记脏区**。
//!
//! 这是全框架**唯一需要"匹配"的地方**，刻意做得极小（`docs/architecture-v3.md` §3.4）：
//!
//! | 规则 | 实现 |
//! |---|---|
//! | **位置即身份** | 同一 `view()` 每次按相同顺序产出相同结构 ⇒ 按下标对齐天然稳定 |
//! | **一层 key** | 仅 `keyed_list` 内按下标 → key 匹配；不做跨父 `Move`、不做 `type_name` 回退匹配 |
//! | **类型不同即重建** | 位置上的 `KindTag` 变了 ⇒ 该节点换 `Kind`、子树销毁重建（宁可丢视图态，不可错配） |
//! | **未变化即零操作** | 逐字段比（`FlexStyle`/`PaintStyle`/`TextStyle`/`KindDesc`），相同则完全不碰该节点 |
//!
//! 脏区：**在 patch 发生的那一刻登记**（不是靠"签名哈希比较"猜），
//! 布局后新矩形由 M2 的布局阶段补登。
//!
//! 层根的匹配键 = `(layer, owner, 同组序号)`；父层消失 ⇒ 嵌套子层整棵销毁。

use crate::track::{
    Flags, Key, Kind, KindDesc, KindTag, Layer, NodeId, RootId, Track,
};
use crate::view::{DescNode, ViewBuf};

/// 一次对齐的统计（测试与调试用；也可用于判断"这一帧有没有干活"）
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct AlignStats {
    /// 新建的节点数（含子树）
    pub created: usize,
    /// 销毁的节点数（含子树）
    pub destroyed: usize,
    /// 被 patch 的节点数（内容/样式/属性有变化）
    pub patched: usize,
    /// 完全没动的节点数
    pub unchanged: usize,
    /// 新建的层根数
    pub roots_created: usize,
    /// 移除的层根数
    pub roots_removed: usize,
}

/// 把描述对齐到保留树（幂等：同一份描述重复对齐不产生任何操作）
pub fn align(track: &mut Track, view: &ViewBuf) -> AlignStats {
    let mut out = AlignStats::default();
    let mut map: Vec<RootId> = Vec::with_capacity(view.roots.len());
    let mut claimed: Vec<RootId> = Vec::new();

    for (i, dr) in view.roots.iter().enumerate() {
        let owner = dr.owner.map(|o| map[o as usize]);
        // 同 (layer, owner) 组内的声明序号
        let ordinal = view.roots[..i]
            .iter()
            .filter(|r| r.layer == dr.layer && r.owner == dr.owner)
            .count();
        let candidate = track
            .roots()
            .iter()
            .filter(|r| r.layer == dr.layer && r.owner == owner && !claimed.contains(&r.id))
            .map(|r| r.id)
            .nth(ordinal);

        match candidate {
            Some(rid) => {
                let node = track.root(rid).expect("根存在").node;
                // 层选项变化（backdrop / 阻断 / 锚点 / 焦点策略）
                let opts_changed = track.root(rid).expect("根存在").opts != dr.opts;
                if opts_changed {
                    if let Some(r) = track.root_mut(rid) {
                        r.opts = dr.opts.clone();
                    }
                    track.damage_whole_window();
                }
                // 根节点本身也可能因"类型不同"被重建
                let dd = view.node(dr.node);
                if tag_mismatch(track, node, dd) {
                    let fresh = create_subtree(track, view, dr.node, &mut out);
                    if let Some(r) = track.root_mut(rid) {
                        r.node = fresh;
                    }
                    out.destroyed += track.destroy(node);
                } else {
                    align_node(track, view, dr.node, node, &mut out);
                }
                claimed.push(rid);
                map.push(rid);
            }
            None => {
                let node = create_subtree(track, view, dr.node, &mut out);
                let rid = track.add_root(dr.layer, owner, node);
                // 新根也要带上描述里的层选项（锚点 / backdrop / 命中策略…）。
                // `add_root` 只给默认 opts —— 锚定 popup 曾在这里丢掉 anchor，
                // 导致"布局当普通根填满整窗、落位无从谈起"。
                if track.root(rid).map(|r| r.opts != dr.opts).unwrap_or(false)
                    && let Some(r) = track.root_mut(rid)
                {
                    r.opts = dr.opts.clone();
                }
                // 层根标签：布局后可按标签找回这个层（框架的 loading 遮罩靠它定位）
                if dr.tag.is_some() && let Some(r) = track.root_mut(rid) {
                    r.tag = dr.tag;
                }
                out.roots_created += 1;
                // ⚠ 必须一并登记为"已认领"，否则末尾的"清理未认领根"会立刻把它删掉
                claimed.push(rid);
                map.push(rid);
            }
        }
    }

    // 描述里没有的层根 → 消失（连同嵌套子层）。框架自管层（tooltip 等）跳过——
    // 它们的生命周期归框架，不归 view()。
    let stale: Vec<RootId> = track
        .roots()
        .iter()
        .filter(|r| !r.framework)
        .map(|r| r.id)
        .filter(|id| !claimed.contains(id))
        .collect();
    for rid in stale {
        // 返回值含"一并被级联移除的嵌套子层"
        out.roots_removed += track.remove_root(rid);
    }

    out
}

/// 按描述新建一棵子树（返回游离根，由调用方挂载）
fn create_subtree(track: &mut Track, view: &ViewBuf, idx: u32, out: &mut AlignStats) -> NodeId {
    let d = view.node(idx);
    let id = track.create(Kind::from_desc(&d.kind), d.key.clone());
    write_all(track, d, id);

    for &ci in &d.children {
        let child = create_subtree(track, view, ci, out);
        track.append_child(id, child);
    }

    out.created += 1;
    id
}

/// 全新节点：把描述里的样式/属性/handlers 全量写入（新节点本来就全脏）
fn write_all(track: &mut Track, d: &DescNode, id: NodeId) {
    if let Some(n) = track.get_mut(id) {
        n.layout = d.layout.clone();
        n.paint = d.paint.clone();
        n.text = d.text.clone();
        n.image = d.image;
        n.visibility = d.visibility;
        n.hit_test_visible = d.hit_test_visible;
        n.clip = d.clip;
        n.transform = d.transform;
        n.tab_stop = d.tab_stop;
        n.tab_index = d.tab_index;
        n.tooltip = d.tooltip.clone();
        n.key = d.key.clone();
        n.handlers = d.handlers.clone();
        n.bindings = d.bindings.clone();
        n.interaction.enabled = d.enabled;
    }
}

/// 该位置上的组件类型是否与描述不同（不同 ⇒ 调用方走"重建"路径）
fn tag_mismatch(track: &Track, id: NodeId, d: &DescNode) -> bool {
    track
        .get(id)
        .map(|n| n.kind.tag() != d.kind.tag())
        .unwrap_or(true)
}

/// 对齐单个节点（**调用方已保证类型一致**）；
///
/// "类型不同即重建"由调用方处理（见 [`tag_mismatch`]）：因为重建需要知道
/// 该节点在父节点 `children` 里的位置，只有调用方手里有。
fn align_node(track: &mut Track, view: &ViewBuf, d_idx: u32, id: NodeId, out: &mut AlignStats) {
    let d = view.node(d_idx);
    debug_assert!(
        !tag_mismatch(track, id, d),
        "align_node 只处理类型一致的节点"
    );

    let mut size_changed = false;
    let mut layout_changed = false;
    // FlexStyle / 可见性变化 ⇒ 节点在父的**流**里变了，兄弟要重排（`mark_flow_dirty`）
    let mut flow_changed = false;
    let mut paint_changed = false;

    if let Some(n) = track.get_mut(id) {
        // ② 组件描述（desc 组；state 组保留）
        if d.kind.apply_to(&mut n.kind) {
            size_changed = true;
        }

        // ② 样式（逐项比）
        if n.layout != d.layout {
            n.layout = d.layout.clone();
            flow_changed = true;
        }
        if n.text != d.text {
            n.text = d.text.clone();
            layout_changed = true;
            paint_changed = true;
        }
        if n.paint != d.paint {
            n.paint = d.paint.clone();
            paint_changed = true;
        }
        if n.image != d.image {
            n.image = d.image;
            paint_changed = true;
        }

        // ③ 可视 / 命中 / 变换
        if n.visibility != d.visibility {
            n.visibility = d.visibility;
            flow_changed = true;
            paint_changed = true;
        }
        if n.hit_test_visible != d.hit_test_visible {
            n.hit_test_visible = d.hit_test_visible;
        }
        if n.clip != d.clip {
            n.clip = d.clip;
            paint_changed = true;
        }
        if n.transform != d.transform {
            n.transform = d.transform;
            paint_changed = true;
        }
        if n.tab_stop != d.tab_stop {
            n.tab_stop = d.tab_stop;
        }
        if n.tab_index != d.tab_index {
            n.tab_index = d.tab_index;
        }
        if n.key != d.key {
            n.key = d.key.clone();
        }
        // tooltip 不影响布局/绘制，只更新框架会话读取的内容（变化的 tooltip
        // 若正开着，由 tooltip 会话下一帧自然刷新内容——层根节点是框架建的，
        // 这里管不到，见 WindowCtx 的 tooltip 逻辑）
        if n.tooltip != d.tooltip {
            n.tooltip = d.tooltip.clone();
        }
        if n.interaction.enabled != d.enabled {
            n.interaction.enabled = d.enabled;
            paint_changed = true;
        }

        // ④ handlers / bindings：整体替换（闭包每次 view() 都是新的），**不置脏**
        n.handlers = d.handlers.clone();
        n.bindings = d.bindings.clone();
    }

    if size_changed || layout_changed {
        // 自身内容变了：只有"尺寸未确定"的节点才会把影响传给祖先（边界规则）
        track.mark_layout_dirty(id);
    }
    if flow_changed {
        // 在父的流里变了：兄弟位置随之变化
        track.mark_flow_dirty(id);
    }
    if size_changed || flow_changed || paint_changed {
        track.mark_paint_dirty(id);
    }
    if size_changed || layout_changed || flow_changed || paint_changed {
        out.patched += 1;
    } else {
        out.unchanged += 1;
    }

    // ⑤ 子节点
    if d.child_keys.iter().any(|k| k.is_some()) {
        align_keyed_children(track, view, d, id, out);
    } else {
        align_positional_children(track, view, d, id, out);
    }
}

fn align_positional_children(
    track: &mut Track,
    view: &ViewBuf,
    d: &DescNode,
    id: NodeId,
    out: &mut AlignStats,
) {
    let existing: Vec<NodeId> = track.children(id).to_vec();
    let n = d.children.len().max(existing.len());
    for i in 0..n {
        match (d.children.get(i), existing.get(i)) {
            (Some(&ci), Some(cid)) => {
                let old = *cid;
                if tag_mismatch(track, old, view.node(ci)) {
                    // 先建后销毁：位置信息（下标 i）只在这里有效
                    let fresh = create_subtree(track, view, ci, out);
                    track.replace_child_at(id, i, fresh);
                    out.destroyed += track.destroy(old);
                } else {
                    align_node(track, view, ci, old, out);
                }
            }
            (Some(&ci), None) => {
                let c = create_subtree(track, view, ci, out);
                track.append_child(id, c);
            }
            (None, Some(cid)) => {
                out.destroyed += track.destroy(*cid);
            }
            (None, None) => unreachable!(),
        }
    }
}

fn align_keyed_children(
    track: &mut Track,
    view: &ViewBuf,
    d: &DescNode,
    id: NodeId,
    out: &mut AlignStats,
) {
    // 现有子节点按 key 建池
    let mut pool: Vec<(Option<Key>, NodeId)> = track
        .children(id)
        .iter()
        .map(|c| (track.get(*c).and_then(|n| n.key.clone()), *c))
        .collect();

    let mut new_order: Vec<NodeId> = Vec::with_capacity(d.children.len());
    for (i, &ci) in d.children.iter().enumerate() {
        let want = d.child_keys.get(i).cloned().flatten();
        let reused = want.as_ref().and_then(|k| {
            pool.iter()
                .position(|(pk, _)| pk.as_ref() == Some(k))
                .map(|pos| pool.remove(pos).1)
        });

        let child = match reused {
            Some(c) => {
                if tag_mismatch(track, c, view.node(ci)) {
                    let fresh = create_subtree(track, view, ci, out);
                    track.append_child(id, fresh);
                    out.destroyed += track.destroy(c);
                    fresh
                } else {
                    align_node(track, view, ci, c, out);
                    c
                }
            }
            None => {
                let c = create_subtree(track, view, ci, out);
                track.append_child(id, c);
                c
            }
        };
        new_order.push(child);
    }

    // 未被认领的 → 该 key 消失了
    for (_, c) in pool {
        out.destroyed += track.destroy(c);
    }

    if track.children(id) != new_order.as_slice() {
        track.set_children(id, &new_order);
    }
}

/// 便捷查询：节点是否"需要重排"（M1 的测试与 M2 布局阶段用）
pub fn needs_layout(track: &Track, id: NodeId) -> bool {
    track
        .get(id)
        .map(|n| {
            n.flags.contains(Flags::MEASURE_DIRTY) || n.flags.contains(Flags::ARRANGE_DIRTY)
        })
        .unwrap_or(false)
}

/// 便捷查询：节点标签（测试断言用）
pub fn tag_of(track: &Track, id: NodeId) -> Option<KindTag> {
    track.get(id).map(|n| n.kind.tag())
}

/// 便捷查询：`KindDesc` 的标签（测试用）
pub fn desc_tag(desc: &KindDesc) -> KindTag {
    desc.tag()
}

/// 便捷查询：某层当前有几个根
pub fn root_count(track: &Track, layer: Layer) -> usize {
    track.roots_of(layer).count()
}

#[cfg(test)]
mod tests {
    use super::*;
   use crate::track::{LayerOpts, Placement, Visibility};
    use crate::view::ViewBuf;
    use lieui_geom::{Color, Rect};

    /// 造一个 counter 风格的描述
    fn counter_view(text: &str) -> ViewBuf {
       let mut v = ViewBuf::new();
        v.begin();
        v.column(|c| {
            c.gap(12.0);
            c.text("Counter").font_size(48.0);
            c.text(text).font_size(72.0).color(Color::RED);
            c.row(|r| {
                r.gap(4.0);
                r.button("-1").on_tap(|| {});
                r.button("+1").on_tap(|| {});
            });
        });
        v
    }

    fn find_text(track: &Track, id: NodeId, needle: &str) -> Option<NodeId> {
        track.descendants(id).into_iter().find(|n| {
            matches!(track.get(*n).map(|x| &x.kind), Some(Kind::Text(s)) if s == needle)
        })
    }

    fn content_node(track: &Track) -> NodeId {
        track.content_root().expect("有内容根").node
    }

    #[test]
    fn first_align_builds_the_whole_tree() {
       let v = counter_view("0");
        let mut t = Track::new();

        let st = align(&mut t, &v);
        // column(1) + text(2) + row(1) + button(2) = 6 个节点
        assert_eq!(st.created, 6);
        assert_eq!(t.len(), 6);
        assert_eq!(st.destroyed, 0);
        assert_eq!(st.roots_created, 1);
        assert!(t.damage_all, "首个内容根挂载应整窗脏");

        let root = content_node(&t);
        assert_eq!(t.children(root).len(), 3);
        assert!(find_text(&t, root, "Counter").is_some());
        assert!(find_text(&t, root, "0").is_some());
    }

    #[test]
    fn identical_desc_is_a_no_op() {
       let v = counter_view("0");
        let mut t = Track::new();
        align(&mut t, &v);
        let _ = t.take_damage();

        let st = align(&mut t, &v);
        assert_eq!(st.created, 0);
        assert_eq!(st.destroyed, 0);
        assert_eq!(st.patched, 0, "未变化节点必须零操作");
        assert_eq!(st.roots_created, 0);
        assert_eq!(st.roots_removed, 0);
        assert_eq!(st.unchanged, 6);
        let (rects, all) = t.take_damage();
        assert!(rects.is_empty() && !all, "零变化不应产生脏区");
    }

    #[test]
    fn text_change_patches_one_node_and_keeps_ids() {
       let mut t = Track::new();
        let v1 = counter_view("0");
        align(&mut t, &v1);
        let _ = t.take_damage();

        let root = content_node(&t);
        let zero = find_text(&t, root, "0").unwrap();
        let title = find_text(&t, root, "Counter").unwrap();

        let v2 = counter_view("1");
        let st = align(&mut t, &v2);

        assert_eq!(st.patched, 1, "只有那一个文本节点被 patch");
        assert_eq!(st.created, 0);
        assert_eq!(st.destroyed, 0);
        // 节点身份不变
        assert!(matches!(t.get(zero).map(|n| &n.kind), Some(Kind::Text(s)) if s == "1"));
        assert_eq!(find_text(&t, root, "0"), None);
        assert!(t.contains(title), "其它节点不受影响");
        // 该节点被标脏
        assert!(
            t.get(zero)
                .unwrap()
                .flags
                .contains(Flags::MEASURE_DIRTY | Flags::PAINT_DIRTY)
        );
    }

    #[test]
    fn slider_value_update_preserves_dragging_state() {
       let mut t = Track::new();

        let mut v1 = ViewBuf::new();
        v1.begin();
        v1.column(|c| {
            c.slider(0.2);
        });
        align(&mut t, &v1);

        let root = content_node(&t);
        let sid = t.children(root)[0];
        if let Some(n) = t.get_mut(sid)
            && let Kind::Slider { dragging, .. } = &mut n.kind
        {
            *dragging = true; // 模拟"用户正在拖"
        }

        let mut v2 = ViewBuf::new();
        v2.begin();
        v2.column(|c| {
            c.slider(0.6);
        });
        let st = align(&mut t, &v2);

        assert_eq!(st.patched, 1);
        match &t.get(sid).unwrap().kind {
            Kind::Slider { value, dragging, .. } => {
                assert_eq!(*value, 0.6, "desc 被对齐");
                assert!(*dragging, "state 组必须跨帧保留");
            }
            _ => panic!("kind 不应被替换"),
        }
    }

    #[test]
    fn tag_change_rebuilds_that_position() {
       let mut t = Track::new();

        let mut v1 = ViewBuf::new();
        v1.begin();
        v1.column(|c| {
            c.text("a");
        });
        align(&mut t, &v1);
        let root = content_node(&t);
        let old = t.children(root)[0];

        // 同一位置换成 button
        let mut v2 = ViewBuf::new();
        v2.begin();
        v2.column(|c| {
            c.button("ok");
        });
        let st = align(&mut t, &v2);
        let new = t.children(root)[0];

        assert_eq!(st.destroyed, 1);
        assert_eq!(st.created, 1);
        assert_ne!(old, new, "类型变了应重建该位置");
        assert_eq!(tag_of(&t, new), Some(KindTag::Button));
    }

    #[test]
    fn keyed_list_reuses_nodes_on_reorder_and_destroys_missing() {
       let mut t = Track::new();

        let build = |ids: &[u64]| {
            let mut v = ViewBuf::new();
            v.begin();
            let items: Vec<u64> = ids.to_vec();
            v.column(|c| {
                c.keyed_list(items, |id| *id, |v, id| {
                    v.text(format!("row {id}"));
                });
            });
            v
        };

        let v1 = build(&[1, 2, 3]);
        align(&mut t, &v1);
        let root = content_node(&t);
        let before: Vec<NodeId> = t.children(root).to_vec();
        assert_eq!(before.len(), 3);

        // 重排 + 去掉 2 + 新增 4
        let v2 = build(&[3, 1, 4]);
        let st = align(&mut t, &v2);

        let after: Vec<NodeId> = t.children(root).to_vec();
        assert_eq!(after.len(), 3);
        assert_eq!(st.created, 1, "只新建 4");
        assert_eq!(st.destroyed, 1, "只销毁 2");
        // 1 与 3 的节点被复用（身份不变），顺序变成 [3, 1, 4]
        let key_of = |id: NodeId| t.get(id).and_then(|n| n.key.clone());
        assert_eq!(key_of(after[0]), Some(Key::U64(3)));
        assert_eq!(key_of(after[1]), Some(Key::U64(1)));
        assert_eq!(key_of(after[2]), Some(Key::U64(4)));
        assert_eq!(after[0], before[2], "3 复用原节点");
        assert_eq!(after[1], before[0], "1 复用原节点");
        assert_ne!(after[2], before[1]);
    }

    #[test]
    fn layers_appear_and_disappear_and_nesting_cascades() {
       let mut t = Track::new();

        let mut v1 = ViewBuf::new();
        v1.begin();
        v1.column(|c| { c.text("body"); });
        align(&mut t, &v1);
        assert_eq!(root_count(&t, Layer::Modal), 0);

        // 打开 modal，并在其内部声明 popup
        let mut v2 = ViewBuf::new();
        v2.begin();
        v2.column(|c| { c.text("body"); });
        v2.modal(|m| {
            m.text("确认？");
            m.popup_at("more", Placement::RightOf, |p| { p.text("sub"); });
        });
        let st = align(&mut t, &v2);
        assert_eq!(st.roots_created, 2);
        assert_eq!(root_count(&t, Layer::Modal), 1);
        assert_eq!(root_count(&t, Layer::Popup), 1);

        // 关闭 modal → 嵌套 popup 一起走
        let v3 = v1; // 回到只有 content
        let st = align(&mut t, &v3);
        assert_eq!(st.roots_removed, 2);
        assert_eq!(root_count(&t, Layer::Modal), 0);
        assert_eq!(root_count(&t, Layer::Popup), 0);
    }

    #[test]
    fn modal_appearance_marks_whole_window_dirty() {
       let mut t = Track::new();
        let mut v1 = ViewBuf::new();
        v1.begin();
        v1.column(|c| { c.text("body"); });
        align(&mut t, &v1);
        let _ = t.take_damage();

        let mut v2 = ViewBuf::new();
        v2.begin();
        v2.column(|c| { c.text("body"); });
        v2.modal(|m| { m.text("确认？"); });
        align(&mut t, &v2);

        let (_, all) = t.take_damage();
        assert!(all, "backdrop 覆盖全屏 → 必须整窗脏");
    }

    #[test]
    fn overlay_is_hit_test_transparent_by_default() {
       let mut t = Track::new();
        let mut v = ViewBuf::new();
        v.begin();
        v.column(|c| { c.text("body"); });
        v.overlay(|o| { o.text("WATERMARK"); });
        align(&mut t, &v);

        let ov = t.roots_of(Layer::Overlay).next().unwrap();
        assert!(!ov.opts.hit_test_visible);
        assert_eq!(ov.opts, LayerOpts::for_layer(Layer::Overlay));
    }

    #[test]
    fn layer_opts_change_marks_whole_window_dirty() {
       let mut t = Track::new();
        let mut v1 = ViewBuf::new();
        v1.begin();
        v1.column(|c| { c.text("body"); });
        v1.popup_at("k", Placement::Below, |p| { p.text("menu"); });
        align(&mut t, &v1);

        // 同一 popup 换锚点方位
        let mut v2 = ViewBuf::new();
        v2.begin();
        v2.column(|c| { c.text("body"); });
        v2.popup_at("k", Placement::Above, |p| { p.text("menu"); });
        align(&mut t, &v2);

        let (_, all) = t.take_damage();
        assert!(all, "锚点变化会影响遮挡/位置 → 整窗脏");
    }

    #[test]
    fn visibility_change_marks_layout_and_paint() {
       let mut t = Track::new();

        let mut v1 = ViewBuf::new();
        v1.begin();
        v1.column(|c| {
            c.text("a");
        });
        align(&mut t, &v1);
        let _ = t.take_damage();
        let root = content_node(&t);

        let mut v2 = ViewBuf::new();
        v2.begin();
        v2.column(|c| {
            c.text("a").collapsed();
        });
        let st = align(&mut t, &v2);

        assert_eq!(st.patched, 1);
        let id = t.children(root)[0];
        assert_eq!(t.get(id).unwrap().visibility, Visibility::Collapsed);
        let f = t.get(id).unwrap().flags;
        assert!(f.contains(Flags::MEASURE_DIRTY | Flags::PAINT_DIRTY));
    }

    #[test]
    fn damage_uses_old_rect_of_patched_node() {
       let mut t = Track::new();
        let mut v1 = ViewBuf::new();
        v1.begin();
        v1.column(|c| { c.text("0"); });
        align(&mut t, &v1);
        let root = content_node(&t);
        let id = t.children(root)[0];

        // 手工给一个"上一帧"的布局结果
        let mut cl = lieui_layout::ComputedLayout::default();
        cl.x = 5.0;
        cl.y = 6.0;
        cl.width = 7.0;
        cl.height = 8.0;
        t.get_mut(id).unwrap().computed = cl;
        let _ = t.take_damage();

        let mut v2 = ViewBuf::new();
        v2.begin();
        v2.column(|c| { c.text("1"); });
        align(&mut t, &v2);

        let (rects, all) = t.take_damage();
        assert!(!all);
        assert_eq!(rects, vec![Rect::new(5.0, 6.0, 7.0, 8.0)]);
    }

    #[test]
    fn desc_and_track_tags_are_comparable() {
        assert_eq!(desc_tag(&KindDesc::Box), KindTag::Box);
        assert_eq!(Kind::from_desc(&KindDesc::Slider { value: 1.0, min: 0.0, max: 1.0 }).tag(), KindTag::Slider);
    }

    #[test]
    fn needs_layout_reflects_flags() {
       let mut t = Track::new();
        let v = counter_view("0");
        align(&mut t, &v);
        let root = content_node(&t);
        assert!(needs_layout(&t, root));
    }
}
