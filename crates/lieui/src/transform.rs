//! 2D 仿射变换：**绘制 / 命中 / 裁剪共用同一矩阵**。
//!
//! 设计来源（`docs/architecture-v3.md` §3.13）：对齐 WinUI 的
//! `RenderTransform` + `RenderTransformOrigin` + `CenterPoint` + `Translation` + `Scale` + `Rotation`，
//! 但收敛成**一个** `Node.transform` 字段（见 [`crate::track::Transform`]）。
//!
//! 修复的现状缺口：旧实现的 `hit_test_rec` 只做矩形包含，完全不处理变换与裁剪，
//! 所以"缩放/旋转过的元素"命中区域是错的。
//!
//! 矩阵约定（PostScript 顺序）：`[a, b, c, d, e, f]` 表示
//! `x' = a·x + c·y + e`，`y' = b·x + d·y + f`。

use lieui_geom::{Point, Rect};

use crate::track::Transform;

/// 2D 仿射矩阵
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Affine {
    /// `[a, b, c, d, e, f]`
    pub m: [f32; 6],
}

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        m: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    };

    pub const fn new(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Self {
        Self { m: [a, b, c, d, e, f] }
    }

    pub const fn translate(tx: f32, ty: f32) -> Self {
        Self::new(1.0, 0.0, 0.0, 1.0, tx, ty)
    }

    pub const fn scale(sx: f32, sy: f32) -> Self {
        Self::new(sx, 0.0, 0.0, sy, 0.0, 0.0)
    }

    pub fn rotate_deg(deg: f32) -> Self {
        let r = deg.to_radians();
        let (s, c) = r.sin_cos();
        Self::new(c, s, -s, c, 0.0, 0.0)
    }

    pub fn is_identity(&self) -> bool {
        self.m == Self::IDENTITY.m
    }

    /// `self ∘ rhs`：**先应用 `rhs`，再应用 `self`**（与矩阵乘法一致）
    pub fn then(&self, rhs: Affine) -> Affine {
        let [a1, b1, c1, d1, e1, f1] = self.m;
        let [a2, b2, c2, d2, e2, f2] = rhs.m;
        Affine {
            m: [
                a1 * a2 + c1 * b2,
                b1 * a2 + d1 * b2,
                a1 * c2 + c1 * d2,
                b1 * c2 + d1 * d2,
                a1 * e2 + c1 * f2 + e1,
                b1 * e2 + d1 * f2 + f1,
            ],
        }
    }

    pub fn apply(&self, p: Point) -> Point {
        let [a, b, c, d, e, f] = self.m;
        Point::new(a * p.x + c * p.y + e, b * p.x + d * p.y + f)
    }

    /// 只作用于向量（忽略平移）
    pub fn apply_vector(&self, v: (f32, f32)) -> (f32, f32) {
        let [a, b, c, d, _, _] = self.m;
        (a * v.0 + c * v.1, b * v.0 + d * v.1)
    }

    /// 逆矩阵；退化矩阵（行列式 ≈ 0）返回 `None`
    pub fn inverse(&self) -> Option<Affine> {
        let [a, b, c, d, e, f] = self.m;
        let det = a * d - b * c;
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        Some(Affine {
            m: [
                d * inv,
                -b * inv,
                -c * inv,
                a * inv,
                (c * f - d * e) * inv,
                (b * e - a * f) * inv,
            ],
        })
    }

    /// 轴对齐矩形经本变换后的**包围盒**（用于绘制/裁剪；命中请用逆变换，不要用包围盒）
    pub fn bounding_box(&self, r: Rect) -> Rect {
        let pts = [
            self.apply(Point::new(r.x, r.y)),
            self.apply(Point::new(r.x + r.width, r.y)),
            self.apply(Point::new(r.x, r.y + r.height)),
            self.apply(Point::new(r.x + r.width, r.y + r.height)),
        ];
        let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
        let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
        for p in pts {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }
}

impl Transform {
    /// 由"渲染变换参数 + 元素矩形"构造矩阵。
    ///
    /// 语义：以 `origin`（归一化，0.5/0.5 = 元素中心）为基准做 **缩放 → 旋转**，
    /// 然后再施加 `translate`：
    /// `M = T(origin_abs + translate) · R(θ) · S(sx, sy) · T(-origin_abs)`
    pub fn matrix(&self, rect: Rect) -> Affine {
        if self.is_identity() {
            return Affine::IDENTITY;
        }
        let ox = rect.x + self.origin.0 * rect.width;
        let oy = rect.y + self.origin.1 * rect.height;

        Affine::translate(ox + self.translate.0, oy + self.translate.1)
            .then(Affine::rotate_deg(self.rotation_deg))
            .then(Affine::scale(self.scale.0, self.scale.1))
            .then(Affine::translate(-ox, -oy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Point, b: Point) {
        assert!(
            (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3,
            "期望 {b:?}，实际 {a:?}"
        );
    }

    #[test]
    fn identity_is_a_no_op() {
        let m = Affine::IDENTITY;
        approx(m.apply(Point::new(3.0, 4.0)), Point::new(3.0, 4.0));
        assert!(m.is_identity());
        assert!(m.inverse().unwrap().is_identity());
    }

    #[test]
    fn composition_applies_rhs_first() {
        // 先平移 (10,0)，再缩放 2 倍
        let m = Affine::scale(2.0, 2.0).then(Affine::translate(10.0, 0.0));
        approx(m.apply(Point::new(1.0, 1.0)), Point::new(22.0, 2.0));
    }

    #[test]
    fn inverse_round_trips() {
        let m = Affine::translate(5.0, -3.0)
            .then(Affine::rotate_deg(30.0))
            .then(Affine::scale(2.0, 0.5));
        let inv = m.inverse().unwrap();
        let p = Point::new(7.0, 11.0);
        approx(inv.apply(m.apply(p)), p);
    }

    #[test]
    fn degenerate_matrix_has_no_inverse() {
        assert!(Affine::scale(0.0, 1.0).inverse().is_none());
    }

    #[test]
    fn rotate_90_maps_axes() {
        let m = Affine::rotate_deg(90.0);
        approx(m.apply(Point::new(1.0, 0.0)), Point::new(0.0, 1.0));
    }

    #[test]
    fn transform_matrix_about_center() {
        let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
        let t = Transform {
            scale: (2.0, 2.0),
            ..Transform::default()
        };
        let m = t.matrix(rect);
        // 中心不动
        approx(m.apply(Point::new(50.0, 25.0)), Point::new(50.0, 25.0));
        // 左边缘外扩一倍
        approx(m.apply(Point::new(0.0, 25.0)), Point::new(-50.0, 25.0));
    }

    #[test]
    fn identity_transform_short_circuits() {
        let rect = Rect::new(10.0, 10.0, 20.0, 20.0);
        assert!(Transform::default().matrix(rect).is_identity());
    }

    #[test]
    fn translate_only() {
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let t = Transform {
            translate: (5.0, -5.0),
            ..Transform::default()
        };
        let m = t.matrix(rect);
        approx(m.apply(Point::new(0.0, 0.0)), Point::new(5.0, -5.0));
    }

    #[test]
    fn bounding_box_covers_rotated_rect() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let m = Affine::rotate_deg(45.0);
        let bb = m.bounding_box(r);
        let s = 10.0 * std::f32::consts::SQRT_2;
        assert!((bb.width - s).abs() < 1e-2 && (bb.height - s).abs() < 1e-2, "{bb:?}");
    }

    #[test]
    fn apply_vector_ignores_translation() {
        let m = Affine::translate(100.0, 100.0);
        assert_eq!(m.apply_vector((1.0, 2.0)), (1.0, 2.0));
    }
}
