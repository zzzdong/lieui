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
    // ★ 在动手之前开新纪元（D58）：本轮期间新提的脏标不能被末尾的
    //   `clear_layout_flags` 清掉，它们属于下一轮。
    track.begin_layout_epoch();

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
                (Size::new(VALUE_UNDEFINED, VALUE_UNDEFINED), None)
            } else {
                (window, None)
            }
        } else {
            let r = track.get(b).map(|n| n.rect()).unwrap_or_default();
            // 边界自身尺寸确定 ⇒ 父给它的大小没变，沿用现有 computed 尺寸
            (Size::new(r.width.max(0.0), r.height.max(0.0)), Some((r.x, r.y)))
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

/// 求锚定层的目标原点（含翻转与钳制）；`Key` 锚点查不到 ⇒ `None`（保持原位）
fn anchored_origin(track: &Track, layer: NodeId, anchor: &crate::track::Anchor, window: Size) -> Option<(f32, f32)> {
    let lr = track.get(layer).map(|n| n.rect()).unwrap_or_default();
    let (sw, sh) = (lr.width, lr.height);
    // 锚点矩形。**点锚点 = 零尺寸的退化矩形** —— 于是下面那套翻转 / 钳制逻辑
    // 对"锚在鼠标上的右键菜单"一字不改地生效（贴鼠标；下方放不下翻上方；靠边平移回视口）。
    let ar = match &anchor.target {
        crate::track::AnchorTarget::Key(key) => {
            let id = track.find_by_key(key)?;
            track.get(id).map(|n| n.rect()).unwrap_or_default()
        }
        crate::track::AnchorTarget::Node(id) => track.get(*id).map(|n| n.rect()).unwrap_or_default(),
        crate::track::AnchorTarget::Point(p) => Rect::new(p.x, p.y, 0.0, 0.0),
    };

    let mut p = anchor.placement;
    // 翻转：默认侧放不下 **且** 另一侧放得下才翻（避免在两个都放不下时来回抖动）
    match p {
        Placement::Below if ar.bottom() + ANCHOR_GAP + sh > window.height && ar.y - ANCHOR_GAP - sh >= 0.0 => {
            p = Placement::Above
        }
        Placement::Above if ar.y - ANCHOR_GAP - sh < 0.0 && ar.bottom() + ANCHOR_GAP + sh <= window.height => {
            p = Placement::Below
        }
        Placement::RightOf if ar.right() + ANCHOR_GAP + sw > window.width && ar.x - ANCHOR_GAP - sw >= 0.0 => {
            p = Placement::LeftOf
        }
        Placement::LeftOf if ar.x - ANCHOR_GAP - sw < 0.0 && ar.right() + ANCHOR_GAP + sw <= window.width => {
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
        Kind::Input { text, placeholder, .. } => {
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

fn write_back(track: &mut Track, id: NodeId, flex: &FlexNode, ox: f32, oy: f32, st: &mut LayoutStats) {
    let x = ox + flex.get_left();
    let y = oy + flex.get_top();
    let w = flex.get_width();
    let h = flex.get_height();

    let is_scroll = track.get(id).map(|n| n.layout.overflow_scroll).unwrap_or(false);
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
        // 文本节点：记下引擎这次测度用的**换行宽度**，绘制时复用同一约束
        //（否则测度按约束换行、绘制不换行 ⇒ 盒子两行高、只画一行 ⇒ 看起来顶对齐）
        if matches!(&n.kind, Kind::Text(_) | Kind::Button { .. }) {
            n.text_wrap = flex.measured_wrap_width;
        }
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
    // ★ D-a：文本/ Input **直接复用 flex 本轮测量的内容尺寸**，不再重新测一遍。
    //
    //   flex 引擎在 `layout_single_node` 里已经测过一次（并记在 `measured_content`），
    //   此前这里对每个文本叶子**又测一次** —— 每次布局多一次 `measure_text`
    //   （含缓存键构造 `text.to_owned()` 的一次堆分配）。
    //
    //   用 `measured_content` 而不是 `layout_result.dim` 是**必须的**：后者是**布局后**尺寸
    //   （可能被 flex 拉伸/收缩），而 `desired` 走 `paint_bounds` 的文本收缩路径，
    //   误用会让脏区偏大 ⇒ "精确脏区"退化。
    //
    //   `[0.0, 0.0]` = 本轮没测量（引擎走了非文本分支）⇒ 回落到原路径。
    let mc = flex.measured_content;
    let has_measured_content = mc[0] > 0.0 && mc[1] > 0.0;
    if has_measured_content
        && matches!(
            track.get(id).map(|n| &n.kind),
            Some(Kind::Text(_)) | Some(Kind::Input { .. })
        )
    {
        return Size::new(mc[0], mc[1]);
    }
    match track.get(id).map(|n| &n.kind) {
        Some(Kind::Text(s)) => match track.get(id) {
            Some(n) => {
                let (mw, mh) = TextEngine::measure_text(s, &n.text.spec);
                Size::new(mw as f32, mh as f32)
            }
            None => Size::new(w, h),
        },
        Some(Kind::Input { text, placeholder, .. }) => match track.get(id) {
            Some(n) => {
                let s = if text.is_empty() { placeholder } else { text };
                let (mw, mh) = TextEngine::measure_text(s, &n.text.spec);
                Size::new(mw as f32, mh as f32)
            }
            None => Size::new(w, h),
        },
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
#[path = "layout_tests.rs"]
mod tests;
