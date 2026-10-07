use super::*;
use crate::track::{Anchor, Flags, Key, Kind, Layer};
use lieui_layout::FlexDirection;

fn text(t: &mut Track, s: &str, size: f64) -> NodeId {
    let id = t.create(Kind::Text(s.to_string()), None);
    t.get_mut(id).unwrap().text.spec.font_size = size;
    id
}

fn fixed(t: &mut Track, w: f32, h: f32) -> NodeId {
    let id = t.create(Kind::Box, None);
    t.get_mut(id).unwrap().layout.dim = [w, h];
    id
}

// ── 锚定层落位 ──

const WIN: Size = Size::new(300.0, 200.0);

/// 内容根 + (ax, ay) 处的锚点（100×30，key="anchor"）+ 锚定 popup（含一段文本）
fn anchored(ax: f32, ay: f32, placement: Placement) -> (Track, NodeId, NodeId) {
    let mut t = Track::new();
    let content = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, content);

    let anchor = fixed(&mut t, 100.0, 30.0);
    {
        let n = t.get_mut(anchor).unwrap();
        n.key = Some(Key::from("anchor"));
        n.layout = n.layout.clone().margin_left(ax).margin_top(ay);
    }
    t.append_child(content, anchor);

    let popup = t.create(Kind::Box, None);
    let txt = text(&mut t, "菜单项", 14.0);
    t.append_child(popup, txt);
    let rid = t.add_root(Layer::Popup, None, popup);
    t.root_mut(rid).unwrap().opts.anchor = Some(Anchor {
        target: crate::track::AnchorTarget::Key(Key::from("anchor")),
        placement,
    });
    (t, anchor, popup)
}

/// 布局 + 落位，返回 (锚点 rect, 层根 rect)
fn place(t: &mut Track) -> (Rect, Rect) {
    layout(t, WIN);
    place_anchored_layers(t, WIN);
    let anchor = t.find_by_key(&Key::from("anchor")).unwrap();
    let popup = t.roots().iter().find(|r| r.layer == Layer::Popup).unwrap().node;
    (t.get(anchor).unwrap().rect(), t.get(popup).unwrap().rect())
}

/// 内容根 + 一个**点锚点**的 popup（没有锚点节点 —— 点本身就是锚点）
fn point_anchored(px: f32, py: f32, placement: Placement) -> (Track, NodeId) {
    let mut t = Track::new();
    let content = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, content);

    let popup = t.create(Kind::Box, None);
    let txt = text(&mut t, "菜单项", 14.0);
    t.append_child(popup, txt);
    let rid = t.add_root(Layer::Popup, None, popup);
    t.root_mut(rid).unwrap().opts.anchor = Some(Anchor {
        target: crate::track::AnchorTarget::Point(Point::new(px, py)),
        placement,
    });
    (t, popup)
}

/// 布局 + 落位，返回层根 rect（点锚点没有"锚点 rect"可言）
fn place_point(t: &mut Track) -> Rect {
    layout(t, WIN);
    place_anchored_layers(t, WIN);
    let popup = t.roots().iter().find(|r| r.layer == Layer::Popup).unwrap().node;
    t.get(popup).unwrap().rect()
}

/// 点锚点：菜单贴在**鼠标点**下方一个 `ANCHOR_GAP`，而不是"某个节点的左边缘"
#[test]
fn point_anchor_puts_the_popup_under_the_cursor() {
    let (mut t, _) = point_anchored(120.0, 60.0, Placement::Below);
    let pr = place_point(&mut t);
    assert!((pr.x - 120.0).abs() < 0.5, "左上角与点横向对齐：{pr:?}");
    assert!(
        (pr.y - (60.0 + ANCHOR_GAP)).abs() < 0.5,
        "点在菜单上方一个 ANCHOR_GAP：{pr:?}"
    );
}

/// 点贴近下边缘 ⇒ 翻到点**上方**（与锚节点同一套翻转规则）
///
/// 断言精确落点（而不是"在窗口内"）：否则锚点被忽略时也会通过 —— 那条断言太弱，
/// 曾经让"点锚点退化成原点"的错误实现蒙混过关。
#[test]
fn point_anchor_flips_above_near_the_bottom() {
    let (mut t, _) = point_anchored(60.0, WIN.height - 4.0, Placement::Below);
    let pr = place_point(&mut t);
    let py = WIN.height - 4.0;
    assert!(
        (pr.bottom() - (py - ANCHOR_GAP)).abs() < 0.5,
        "应翻到点上方一个 GAP：{pr:?}（点 y={py}）"
    );
    assert!(pr.y >= 0.0 && pr.bottom() <= WIN.height, "不出视口：{pr:?}");
}

/// 点贴近右边缘 ⇒ 平移回视口内、**右边缘正好贴住窗口右边缘**（`Fixed` 做不到这件事）
#[test]
fn point_anchor_is_pulled_back_inside_the_window() {
    let (mut t, _) = point_anchored(WIN.width - 2.0, 40.0, Placement::Below);
    let pr = place_point(&mut t);
    assert!((pr.x - (WIN.width - pr.width)).abs() < 0.5, "被平移到右边缘：{pr:?}");
    assert!((pr.y - 44.0).abs() < 0.5, "纵向不受影响：{pr:?}");
}

/// 点锚点**不需要任何节点存在** ⇒ 锚在虚拟列表行上也安全
/// （行被回收 / 滚出窗口时，`Key` 锚点会失效并让菜单停在原位，点锚点不受影响）
#[test]
fn point_anchor_needs_no_node() {
    let (mut t, _) = point_anchored(30.0, 30.0, Placement::Below);
    assert_eq!(t.roots().len(), 2, "只有内容根 + 弹层根");
    let pr = place_point(&mut t);
    assert!((pr.y - (30.0 + ANCHOR_GAP)).abs() < 0.5, "{pr:?}");
}

/// 点锚点也能用 `RightOf`（贴着点的右侧），确认不是只对 `Below` 生效
#[test]
fn point_anchor_honors_other_placements() {
    let (mut t, _) = point_anchored(100.0, 100.0, Placement::RightOf);
    let pr = place_point(&mut t);
    assert!(
        (pr.x - (100.0 + ANCHOR_GAP)).abs() < 0.5,
        "RightOf：点在菜单左侧一个 GAP：{pr:?}"
    );
    assert!((pr.y - 100.0).abs() < 0.5, "纵向与点对齐：{pr:?}");
}

#[test]
fn popup_places_below_the_anchor_with_a_gap() {
    let (mut t, _, popup) = anchored(50.0, 40.0, Placement::Below);
    let (ar, pr) = place(&mut t);
    assert_eq!(pr.x, ar.x, "Below 与锚点左对齐");
    assert!((pr.y - (ar.bottom() + ANCHOR_GAP)).abs() < 0.5, "{ar:?} -> {pr:?}");
    let _ = popup;
}

#[test]
fn popup_shrink_wraps_instead_of_filling_the_window() {
    let (mut t, _, _) = anchored(50.0, 40.0, Placement::Below);
    let (_, pr) = place(&mut t);
    assert!(pr.width < WIN.width && pr.height < WIN.height, "内容自适应：{pr:?}");
}

#[test]
fn popup_flips_above_when_below_overflows() {
    // 锚点贴底：下方放不下（190+4+高 > 200），上方放得下 ⇒ 翻到 Above
    let (mut t, ar_id, _) = anchored(20.0, 160.0, Placement::Below);
    let (_, pr) = place(&mut t);
    let ar = t.get(ar_id).unwrap().rect();
    assert!(pr.bottom() <= ar.y + 0.5, "层应在锚点上方：{ar:?} -> {pr:?}");
    assert!(pr.y >= 0.0);
}

#[test]
fn popup_clamps_inside_the_window_when_neither_side_fits() {
    // 层高 190：下方 194+190 放不下，上方 160-4-190 < 0 也不行 ⇒ 钳到底边
    let (mut t, _, popup) = anchored(20.0, 160.0, Placement::Below);
    t.get_mut(popup).unwrap().layout.dim = [80.0, 190.0];
    let (_, pr) = place(&mut t);
    assert!((pr.y - (WIN.height - pr.height)).abs() < 0.5, "钳到视口内：{pr:?}");
    assert!(pr.y >= 0.0 && pr.bottom() <= WIN.height);
}

#[test]
fn right_of_flips_to_left_of_near_the_right_edge() {
    // 锚点右缘 280 + 4 + 层宽(~42) > 300 ⇒ 翻到左侧
    let (mut t, ar_id, _) = anchored(180.0, 20.0, Placement::RightOf);
    let (_, pr) = place(&mut t);
    let ar = t.get(ar_id).unwrap().rect();
    assert!(pr.right() <= ar.x + 0.5, "层应在锚点左侧：{ar:?} -> {pr:?}");
}

#[test]
fn screen_center_centers_the_layer() {
    let (mut t, _, _) = anchored(50.0, 40.0, Placement::ScreenCenter);
    let (_, pr) = place(&mut t);
    assert!((pr.x - (WIN.width - pr.width) * 0.5).abs() < 0.5, "{pr:?}");
    assert!((pr.y - (WIN.height - pr.height) * 0.5).abs() < 0.5, "{pr:?}");
}

#[test]
fn missing_anchor_key_keeps_the_layer_in_place() {
    let (mut t, _, popup) = anchored(50.0, 40.0, Placement::Below);
    // 改掉锚点 key ⇒ 解析失败 ⇒ 不动
    if let Some(n) = t.get_mut(t.find_by_key(&Key::from("anchor")).unwrap()) {
        n.key = Some(Key::from("other"));
    }
    layout(&mut t, WIN);
    let before = t.get(popup).unwrap().rect();
    let moved = place_anchored_layers(&mut t, WIN);
    assert_eq!(moved, 0);
    assert_eq!(t.get(popup).unwrap().rect(), before);
}

#[test]
fn moving_the_layer_registers_old_and_new_damage() {
    let (mut t, _, popup) = anchored(50.0, 40.0, Placement::Below);
    layout(&mut t, WIN);
    let _ = t.take_damage(); // 清掉布局阶段的脏区
    let moved = place_anchored_layers(&mut t, WIN);
    assert_eq!(moved, 1);
    let (rects, all) = t.take_damage();
    assert!(!all);
    assert!(rects.len() >= 2, "旧 ∪ 新位置都要重绘：{rects:?}");
    let pr = t.get(popup).unwrap().rect();
    assert!(rects.iter().any(|r| r.intersects(&pr)), "新位置在脏区里");
}

#[test]
fn fixed_placement_is_taken_literally() {
    let (mut t, _, _) = anchored(50.0, 40.0, Placement::Fixed { x: 123.0, y: 77.0 });
    let (_, pr) = place(&mut t);
    assert!((pr.x - 123.0).abs() < 0.5 && (pr.y - 77.0).abs() < 0.5, "{pr:?}");
}

#[test]
fn column_with_padding_and_gap_positions_children() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    {
        let n = t.get_mut(root).unwrap();
        n.layout = n.layout.clone().padding_all(10.0).gap(5.0);
        n.layout.flex_direction = FlexDirection::Column;
    }
    t.add_root(Layer::Content, None, root);

    let a = text(&mut t, "hello", 16.0);
    let b = text(&mut t, "world", 16.0);
    t.append_child(root, a);
    t.append_child(root, b);

    layout(&mut t, Size::new(300.0, 200.0));

    assert_eq!(rect_of(&t, root), Rect::new(0.0, 0.0, 300.0, 200.0), "自适应根填满窗口");
    assert_eq!(rect_of(&t, a).x, 10.0, "padding 生效");
    assert_eq!(rect_of(&t, a).y, 10.0);
    assert_eq!(rect_of(&t, b).y, rect_of(&t, a).height + 10.0 + 5.0, "gap 生效");
    // 文本叶子的 DesiredSize = 测度尺寸
    assert!(t.get(a).unwrap().desired.width > 0.0);
    assert!(t.get(a).unwrap().desired.height > 0.0);
}

#[test]
fn row_children_share_width_by_flex_grow() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.get_mut(root).unwrap().layout.flex_direction = FlexDirection::Row;
    t.get_mut(root).unwrap().layout.dim = [300.0, 100.0];
    t.add_root(Layer::Content, None, root);

    let a = fixed(&mut t, 100.0, 100.0);
    let b = t.create(Kind::Box, None);
    t.get_mut(b).unwrap().layout.flex_grow = 1.0;
    t.append_child(root, a);
    t.append_child(root, b);

    layout(&mut t, Size::new(300.0, 100.0));
    assert_eq!(rect_of(&t, a), Rect::new(0.0, 0.0, 100.0, 100.0));
    assert_eq!(rect_of(&t, b), Rect::new(100.0, 0.0, 200.0, 100.0), "吃掉剩余宽");
}

/// **测度与绘制口径一致**：窄容器里的长文本，测度按约束换行 ⇒ 盒子是两行高，
/// 绘制必须用同一约束重排（否则只画一行、贴在盒子顶部 ⇒ 看起来顶对齐）。
/// 这里用**像素**验证：画出的墨迹带覆盖盒子的绝大部分高度。
#[test]
fn wrapped_text_draws_with_the_same_wrap_width_as_measure() {
    use vello_cpu::color::PremulRgba8;

    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    {
        let n = t.get_mut(root).unwrap();
        n.layout.flex_direction = FlexDirection::Column;
        n.layout.dim = [120.0, 120.0];
    }
    t.add_root(Layer::Content, None, root);

    let text = t.create(
        Kind::Text("这是一段足够长的中文文本，用来验证换行测度与绘制一致".into()),
        None,
    );
    {
        let n = t.get_mut(text).unwrap();
        n.text.spec.font_size = 14.0;
        n.text.color = lieui_geom::Color::BLACK;
    }
    t.append_child(root, text);

    layout(&mut t, Size::new(120.0, 120.0));
    let rect = rect_of(&t, text);
    assert!(rect.height > 30.0, "应当换行成多行：h={}", rect.height);

    let mut r = crate::render::Renderer::new(Size::new(120.0, 120.0), lieui_geom::Color::WHITE);
    r.render(&t, &[], true);
    let pix = r.pixmap();
    let pw = usize::from(pix.width());
    let (mut top, mut bottom) = (usize::MAX, 0usize);
    for y in 0..120usize {
        for x in 0..120usize {
            let p: PremulRgba8 = pix.data()[y * pw + x];
            if p.r < 240 || p.g < 240 || p.b < 240 {
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
    }
    let drawn = (bottom - top + 1) as f32;
    assert!(
        drawn > rect.height * 0.6,
        "画出的墨迹带要覆盖盒子的大部分（不再是只画第一行）：\
             墨迹 {top}..{bottom}（{drawn}px） vs 盒子高 {}",
        rect.height
    );
}

/// 光学对齐：`spec.optical_align` 让文本节点按**墨迹高度**参与布局，
/// 于是 `align_items(Center)` 居中的是墨迹盒（视觉中心）而不是行盒。
#[test]
fn optical_align_changes_the_measured_height() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    {
        let n = t.get_mut(root).unwrap();
        n.layout.dim = [300.0, 100.0];
        n.layout.flex_direction = FlexDirection::Row;
        n.layout.align_items = lieui_layout::FlexAlign::Center;
    }
    t.add_root(Layer::Content, None, root);

    let plain = t.create(Kind::Text("abc".into()), None);
    let optical = t.create(Kind::Text("abc".into()), None);
    t.get_mut(plain).unwrap().text.spec.font_size = 13.0;
    {
        let n = t.get_mut(optical).unwrap();
        n.text.spec.font_size = 13.0;
        n.text.spec.optical_align = true;
    }
    t.append_child(root, plain);
    t.append_child(root, optical);
    layout(&mut t, Size::new(300.0, 100.0));

    let h_plain = rect_of(&t, plain).height;
    let h_optical = rect_of(&t, optical).height;
    assert!(
        h_optical < h_plain,
        "光学高度 = 墨迹高 < 行盒高：{h_optical} < {h_plain}"
    );
    // 两者都居中（盒中心都落在行中心）
    assert!((rect_of(&t, plain).center().y - 50.0).abs() < 0.01);
    assert!((rect_of(&t, optical).center().y - 50.0).abs() < 0.01);
}

#[test]
fn fixed_size_subtree_is_a_boundary() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);

    let card = fixed(&mut t, 300.0, 200.0);
    t.append_child(root, card);
    let label = text(&mut t, "hi", 16.0);
    t.append_child(card, label);

    let st = layout(&mut t, Size::new(500.0, 400.0));
    assert_eq!(st.boundaries, 1, "只有内容根是边界");
    assert_eq!(rect_of(&t, card), Rect::new(0.0, 0.0, 300.0, 200.0));

    // 文本被拉伸到 card 的宽度 ⇒ 宽度不是好信号，用 DesiredSize（无限宽测度）观察重新测度
    let old_desired = t.get(label).unwrap().desired.width;

    // 改文本：label 没有确定尺寸，脏标记冒泡到**card**（尺寸确定）为止
    t.get_mut(label).unwrap().kind = Kind::Text("hi there, this is a much longer text".into());
    t.mark_layout_dirty(label);

    let st2 = layout(&mut t, Size::new(500.0, 400.0));
    assert_eq!(st2.boundaries, 1, "边界收敛到 card");
    assert_eq!(st2.nodes, 2, "只重建了 card 子树（不含 root）");
    assert_eq!(rect_of(&t, card), Rect::new(0.0, 0.0, 300.0, 200.0), "card 尺寸不变");
    assert!(t.get(label).unwrap().desired.width > old_desired, "文本被重新测度");
}

#[test]
fn window_resize_marks_all_roots_as_boundaries() {
    let mut t = Track::new();
    let content = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, content);

    let modal = t.create(Kind::Box, None);
    t.add_root(Layer::Modal, None, modal);

    layout(&mut t, Size::new(300.0, 100.0));
    assert_eq!(rect_of(&t, content), Rect::new(0.0, 0.0, 300.0, 100.0));

    t.mark_all_layout_dirty();
    let st = layout(&mut t, Size::new(400.0, 200.0));
    assert_eq!(st.boundaries, 2, "两个层根");
    assert_eq!(rect_of(&t, content), Rect::new(0.0, 0.0, 400.0, 200.0));
    assert_eq!(rect_of(&t, modal), Rect::new(0.0, 0.0, 400.0, 200.0));
}

#[test]
fn collapsed_children_are_excluded_from_the_flex_tree() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.get_mut(root).unwrap().layout.flex_direction = FlexDirection::Row;
    t.get_mut(root).unwrap().layout.dim = [300.0, 100.0];
    t.add_root(Layer::Content, None, root);

    let a = fixed(&mut t, 100.0, 100.0);
    let b = fixed(&mut t, 100.0, 100.0);
    let c = fixed(&mut t, 100.0, 100.0);
    for k in [a, b, c] {
        t.append_child(root, k);
    }
    layout(&mut t, Size::new(300.0, 100.0));
    assert_eq!(rect_of(&t, b).x, 100.0);

    // 收起 b（走 Cmd 路径，会触发 flow dirty）
    crate::cmd::apply_cmds(
        &mut t,
        &[crate::cmd::Cmd::SetVisibility {
            id: b,
            visibility: crate::track::Visibility::Collapsed,
        }],
    );
    let st = layout(&mut t, Size::new(300.0, 100.0));
    assert!(st.ran);
    assert_eq!(rect_of(&t, b).x, 100.0, "收起节点不重排（保留旧 rect）");
    assert_eq!(rect_of(&t, c).x, 100.0, "兄弟补位到 b 的位置");
}

#[test]
fn scroll_container_reports_content_size_and_shifts_children() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    {
        let n = t.get_mut(root).unwrap();
        n.layout.dim = [100.0, 100.0];
        n.layout.overflow_scroll = true;
    }
    t.add_root(Layer::Content, None, root);

    let child = fixed(&mut t, 80.0, 300.0);
    t.append_child(root, child);

    layout(&mut t, Size::new(100.0, 100.0));
    assert_eq!(t.get(root).unwrap().content_size, Size::new(80.0, 300.0));
    assert_eq!(rect_of(&t, child), Rect::new(0.0, 0.0, 80.0, 300.0));

    // 滚动偏移：子节点整体上移（布局不重算，只平移）
    t.set_scroll_offset(root, (0.0, 50.0));
    t.mark_layout_dirty(root);
    layout(&mut t, Size::new(100.0, 100.0));
    assert_eq!(rect_of(&t, child).y, -50.0);

    // 越界偏移被钳制到内容尺寸
    t.set_scroll_offset(root, (0.0, 1000.0));
    t.mark_layout_dirty(root);
    let st = layout(&mut t, Size::new(100.0, 100.0));
    assert_eq!(st.clamped_scroll, 1);
    assert_eq!(t.scroll_offset(root), (0.0, 200.0));
    assert_eq!(rect_of(&t, child).y, -200.0);
}

#[test]
fn scroll_children_do_not_shrink_to_the_viewport() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    {
        let n = t.get_mut(root).unwrap();
        n.layout.dim = [100.0, 100.0];
        n.layout.flex_direction = FlexDirection::Column;
        n.layout.overflow_scroll = true;
    }
    t.add_root(Layer::Content, None, root);

    // 内容 300 高，若无"禁止收缩"处理会被压到视口高度
    let child = fixed(&mut t, 100.0, 300.0);
    t.append_child(root, child);

    layout(&mut t, Size::new(100.0, 100.0));
    assert_eq!(rect_of(&t, child).height, 300.0, "滚动内容保持自然高度");
    assert_eq!(t.get(root).unwrap().content_size.height, 300.0);
}

#[test]
fn layout_is_a_no_op_when_nothing_is_dirty() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    layout(&mut t, Size::new(300.0, 100.0));

    let st = layout(&mut t, Size::new(300.0, 100.0));
    assert!(!st.ran);
    assert_eq!(st.boundaries, 0);
}

#[test]
fn layout_reports_old_and_new_bounds_as_damage() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.get_mut(root).unwrap().layout.flex_direction = FlexDirection::Column;
    t.add_root(Layer::Content, None, root);

    let a = text(&mut t, "a", 16.0);
    t.append_child(root, a);
    layout(&mut t, Size::new(300.0, 100.0));
    let _ = t.take_damage();

    // 加一个兄弟 ⇒ a 之后的区域变脏（旧 bounds ∪ 新 bounds）
    t.get_mut(a).unwrap().flags = Flags::EMPTY;
    let b = text(&mut t, "b", 16.0);
    t.append_child(root, b);
    let st = layout(&mut t, Size::new(300.0, 100.0));
    assert!(st.ran);
    let (rects, _all) = t.take_damage();
    assert!(!rects.is_empty(), "布局变化应登记脏矩形");
}

#[test]
fn layout_subtree_helper_lays_out_one_subtree() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let card = fixed(&mut t, 120.0, 40.0);
    t.append_child(root, card);
    let inner = text(&mut t, "x", 12.0);
    t.append_child(card, inner);

    let st = layout_subtree(&mut t, card, Size::new(120.0, 40.0));
    assert!(st.ran);
    assert_eq!(st.nodes, 2);
    assert_eq!(rect_of(&t, card), Rect::new(0.0, 0.0, 120.0, 40.0));
    assert!(contains(&t, inner, Point::new(1.0, 1.0)));
}

/// 回归（D-a）：文本节点布局后，`desired` 必须是**内容测量尺寸**，
/// 且 `TextSpec` 按引用传递（不再 `clone()`）后行为不变。
///
/// `desired` 走`paint_bounds` 的文本收缩路径 —— 它若变成布局后尺寸（被 flex 拉伸过），
/// 脏区就会偏大，"精确脏区"退化。
#[test]
fn text_desired_size_is_the_measured_content_size() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.get_mut(root).unwrap().layout.dim = [300.0, 200.0];
    t.add_root(Layer::Content, None, root);

    // 固定**内容**尺寸的文本（显式 width/height 会被 flex 用作 basis）
    let id = t.create(Kind::Text("Hello".to_string()), None);
    {
        let n = t.get_mut(id).unwrap();
        n.text.spec.font_size = 20.0;
        n.layout.dim = [VALUE_UNDEFINED, VALUE_UNDEFINED];
    }
    t.append_child(root, id);

    layout(&mut t, Size::new(300.0, 200.0));

    let n = t.get(id).unwrap();
    assert!(
        n.desired.width > 0.0 && n.desired.height > 0.0,
        "文本应有内容尺寸：{:?}",
        n.desired
    );
    // 内容宽度不应被拉成整行宽（那是 stretch 的结果，不是内容尺寸）
    assert!(
        n.desired.width < 300.0,
        "desired 应是内容宽度而非拉伸后的宽度：{}",
        n.desired.width
    );
    // 内容高度应接近字号（单行）
    assert!(
        n.desired.height < 200.0,
        "desired 高度应是单行高度：{}",
        n.desired.height
    );
}

/// 配套：Input（placeholder / text 两种来源）同样走内容测量，且不被拉伸。
#[test]
fn input_desired_size_uses_placeholder_when_empty() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.get_mut(root).unwrap().layout.dim = [300.0, 200.0];
    t.add_root(Layer::Content, None, root);

    let id = t.create(
        Kind::Input {
            text: String::new(),
            placeholder: "ph".to_string(),
            caret: 0,
            anchor: 0,
            preedit: String::new(),
            scroll: 0.0,
        },
        None,
    );
    {
        let n = t.get_mut(id).unwrap();
        n.text.spec.font_size = 20.0;
        n.layout.dim = [VALUE_UNDEFINED, VALUE_UNDEFINED];
    }
    t.append_child(root, id);

    layout(&mut t, Size::new(300.0, 200.0));

    let n = t.get(id).unwrap();
    assert!(n.desired.width > 0.0, "空Input 应按 placeholder 测量：{:?}", n.desired);
    assert!(
        n.desired.width < 300.0,
        "desired 不应是拉伸后的宽度：{}",
        n.desired.width
    );
}

/// ======================================================================
/// D-a 前置护栏：`build()` 必须是**纯函数**（同样的树 ⇒ 同样的结果）
/// ======================================================================
///
/// ## 为什么这是缓存的前提
///
/// 后续要做**边界级 FlexNode 树缓存**：整棵树构建一次，之后命中就复用。
/// 这只在 `build()` 是纯函数时成立 —— 若同样的 `Track` 能构建出**不同**的
/// `FlexNode`，那"复用上一次的结果"就会产出错误布局，而且**不报错**。
///
/// ## 为什么现在写
///
/// 本项目此前的 D52失败教训：**搬移/缓存前必须有测试证明"结果不变"**。
/// 没有这条，出了问题无法区分是"搬错了"还是"本来就不确定"。
#[cfg(test)]
mod build_purity {
    use super::*;
    use crate::track::Kind;

    /// 一棵覆盖各类测量输入的树：容器 / 文本 / 图片 / 嵌套容器。
    fn sample_tree() -> (Track, NodeId) {
        let mut t = Track::new();
        let root = t.create(Kind::Box, None);
        t.get_mut(root).unwrap().layout.dim = [300.0, 200.0];
        t.add_root(Layer::Content, None, root);

        let txt = t.create(Kind::Text("hello world".into()), None);
        t.append_child(root, txt);

        let img = t.create(
            Kind::Image(std::sync::Arc::new(crate::track::ImageData {
                width: 32,
                height: 16,
                rgba: Vec::new(),
            })),
            None,
        );
        t.append_child(root, img);

        let inner = t.create(Kind::Box, None);
        t.get_mut(inner).unwrap().layout.dim = [100.0, 50.0];
        t.append_child(root, inner);
        for i in 0..3 {
            let b = t.create(Kind::Box, None);
            t.get_mut(b).unwrap().layout.dim = [20.0, 10.0];
            t.append_child(inner, b);
            let _ = i;
        }
        (t, root)
    }

    /// 从 `FlexNode` 抽取**影响布局结果**的字段摘要（`FlexNode` 本身无 `PartialEq`）。
    fn shape(n: &lieui_layout::FlexNode) -> String {
        let mut s = format!(
            "style={:?}|intrinsic={:?}|measure={:?}|children=[",
            n.style,
            n.intrinsic_size,
            n.measure_text.as_ref().map(|(t, _)| t.as_str())
        );
        for c in &n.children {
            s.push_str(&format!("{}:{};", c.id, shape(c)));
        }
        s.push(']');
        s
    }

    /// ★ 核心：`build()` 是纯函数 —— 同一棵树连build 两次，结果**完全一致**。
    ///
    /// 这条测试是后续缓存的**前提**：若它不成立，缓存必然产出错误布局。
    #[test]
    fn build_is_idempotent() {
        let (t, root) = sample_tree();
        let a = build(&t, root);
        let b = build(&t, root);
        assert_eq!(shape(&a), shape(&b), "★ build() 必须是纯函数");
    }

    /// ★ 反向：改了**样式**必须让结果变化 —— 否则缓存会返回过期布局。
    #[test]
    fn build_reflects_style_change() {
        let (mut t, root) = sample_tree();
        let before = shape(&build(&t, root));
        // ★ 默认就是 Column（实测），所以要设成 **Row** 才有变化。
        //   第一版我设成 Column ⇒ 等于没改 ⇒ 测试失败才发现。
        t.get_mut(root).unwrap().layout.flex_direction = FlexDirection::Row;
        let after = shape(&build(&t, root));
        assert_ne!(before, after, "样式变了，build 结果必须跟着变");
    }

    /// ★ 反向：改了**文本内容**必须让结果变化。
    #[test]
    fn build_reflects_text_change() {
        let (mut t, root) = sample_tree();
        let txt = t.children(root)[0];
        let before = shape(&build(&t, root));
        t.get_mut(txt).unwrap().kind = Kind::Text("different content".into());
        let after = shape(&build(&t, root));
        assert_ne!(before, after, "文本变了，build 结果必须跟着变");
    }

    /// ★ 反向：**子节点列表**变化必须让结果变化（增删都算）。
    #[test]
    fn build_reflects_children_change() {
        let (mut t, root) = sample_tree();
        let before = shape(&build(&t, root));
        let extra = t.create(Kind::Box, None);
        t.get_mut(extra).unwrap().layout.dim = [7.0, 7.0];
        t.append_child(root, extra);
        let after = shape(&build(&t, root));
        assert_ne!(before, after, "加了子节点，build 结果必须跟着变");
    }

    /// ★ `visibility = Collapsed` 的子节点被 `layout_children` 过滤掉 ⇒ 结果变化。
    ///
    /// 这条是**最容易被漏掉的失效路径**：节点还在树上，只是不参与布局了。
    #[test]
    fn build_reflects_collapsed_child() {
        let (mut t, root) = sample_tree();
        let txt = t.children(root)[0];
        let before = shape(&build(&t, root));
        t.get_mut(txt).unwrap().visibility = crate::track::Visibility::Collapsed;
        let after = shape(&build(&t, root));
        assert_ne!(before, after, "Collapsed 子节点退出布局，build 结果必须跟着变");
    }

    /// ★ 父节点切成滚动容器会**改写子style**（`flex_shrink = 0`）
    /// —— 这条是最隐蔽的失效路径，父变了而子没变。
    #[test]
    fn build_reflects_parent_scroll_rewrite() {
        let (mut t, root) = sample_tree();
        let inner = t.children(root)[2];
        // 先让子节点有可被改写的 style
        t.get_mut(inner).unwrap().layout.flex_shrink = 1.0;
        let before = shape(&build(&t, root));
        t.get_mut(root).unwrap().layout.overflow_scroll = true;
        let after = shape(&build(&t, root));
        assert_ne!(before, after, "父节点切��滚动容器会改写子 style，build 结果必须跟着变");
    }
}
