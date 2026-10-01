//! 布局适配器：保留树 ⇄ Taitank Flex 引擎（`lieui-layout`）。
//!
//! 这一层替换旧实现里 `src/layout/context.rs`（把 `ElementTree` 编译成 `FlexNode`）。
//! 与旧实现的差别（设计 §3.6）：
//!
//! | | 旧实现 | v3 |
//! |---|---|---|
//! | 重排范围 | 只要 `has_dirty_node()` 为真 ⇒ **整窗**重建 flex 树 | **边界集合**：只重排"自身脏、父不脏"的子树 |
//! | 边界判据 | 无 | 宽高都确定（`dim` 两轴 defined）的节点是边界 |
//! | 结果写回 | `ElementTree::set_layout` | 直接写 `Node.computed` / `Node.desired` / `Node.content_size` |
//! | 脏区 | 无（渲染层自己整屏重绘） | 旧 bounds ∪ 新 bounds 精确登记 |
//!
//! 引擎入口语义（Taitank）：`FlexNode::layout(parent_w, parent_h, dir)` 里，
//! 若自身 `dim` 未定义则填满可用空间（减自身 margin），因此**层根**用窗口尺寸、
//! **嵌套边界**用它的现有 `computed` 尺寸（它尺寸确定，父给它的大小必然未变）。

use lieui_geom::{Point, Rect, Size};
use lieui_layout::types::Direction;
use lieui_layout::{ComputedLayout, FlexDirection, FlexNode, VALUE_UNDEFINED};
use lieui_text::TextEngine;

use crate::track::{Kind, NodeId, Placement, Track, Visibility};

/// 一次布局的结果统计
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutStats {
    pub ran: bool,
    /// 本次重排的边界数（= 参与重排的子树个数）
    pub boundaries: usize,
    /// 写回的节点数
    pub nodes: usize,
    /// 位置或尺寸真的变了的节点数
    pub moved: usize,
    /// 扫过的滚动容器数
    pub scroll_containers: usize,
    /// 滚动偏移被钳制的容器数
    pub clamped_scroll: usize,
}

/// 需要重排吗
pub fn needs_layout(track: &Track) -> bool {
    track.has_layout_dirty()
}

/// 按**边界集合**重排（`App::frame` 的唯一入口）。
///
/// `window` 是客户区尺寸，供层根填满。
pub fn layout(track: &mut Track, window: Size) -> LayoutStats {
    let boundaries = track.layout_boundaries();
    if boundaries.is_empty() {
        return LayoutStats::default();
    }

    let mut st = LayoutStats {
        ran: true,
        boundaries: boundaries.len(),
        ..Default::default()
    };

    let root_nodes: Vec<NodeId> = track.roots().iter().map(|r| r.node).collect();
    // 锚定层根（popup/tooltip）：**内容自适应尺寸**，位置由 `place_anchored_layers` 给
    let anchored_roots: Vec<NodeId> = track
        .roots()
        .iter()
        .filter(|r| r.opts.anchor.is_some())
        .map(|r| r.node)
        .collect();

    for b in boundaries {
        if !track.contains(b) {
            continue;
        }
        let is_root = root_nodes.contains(&b);
        let (avail, base) = if is_root {
            if anchored_roots.contains(&b) {
                // 可用空间未定义 ⇒ 引擎收缩到内容（否则层根会被拉伸成整窗，
                // 锚点定位就无从谈起）。显式定宽的层不受影响（dim 仍生效）。
                (
                    Size::new(VALUE_UNDEFINED, VALUE_UNDEFINED),
                    None,
                )
            } else {
                (window, None)
            }
        } else {
            let r = track.get(b).map(|n| n.rect()).unwrap_or_default();
            // 边界自身尺寸确定 ⇒ 父给它的大小没变，沿用现有 computed 尺寸
            (
                Size::new(r.width.max(0.0), r.height.max(0.0)),
                Some((r.x, r.y)),
            )
        };

        let mut flex = build(track, b);
        flex.layout(avail.width, avail.height, Direction::Ltr);

        // 写回起点：让 `base + flex.get_left()` 复现边界原本的绝对位置
        let (ox, oy) = match base {
            Some((bx, by)) => (bx - flex.get_left(), by - flex.get_top()),
            None => (0.0, 0.0),
        };
        write_back(track, b, &flex, ox, oy, &mut st);
    }

    track.clear_layout_flags();
    st
}

/// 锚定层与锚点之间的间距
pub const ANCHOR_GAP: f32 = 4.0;

/// 布局**之后**解析锚定层的位置（popup / tooltip）。
///
/// 为什么必须在布局后：锚点 rect 要等布局才知道，而层自身的尺寸也要等它自己的
/// 布局跑完（`popup_at` 声明的子树宽度不定）。所以定位是独立的一步：
///
/// 1. 每个带 `opts.anchor` 的层根，按锚点 rect + `Placement` 求目标原点；
/// 2. 默认侧放不下、另一侧放得下 ⇒ 翻转（Below↔Above / LeftOf↔RightOf，一次决策）；
/// 3. 仍放不下 ⇒ 钳到视口内（超大层贴边，`Fixed` 交给调用者自担）；
/// 4. 平移整棵层子树（根 + 全部后代），旧位置 ∪ 新位置登记脏区。
///
/// 返回移动了的层数（调用方据此感知"本帧有合成变化"）。
pub fn place_anchored_layers(track: &mut Track, window: Size) -> usize {
    let anchored: Vec<(NodeId, crate::track::Anchor)> = track
        .roots()
        .iter()
        .filter_map(|r| r.opts.anchor.clone().map(|a| (r.node, a)))
        .collect();

    let mut moved = 0;
    for (node, anchor) in anchored {
        let Some(target) = anchored_origin(track, node, &anchor, window) else {
            continue;
        };
        let Some(cur) = track.get(node).map(|n| n.rect()) else {
            continue;
        };
        let (dx, dy) = (target.0 - cur.x, target.1 - cur.y);
        if dx.abs() < 0.01 && dy.abs() < 0.01 {
            continue;
        }

        // `descendants` 含自身（destroy 的语义）；这里正好要平移整棵子树
        for id in track.descendants(node) {
            if let Some(n) = track.get_mut(id) {
                n.computed.x += dx;
                n.computed.y += dy;
            }
        }
        // 旧位置 ∪ 新位置都要重绘
        track.damage_rect(cur);
        track.damage_rect(Rect::new(cur.x + dx, cur.y + dy, cur.width, cur.height));
        moved += 1;
    }
    moved
}

/// 求锚定层的目标原点（含翻转与钳制）；锚点 key 不存在 ⇒ `None`（保持原位）
fn anchored_origin(
    track: &Track,
    layer: NodeId,
    anchor: &crate::track::Anchor,
    window: Size,
) -> Option<(f32, f32)> {
    let lr = track.get(layer).map(|n| n.rect()).unwrap_or_default();
    let (sw, sh) = (lr.width, lr.height);
    let a = track.find_by_key(&anchor.key)?;
    let ar = track.get(a).map(|n| n.rect()).unwrap_or_default();

    let mut p = anchor.placement;
    // 翻转：默认侧放不下 **且** 另一侧放得下才翻（避免在两个都放不下时来回抖动）
    match p {
        Placement::Below
            if ar.bottom() + ANCHOR_GAP + sh > window.height
                && ar.y - ANCHOR_GAP - sh >= 0.0 =>
        {
            p = Placement::Above
        }
        Placement::Above
            if ar.y - ANCHOR_GAP - sh < 0.0
                && ar.bottom() + ANCHOR_GAP + sh <= window.height =>
        {
            p = Placement::Below
        }
        Placement::RightOf
            if ar.right() + ANCHOR_GAP + sw > window.width
                && ar.x - ANCHOR_GAP - sw >= 0.0 =>
        {
            p = Placement::LeftOf
        }
        Placement::LeftOf
            if ar.x - ANCHOR_GAP - sw < 0.0
                && ar.right() + ANCHOR_GAP + sw <= window.width =>
        {
            p = Placement::RightOf
        }
        _ => {}
    }

    let (mut x, mut y) = match p {
        Placement::Below => (ar.x, ar.bottom() + ANCHOR_GAP),
        Placement::Above => (ar.x, ar.y - ANCHOR_GAP - sh),
        Placement::RightOf => (ar.right() + ANCHOR_GAP, ar.y),
        Placement::LeftOf => (ar.x - ANCHOR_GAP - sw, ar.y),
        Placement::ScreenCenter => ((window.width - sw) * 0.5, (window.height - sh) * 0.5),
        Placement::Fixed { x, y } => (x, y),
    };
    // 钳到视口（Fixed 除外）
    if !matches!(p, Placement::Fixed { .. }) {
        x = x.clamp(0.0, (window.width - sw).max(0.0));
        y = y.clamp(0.0, (window.height - sh).max(0.0));
    }
    Some((x, y))
}

/// 单棵子树重排（工具/测试用；`App::frame` 走 [`layout`]）
pub fn layout_subtree(track: &mut Track, id: NodeId, available: Size) -> LayoutStats {
    if !track.contains(id) {
        return LayoutStats::default();
    }
    let mut flex = build(track, id);
    flex.layout(available.width, available.height, Direction::Ltr);

    let r = track.get(id).map(|n| n.rect()).unwrap_or_default();
    let (ox, oy) = (r.x - flex.get_left(), r.y - flex.get_top());

    let mut st = LayoutStats {
        ran: true,
        boundaries: 1,
        ..Default::default()
    };
    write_back(track, id, &flex, ox, oy, &mut st);
    track.clear_layout_flags();
    st
}

// ───────────────────────── 建树 ─────────────────────────

/// 参与布局的子节点（`Collapsed` 不参与布局，因此不进 flex 树）
pub(crate) fn layout_children(track: &Track, id: NodeId) -> Vec<NodeId> {
    track
        .children(id)
        .iter()
        .copied()
        .filter(|c| {
            track
                .get(*c)
                .map(|n| n.visibility != Visibility::Collapsed)
                .unwrap_or(false)
        })
        .collect()
}

/// 把保留子树转成 `FlexNode` 子树（叶子带上测度信息）
pub(crate) fn build(track: &Track, id: NodeId) -> FlexNode {
    let Some(n) = track.get(id) else {
        return FlexNode::new(id.to_u64(), Default::default());
    };

    let mut flex = FlexNode::new(id.to_u64(), n.layout.clone());

    match &n.kind {
        // 文本叶子：交给引擎按约束宽度重新测量（支持换行）
        Kind::Text(s) => flex.measure_text = Some((s.clone(), n.text.spec.clone())),
        // 按钮的文字参与固有尺寸，容器 padding 由 style 提供
        Kind::Button { label } => flex.measure_text = Some((label.clone(), n.text.spec.clone())),
        // 输入框：空文本时按 placeholder 测度，避免"有提示语就有高度、清空后盒子塌掉"
        Kind::Input {
            text, placeholder, ..
        } => {
            let s = if text.is_empty() { placeholder } else { text };
            flex.measure_text = Some((s.clone(), n.text.spec.clone()));
        }
        // 图片有固定固有尺寸
        Kind::Image(img) => flex.intrinsic_size = Some((img.width as f32, img.height as f32)),
        // 自定义节点：声明自己的固有内容尺寸（未显式定宽高时生效）
        Kind::Custom(cell) => {
            let s = cell.intrinsic_size();
            if s.width > 0.0 || s.height > 0.0 {
                flex.intrinsic_size = Some((s.width, s.height));
            }
        }
        _ => {}
    }

    for c in layout_children(track, id) {
        flex.children.push(build(track, c));
    }

    // 滚动容器的直接子节点必须保持自然尺寸，禁止被收缩到视口尺寸，
    // 否则滚动内容会塌陷（旧实现的同名处理）
    if n.layout.overflow_scroll {
        for c in flex.children.iter_mut() {
            c.style.flex_shrink = 0.0;
            c.style.flex_grow = 0.0;
        }
    }

    flex
}

// ───────────────────────── 写回 ─────────────────────────

fn write_back(
    track: &mut Track,
    id: NodeId,
    flex: &FlexNode,
    ox: f32,
    oy: f32,
    st: &mut LayoutStats,
) {
    let x = ox + flex.get_left();
    let y = oy + flex.get_top();
    let w = flex.get_width();
    let h = flex.get_height();

    let is_scroll = track
        .get(id)
        .map(|n| n.layout.overflow_scroll)
        .unwrap_or(false);
    let children = layout_children(track, id);

    // ① 写 computed / desired，并把"旧 bounds ∪ 新 bounds"并入脏区
    let old_rect = track.get(id).map(|n| n.rect()).unwrap_or_default();
    let desired = desired_size(track, id, flex, w, h);
    if let Some(n) = track.get_mut(id) {
        n.computed = ComputedLayout {
            x,
            y,
            width: w,
            height: h,
            overflow_scroll: is_scroll,
        };
        n.desired = desired;
    }
    let new_rect = Rect::new(x, y, w, h);
    if !rect_eq(old_rect, new_rect) {
        st.moved += 1;
        track.damage_rect(old_rect);
        track.damage_rect(new_rect);
    }
    st.nodes += 1;

    // ② 滚动容器：算内容尺寸、钳制偏移、把子原点平移 -offset
    let (child_ox, child_oy) = if is_scroll {
        st.scroll_containers += 1;

        let mut cw = 0.0f32;
        let mut ch = 0.0f32;
        for c in flex.children.iter() {
            cw = cw.max(c.get_left() + c.get_width() + c.get_layout_end_margin(FlexDirection::Row));
            ch = ch.max(c.get_top() + c.get_height() + c.get_layout_end_margin(FlexDirection::Column));
        }
        let content = Size::new(cw, ch);

        let (sx, sy) = track.scroll_offset(id);
        let nx = sx.clamp(0.0, (cw - w).max(0.0));
        let ny = sy.clamp(0.0, (ch - h).max(0.0));
        if (nx - sx).abs() > 1e-4 || (ny - sy).abs() > 1e-4 {
            st.clamped_scroll += 1;
            track.set_scroll_offset(id, (nx, ny));
        }
        if let Some(n) = track.get_mut(id) {
            n.content_size = content;
        }
        (x - nx, y - ny)
    } else {
        (x, y)
    };

    // ③ 递归（结构与 `build` 一致：同一批 `layout_children`）
    for (c, fc) in children.iter().zip(flex.children.iter()) {
        write_back(track, *c, fc, child_ox, child_oy, st);
    }
}

/// `≈ DesiredSize`：叶子取固有尺寸，容器取引擎分配尺寸（滚动容器的内容尺寸见 `Node.content_size`）
fn desired_size(track: &Track, id: NodeId, flex: &FlexNode, w: f32, h: f32) -> Size {
    if !flex.children.is_empty() {
        return Size::new(w, h);
    }
    match track.get(id).map(|n| &n.kind) {
        Some(Kind::Text(s)) => {
            let spec = track.get(id).map(|n| n.text.spec.clone()).unwrap_or_default();
            let (mw, mh) = TextEngine::measure_text(s, &spec);
            Size::new(mw as f32, mh as f32)
        }
        Some(Kind::Input {
            text, placeholder, ..
        }) => {
            let spec = track.get(id).map(|n| n.text.spec.clone()).unwrap_or_default();
            let s = if text.is_empty() { placeholder } else { text };
            let (mw, mh) = TextEngine::measure_text(s, &spec);
            Size::new(mw as f32, mh as f32)
        }
        Some(Kind::Image(img)) => Size::new(img.width as f32, img.height as f32),
        Some(Kind::Custom(cell)) => {
            let s = cell.intrinsic_size();
            if s.width > 0.0 || s.height > 0.0 {
                Size::new(s.width, s.height)
            } else {
                Size::new(w, h)
            }
        }
        _ => Size::new(w, h),
    }
}

fn rect_eq(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < 1e-3
        && (a.y - b.y).abs() < 1e-3
        && (a.width - b.width).abs() < 1e-3
        && (a.height - b.height).abs() < 1e-3
}

/// 便捷：把节点矩形读成 `Rect`（命中/绘制/测试都用）
pub fn rect_of(track: &Track, id: NodeId) -> Rect {
    track.get(id).map(|n| n.rect()).unwrap_or_default()
}

/// 便捷：点是否在节点矩形内（不含变换；带变换的命中请用 [`crate::hit`]）
pub fn contains(track: &Track, id: NodeId, p: Point) -> bool {
    rect_of(track, id).contains(p)
}

#[cfg(test)]
mod tests {
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
        t.root_mut(rid).unwrap().opts.anchor =
            Some(Anchor { key: Key::from("anchor"), placement });
        (t, anchor, popup)
    }

    /// 布局 + 落位，返回 (锚点 rect, 层根 rect)
    fn place(t: &mut Track) -> (Rect, Rect) {
        layout(t, WIN);
        place_anchored_layers(t, WIN);
        let anchor = t.find_by_key(&Key::from("anchor")).unwrap();
        let popup = t
            .roots()
            .iter()
            .find(|r| r.layer == Layer::Popup)
            .unwrap()
            .node;
        (t.get(anchor).unwrap().rect(), t.get(popup).unwrap().rect())
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
            n.layout = n
                .layout
                .clone()
                .padding_all(10.0)
                .gap(5.0);
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
        assert_eq!(
            rect_of(&t, b).y,
            rect_of(&t, a).height + 10.0 + 5.0,
            "gap 生效"
        );
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
        assert!(
            t.get(label).unwrap().desired.width > old_desired,
            "文本被重新测度"
        );
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
}
