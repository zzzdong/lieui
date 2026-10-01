//! 命中测试：**纯函数**，无副作用（设计的硬约束，见 §3.7）。
//!
//! 修复现状缺口（旧 `hit_test_rec` 只做矩形包含）：
//! - **变换**：缩放/旋转过的元素按逆矩阵判定（`Node.transform`），且父变换作用于整个子树；
//! - **裁剪**：`Node.clip` 拒掉外面的点（滚动容器由自身矩形天然裁剪）；
//! - **可见性**：`Collapsed` 不参与布局与命中；`Hidden` 占位但不绘制 ⇒ **也不可命中**
//!   （命中跟随渲染）；`hit_test_visible = false` 让**整棵子树**对命中透明（≈ WinUI `IsHitTestVisible`）；
//! - **层序**：从最高的层往下找；`blocks_below`（Modal）**无条件吸收**点击，避免"点在小对话框旁边
//!   漏到背后内容"；
//! - **不做事**：popup 的"点击外部关闭"**移出命中测试**（旧实现把副作用藏在 `hit_test_top` 里），
//!   改为由 `App` 在命中之外派发 `Dismissed`。

use lieui_geom::Point;

use crate::event::PointerId;
use crate::track::{Layer, NodeId, Track, Visibility};
use crate::transform::Affine;

/// 层序：从**最上**到最下（`Layer` 枚举序即 z 序，这里反过来遍历）
const LAYER_TOP_DOWN: [Layer; 6] = [
    Layer::DragPreview,
    Layer::Modal,
    Layer::Tooltip,
    Layer::Popup,
    Layer::Overlay,
    Layer::Content,
];

/// 命中链：`path[0]` = 层根（最外），`path.last()` = 命中目标（最深）。
/// 空 `Vec` = 什么都没命中（命中窗口背景）。
pub fn hit_path(track: &Track, p: Point) -> Vec<NodeId> {
    for layer in LAYER_TOP_DOWN {
        // 同层内：后声明的盖在上面 ⇒ 逆序遍历
        let roots: Vec<NodeId> = track.roots_of(layer).map(|r| r.node).collect();
        for root in roots.into_iter().rev() {
            let Some(r) = track.roots().iter().find(|r| r.node == root) else {
                continue;
            };
            // 整层穿透（水印 / 拖拽预览的默认值）
            if !r.opts.hit_test_visible {
                continue;
            }
            let mut path = Vec::new();
            if descend(track, root, p, Affine::IDENTITY, &mut path) {
                return path;
            }
            // 阻断下层（Modal）：无条件吸收，backdrop 之外也不漏给下层
            if r.opts.blocks_below {
                return vec![root];
            }
        }
    }
    Vec::new()
}

/// 命中目标（最深节点）
pub fn hit_test(track: &Track, p: Point) -> Option<NodeId> {
    hit_path(track, p).last().copied()
}

/// 从某节点到根的命中链（捕获路由用）
pub fn path_to(track: &Track, id: NodeId) -> Vec<NodeId> {
    if !track.contains(id) {
        return Vec::new();
    }
    let mut path: Vec<NodeId> = track.ancestors(id).collect();
    path.reverse();
    path.push(id);
    path
}

/// **指针捕获优先**的命中链：捕获存在时事件发给被捕获节点的祖先链（冒泡仍能到祖先），
/// 否则按真实位置命中（≈ WinUI `PointerCaptures`）。
pub fn hit_path_for(track: &Track, pointer: PointerId, p: Point) -> Vec<NodeId> {
    match track.captured_by(pointer) {
        Some(id) if track.contains(id) => path_to(track, id),
        _ => hit_path(track, p),
    }
}

/// 递归下降：`to_parent` 把窗口坐标映射到"本节点未变换坐标系"
fn descend(track: &Track, id: NodeId, p: Point, to_parent: Affine, out: &mut Vec<NodeId>) -> bool {
    let Some(n) = track.get(id) else {
        return false;
    };
    // 不绘制的东西不参与命中（Hidden 占位但不可见 ⇒ 不可命中；Collapsed 连布局都不参与）
    if n.visibility != Visibility::Visible {
        return false;
    }

    let rect = n.rect();
    // 自身变换的逆：先退到父坐标系，再退掉自身变换
    let to_local = match n.transform.matrix(rect).inverse() {
        Some(inv) => inv.then(to_parent),
        // 退化变换（scale=0 等）：无法给出可信命中区域 ⇒ 不可命中
        None => return false,
    };

    let q = to_local.apply(p);
    if !rect.contains(q) {
        return false;
    }
    // `Clip`（≈ UIElement.Clip）
    if let Some(clip) = n.clip
        && !clip.contains(q)
    {
        return false;
    }
    // 整棵子树对命中透明（≈ UIElement.IsHitTestVisible）
    if !n.hit_test_visible {
        return false;
    }

    out.push(id);

    // 后声明的子节点在上 ⇒ 逆序
    for c in n.children.iter().rev() {
        if descend(track, *c, p, to_local, out) {
            return true;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::{Cmd, apply_cmds};
    use crate::layout::{layout, rect_of};
    use crate::track::{Kind, Layer};
    use lieui_geom::{Rect, Size};

    const WINDOW: Size = Size::new(300.0, 100.0);

    fn fixed(t: &mut Track, w: f32, h: f32) -> NodeId {
        let id = t.create(Kind::Box, None);
        t.get_mut(id).unwrap().layout.dim = [w, h];
        id
    }

    fn relayout(t: &mut Track) {
        layout(t, WINDOW);
    }

    fn set_visible(t: &mut Track, id: NodeId, v: Visibility) {
        apply_cmds(
            t,
            &[Cmd::SetVisibility {
                id,
                visibility: v,
            }],
        );
    }

    /// 内容根（撑满 300×100）+ 3 个 100×100 子节点（row，x = 0 / 100 / 200）
    fn setup() -> (Track, NodeId, Vec<NodeId>) {
        let mut t = Track::new();
        let root = t.create(Kind::Box, None);
        t.get_mut(root).unwrap().layout.flex_direction = lieui_layout::FlexDirection::Row;
        t.add_root(Layer::Content, None, root);

        let kids: Vec<NodeId> = (0..3).map(|_| fixed(&mut t, 100.0, 100.0)).collect();
        for k in &kids {
            t.append_child(root, *k);
        }
        relayout(&mut t);
        (t, root, kids)
    }

    #[test]
    fn hits_the_child_under_the_point() {
        let (t, root, kids) = setup();
        assert_eq!(rect_of(&t, kids[1]), Rect::new(100.0, 0.0, 100.0, 100.0));
        let p = Point::new(150.0, 50.0);
        assert_eq!(hit_path(&t, p), vec![root, kids[1]]);
        assert_eq!(hit_test(&t, p), Some(kids[1]));
    }

    #[test]
    fn background_hit_returns_empty() {
        let (t, _, _) = setup();
        assert!(hit_path(&t, Point::new(-10.0, -10.0)).is_empty());
    }

    #[test]
    fn collapsed_child_shifts_siblings_and_is_unhittable() {
        let (mut t, root, kids) = setup();
        // 收起中间那个：右边的应补位到 x=100
        set_visible(&mut t, kids[1], Visibility::Collapsed);
        relayout(&mut t);

        assert_eq!(rect_of(&t, kids[2]).x, 100.0, "兄弟补位");
        assert_eq!(hit_test(&t, Point::new(150.0, 50.0)), Some(kids[2]));

        // 收起最右边：200..300 变成空白，命中回落到层根
        set_visible(&mut t, kids[2], Visibility::Collapsed);
        relayout(&mut t);
        let path = hit_path(&t, Point::new(250.0, 50.0));
        assert_eq!(path, vec![root], "空白处命中层根");
        assert!(!path.contains(&kids[2]));
    }

    #[test]
    fn hidden_occupies_space_but_is_not_hittable() {
        let (mut t, root, kids) = setup();
        set_visible(&mut t, kids[0], Visibility::Hidden);
        relayout(&mut t);

        // 仍占位（兄弟不补位）
        assert_eq!(rect_of(&t, kids[0]), Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(rect_of(&t, kids[1]).x, 100.0);

        // 但不绘制 ⇒ 不可命中，回落到层根
        assert_eq!(hit_test(&t, Point::new(50.0, 50.0)), Some(root));
    }

    #[test]
    fn hit_test_visible_false_makes_the_subtree_transparent() {
        let (mut t, root, kids) = setup();
        let inner = fixed(&mut t, 50.0, 50.0);
        t.append_child(kids[0], inner);
        relayout(&mut t);
        assert_eq!(hit_test(&t, Point::new(10.0, 10.0)), Some(inner));

        t.get_mut(kids[0]).unwrap().hit_test_visible = false;
        let path = hit_path(&t, Point::new(10.0, 10.0));
        assert!(!path.contains(&kids[0]), "整棵子树穿透");
        assert!(!path.contains(&inner));
        assert_eq!(path, vec![root], "只剩层根");
    }

    #[test]
    fn clip_rejects_outside_points() {
        let (mut t, root, kids) = setup();
        t.get_mut(kids[1]).unwrap().clip = Some(Rect::new(100.0, 0.0, 10.0, 10.0));
        let path = hit_path(&t, Point::new(150.0, 50.0));
        assert!(!path.contains(&kids[1]), "clip 之外不命中");
        assert_eq!(path, vec![root]);
    }

    #[test]
    fn transform_shrinks_the_hit_region() {
        let (mut t, root, kids) = setup();
        // 缩到一半（绕中心）⇒ 原矩形四角已在命中区外
        t.get_mut(kids[1]).unwrap().transform.scale = (0.5, 0.5);

        assert_eq!(hit_test(&t, Point::new(150.0, 50.0)), Some(kids[1]), "中心仍命中");
        let path = hit_path(&t, Point::new(103.0, 5.0));
        assert!(!path.contains(&kids[1]));
        assert_eq!(path, vec![root]);
    }

    #[test]
    fn transform_translate_moves_the_hit_region() {
        let (mut t, _, kids) = setup();
        // kids[0] 原本占 0..100，平移到 +50 ⇒ 命中区变成 50..150
        t.get_mut(kids[0]).unwrap().transform.translate = (50.0, 0.0);

        assert_eq!(hit_test(&t, Point::new(60.0, 50.0)), Some(kids[0]), "新位置命中");
        assert_ne!(hit_test(&t, Point::new(20.0, 50.0)), Some(kids[0]), "原位不再命中");

        // 平移后与 kids[1] 重叠：后声明的在上 ⇒ 命中 kids[1]
        assert_eq!(hit_test(&t, Point::new(150.0, 50.0)), Some(kids[1]));
    }

    #[test]
    fn parent_transform_applies_to_children() {
        let (mut t, _, kids) = setup();
        // 父容器放大 2 倍（绕中心 (50,50)）
        t.get_mut(kids[0]).unwrap().transform.scale = (2.0, 2.0);
        let child = fixed(&mut t, 50.0, 50.0);
        t.append_child(kids[0], child);
        relayout(&mut t);
        assert_eq!(rect_of(&t, child), Rect::new(0.0, 0.0, 50.0, 50.0));

        // 子节点随父一起放大：屏幕点 (30,30) 退到父坐标系是 (40,40) ⇒ 命中子节点
        assert_eq!(hit_test(&t, Point::new(30.0, 30.0)), Some(child));
        // (90,90) 退回去是 (70,70)：在父内、在子外 ⇒ 命中父
        assert_eq!(hit_test(&t, Point::new(90.0, 90.0)), Some(kids[0]));
    }

    #[test]
    fn degenerate_transform_is_unhittable() {
        let (mut t, root, kids) = setup();
        t.get_mut(kids[1]).unwrap().transform.scale = (0.0, 1.0);
        assert_ne!(hit_test(&t, Point::new(150.0, 50.0)), Some(kids[1]));
        assert_eq!(hit_test(&t, Point::new(150.0, 50.0)), Some(root));
    }

    #[test]
    fn topmost_layer_wins() {
        let (mut t, _, _) = setup();
        let popup = fixed(&mut t, 100.0, 100.0);
        t.add_root(Layer::Popup, None, popup);
        relayout(&mut t);

        assert_eq!(hit_path(&t, Point::new(50.0, 50.0)), vec![popup]);
    }

    #[test]
    fn modal_absorbs_everything_below() {
        let (mut t, content, _kids) = setup();
        let modal = fixed(&mut t, 100.0, 100.0); // 故意比窗口小
        t.add_root(Layer::Modal, None, modal);
        relayout(&mut t);

        // 覆盖区：命中 modal
        assert_eq!(hit_path(&t, Point::new(50.0, 50.0)), vec![modal]);

        // modal 之外：仍被吸收（backdrop 语义），不漏给下层内容
        let path = hit_path(&t, Point::new(250.0, 50.0));
        assert_eq!(path, vec![modal]);
        assert!(!path.contains(&content));
    }

    #[test]
    fn overlay_layer_passes_through_by_default() {
        let (mut t, content, _) = setup();
        let overlay = fixed(&mut t, 300.0, 100.0);
        t.add_root(Layer::Overlay, None, overlay);
        relayout(&mut t);

        let path = hit_path(&t, Point::new(50.0, 50.0));
        assert!(!path.contains(&overlay), "水印层默认命中穿透");
        assert!(path.contains(&content));
    }

    #[test]
    fn later_root_in_the_same_layer_is_on_top() {
        let mut t = Track::new();
        let bottom = fixed(&mut t, 100.0, 100.0);
        let top = fixed(&mut t, 100.0, 100.0);
        t.add_root(Layer::Popup, None, bottom);
        t.add_root(Layer::Popup, None, top);
        relayout(&mut t);

        assert_eq!(hit_path(&t, Point::new(50.0, 50.0)), vec![top]);
    }

    #[test]
    fn path_to_returns_root_first() {
        let (t, root, kids) = setup();
        assert_eq!(path_to(&t, kids[0]), vec![root, kids[0]]);
        assert!(path_to(&t, NodeId::NULL).is_empty());
    }

    #[test]
    fn capture_redirects_the_path() {
        let (mut t, root, kids) = setup();
        let p = PointerId(0);
        t.capture_pointer(p, kids[0]);

        assert_eq!(
            hit_path_for(&t, p, Point::new(250.0, 50.0)),
            vec![root, kids[0]],
            "捕获优先于真实位置"
        );

        t.release_pointer(p);
        assert_eq!(hit_path_for(&t, p, Point::new(250.0, 50.0)).last(), Some(&kids[2]));
    }
}
