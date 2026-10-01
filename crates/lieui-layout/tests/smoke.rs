//! M0 冒烟：抽取成独立 crate 后，Flex 引擎仍能正常求解。
//!
//! 断言一律用"确定性 + 容差"的方式（固定尺寸断言精确值，位置关系断言不等式），
//! 避免把 item_space / justify 的具体语义写死在测试里。

use lieui_layout::*;

fn fixed_leaf(id: u64, w: f32, h: f32) -> FlexNode {
    let mut s = FlexStyle::default();
    s.dim[Dimension::Width as usize] = w;
    s.dim[Dimension::Height as usize] = h;
    FlexNode::new(id, s)
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn row_lays_out_fixed_children() {
    let mut root = FlexNode::new_row(1);
    root.style.dim[Dimension::Width as usize] = 200.0;
    root.style.dim[Dimension::Height as usize] = 100.0;
    root.style.item_space = 10.0;

    root.add_child(fixed_leaf(2, 50.0, 60.0));
    root.add_child(fixed_leaf(3, 30.0, 60.0));

    root.layout(200.0, 100.0, LayoutDirection::Ltr);

    assert!(approx(root.get_width(), 200.0), "root width = {}", root.get_width());
    assert!(approx(root.get_height(), 100.0), "root height = {}", root.get_height());
    assert!(approx(root.children[0].get_width(), 50.0));
    assert!(approx(root.children[1].get_width(), 30.0));
    // 第二个子节点必须排第一个之后（间距语义不写死）
    assert!(
        root.children[1].get_left() >= 50.0,
        "second child left = {}",
        root.children[1].get_left()
    );
}

#[test]
fn column_stacks_children() {
    let mut root = FlexNode::new_column(1);
    root.style.dim[Dimension::Width as usize] = 100.0;
    root.add_child(fixed_leaf(2, 100.0, 20.0));
    root.add_child(fixed_leaf(3, 100.0, 20.0));

    root.layout(100.0, 200.0, LayoutDirection::Ltr);

    assert!(approx(root.children[0].get_height(), 20.0));
    assert!(
        root.children[1].get_top() >= 20.0,
        "second child top = {}",
        root.children[1].get_top()
    );
}

#[test]
fn flex_grow_fills_main_axis() {
    let mut root = FlexNode::new_row(1);
    root.style.dim[Dimension::Width as usize] = 200.0;

    let mut a = fixed_leaf(2, 50.0, 10.0);
    a.style.flex_grow = 1.0;
    root.add_child(a);

    root.layout(200.0, 50.0, LayoutDirection::Ltr);

    assert!(
        root.children[0].get_width() > 50.0,
        "grow did not happen: {}",
        root.children[0].get_width()
    );
}

#[test]
fn text_leaf_remeasures_under_constraint() {
    // 文本叶子在约束宽度下重新测量（走的是 lieui-text 的 parley 路径）。
    // 只做"不 panic + 尺寸有效"的弱断言：具体换行结果依赖系统字体。
    let spec = lieui_text::TextSpec::new(16.0);
    let mut root = FlexNode::new_column(1);
    root.style.dim[Dimension::Width as usize] = 120.0;

    let mut t = FlexNode::new(2, FlexStyle::default());
    t.intrinsic_size = Some((400.0, 20.0));
    t.measure_text = Some((
        "hello world hello world hello world hello world".into(),
        spec,
    ));
    root.add_child(t);

    root.layout(120.0, 400.0, LayoutDirection::Ltr);

    let w = root.children[0].get_width();
    let h = root.children[0].get_height();
    assert!(w > 0.0 && h > 0.0, "text size = {w}x{h}");
}
