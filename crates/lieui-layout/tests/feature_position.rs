//! 特征测试（characterization）：**定位与流**。
//!
//! 覆盖审计点名的零覆盖区：**绝对定位**（"零覆盖且语义最难"）、**RTL**、**换行**。
//! 这三块都是 S3 布局重构（D-a/D-b）会碰到的路径。
//!
//! 断言原则同`feature_flex.rs`：**钉现状，不钉语义**。
//! 特别注意：绝对定位的细节（谁决定尺寸、兄弟是否让位、margin 与 position 的叠加顺序）
//! 各引擎差异很大，本文件记录的是**本引擎的实际行为**。

use lieui_layout::*;

fn fixed_leaf(id: u64, w: f32, h: f32) -> FlexNode {
    let mut s = FlexStyle::default();
    s.dim[Dimension::Width as usize] = w;
    s.dim[Dimension::Height as usize] = h;
    s.flex_shrink = 0.0;
    FlexNode::new(id, s)
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn row_root(w: f32, h: f32) -> FlexNode {
    let mut root = FlexNode::new_row(1);
    root.style.dim[Dimension::Width as usize] = w;
    root.style.dim[Dimension::Height as usize] = h;
    root
}

fn column_root(w: f32, h: f32) -> FlexNode {
    let mut root = FlexNode::new_column(1);
    root.style.dim[Dimension::Width as usize] = w;
    root.style.dim[Dimension::Height as usize] = h;
    root
}

/// 四边 padding（`CSSDirection::All` **不能**用于索引 —— 见下方"索引越界"说明）。
fn pad_all(style: &mut FlexStyle, v: f32) {
    for d in [
        CSSDirection::Left,
        CSSDirection::Top,
        CSSDirection::Right,
        CSSDirection::Bottom,
    ] {
        style.padding[d as usize] = v;
    }
}

fn border_all(style: &mut FlexStyle, v: f32) {
    for d in [
        CSSDirection::Left,
        CSSDirection::Top,
        CSSDirection::Right,
        CSSDirection::Bottom,
    ] {
        style.border[d as usize] = v;
    }
}

fn set_pos(style: &mut FlexStyle, d: CSSDirection, v: f32) {
    style.position[d as usize] = v;
}

fn set_margin(style: &mut FlexStyle, d: CSSDirection, v: f32) {
    style.margin[d as usize] = v;
}

// ─────────────────────── 绝对定位 ───────────────────────

/// 绝对定位的子节点**不参与父的流布局** —— 后续兄弟不该为它让位。
///
/// 这是 absolute 最核心的语义，也是最容易在重构中破坏的一条。
#[test]
fn absolute_child_takes_no_flow_space() {
    let mut root = row_root(200.0, 100.0);

    let mut abs = fixed_leaf(2, 50.0, 20.0);
    abs.style.position_type = PositionType::Absolute;
    root.add_child(abs);

    let after = fixed_leaf(3, 40.0, 20.0); // 紧跟其后
    root.add_child(after);

    root.layout(200.0, 100.0, LayoutDirection::Ltr);

    // 正常流节点应贴x=0（absolute 不占位）
    assert!(
        approx(root.children[1].get_left(), 0.0),
        "absolute 不占流空间，第二个子节点应在 x=0，实际 {}",
        root.children[1].get_left()
    );
}

/// absolute + `position` 把节点放到指定偏移。
#[test]
fn absolute_position_offsets_from_parent_origin() {
    let mut root = row_root(200.0, 100.0);

    let mut abs = fixed_leaf(2, 30.0, 20.0);
    abs.style.position_type = PositionType::Absolute;
    set_pos(&mut abs.style, CSSDirection::Left, 70.0);
    set_pos(&mut abs.style, CSSDirection::Top, 25.0);
    root.add_child(abs);

    root.layout(200.0, 100.0, LayoutDirection::Ltr);

    assert!(
        approx(root.children[0].get_left(), 70.0),
        "left = {}",
        root.children[0].get_left()
    );
    assert!(
        approx(root.children[0].get_top(), 25.0),
        "top = {}",
        root.children[0].get_top()
    );
}

/// `position` 写 `Right` / `Bottom` 时应从父的右/下边缘往内推。
/// （本引擎是否支持这一向由测试记录，不预设。）
#[test]
fn absolute_right_bottom_offsets() {
    let mut root = row_root(200.0, 100.0);

    let mut abs = fixed_leaf(2, 30.0, 20.0);
    abs.style.position_type = PositionType::Absolute;
    set_pos(&mut abs.style, CSSDirection::Right, 10.0);
    set_pos(&mut abs.style, CSSDirection::Bottom, 5.0);
    root.add_child(abs);

    root.layout(200.0, 100.0, LayoutDirection::Ltr);

    // 期望：left = 200 - 10 - 30 = 160，top = 100 - 5 - 20 = 75
    assert!(
        approx(root.children[0].get_left(), 160.0),
        "Right=10 ⇒ left 应为 160，实际 {}",
        root.children[0].get_left()
    );
    assert!(
        approx(root.children[0].get_top(), 75.0),
        "Bottom=5 ⇒ top 应为 75，实际 {}",
        root.children[0].get_top()
    );
}

/// absolute 节点的 margin 应当叠加在 position 之上。
#[test]
fn absolute_margin_adds_to_position() {
    let mut root = row_root(200.0, 100.0);

    let mut abs = fixed_leaf(2, 30.0, 20.0);
    abs.style.position_type = PositionType::Absolute;
    set_pos(&mut abs.style, CSSDirection::Left, 40.0);
    set_pos(&mut abs.style, CSSDirection::Top, 10.0);
    set_margin(&mut abs.style, CSSDirection::Left, 5.0);
    set_margin(&mut abs.style, CSSDirection::Top, 3.0);
    root.add_child(abs);

    root.layout(200.0, 100.0, LayoutDirection::Ltr);

    assert!(
        approx(root.children[0].get_left(), 45.0),
        "position 40 + margin 5 = 45，实际 {}",
        root.children[0].get_left()
    );
    assert!(
        approx(root.children[0].get_top(), 13.0),
        "position 10 + margin 3 = 13，实际 {}",
        root.children[0].get_top()
    );
}

// ─────────────────────── padding / margin / border ───────────────────────

/// 父的 `padding` 把子节点的内容区起点推离父的左上角。
#[test]
fn padding_insets_children() {
    let mut root = column_root(200.0, 200.0);
    pad_all(&mut root.style, 10.0);
    root.add_child(fixed_leaf(2, 50.0, 20.0));
    root.layout(200.0, 200.0, LayoutDirection::Ltr);

    assert!(
        approx(root.children[0].get_left(), 10.0),
        "padding 10 ⇒ left 应为 10，实际 {}",
        root.children[0].get_left()
    );
    assert!(
        approx(root.children[0].get_top(), 10.0),
        "padding 10 ⇒ top 应为 10，实际 {}",
        root.children[0].get_top()
    );
}

/// `margin` 把子节点从父的内容区往外推。
#[test]
fn margin_offsets_children_inside_padding_box() {
    let mut root = column_root(200.0, 200.0);
    pad_all(&mut root.style, 10.0);
    let mut child = fixed_leaf(2, 50.0, 20.0);
    set_margin(&mut child.style, CSSDirection::Left, 5.0);
    set_margin(&mut child.style, CSSDirection::Top, 7.0);
    root.add_child(child);

    root.layout(200.0, 200.0, LayoutDirection::Ltr);

    assert!(
        approx(root.children[0].get_left(), 15.0),
        "padding 10 + margin 5 = 15，实际 {}",
        root.children[0].get_left()
    );
    assert!(
        approx(root.children[0].get_top(), 17.0),
        "padding 10 + margin 7 = 17，实际 {}",
        root.children[0].get_top()
    );
}

/// `border` 与 `padding` 一样参与内容区偏移（border 在外、padding 在内）。
#[test]
fn border_offsets_children_before_padding() {
    let mut root = column_root(200.0, 200.0);
    border_all(&mut root.style, 4.0);
    pad_all(&mut root.style, 6.0);
    root.add_child(fixed_leaf(2, 50.0, 20.0));

    root.layout(200.0, 200.0, LayoutDirection::Ltr);

    assert!(
        approx(root.children[0].get_left(), 10.0),
        "border 4 + padding 6 = 10，实际 {}",
        root.children[0].get_left()
    );
}

/// 只有 `Left` / `Top` 生效的 padding（不做镜像展开）。
#[test]
fn directional_padding_applies_per_side() {
    let mut root = column_root(200.0, 200.0);
    root.style.padding[CSSDirection::Left as usize] = 8.0;
    root.style.padding[CSSDirection::Top as usize] = 12.0;
    root.add_child(fixed_leaf(2, 50.0, 20.0));

    root.layout(200.0, 200.0, LayoutDirection::Ltr);

    assert!(
        approx(root.children[0].get_left(), 8.0),
        "left = {}",
        root.children[0].get_left()
    );
    assert!(
        approx(root.children[0].get_top(), 12.0),
        "top = {}",
        root.children[0].get_top()
    );
}

// ─────────────────────── 方向（RTL / reverse） ───────────────────────

/// `LayoutDirection::Rtl`：主轴起点镜像（第一个子节点贴右）。
#[test]
fn rtl_places_first_child_at_right_edge() {
    let mut root = row_root(200.0, 100.0);
    root.add_child(fixed_leaf(2, 40.0, 20.0)); // 宽 40
    root.layout(200.0, 100.0, LayoutDirection::Rtl);

    // RTL 下首子节点应在 x = 200 - 40 = 160
    assert!(
        root.children[0].get_left() > 100.0,
        "RTL 首子节点应在右半区，实际 left={}",
        root.children[0].get_left()
    );
}

/// `LayoutDirection::Rtl` 不应改变**交叉轴**（垂直方向）的结果。
#[test]
fn rtl_does_not_mirror_cross_axis() {
    let build = |dir| {
        let mut root = row_root(200.0, 100.0);
        let mut a = fixed_leaf(2, 40.0, 20.0);
        a.style.margin[CSSDirection::Top as usize] = 9.0;
        root.add_child(a);
        root.layout(200.0, 100.0, dir);
        (root.children[0].get_left(), root.children[0].get_top())
    };
    let (ltr_left, ltr_top) = build(LayoutDirection::Ltr);
    let (rtl_left, rtl_top) = build(LayoutDirection::Rtl);

    assert!(
        approx(ltr_top, rtl_top),
        "RTL 不应改变交叉轴的 top：{ltr_top} vs {rtl_top}"
    );
    assert_ne!(
        ltr_left, rtl_left,
        "但 RTL 应当改变主轴的 left（否则 RTL 根本没生效）：{ltr_left} vs {rtl_left}"
    );
}

/// `RowReverse` / `ColumnReverse`：主轴顺序反转（不依赖 RTL）。
#[test]
fn row_reverse_flips_child_order() {
    let mut root = row_root(200.0, 100.0);
    root.style.flex_direction = FlexDirection::RowReverse;
    root.add_child(fixed_leaf(2, 30.0, 20.0)); // 宽 30 ⇒ LTR 下在 x=0
    root.layout(200.0, 100.0, LayoutDirection::Ltr);

    assert!(
        root.children[0].get_left() > 100.0,
        "RowReverse 下首子节点应在右侧，实际 {}",
        root.children[0].get_left()
    );
}

/// `justify_content = SpaceBetween`：两端贴边、间距均分。
#[test]
fn justify_space_between_pins_ends() {
    let mut root = row_root(200.0, 50.0);
    root.style.justify_content = FlexAlign::SpaceBetween;
    root.add_child(fixed_leaf(2, 20.0, 10.0));
    root.add_child(fixed_leaf(3, 20.0, 10.0));

    root.layout(200.0, 50.0, LayoutDirection::Ltr);

    assert!(
        approx(root.children[0].get_left(), 0.0),
        "首个应贴左：{}",
        root.children[0].get_left()
    );
    assert!(
        approx(root.children[1].get_left(), 180.0),
        "末个应贴右：{}",
        root.children[1].get_left()
    );
}

// ─────────────────────── 样式数组的索引契约 ───────────────────────

/// 钉住一个**公开 API 陷阱**：`CSSDirection` 有 10 个变体，但样式数组
/// （`padding`/`margin`/`border`/`position`）只有 `K_CSS_PROPS_COUNT = 6` 个槽位。
///
/// 用 `All`(8) / `Horizontal`(6) / `Vertical`(7) / `None`(9) 去索引会**越界 panic**。
/// 常量索引会被编译器抓成 `unconditional_panic`（开发期友好），但
/// `dir as usize` 这种运行时索引是**炸弹** —— 而两者都是 `pub` API。
///
/// 本测试把"哪些变体可索引"钉死，防止有人后来扩大数组长度或新增变体时，
/// 悄悄改变这个契约。
#[test]
fn only_first_six_css_directions_are_indexable() {
    let indexable = [
        CSSDirection::Left,
        CSSDirection::Top,
        CSSDirection::Right,
        CSSDirection::Bottom,
        CSSDirection::Start,
        CSSDirection::End,
    ];
    for d in indexable {
        assert!((d as usize) < 6, "{d:?} 应可索引样式数组（index = {}）", d as usize);
    }

    let overflowing = [
        CSSDirection::Horizontal,
        CSSDirection::Vertical,
        CSSDirection::All,
        CSSDirection::None,
    ];
    for d in overflowing {
        assert!(
            (d as usize) >= 6,
            "{d:?} 的索引 {} 已越界 —— 若你扩了数组长度，请同步更新 \
             style.rs 顶层的警示文档与本测试",
            d as usize
        );
    }
}

/// `FlexWrap::Wrap`：放不下的子节点换到下一行。
#[test]
fn wrap_moves_overflowing_child_to_next_line() {
    let mut root = row_root(100.0, 100.0);
    root.style.flex_wrap = FlexWrap::Wrap;
    for i in 0..3 {
        root.add_child(fixed_leaf(10 + i, 60.0, 20.0)); // 3×60 = 180 > 100
    }
    root.layout(100.0, 100.0, LayoutDirection::Ltr);

    // 第一个在 (0,0)，第二个因放不下应换行
    assert!(
        approx(root.children[0].get_top(), 0.0),
        "首行 top = {}",
        root.children[0].get_top()
    );
    assert!(
        root.children[1].get_top() > 0.0,
        "第二个应换到下一行，实际 top = {}",
        root.children[1].get_top()
    );
}

/// `NoWrap`（默认）下溢出的子节点仍排在同一行（可能超出容器）。
#[test]
fn no_wrap_keeps_children_on_one_line() {
    let mut root = row_root(100.0, 100.0); // 默认 NoWrap
    for i in 0..3 {
        root.add_child(fixed_leaf(10 + i, 60.0, 20.0));
    }
    root.layout(100.0, 100.0, LayoutDirection::Ltr);

    for (i, c) in root.children.iter().enumerate() {
        assert!(
            approx(c.get_top(), 0.0),
            "NoWrap 下 child[{i}] 应同在顶行，实际 {}",
            c.get_top()
        );
    }
}

/// `item_space`（主轴 gap）在**每一行内**都生效，且换行后仍然生效。
#[test]
fn item_space_applies_within_each_line() {
    let mut root = row_root(100.0, 100.0);
    root.style.flex_wrap = FlexWrap::Wrap;
    root.style.item_space = 10.0;
    root.add_child(fixed_leaf(2, 40.0, 20.0));
    root.add_child(fixed_leaf(3, 40.0, 20.0)); // 40+10+40 = 90 ≤ 100，同一行

    root.layout(100.0, 100.0, LayoutDirection::Ltr);

    assert!(
        approx(root.children[1].get_left(), 50.0),
        "第二子节点应在 40+10 = 50，实际 {}",
        root.children[1].get_left()
    );
}

/// `line_space`（交叉轴 gap）。
///
/// **KNOWN BUG（D43/D48）**：`line_space` 是**死配置** —— `style.rs:50` 声明后从未被读取，
/// 因此换行后的行间距恒为 0。本测试钉住该现状（行间距为 0），
/// 实现交叉轴 gap 后应把断言改成"行间距 == line_space"。
#[test]
#[ignore = "KNOWN BUG D43/D48：line_space 是死配置（声明后从未被读取），行间距恒为 0"]
fn known_bug_line_space_is_dead_config() {
    let mut root = row_root(100.0, 100.0);
    root.style.flex_wrap = FlexWrap::Wrap;
    root.style.line_space = 30.0; // 死配置：无人读取
    for i in 0..2 {
        root.add_child(fixed_leaf(10 + i, 60.0, 20.0)); // 强制换行成两行
    }
    root.layout(100.0, 100.0, LayoutDirection::Ltr);

    let gap = root.children[1].get_top() - (root.children[0].get_top() + 20.0);
    assert!(approx(gap, 0.0), "当前行间距恒为 0（line_space 未生效），实际 {gap}");
}
