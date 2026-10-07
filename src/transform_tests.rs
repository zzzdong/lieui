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
