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
    track
        .descendants(id)
        .into_iter()
        .find(|n| matches!(track.get(*n).map(|x| &x.kind), Some(Kind::Text(s)) if s == needle))
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
            c.keyed_list(
                items,
                |id| *id,
                |v, id| {
                    v.text(format!("row {id}"));
                },
            );
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
    v1.column(|c| {
        c.text("body");
    });
    align(&mut t, &v1);
    assert_eq!(root_count(&t, Layer::Modal), 0);

    // 打开 modal，并在其内部声明 popup
    let mut v2 = ViewBuf::new();
    v2.begin();
    v2.column(|c| {
        c.text("body");
    });
    v2.modal(|m| {
        m.text("确认？");
        m.popup_at("more", Placement::RightOf, |p| {
            p.text("sub");
        });
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
    v1.column(|c| {
        c.text("body");
    });
    align(&mut t, &v1);
    let _ = t.take_damage();

    let mut v2 = ViewBuf::new();
    v2.begin();
    v2.column(|c| {
        c.text("body");
    });
    v2.modal(|m| {
        m.text("确认？");
    });
    align(&mut t, &v2);

    let (_, all) = t.take_damage();
    assert!(all, "backdrop 覆盖全屏 → 必须整窗脏");
}

#[test]
fn overlay_is_hit_test_transparent_by_default() {
    let mut t = Track::new();
    let mut v = ViewBuf::new();
    v.begin();
    v.column(|c| {
        c.text("body");
    });
    v.overlay(|o| {
        o.text("WATERMARK");
    });
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
    v1.column(|c| {
        c.text("body");
    });
    v1.popup_at("k", Placement::Below, |p| {
        p.text("menu");
    });
    align(&mut t, &v1);

    // 同一 popup 换锚点方位
    let mut v2 = ViewBuf::new();
    v2.begin();
    v2.column(|c| {
        c.text("body");
    });
    v2.popup_at("k", Placement::Above, |p| {
        p.text("menu");
    });
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
    v1.column(|c| {
        c.text("0");
    });
    align(&mut t, &v1);
    let root = content_node(&t);
    let id = t.children(root)[0];

    // 手工给一个"上一帧"的布局结果
    let cl = lieui_layout::ComputedLayout {
        x: 5.0,
        y: 6.0,
        width: 7.0,
        height: 8.0,
        ..Default::default()
    };
    t.get_mut(id).unwrap().computed = cl;
    let _ = t.take_damage();

    let mut v2 = ViewBuf::new();
    v2.begin();
    v2.column(|c| {
        c.text("1");
    });
    align(&mut t, &v2);

    let (rects, all) = t.take_damage();
    assert!(!all);
    assert_eq!(rects, vec![Rect::new(5.0, 6.0, 7.0, 8.0)]);
}

#[test]
fn desc_and_track_tags_are_comparable() {
    assert_eq!(desc_tag(&KindDesc::Box), KindTag::Box);
    assert_eq!(
        Kind::from_desc(&KindDesc::Slider {
            value: 1.0,
            min: 0.0,
            max: 1.0
        })
        .tag(),
        KindTag::Slider
    );
}

#[test]
fn needs_layout_reflects_flags() {
    let mut t = Track::new();
    let v = counter_view("0");
    align(&mut t, &v);
    let root = content_node(&t);
    assert!(needs_layout(&t, root));
}
