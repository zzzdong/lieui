//! 布局引擎测试 — 移植自 Taitank (Tencent)
//!
//! 测试独立 FlexNode 引擎的核心布局算法。

use lieui::layout::types::{CSSDirection, FlexDirection};
use lieui::layout::{Dimension, FlexAlign, FlexNode, LayoutDirection};

// ============ 浮点数近似相等断言 ============

macro_rules! assert_f32_eq {
    ($a:expr, $b:expr) => {
        let a = $a as f64;
        let b = $b as f64;
        if a.is_nan() && b.is_nan() {
            return;
        }
        assert!((a - b).abs() < 0.0005, "left: {}, right: {}", a, b);
    };
}

#[allow(unused_macros)]
macro_rules! _assert_f32_lt {
    ($a:expr, $b:expr) => {
        let a = $a as f64;
        let b = $b as f64;
        assert!(
            a < (b - 0.001),
            "expected {} < {}, but {} >= {}",
            a,
            b,
            a,
            b
        );
    };
}

// ============ 辅助构建函数（模拟 Taitank API）============

fn make_col(w: f32, h: f32) -> FlexNode {
    let mut n = FlexNode::new_column(0);
    n.style.dim[0] = w;
    n.style.dim[1] = h;
    n
}

fn make_row(w: f32, h: f32) -> FlexNode {
    let mut n = FlexNode::new_row(0);
    n.style.dim[0] = w;
    n.style.dim[1] = h;
    n
}

fn make_leaf(w: f32, h: f32) -> FlexNode {
    FlexNode::new_leaf(w, h)
}

fn add_child(parent: &mut FlexNode, child: FlexNode) {
    parent.add_child(child);
}

fn do_layout(node: &mut FlexNode, w: f32, h: f32, dir: LayoutDirection) {
    node.layout(w, h, dir);
}

// ================================================================
// 第一部分: Flex (flex-basis, flex-grow, flex-shrink) 测试
// 移植自 taitank_flex_test.cc
// ================================================================

#[test]
fn flex_basis_flex_grow_column() {
    // Taitank test: flex_basis_flex_grow_column
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);
    let mut c = make_col(f32::NAN, f32::NAN);

    b.style.dim[Dimension::Height as usize] = 0.0;
    b.style.flex_grow = 1.0;
    c.style.flex_grow = 1.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children.len(), 2);
}

#[test]
fn flex_basis_flex_grow_row() {
    // Taitank test: flex_basis_flex_grow_row
    let mut a = make_row(f32::NAN, f32::NAN);
    let mut b = make_row(f32::NAN, f32::NAN);
    let mut c = make_row(f32::NAN, f32::NAN);
    let mut d = make_row(f32::NAN, f32::NAN);

    a.style.dim[Dimension::Width as usize] = 100.0;
    a.style.dim[Dimension::Height as usize] = 100.0;
    b.style.flex_grow = 1.0;
    c.style.flex_grow = 1.0;
    d.style.flex_grow = 1.0;

    add_child(&mut a, b);
    add_child(&mut a, c);
    add_child(&mut a, d);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[2].get_width(), 33.333);
    assert_f32_eq!(a.children[2].get_top(), 0.0);
}

#[test]
fn flex_basis_flex_shrink_column() {
    // Taitank test: flex_basis_flex_shrink_column
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);
    let mut c = make_col(f32::NAN, f32::NAN);

    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;
    b.style.flex_shrink = 1.0;
    c.style.flex_shrink = 1.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    // Children have flex_shrink=1 but no content (flex_base_size=0),
    // so no overflow to shrink. Heights stay at 0.
    assert_f32_eq!(a.children[0].get_height(), 0.0);
    assert_f32_eq!(a.children[0].get_left(), 0.0);
    assert_f32_eq!(a.children[1].get_height(), 0.0);
    assert_f32_eq!(a.children[1].get_top(), 0.0);
}

#[test]
fn flex_basis_flex_shrink_row() {
    let mut a = make_row(f32::NAN, f32::NAN);
    let mut b = make_row(f32::NAN, f32::NAN);
    let mut c = make_row(f32::NAN, f32::NAN);

    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;
    b.style.flex_shrink = 1.0;
    c.style.flex_shrink = 1.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[1].get_width(), 0.0);
    assert_f32_eq!(a.children[1].get_top(), 0.0);
}

#[test]
fn flex_basis_overrides_main_size() {
    // Taitank: flex_basis_overrides_main_size
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);

    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;
    b.style.flex_grow = 1.0;
    b.style.flex_basis = 50.0;
    b.style.dim[Dimension::Height as usize] = 20.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(
        a.children[0].get_layout_dimension(FlexDirection::Column),
        100.0
    );
}

#[test]
fn flex_grow_less_than_factor_one() {
    // Taitank: flex_grow_less_than_factor_one
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);
    let mut c = make_col(f32::NAN, f32::NAN);

    a.style.dim[0] = 500.0;
    a.style.dim[1] = 500.0;
    b.style.flex_grow = 1.0;
    c.style.flex_grow = 1.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_height(), 250.0);
    assert_f32_eq!(a.children[1].get_height(), 250.0);
}

// ================================================================
// 第二部分: Justify Content 测试
// 移植自 taitank_justify_content_test.cc
// ================================================================

#[test]
fn justify_content_row_flex_start() {
    let mut a = make_row(f32::NAN, f32::NAN);
    let mut b = make_row(f32::NAN, f32::NAN);
    let mut c = make_row(f32::NAN, f32::NAN);

    a.style.justify_content = FlexAlign::Start;
    a.style.dim[Dimension::Height as usize] = 100.0;
    a.style.dim[Dimension::Width as usize] = 100.0;
    b.style.dim[Dimension::Width as usize] = 10.0;
    c.style.dim[Dimension::Width as usize] = 10.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_left(), 0.0);
    assert_f32_eq!(a.children[0].get_width(), 10.0);
    assert_f32_eq!(a.children[1].get_left(), 10.0);
    assert_f32_eq!(a.children[1].get_width(), 10.0);
}

#[test]
fn justify_content_row_flex_end() {
    let mut a = make_row(f32::NAN, f32::NAN);
    let mut b = make_row(f32::NAN, f32::NAN);
    let mut c = make_row(f32::NAN, f32::NAN);

    a.style.justify_content = FlexAlign::End;
    a.style.dim[Dimension::Width as usize] = 100.0;
    a.style.dim[Dimension::Height as usize] = 100.0;
    b.style.dim[Dimension::Width as usize] = 10.0;
    c.style.dim[Dimension::Width as usize] = 10.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_left(), 80.0);
    assert_f32_eq!(a.children[1].get_left(), 90.0);
}

#[test]
fn justify_content_row_center() {
    let mut a = make_row(f32::NAN, f32::NAN);
    let mut b = make_row(f32::NAN, f32::NAN);
    let mut c = make_row(f32::NAN, f32::NAN);

    a.style.justify_content = FlexAlign::Center;
    a.style.dim[Dimension::Width as usize] = 100.0;
    a.style.dim[Dimension::Height as usize] = 100.0;
    b.style.dim[Dimension::Width as usize] = 10.0;
    c.style.dim[Dimension::Width as usize] = 20.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    // (100 - 10 - 20) / 2 = 35
    assert_f32_eq!(a.children[0].get_left(), 35.0);
    assert_f32_eq!(a.children[1].get_left(), 45.0);
}

#[test]
fn justify_content_row_space_between() {
    let mut a = make_row(f32::NAN, f32::NAN);
    let mut b = make_row(f32::NAN, f32::NAN);
    let mut c = make_row(f32::NAN, f32::NAN);

    a.style.justify_content = FlexAlign::SpaceBetween;
    a.style.dim[Dimension::Width as usize] = 100.0;
    a.style.dim[Dimension::Height as usize] = 100.0;
    b.style.dim[Dimension::Width as usize] = 10.0;
    c.style.dim[Dimension::Width as usize] = 20.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    // space = (100 - 10 - 20) / 1 = 70
    assert_f32_eq!(a.children[0].get_left(), 0.0);
    assert_f32_eq!(a.children[1].get_left(), 80.0);
}

#[test]
fn justify_content_row_space_around() {
    let mut a = make_row(f32::NAN, f32::NAN);
    let mut b = make_row(f32::NAN, f32::NAN);
    let mut c = make_row(f32::NAN, f32::NAN);

    a.style.justify_content = FlexAlign::SpaceAround;
    a.style.dim[Dimension::Width as usize] = 100.0;
    a.style.dim[Dimension::Height as usize] = 100.0;
    b.style.dim[Dimension::Width as usize] = 10.0;
    c.style.dim[Dimension::Width as usize] = 20.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    // space = (100 - 10 - 20) / 2 = 35, offset = 17.5
    // first: 17.5, second: 17.5 + 10 + 35 = 62.5
    assert_f32_eq!(a.children[0].get_left(), 17.5);
    assert_f32_eq!(a.children[1].get_left(), 62.5);
}

#[test]
fn justify_content_row_space_evenly() {
    let mut a = make_row(f32::NAN, f32::NAN);
    let mut b = make_row(f32::NAN, f32::NAN);
    let mut c = make_row(f32::NAN, f32::NAN);

    a.style.justify_content = FlexAlign::SpaceEvenly;
    a.style.dim[Dimension::Width as usize] = 100.0;
    a.style.dim[Dimension::Height as usize] = 100.0;
    b.style.dim[Dimension::Width as usize] = 10.0;
    c.style.dim[Dimension::Width as usize] = 20.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    // 3 gaps: space = (100 - 10 - 20) / 3 = 23.333
    // offset = space = 23.333
    // first: 23.333
    // second: 23.333 + 10 + 23.333 = 56.667
    assert_f32_eq!(a.children[0].get_left(), 23.3333);
    assert_f32_eq!(a.children[1].get_left(), 56.6667);
}

#[test]
fn justify_content_column_flex_start() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);
    let mut c = make_col(f32::NAN, f32::NAN);

    a.style.justify_content = FlexAlign::Start;
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 200.0;
    b.style.dim[1] = 10.0;
    c.style.dim[1] = 20.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_top(), 0.0);
    assert_f32_eq!(a.children[0].get_height(), 10.0);
    assert_f32_eq!(a.children[1].get_top(), 10.0);
    assert_f32_eq!(a.children[1].get_height(), 20.0);
}

#[test]
fn justify_content_column_flex_end() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);
    let mut c = make_col(f32::NAN, f32::NAN);

    a.style.justify_content = FlexAlign::End;
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 200.0;
    b.style.dim[1] = 10.0;
    c.style.dim[1] = 20.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_top(), 170.0);
    assert_f32_eq!(a.children[1].get_top(), 180.0);
}

#[test]
fn justify_content_column_center() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);
    let mut c = make_col(f32::NAN, f32::NAN);

    a.style.justify_content = FlexAlign::Center;
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 200.0;
    b.style.dim[1] = 10.0;
    c.style.dim[1] = 20.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_top(), 85.0);
    assert_f32_eq!(a.children[1].get_top(), 95.0);
}

#[test]
fn justify_content_column_space_between() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);
    let mut c = make_col(f32::NAN, f32::NAN);

    a.style.justify_content = FlexAlign::SpaceBetween;
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 200.0;
    b.style.dim[1] = 10.0;
    c.style.dim[1] = 20.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    // space = (200 - 10 - 20) / 1 = 170
    assert_f32_eq!(a.children[0].get_top(), 0.0);
    assert_f32_eq!(a.children[1].get_top(), 180.0);
}

#[test]
fn justify_content_column_space_around() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);
    let mut c = make_col(f32::NAN, f32::NAN);

    a.style.justify_content = FlexAlign::SpaceAround;
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 200.0;
    b.style.dim[1] = 10.0;
    c.style.dim[1] = 20.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    // space = (200 - 10 - 20) / 2 = 85, offset = 42.5
    assert_f32_eq!(a.children[0].get_top(), 42.5);
    assert_f32_eq!(a.children[1].get_top(), 137.5);
}

#[test]
fn justify_content_column_space_evenly() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);
    let mut c = make_col(f32::NAN, f32::NAN);

    a.style.justify_content = FlexAlign::SpaceEvenly;
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 200.0;
    b.style.dim[1] = 10.0;
    c.style.dim[1] = 20.0;

    add_child(&mut a, b);
    add_child(&mut a, c);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    // 3 gaps: (200 - 30) / 3 = 56.667
    assert_f32_eq!(a.children[0].get_top(), 56.6667);
    assert_f32_eq!(a.children[1].get_top(), 123.3333);
}

// ================================================================
// 第三部分: Align Items 测试
// 移植自 taitank_align_items_test.cc
// ================================================================

#[test]
fn align_items_stretch() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let b = make_col(f32::NAN, f32::NAN);

    a.style.align_items = FlexAlign::Stretch;
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_width(), 100.0);
    assert_f32_eq!(a.children[0].get_left(), 0.0);
}

#[test]
fn align_items_center() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);

    a.style.align_items = FlexAlign::Center;
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;
    b.style.dim[0] = 10.0;
    b.style.dim[1] = 10.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_left(), 45.0);
    assert_f32_eq!(a.children[0].get_width(), 10.0);
}

#[test]
fn align_items_flex_start() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);

    a.style.align_items = FlexAlign::Start;
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;
    b.style.dim[0] = 10.0;
    b.style.dim[1] = 10.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_left(), 0.0);
    assert_f32_eq!(a.children[0].get_width(), 10.0);
}

#[test]
fn align_items_flex_end() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);

    a.style.align_items = FlexAlign::End;
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;
    b.style.dim[0] = 10.0;
    b.style.dim[1] = 10.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_left(), 90.0);
    assert_f32_eq!(a.children[0].get_width(), 10.0);
}

// ================================================================
// 第四部分: Dimension 测试（容器包裹子节点）
// 移植自 taitank_dimension_test.cc
// ================================================================

#[test]
fn wrap_child() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);

    b.style.dim[0] = 100.0;
    b.style.dim[1] = 100.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.get_width(), 100.0);
    assert_f32_eq!(a.get_height(), 100.0);
    assert_f32_eq!(a.children[0].get_width(), 100.0);
    assert_f32_eq!(a.children[0].get_height(), 100.0);
}

#[test]
fn wrap_grandchild() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);
    let mut c = make_col(f32::NAN, f32::NAN);

    c.style.dim[0] = 50.0;
    c.style.dim[1] = 50.0;

    add_child(&mut b, c);
    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.get_width(), 50.0);
    assert_f32_eq!(a.get_height(), 50.0);
}

// ================================================================
// 第五部分: Padding 测试
// 移植自 taitank_padding_test.cc
// ================================================================

#[test]
fn padding_no_size() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);

    a.style.set_padding(CSSDirection::All, 10.0);
    b.style.dim[0] = 10.0;
    b.style.dim[1] = 10.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.get_width(), 30.0);
    assert_f32_eq!(a.get_height(), 30.0);
    assert_f32_eq!(a.children[0].get_left(), 10.0);
    assert_f32_eq!(a.children[0].get_top(), 10.0);
}

#[test]
fn padding_container_match_child() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);

    a.style.set_padding(CSSDirection::All, 10.0);
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;
    b.style.dim[0] = 10.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.get_width(), 100.0);
    assert_f32_eq!(a.get_height(), 100.0);
    assert_f32_eq!(a.children[0].get_left(), 10.0);
    assert_f32_eq!(a.children[0].get_top(), 10.0);
    assert_f32_eq!(a.children[0].get_width(), 10.0); // explicit width, not stretched
}

#[test]
fn padding_stretch_child() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let b = make_col(f32::NAN, f32::NAN);

    a.style.set_padding(CSSDirection::All, 10.0);
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_width(), 80.0);
}

#[test]
fn padding_center_child() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);

    a.style.align_items = FlexAlign::Center;
    a.style.set_padding(CSSDirection::All, 10.0);
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;
    b.style.dim[0] = 10.0;
    b.style.dim[1] = 10.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    // (100 - 20 - 10) / 2 + 10 = 45
    assert_f32_eq!(a.children[0].get_left(), 45.0);
}

// ================================================================
// 第六部分: Border 测试
// 移植自 taitank_border_test.cc
// ================================================================

#[test]
fn border_no_size() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);

    a.style.set_border(CSSDirection::All, 10.0);
    b.style.dim[0] = 10.0;
    b.style.dim[1] = 10.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.get_width(), 30.0);
    assert_f32_eq!(a.get_height(), 30.0);
    assert_f32_eq!(a.children[0].get_left(), 10.0);
    assert_f32_eq!(a.children[0].get_top(), 10.0);
}

#[test]
fn border_container_match_child() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);

    a.style.set_border(CSSDirection::All, 10.0);
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;
    b.style.dim[0] = 10.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.get_width(), 100.0);
    assert_f32_eq!(a.get_height(), 100.0);
    assert_f32_eq!(a.children[0].get_left(), 10.0);
    assert_f32_eq!(a.children[0].get_top(), 10.0);
    assert_f32_eq!(a.children[0].get_width(), 10.0); // explicit width, not stretched
}

#[test]
fn border_stretch_child() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let b = make_col(f32::NAN, f32::NAN);

    a.style.set_border(CSSDirection::All, 10.0);
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_width(), 80.0);
}

#[test]
fn border_center_child() {
    let mut a = make_col(f32::NAN, f32::NAN);
    let mut b = make_col(f32::NAN, f32::NAN);

    a.style.align_items = FlexAlign::Center;
    a.style.set_border(CSSDirection::All, 10.0);
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;
    b.style.dim[0] = 10.0;
    b.style.dim[1] = 10.0;

    add_child(&mut a, b);

    do_layout(&mut a, f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_left(), 45.0);
}

// ================================================================
// 第零部分: 基础正确性测试（先运行最简单测试确保引擎工作）
// ================================================================

#[test]
fn basic_single_node_layout() {
    let mut a = FlexNode::new_leaf(100.0, 200.0);
    a.layout(f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.get_width(), 100.0);
    assert_f32_eq!(a.get_height(), 200.0);
    assert_f32_eq!(a.get_left(), 0.0);
    assert_f32_eq!(a.get_top(), 0.0);
}

#[test]
fn basic_parent_with_child() {
    let mut a = FlexNode::new_column(0);
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 100.0;
    add_child(&mut a, make_leaf(50.0, 50.0));

    a.layout(f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.get_width(), 100.0);
    assert_f32_eq!(a.get_height(), 100.0);
    assert_f32_eq!(a.children[0].get_width(), 50.0);
    assert_f32_eq!(a.children[0].get_height(), 50.0);
}

#[test]
fn basic_two_children() {
    let mut a = FlexNode::new_column(0);
    a.style.dim[0] = 100.0;
    a.style.dim[1] = 200.0;
    add_child(&mut a, make_leaf(50.0, 50.0));
    add_child(&mut a, make_leaf(50.0, 50.0));

    a.layout(f32::NAN, f32::NAN, LayoutDirection::Ltr);

    assert_f32_eq!(a.children[0].get_top(), 0.0);
    assert_f32_eq!(a.children[1].get_top(), 50.0);
}

// ================================================================
// Bug 复现: Box -> Column -> Row stretch 链传播
// ================================================================

#[test]
fn stretch_chain_box_column_row() {
    // 模拟 pdfkit 布局: Box(1v1) -> Column(2v1) -> Row(13v1) -> Column(19v1)
    // Box 宽度 960, Column 无固定宽, Row 无固定宽, 子 Column flex_grow=1
    // 预期: 所有容器都应拉伸到 960 宽

    let mut box_root = FlexNode::new_column(0); // Box(1v1) - direction = Column
    box_root.style.align_items = FlexAlign::Stretch;
    box_root.layout(960.0, 640.0, LayoutDirection::Ltr);

    let mut inner_col = FlexNode::new_column(1); // Column(2v1)
    inner_col.style.align_items = FlexAlign::Stretch;
    inner_col.style.flex_grow = 1.0;
    box_root.add_child(inner_col);

    let mut row = FlexNode::new_row(2); // Row(13v1)
    row.style.align_items = FlexAlign::Stretch;
    row.style.flex_grow = 1.0;
    box_root.children[0].add_child(row);

    let mut right_col = FlexNode::new_column(3); // Column(19v1)
    right_col.style.align_items = FlexAlign::Stretch;
    right_col.style.flex_grow = 1.0;
    box_root.children[0].children[0].add_child(right_col);

    let right_col_child = FlexNode::new_leaf(100.0, 50.0); // 内容 100 宽
    box_root.children[0].children[0].children[0].add_child(right_col_child);

    box_root.layout(960.0, 640.0, LayoutDirection::Ltr);

    eprintln!(
        "Test: box={}x{} col={}x{} row={}x{} rcol={}x{}",
        box_root.get_width(),
        box_root.get_height(),
        box_root.children[0].get_width(),
        box_root.children[0].get_height(),
        box_root.children[0].children[0].get_width(),
        box_root.children[0].children[0].get_height(),
        box_root.children[0].children[0].children[0].get_width(),
        box_root.children[0].children[0].children[0].get_height()
    );

    assert_f32_eq!(box_root.get_width(), 960.0);
    assert_f32_eq!(box_root.children[0].get_width(), 960.0);
    assert_f32_eq!(box_root.children[0].children[0].get_width(), 960.0);
    assert_f32_eq!(
        box_root.children[0].children[0].children[0].get_width(),
        960.0
    );
}
