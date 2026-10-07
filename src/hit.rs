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
#[path = "hit_tests.rs"]
mod tests;
