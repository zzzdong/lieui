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
use crate::track::{NodeId, Track, Visibility};
use crate::transform::Affine;

/// 命中链：`path[0]` = 层根（最外），`path.last()` = 命中目标（最深）。
/// 空 `Vec` = 什么都没命中（命中窗口背景）。
pub fn hit_path(track: &Track, p: Point) -> Vec<NodeId> {
    // ★ z 序遍历（从上到下）：与渲染侧 `scene.rs` 的 `z_ordered_roots()`
    //   **同源、反向** ⇒ 绘制顺序与命中顺序必然一致（同 key 内仍"后声明在上"）。
    for r in track.z_ordered_roots_top_down() {
        let root = r.node;
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
        apply_cmds(t, &[Cmd::SetVisibility { id, visibility: v }]);
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

/// 裁决测试（D5）：带 `transform` + `clip` 的节点，**命中区域必须与绘制区域一致**。
///
/// ## 背景：这份测试是为了裁决一份P0 缺陷指控
///
/// 四份审计里有三份把 D5 列为 P0，理由是"hit 用逆变换后的**局部点**测 `clip`，
/// 而 render 把 `clip` 当与 `rect()` 同空间取交⇒ 两者坐标系不一致"。
///
/// 代码核实结果是**两侧其实同语义**：
/// - 渲染侧 `Op::PushClip { rect: c, transform }`（`render/scene.rs:462`）
///   —— `c` 与 `rect()` 同为**节点本地空间**，且 `transform` 被显式携带，
///   由光栅器映射到窗口空间。
/// - 命中侧（`hit.rs:104-108`）先把点逆变换到本地，再 `clip.contains(q)`。
///
/// 两边都在本地空间测clip，**没有不一致**。但"读代码得出结论"不如"用测试钉住契约"，
/// 这条测试就是那个契约：若将来有人把 clip 改成窗口空间（或漏掉 transform 传递），
/// 它会立刻失败。
#[cfg(test)]
mod d5_clip_space {
    use super::hit_path;
    use crate::layout::layout;
    use crate::track::{Kind, Layer, NodeId, Track, Transform};
    use lieui_geom::{Point, Rect, Size};

    const WINDOW: Size = Size::new(300.0, 100.0);

    /// 节点本地 `0,0,50,50` + `translate(100,0)` ⇒ 窗口 `100,0,150,50`；
    /// `clip` 本地 `0,0,25,25` ⇒ 窗口 `100,0,125,25`。
    ///
    /// **本测试如何区分两种语义**：
    /// - 正确（clip 在本地空间）：`(110,10)` 落在 clip 窗口矩形内 ⇒ 可命中。
    /// - 错误（clip 被当成窗口空间）：窗口 clip 变成 `(0,0,25,25)`，位于原点，
    ///   于是 `(110,10)` 会被拒 —— 恰好是 `translate(100,0)` 把它推出原点裁剪区。
    #[test]
    fn clip_is_tested_in_node_local_space() {
        let mut t = Track::new();
        let root = t.create(Kind::Box, None);
        t.get_mut(root).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
        t.add_root(Layer::Content, None, root);

        let child = t.create(Kind::Box, None);
        {
            let n = t.get_mut(child).unwrap();
            n.layout.dim = [50.0, 50.0];
            // 纯平移（scale=1、rotation=0 ⇒ origin 不影响结果）
            n.transform = Transform {
                translate: (100.0, 0.0),
                ..Default::default()
            };
            n.clip = Some(Rect::new(0.0, 0.0, 25.0, 25.0));
        }
        t.append_child(root, child);
        layout(&mut t, WINDOW);

        let child_id: NodeId = child;
        assert!(
            hit_path(&t, Point::new(110.0, 10.0)).contains(&child_id),
            "clip 内的点应可命中（clip 在节点本地空间）"
        );
        assert!(
            !hit_path(&t, Point::new(140.0, 10.0)).contains(&child_id),
            "节点矩形内但 clip 外的点不应命中"
        );
    }

    /// 配套：反向确认节点自身矩形仍生效（clip 不得把节点"放大"到矩形外）。
    #[test]
    fn clip_cannot_widen_the_node_rect() {
        let mut t = Track::new();
        let root = t.create(Kind::Box, None);
        t.get_mut(root).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
        t.add_root(Layer::Content, None, root);

        let child = t.create(Kind::Box, None);
        {
            let n = t.get_mut(child).unwrap();
            n.layout.dim = [20.0, 20.0];
            // clip 比节点大：50,0,80,60（本地）⇒ 交集应仍受节点矩形限制
            n.clip = Some(Rect::new(0.0, 0.0, 80.0, 60.0));
        }
        t.append_child(root, child);
        layout(&mut t, WINDOW);

        let child_id: NodeId = child;
        assert!(hit_path(&t, Point::new(10.0, 10.0)).contains(&child_id));
        assert!(
            !hit_path(&t, Point::new(30.0, 10.0)).contains(&child_id),
            "clip 大于节点时不应把可命中区扩到节点矩形之外"
        );
    }
}

/// 层序回归测试（D6 / P1「`Layer` 增变体不报错」）。
///
/// ## 背景：这里修的不是"嵌套 z 序"，而是**两份手写层序数组的漂移风险**
///
/// 审计把 D6 列为"Modal 内的 Popup 被 backdrop 盖住且收不到点击"。核实结论：
///
/// |断言 | 核实结果 |
/// |---|---|
/// | `Root.owner`（嵌套父层）无消费点 | ✅ 属实 |
/// | 生产代码能创建嵌套层 | ❌ **不能** —— `owner` 唯一来源 `view.rs:227` 恒为 `None`，唯一传 `Some(modal)` 的是**测试** |
/// | "Modal 盖住 Popup"是缺陷 |❌ **不是** —— 枚举序 `Popup(2) < Modal(4)`，Modal 本就该盖住 Popup 并阻断下层（模态框的语义） |
///
/// ⇒ D6 的**现象当前不可达，且层序本身是正确设计**。真正成立且值得修的是：
/// **渲染层序与命中层序曾是两份手写数组**（`scene.rs:LAYER_BOTTOM_UP` 与
/// `hit.rs:LAYER_TOP_DOWN`），分散两文件、无任何机制保证互逆。漏改一处 ⇒
/// **"画在上面的层收不到点击"**（不报错的错）。现在两侧都从 `Layer::ALL`
/// 派生，结构上不可能不同步；这些测试是那道保证的回归护栏。
#[cfg(test)]
mod layer_order {
    use crate::track::{Kind, Layer, Track};
    use lieui_geom::{Point, Size};

    const WINDOW: Size = Size::new(300.0, 100.0);

    /// 端到端：**Modal 阻断下层**是模态框的核心语义。
    ///
    /// Modal 铺满整窗且 `blocks_below` ⇒ 无论点在哪儿，命中的都必须是 Modal。
    /// 这条同时钉住"命中层序里 Modal 确实排在 Popup 之上"
    /// —— 若两侧层序不同步（例如把Modal 排到 Popup 之下），本测试立刻失败。
    #[test]
    fn modal_blocks_everything_below() {
        let mut t = Track::new();
        let content = t.create(Kind::Box, None);
        t.get_mut(content).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
        t.add_root(Layer::Content, None, content);

        let popup = t.create(Kind::Box, None);
        t.get_mut(popup).unwrap().layout.dim = [100.0, 40.0];
        t.add_root(Layer::Popup, None, popup);

        let modal = t.create(Kind::Box, None);
        t.get_mut(modal).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
        t.add_root(Layer::Modal, None, modal);

        crate::layout::layout(&mut t, WINDOW);

        // Popup 区域内 ⇒ 命中 Modal（Modal 层高于 Popup，且阻断下层）
        let path = super::hit_path(&t, Point::new(50.0, 20.0));
        assert!(path.contains(&modal), "Modal 应盖住并阻断 Popup：{path:?}");
        assert!(!path.contains(&popup), "Modal 打开时 Popup 不该收到命中：{path:?}");
        // Popup 区域外同样命中 Modal（它铺满整窗）
        assert!(
            super::hit_path(&t, Point::new(250.0, 80.0)).contains(&modal),
            "Modal 铺满整窗 ⇒ 任意位置都应命中它"
        );
    }

    /// 同一场景的另一面：**没有 Modal 时 Popup 必须能收到命中**。
    ///
    /// 只测"Modal 阻断"不够 —— 若某次改动让 `blocks_below` 退化成"阻断一切"，
    /// 第一个测试仍会过而本测试会挂。两侧一起钉，方向才完整。
    #[test]
    fn popup_receives_hits_when_no_modal_is_above() {
        let mut t = Track::new();
        let content = t.create(Kind::Box, None);
        t.get_mut(content).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
        t.add_root(Layer::Content, None, content);

        let popup = t.create(Kind::Box, None);
        t.get_mut(popup).unwrap().layout.dim = [100.0, 40.0];
        t.add_root(Layer::Popup, None, popup);

        crate::layout::layout(&mut t, WINDOW);

        let path = super::hit_path(&t, Point::new(50.0, 20.0));
        assert!(path.contains(&popup), "无 Modal 遮挡时 Popup 应可命中：{path:?}");
    }

    /// ★ **`Root.owner` 缺失时全靠这条规则**：同层内**后声明的盖在上面**。
    ///
    /// 现有子菜单正是这么做的（同级 `Popup` + 后声明在上），
    /// 所以"同层序号"这条 z 序规则才是**实际被依赖**的那条 ——
    /// 而它此前只由两份手写数组的**同层遍历方向**（渲染正序 / 命中逆序）隐式保证。
    #[test]
    fn later_declared_root_is_above_within_same_layer() {
        let mut t = Track::new();
        let content = t.create(Kind::Box, None);
        t.get_mut(content).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
        t.add_root(Layer::Content, None, content);

        // 两个**完全重叠**的 Popup，只靠声明顺序分前后
        let bottom = t.create(Kind::Box, None);
        t.get_mut(bottom).unwrap().layout.dim = [100.0, 40.0];
        t.add_root(Layer::Popup, None, bottom);

        let top = t.create(Kind::Box, None);
        t.get_mut(top).unwrap().layout.dim = [100.0, 40.0];
        t.add_root(Layer::Popup, None, top);

        crate::layout::layout(&mut t, WINDOW);

        let path = super::hit_path(&t, Point::new(50.0, 20.0));
        assert!(
            path.contains(&top) && !path.contains(&bottom),
            "同层后声明者应盖在上面（命中须与绘制一致）：{path:?}"
        );
    }

    /// `Overlay`（水印/装饰）默认**命中穿透** —— 排在Popup 之下但仍高于 Content。
    ///
    /// 这条钉住"层的**语义**配置（`LayerOpts`）与层**顺序**是两件独立的事"：
    /// 顺序决定谁先被测，穿透性决定测了算不算。
    #[test]
    fn overlay_is_hit_transparent_by_default() {
        let mut t = Track::new();
        let content = t.create(Kind::Box, None);
        t.get_mut(content).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
        t.add_root(Layer::Content, None, content);

        let overlay = t.create(Kind::Box, None);
        t.get_mut(overlay).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
        t.add_root(Layer::Overlay, None, overlay);

        crate::layout::layout(&mut t, WINDOW);

        let path = super::hit_path(&t, Point::new(50.0, 20.0));
        assert!(!path.contains(&overlay), "Overlay 默认应命中穿透：{path:?}");
        assert!(path.contains(&content), "穿透后应落到它下面的 Content：{path:?}");
    }

    /// `Layer::ALL` 必须是**全部**层，且顺序与枚举声明序（= z 序语义）一致。
    ///
    /// `Content` 最低、`DragPreview` 最高 —— 这两条是语义断言，
    /// 防"有人为了修某个 bug 随手调换 `ALL` 的顺序"。
    #[test]
    fn layer_all_matches_enum_declaration_order() {
        assert_eq!(
            Layer::ALL.len(),
            6,
            "新增 Layer 变体时必须同步更新 Layer::ALL（否则该层不会被渲染/命中遍历到）"
        );
        assert_eq!(Layer::ALL[0], Layer::Content, "Content 必须最低");
        assert_eq!(
            Layer::ALL[Layer::ALL.len() - 1],
            Layer::DragPreview,
            "DragPreview 必须最高"
        );
        let pos = |l: Layer| Layer::ALL.iter().position(|x| *x == l).unwrap();
        assert!(pos(Layer::Overlay) < pos(Layer::Popup));
        assert!(pos(Layer::Popup) < pos(Layer::Tooltip));
        assert!(pos(Layer::Tooltip) < pos(Layer::Modal));
    }
}

/// z 序嵌套的端到端验证（D6 / A 方案）。
///
/// ## 这些测试证明什么
///
/// A 方案（补API + 实现 z 序）的核心承诺是：
/// **嵌套在父层里的层，绘制与命中都在父层之上**。
/// 例如 Modal 里声明的 Popup（`owner` = Modal）不会被 Modal 的 backdrop 盖住、
/// 也能收到点击 —— 否则它既看不见也点不动。
///
/// 此前 `Root.owner` 无消费点、`z = (Layer, 嵌套深度, 序号)` 未实现，
/// 所以"Modal 内弹菜单"这个场景**无法工作**。现在可以了。
#[cfg(test)]
mod nested_z_order {
    use crate::track::{Kind, Layer, RootId, Track};
    use lieui_geom::{Point, Size};

    const WINDOW: Size = Size::new(300.0, 100.0);

    fn boxed(t: &mut Track, w: f32, h: f32) -> crate::track::NodeId {
        let id = t.create(Kind::Box, None);
        t.get_mut(id).unwrap().layout.dim = [w, h];
        id
    }

    /// ★ 核心：Modal 里的嵌套 Popup **盖在 Modal 之上且能收到点击**。
    ///
    /// 关键点：`Popup` 的枚举序**低于** `Modal`（2 < 4）。所以只有当 z 序
    /// **真的消费了 `owner`**（嵌套深度 1 > 0）时，它才会排在 Modal 之上。
    /// 换句话说：**这条测试在 z 序实现之前必然失败** —— 它正是 A 方案的存在理由。
    #[test]
    fn nested_popup_above_its_modal_parent() {
        let mut t = Track::new();
        let content = boxed(&mut t, WINDOW.width, WINDOW.height);
        t.add_root(Layer::Content, None, content);

        // Modal 铺满整窗（backdrop + blocks_below）
        let modal = boxed(&mut t, WINDOW.width, WINDOW.height);
        let modal_rid = t.add_root(Layer::Modal, None, modal);

        // ★ Popup 的枚举序低于 Modal，但 owner = modal ⇒ 深度 1 ⇒ 应在 Modal 之上
        let popup = boxed(&mut t, 120.0, 40.0);
        t.add_root(Layer::Popup, Some(modal_rid), popup);

        crate::layout::layout(&mut t, WINDOW);

        // ① 命中：Popup 区域内的点必须命中 Popup，而不是被 Modal 抢走
        let path = super::hit_path(&t, Point::new(60.0, 20.0));
        assert!(
            path.contains(&popup),
            "嵌套 Popup 应能收到点击（z 序未消费 owner？）：{path:?}"
        );
        assert!(!path.contains(&modal), "Modal 不该抢走嵌套 Popup 上的命中：{path:?}");

        // ② 绘制：Popup 必须**后画**（否则被 Modal 的 backdrop 盖住）
        let order: Vec<crate::track::NodeId> = t.z_ordered_roots().iter().map(|r| r.node).collect();
        let i_popup = order.iter().position(|n| *n == popup).unwrap();
        let i_modal = order.iter().position(|n| *n == modal).unwrap();
        assert!(
            i_popup > i_modal,
            "嵌套 Popup 必须后画（绘制序 idx {} 应 > Modal 的 {}）",
            i_popup,
            i_modal
        );
    }

    /// 命中侧与渲染侧**必须严格反向**遍历同一序列。
    ///
    /// 这是整个 z 序改造的核心不变量：两侧一旦不同步，
    /// 就会出现"画在上面的层点不到"。
    #[test]
    fn hit_order_is_exact_reverse_of_paint_order() {
        let mut t = Track::new();
        let content = boxed(&mut t, WINDOW.width, WINDOW.height);
        t.add_root(Layer::Content, None, content);
        let overlay = boxed(&mut t, WINDOW.width, WINDOW.height);
        t.add_root(Layer::Overlay, None, overlay);
        let popup_a = boxed(&mut t, 60.0, 30.0);
        t.add_root(Layer::Popup, None, popup_a);
        let modal = boxed(&mut t, WINDOW.width, WINDOW.height);
        let modal_rid = t.add_root(Layer::Modal, None, modal);
        let popup_in_modal = boxed(&mut t, 60.0, 30.0);
        t.add_root(Layer::Popup, Some(modal_rid), popup_in_modal);
        let tip = boxed(&mut t, 40.0, 20.0);
        t.add_root(Layer::Tooltip, None, tip);

        let paint: Vec<_> = t.z_ordered_roots().iter().map(|r| r.node).collect();
        let hit: Vec<_> = t.z_ordered_roots_top_down().iter().map(|r| r.node).collect();
        let mut paint_rev = paint.clone();
        paint_rev.reverse();
        assert_eq!(
            paint_rev, hit,
            "命中顺序必须是绘制顺序的严格逆序\n绘制: {paint:?}\n命中: {hit:?}"
        );
    }

    /// 嵌套 **Tooltip** 同理（设计 §3.7 提到Tooltip）。
    #[test]
    fn nested_tooltip_above_its_parent() {
        let mut t = Track::new();
        let content = boxed(&mut t, WINDOW.width, WINDOW.height);
        t.add_root(Layer::Content, None, content);
        let popup = boxed(&mut t, WINDOW.width, WINDOW.height);
        let popup_rid = t.add_root(Layer::Popup, None, popup);
        // Tooltip 枚举序(3) 高于 Popup(2)，本就在上；这里验证**嵌套**也不破坏它
        let tip = boxed(&mut t, 50.0, 20.0);
        t.add_root(Layer::Tooltip, Some(popup_rid), tip);

        let order: Vec<_> = t.z_ordered_roots().iter().map(|r| r.node).collect();
        let i_tip = order.iter().position(|n| *n == tip).unwrap();
        let i_popup = order.iter().position(|n| *n == popup).unwrap();
        assert!(i_tip > i_popup, "嵌套 Tooltip 应在父层之上");
    }

    /// 悬空 `owner` 必须**退化为顶层**而不是panic 或死循环。
    ///
    /// 真实触发路径：`Cmd` 单独移除父层、或对齐阶段先删父再删子。
    #[test]
    fn dangling_owner_degrades_to_top_level() {
        let mut t = Track::new();
        let content = boxed(&mut t, WINDOW.width, WINDOW.height);
        t.add_root(Layer::Content, None, content);

        // 伪造一个指向不存在层的 owner
        let orphan = boxed(&mut t, 50.0, 20.0);
        t.add_root(Layer::Popup, Some(RootId(9999)), orphan);

        let order: Vec<_> = t.z_ordered_roots().iter().map(|r| r.node).collect();
        assert!(order.contains(&orphan), "悬空 owner 的层仍应可排序");
        assert_eq!(order.len(), 2, "两个层都要在序列里");
    }

    /// `owner` 成环时必须**截断**而不是无限循环。
    #[test]
    fn cyclic_owner_is_truncated() {
        let mut t = Track::new();
        let content = boxed(&mut t, WINDOW.width, WINDOW.height);
        t.add_root(Layer::Content, None, content);

        let a = boxed(&mut t, 50.0, 20.0);
        let b = boxed(&mut t, 50.0, 20.0);
        let ra = t.add_root(Layer::Popup, None, a);
        let rb = t.add_root(Layer::Popup, None, b);
        // 人工成环：a→b→a
        t.root_mut(ra).unwrap().owner = Some(rb);
        t.root_mut(rb).unwrap().owner = Some(ra);

        let order: Vec<_> = t.z_ordered_roots().iter().map(|r| r.node).collect();
        assert_eq!(order.len(), 3, "成环不应丢层");
        // a 先声明 ⇒ 同 (layer, 截断深度) 下 a 在下
        assert!(
            order.iter().position(|n| *n == a) < order.iter().position(|n| *n == b),
            "成环截断后仍应保持稳定顺序"
        );
    }
}
