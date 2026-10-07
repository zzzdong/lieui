//! 特征测试（characterization）：**flex 尺寸协商**。
//!
//! 这里是 S3（`refactor-plan` D-a/D-b 布局重构）最需要的保护网：
//! `flex_node.rs` 里 `calculate_items_flex_basis`（180 行）与
//! `resolve_flexible_lengths` 的 shrink 冻结循环都是"算法复杂度最高、测试最少"的部分，
//! 而重构前**零覆盖**。
//!
//! ## 断言原则
//!
//! 1. **钉现状，不钉语义** —— 断言"当前实际输出"，不断言"CSS 规范应该是怎样"。
//!    引擎行为变了测试会失败：那是提醒人同步更新预期，不是引擎 bug。
//!    （唯一例外是标注了`KNOWN BUG` 的条目，见下文。）
//! 2. **确定性 + 容差** —— 固定尺寸用精确值，位置关系用不等式；不写死间距语义。
//! 3. 每条测试注明它**保护**哪段逻辑，便于重构时判断"能改断言吗"。

use lieui_layout::*;

fn fixed_leaf(id: u64, w: f32, h: f32) -> FlexNode {
    let mut s = FlexStyle::default();
    s.dim[Dimension::Width as usize] = w;
    s.dim[Dimension::Height as usize] = h;
    s.flex_shrink = 0.0; // 默认不缩，否则"固定尺寸"的前提不成立
    FlexNode::new(id, s)
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

/// 便捷：造一个固定尺寸的 row 容器
fn row_root(w: f32, h: f32) -> FlexNode {
    let mut root = FlexNode::new_row(1);
    root.style.dim[Dimension::Width as usize] = w;
    root.style.dim[Dimension::Height as usize] = h;
    root
}

fn set_dim(style: &mut FlexStyle, w: f32, h: f32) {
    style.dim[Dimension::Width as usize] = w;
    style.dim[Dimension::Height as usize] = h;
}

// ─────────────────────── flex-grow ───────────────────────

/// 保护：`resolve_flexible_lengths` 的 grow 分配 —— 剩余空间按 grow 比例分。
#[test]
fn grow_distributes_free_space_by_ratio() {
    let mut root = row_root(200.0, 50.0);

    let mut a = fixed_leaf(2, 20.0, 10.0);
    a.style.flex_grow = 1.0;
    let mut b = fixed_leaf(3, 20.0, 10.0);
    b.style.flex_grow = 3.0;
    root.add_child(a);
    root.add_child(b);

    root.layout(200.0, 50.0, LayoutDirection::Ltr);

    // 自由空间 = 200 - 40 = 160；按 1:3 分⇒ 40 / 120
    assert!(
        approx(root.children[0].get_width(), 60.0),
        "a = {}",
        root.children[0].get_width()
    );
    assert!(
        approx(root.children[1].get_width(), 140.0),
        "b = {}",
        root.children[1].get_width()
    );
}

/// grow 为 0 的子节点**不参与**分配（哪怕它是唯一子节点）。
#[test]
fn zero_grow_does_not_absorb_free_space() {
    let mut root = row_root(200.0, 50.0);
    root.add_child(fixed_leaf(2, 30.0, 10.0)); // flex_grow = 0
    root.layout(200.0, 50.0, LayoutDirection::Ltr);
    assert!(
        approx(root.children[0].get_width(), 30.0),
        "grow=0 的子节点不该被拉伸：{}",
        root.children[0].get_width()
    );
}

// ─────────────────────── flex-shrink ───────────────────────

/// 保护：shrink 的**比例**分配 —— 溢出量按 shrink 权重分摊。
#[test]
fn shrink_distributes_overflow_by_weight() {
    let mut root = row_root(100.0, 50.0);

    // 两个 80宽的子节点共 160，溢出 60；shrink 1:3 ⇒ 各减 15 / 45
    let mut a = fixed_leaf(2, 80.0, 10.0);
    a.style.flex_shrink = 1.0;
    let mut b = fixed_leaf(3, 80.0, 10.0);
    b.style.flex_shrink = 3.0;
    root.add_child(a);
    root.add_child(b);

    root.layout(100.0, 50.0, LayoutDirection::Ltr);

    assert!(
        approx(root.children[0].get_width(), 65.0),
        "a = {}",
        root.children[0].get_width()
    );
    assert!(
        approx(root.children[1].get_width(), 35.0),
        "b = {}",
        root.children[1].get_width()
    );
}

/// `flex_shrink = 0` 的子节点在溢出时**保持原尺寸**（宁可溢出也不缩）。
#[test]
fn zero_shrink_keeps_size_and_overflows() {
    let mut root = row_root(100.0, 50.0);
    let a = fixed_leaf(2, 80.0, 10.0); // shrink = 0
    let mut b = fixed_leaf(3, 80.0, 10.0);
    b.style.flex_shrink = 1.0;
    root.add_child(a);
    root.add_child(b);

    root.layout(100.0, 50.0, LayoutDirection::Ltr);

    assert!(approx(root.children[0].get_width(), 80.0), "shrink=0 应保持 80");
    assert!(
        root.children[1].get_width() < 80.0,
        "另一个应被压缩：{}",
        root.children[1].get_width()
    );
}

/// 保护：shrink 的**冻结循环**（`flex_node.rs:603` 的 `while`）。
///
/// 冻结规则：某个子节点收缩到 `flex_basis`（=其 hypothetical size）后**冻结**，
/// 剩余溢出重新分摊给仍未冻结的节点。这条路径零覆盖，且循环**没有迭代上限**（D48）——
/// 若这里出现震荡会挂死 UI 主线程，所以本测试同时充当"该路径能收敛"的守卫。
#[test]
fn shrink_stops_at_basis_and_reallocates_remainder() {
    let mut root = row_root(100.0, 50.0);

    // a 想缩到 40（basis 40），b 只能缩到 30（basis 30）
    let mut a = fixed_leaf(2, 80.0, 10.0);
    set_dim(&mut a.style, 80.0, 10.0);
    a.style.flex_basis = 40.0;
    a.style.flex_shrink = 1.0;
    a.style.min_dim[Dimension::Width as usize] = 40.0; // min 挡住进一步收缩 ⇒ 触发冻结

    let mut b = fixed_leaf(3, 80.0, 10.0);
    b.style.flex_basis = 30.0;
    b.style.flex_shrink = 1.0;
    b.style.min_dim[Dimension::Width as usize] = 30.0;

    root.add_child(a);
    root.add_child(b);
    root.layout(100.0, 50.0, LayoutDirection::Ltr);

    // 两者都该被 min 夹住，且循环必须终止
    assert!(
        approx(root.children[0].get_width(), 40.0),
        "a = {}",
        root.children[0].get_width()
    );
    assert!(
        approx(root.children[1].get_width(), 30.0),
        "b = {}",
        root.children[1].get_width()
    );
    assert!(
        root.get_width() <= 100.01,
        "冻结后总宽不应超过容器（否则说明 min/max 夹取没生效）：{}",
        root.get_width()
    );
}

// ─────────────────────── min / max clamp ───────────────────────

/// 保护：`min_dim` 在**布局末端**夹取（先算再夹，不参与协商）。
#[test]
fn min_dim_clamps_too_small_child() {
    let mut root = row_root(200.0, 50.0);
    let mut a = fixed_leaf(2, 10.0, 10.0);
    a.style.min_dim[Dimension::Width as usize] = 50.0; // min 比 basis 大
    root.add_child(a);
    root.layout(200.0, 50.0, LayoutDirection::Ltr);
    assert!(
        root.children[0].get_width() >= 50.0,
        "应被 min 夹到 50，实际 {}",
        root.children[0].get_width()
    );
}

/// 保护：`max_dim` 夹取（grow 之后仍不得超过 max）。
#[test]
fn max_dim_clamps_grown_child() {
    let mut root = row_root(300.0, 50.0);
    let mut a = fixed_leaf(2, 10.0, 10.0);
    a.style.flex_grow = 1.0;
    a.style.max_dim[Dimension::Width as usize] = 40.0;
    root.add_child(a);
    root.layout(300.0, 50.0, LayoutDirection::Ltr);
    assert!(
        approx(root.children[0].get_width(), 40.0),
        "grow 到 max 就该停：{}",
        root.children[0].get_width()
    );
}

/// min 与 max 冲突时（min > max）不应死循环或产出 NaN。
/// 这是 `resolve_flexible_lengths` 无迭代上限（D48）最值得担心的一类输入。
#[test]
fn conflicting_min_max_does_not_hang_or_nan() {
    let mut root = row_root(200.0, 50.0);
    let mut a = fixed_leaf(2, 10.0, 10.0);
    a.style.flex_grow = 1.0;
    a.style.min_dim[Dimension::Width as usize] = 120.0;
    a.style.max_dim[Dimension::Width as usize] = 40.0; // min > max，语义上非法
    root.add_child(a);
    root.layout(200.0, 50.0, LayoutDirection::Ltr);

    let w = root.children[0].get_width();
    assert!(w.is_finite(), "宽度必须是有限值，实际 {w}");
}

/// shrink 溢出时 `min_dim` 应当**赢**（不允许缩到 min 以下）。
#[test]
fn min_dim_wins_over_shrink() {
    let mut root = row_root(60.0, 50.0);
    let mut a = fixed_leaf(2, 100.0, 10.0);
    a.style.flex_shrink = 1.0;
    a.style.min_dim[Dimension::Width as usize] = 80.0; // 溢出到 80 也不许再缩
    root.add_child(a);
    root.layout(60.0, 50.0, LayoutDirection::Ltr);
    assert!(
        approx(root.children[0].get_width(), 80.0),
        "应停在 min=80：{}",
        root.children[0].get_width()
    );
}

// ─────────────────────── 交叉轴 ───────────────────────

/// 保护：`align_items = Start` 时子节点贴主轴起点，不拉伸。
#[test]
fn align_items_start_does_not_stretch() {
    let mut root = row_root(100.0, 100.0);
    root.style.align_items = FlexAlign::Start;
    root.add_child(fixed_leaf(2, 20.0, 10.0)); // 高 10，容器 100
    root.layout(100.0, 100.0, LayoutDirection::Ltr);
    assert!(
        approx(root.children[0].get_height(), 10.0),
        "Start 不应拉伸：{}",
        root.children[0].get_height()
    );
}

/// 保护：`align_items = Stretch`（默认）时，**交叉轴尺寸未指定**的子节点填满交叉轴。
///
/// 注意 CSS 语义：stretch 只作用于**交叉轴尺寸为 auto** 的子节点；子节点若显式给了
/// `dim[Height]`，stretch **不生效**（`fixed_leaf` 都显式给了高度，所以另见
/// `stretch_does_not_apply_to_fixed_cross_size`）。
#[test]
fn align_items_stretch_fills_cross_axis() {
    let mut root = row_root(100.0, 100.0);
    root.style.align_items = FlexAlign::Stretch;
    let mut a = fixed_leaf(2, 20.0, 0.0);
    a.style.dim[Dimension::Height as usize] = VALUE_UNDEFINED; // 交叉轴 auto
    root.add_child(a);
    root.layout(100.0, 100.0, LayoutDirection::Ltr);
    assert!(
        approx(root.children[0].get_height(), 100.0),
        "交叉轴 auto 的子节点应被 Stretch 填满：{}",
        root.children[0].get_height()
    );
}

/// 保护：显式给了交叉轴尺寸时 **stretch 不生效**（对齐 CSS，避免"以为会拉伸"的误用）。
#[test]
fn stretch_does_not_apply_to_fixed_cross_size() {
    let mut root = row_root(100.0, 100.0);
    root.style.align_items = FlexAlign::Stretch;
    root.add_child(fixed_leaf(2, 20.0, 10.0)); // 显式 height = 10
    root.layout(100.0, 100.0, LayoutDirection::Ltr);
    assert!(
        approx(root.children[0].get_height(), 10.0),
        "显式交叉轴尺寸不该被拉伸：{}",
        root.children[0].get_height()
    );
}

/// 保护：`align_self` **覆盖**父的 `align_items`（WinUI / CSS 同语义）。
#[test]
fn align_self_overrides_parent_align_items() {
    let mut root = row_root(100.0, 100.0);
    root.style.align_items = FlexAlign::Stretch;
    let mut a = fixed_leaf(2, 20.0, 10.0);
    a.style.align_self = FlexAlign::Start; // 覆盖 Stretch
    root.add_child(a);
    root.layout(100.0, 100.0, LayoutDirection::Ltr);
    assert!(
        approx(root.children[0].get_height(), 10.0),
        "align_self 应胜出：{}",
        root.children[0].get_height()
    );
}

/// `align_content = SpaceEvenly`。
///
/// **KNOWN BUG（D48）**：`flex_node.rs:800-816` 里 SpaceEvenly **静默退化为 0**
/// （等同 Start）。本测试**钉住当前行为**，修复后应把断言改成"均匀分布"。
/// **D48 已修**：`align_content: SpaceEvenly` 曾**静默退化为贴顶**
/// （`flex_node.rs` 的 Step 16 match 里**缺该分支** ⇒落 `_ => 0.0`）。
///
/// 与主轴的 `space-evenly`（`flex_line.rs`）不一致 —— 同一语义两处实现，
/// 一处有一处没有，是典型的"退化路径无测试"后果。
#[test]
fn align_content_space_evenly_distributes_evenly() {
    let mut root = row_root(100.0, 100.0);
    root.style.flex_wrap = FlexWrap::Wrap;
    root.style.align_content = FlexAlign::SpaceEvenly;
    root.style.dim[Dimension::Height as usize] = 100.0;
    for i in 0..3 {
        root.add_child(fixed_leaf(10 + i, 100.0, 20.0)); // 每行 100 宽 ⇒ 强制换行
    }
    root.layout(100.0, 100.0, LayoutDirection::Ltr);

    let tops: Vec<f32> = root.children.iter().map(|c| c.get_top()).collect();
    // 3 行 × 20 高 = 60，容器 100 ⇒ 剩余 40⇒ 间隙 = 40 / (3+1) = 10
    let gaps = [tops[1] - tops[0], tops[2] - tops[1]];
    //首行顶部偏移也应等于一个间隙（这是 space-evenly 与 space-around 的区别）
    assert!(
        approx(tops[0], 10.0),
        "space-evenly 的首行偏移应等于一个间隙 10.0，实际 {}（tops={tops:?}）",
        tops[0]
    );
    for g in gaps {
        assert!(
            approx(g, 30.0), // 相邻行间距 = 行高 20 + 间隙 10
            "相邻行间距应恒为 30.0（行高 20 + 间隙 10），实际 {g}"
        );
    }
    // 行间距必须完全均匀 —— 这正是"退化到贴顶"或"只有两端有间距"都过不了的
    assert!(approx(gaps[0], gaps[1]), "各段间距必须相等：{gaps:?}");
    // 末行底部也应留一个间隙
    let last_bottom = tops[2] + 20.0;
    assert!(
        approx(100.0 - last_bottom, 10.0),
        "末行之后也应留一个间隙 10.0，实际 {}",
        100.0 - last_bottom
    );
}
// ─────────────────────── KNOWN BUG：布局对样式有持久副作用 ───────────────────────

/// **D44 已修**：`layout()` 曾**永久改写** `style.flex_basis` 且不恢复
/// （写入在 `layout()` 开头，还原逻辑缺失，而 `dim` 有 `swr`/`shr` 还原）。
///
/// 后果：布局对输入样式产生**持久的单边副作用** ⇒ 复用同一个 `FlexNode`
/// （D-a 的持久缓存）时，第二次布局的协商输入已经不是同一个值。
/// 今天不触发只因框架每次都重建临时树；D-a 之前必须修。
///
/// 现在断言的是**修复后的正确行为**：布局对样式无副作用。
#[test]
fn layout_leaves_flex_basis_untouched() {
    let mut root = row_root(100.0, 50.0);
    let a = fixed_leaf(2, 80.0, 10.0);
    assert!(
        is_undefined(root.style.flex_basis) && is_undefined(a.style.flex_basis),
        "前置条件：两者都应为 undefined"
    );
    root.add_child(a);

    root.layout(100.0, 50.0, LayoutDirection::Ltr);

    assert!(
        is_undefined(root.style.flex_basis),
        "D44：layout 必须还原 flex_basis（当前被写成了 {}）",
        root.style.flex_basis
    );
    assert!(is_undefined(root.children[0].style.flex_basis));
}

/// D44 的直接后果防护：**同一棵树重复布局必须幂等**。
///
/// 修复前，第二次 `layout` 时 `flex_basis` 已从第一次残留下来 ⇒ 协商输入变了。
/// 这条是 D-a（FlexNode 持久化）的前置条件——没有它，持久化就会把残留带进下一帧。
#[test]
fn repeated_layout_is_idempotent() {
    let build = || {
        let mut root = row_root(100.0, 50.0);
        let mut a = fixed_leaf(2, 80.0, 10.0);
        a.style.flex_basis = VALUE_UNDEFINED;
        a.style.flex_shrink = 1.0;
        a.style.min_dim[Dimension::Width as usize] = 20.0;
        root.add_child(a);
        root
    };

    let mut tree = build();
    tree.layout(100.0, 50.0, LayoutDirection::Ltr);
    let w1 = tree.children[0].get_width();
    let h1 = tree.children[0].get_height();
    for _ in 0..3 {
        tree.layout(100.0, 50.0, LayoutDirection::Ltr);
    }
    let w2 = tree.children[0].get_width();
    let h2 = tree.children[0].get_height();

    assert!(
        approx(w1, w2) && approx(h1, h2),
        "重复 layout 必须幂等：第一次 {w1}x{h1}，第四次 {w2}x{h2}"
    );
}

/// **收敛守卫**：给一组容易震荡的输入（min>max、shrink+min 冲突、零尺寸），
/// 全部 layout 必须**终止**且产出有限值。
///
/// `resolve_flexible_lengths` 的循环**没有迭代上限**（D48），一旦震荡就会挂死 UI 主线程
/// —— 而 GUI 框架里"主线程挂死"是最严重的故障形态。本测试是当前唯一的兜底。
#[test]
fn pathological_inputs_terminate_with_finite_results() {
    /// 每个用例往容器里塞一种"刁钻"配置。
    type CaseSetup = Box<dyn Fn(&mut FlexNode)>;

    let cases: Vec<(&str, CaseSetup)> = vec![
        (
            "min>max",
            Box::new(|r: &mut FlexNode| {
                let mut a = fixed_leaf(2, 10.0, 10.0);
                a.style.flex_grow = 1.0;
                a.style.min_dim[Dimension::Width as usize] = 100.0;
                a.style.max_dim[Dimension::Width as usize] = 5.0;
                r.add_child(a);
            }),
        ),
        (
            "shrink 到 min 冲突",
            Box::new(|r: &mut FlexNode| {
                let mut a = fixed_leaf(2, 100.0, 10.0);
                a.style.flex_shrink = 1.0;
                a.style.min_dim[Dimension::Width as usize] = 90.0;
                r.add_child(a);
            }),
        ),
        (
            "零尺寸容器",
            Box::new(|r: &mut FlexNode| {
                r.add_child(fixed_leaf(2, 10.0, 10.0));
            }),
        ),
        (
            "全零 dim",
            Box::new(|r: &mut FlexNode| {
                let mut a = FlexNode::new(2, FlexStyle::default());
                a.style.flex_shrink = 1.0;
                r.add_child(a);
            }),
        ),
    ];

    for (name, setup) in cases {
        let mut root = row_root(0.0, 0.0); // 故意给 0×0，触发最刁钻分支
        set_dim(&mut root.style, 0.0, 0.0);
        setup(&mut root);
        root.layout(0.0, 0.0, LayoutDirection::Ltr);

        for (i, c) in root.children.iter().enumerate() {
            assert!(
                c.get_width().is_finite() && c.get_height().is_finite(),
                "{name}: child[{i}] 产出非有限值 {}x{}",
                c.get_width(),
                c.get_height()
            );
        }
    }
}

/// 回归（D48 / D-c）：冻结循环**必须有迭代上限**。
///
/// bug 表现：`while !resolve_flexible_lengths(..) {}` 没有上限。
/// 若某轮 `total_violation != 0` 而 `min_violations` / `max_violations` 都为空
/// （violation 一正一负、浮点求和没抵消成精确 0），该轮**什么都不冻结**
/// ⇒ 下一轮输入完全相同 ⇒ **死循环** ⇒ GUI 主线程挂死。
///
/// 这条测试构造"min> max 冲突 + shrink"这类刁钻组合并要求它在有限步内返回。
/// 注意：即使真的不收敛，`MAX_FLEX_ITERATIONS` 也会兜住，所以本测试
/// 断言的是**"必定终止且结果有限"**，而不是"必定收敛"。
#[test]
fn flex_negotiation_always_terminates() {
    // min > max（语义非法）+ 强制 shrink + 零基线，最容易触发震荡
    let mut root = row_root(100.0, 50.0);
    for i in 0..4u64 {
        let mut a = fixed_leaf(100 + i, 60.0, 10.0);
        a.style.flex_shrink = 1.0;
        a.style.min_dim[Dimension::Width as usize] = 90.0; // 远大于容器
        a.style.max_dim[Dimension::Width as usize] = 10.0; // 但 max 又很小
        root.add_child(a);
    }
    root.layout(100.0, 50.0, LayoutDirection::Ltr);

    for (i, c) in root.children.iter().enumerate() {
        assert!(
            c.get_width().is_finite() && c.get_height().is_finite(),
            "child[{i}] 产出非有限值{}x{}（震荡的征兆）",
            c.get_width(),
            c.get_height()
        );
    }
}

/// 回归（D-a）：`measured_content` 必须记**内容测量尺寸**，而不是布局后尺寸。
///
/// ## 为什么这两个值必须分开
///
/// `layout_result.dim` 是**布局后**尺寸（可能被 flex 拉伸 / 收缩）；宿主的 `Node.desired`
/// 要的是**内容测量尺寸**（它走 `paint_bounds` 的文本收缩路径）。
/// 若宿主误用 `layout_result.dim`，被拉伸过的文本会让脏区偏大 ⇒ **"精确脏区"退化**。
///
/// 这条测试构造"可拉伸的文本叶子"：容器 200宽、文本内容远窄于容器、
/// `flex_grow = 1` ⇒ 布局后被拉满，但**内容尺寸必须仍是文本本身的宽度**。
#[test]
fn measured_content_is_content_size_not_stretched_size() {
    let mut root = row_root(200.0, 50.0);

    let spec = lieui_text::TextSpec {
        font_size: 20.0,
        ..Default::default()
    };
    let mut leaf = FlexNode::new(2, FlexStyle::default());
    leaf.measure_text = Some(("Hello".to_string(), spec));
    leaf.style.flex_grow = 1.0; // 会被拉伸到满宽
    root.add_child(leaf);

    root.layout(200.0, 50.0, LayoutDirection::Ltr);

    let l = &root.children[0];
    let mc = l.measured_content;

    // ① 内容尺寸：有效但远小于容器
    assert!(
        mc[0] > 0.0 && mc[0] < 100.0,
        "measured_content[0] 应是文本内容宽度（实际 {}）",
        mc[0]
    );
    assert!(mc[1] > 0.0, "measured_content[1] 应是单行高度（实际 {}）", mc[1]);

    // ② 布局后尺寸：被 grow 拉满
    assert!(l.get_width() > 150.0, "布局后应被拉伸到满宽（实际 {}）", l.get_width());

    // ③ ★ 核心断言：两者**不同** —— 这正是"必须分开记"的理由
    assert!(
        mc[0] < l.get_width(),
        "measured_content({}) 必须与布局后尺寸({}) 不同，否则宿主会拿到被拉伸的值",
        mc[0],
        l.get_width()
    );
}

/// 非文本叶子（无 `measure_text`）不产生测量值 —— 约定为 `[0.0, 0.0]`，
/// 宿主据此回落到 `intrinsic_size` 路径。
#[test]
fn measured_content_is_zero_for_non_text_leaves() {
    let mut root = row_root(200.0, 50.0);
    root.add_child(fixed_leaf(2, 40.0, 20.0));
    root.layout(200.0, 50.0, LayoutDirection::Ltr);
    assert_eq!(
        root.children[0].measured_content,
        [0.0, 0.0],
        "无 measure_text 时不应伪造测量值"
    );
}

/// 容器同样不产生测量值（宿主的 `desired_size` 对容器走"取引擎分配尺寸"分支）。
#[test]
fn measured_content_is_zero_for_containers() {
    let mut root = row_root(200.0, 50.0);
    let mut inner = fixed_leaf(2, 40.0, 20.0);
    inner.style.flex_grow = 1.0;
    root.add_child(inner);
    root.layout(200.0, 50.0, LayoutDirection::Ltr);
    assert_eq!(root.measured_content, [0.0, 0.0]);
}

/// **KNOWN BUG（D48 · 已决定"不做"）**：`align_content: Baseline` 未实现。
///
/// `FlexAlign` 共 9 个变体，`flex_node.rs` Step 16 的 match 覆盖了
/// `Start`/`Center`/`End`/`Stretch`/`SpaceBetween`/`SpaceAround`/`SpaceEvenly`，
/// 剩下的 `Auto` / `Baseline` 落`_ => 0.0`（=贴顶）。
///
/// **为什么不实现**（与 SpaceEvenly 不同）：
/// - `Baseline` 的语义依赖**子节点的行盒基线**（文字/图片混排时的对齐基准），
///   而本引擎的测量口径是parley 的**行盒**；把"行盒顶边"冒充"基线"
///   会给出**看似合理但错误**的结果，比明确退化更糟。
/// - 当前无真实用例（仓库内没有混排基线对齐的 UI）。
/// - 维护成本高于收益。
///
/// **所以选择"显式记录"而不是"静默退化"**：这条测试把现状钉住，
/// 将来若决定实现，取消 `#[ignore]` 并改写断言即可。
#[test]
#[ignore = "KNOWN BUG D48：align-content: Baseline 未实现（已决定不做，静默退化为贴顶）"]
fn align_content_baseline_currently_degenerates_to_start() {
    let mut root = row_root(100.0, 100.0);
    root.style.flex_wrap = FlexWrap::Wrap;
    root.style.align_content = FlexAlign::Baseline;
    root.style.dim[Dimension::Height as usize] = 100.0;
    for i in 0..3 {
        root.add_child(fixed_leaf(10 + i, 100.0, 20.0));
    }
    root.layout(100.0, 100.0, LayoutDirection::Ltr);

    let tops: Vec<f32> = root.children.iter().map(|c| c.get_top()).collect();
    // 现状：落 `_ => 0.0` ⇒ 全部贴顶（与 Start 无异）
    assert!(
        tops.iter().all(|t| approx(*t, tops[0])),
        "当前 Baseline 退化为贴顶：{tops:?}"
    );
}

/// ★ 哨兵常量的**陷阱测试**：`VALUE_UNDEFINED` 与 `VALUE_AUTO` 是同一个值。
///
/// ## 为什么这条测试重要
///
/// 两个常量都�� `f32::NAN`，**语义不同但无法区分**（`FlexStyle` 的长度字段是裸 `f32`，
/// 没有 `Option` / 单位标记）。于是：
///
/// - `v == VALUE_AUTO` 恒为 `false`（`NaN != NaN`）——
///   **编译通过、测试也可能通过、但语义是错的**。
/// - 唯一正确的判定方式是 `is_undefined(v)` / `is_defined(v)`（即 `v.is_nan()`）。
///
/// 本测试把这个事实**显式钉住**：将来若引入 `enum Length { Px, Percent, Auto, Undefined }`
/// （`refactor-plan` D17 的前置条件），本测试会失败，提示更新判定方式。
#[test]
fn sentinels_are_distinguishable_only_by_is_nan() {
    // ★ 用 `black_box` 阻止常量折叠 —— 否则 `is_nan()` 在编译期就是常量，
    //   clippy 的 `assertions_on_constants` 会直接拒绝（这条断言必须运行期求值）。
    let a = std::hint::black_box(VALUE_AUTO);
    let b = std::hint::black_box(VALUE_UNDEFINED);

    // 两个常量当前同值
    assert!(a.is_nan() && b.is_nan(), "两个哨兵都应是 NaN");

    // ★ 反例：直接比较恒为 false —— 这就是"看起来能区分其实不能"的陷阱
    assert!(!(a == b), "★ NaN != NaN：直接比较恒为 false。**永远不要这样判定**");

    // 正确的判定方式：靠 is_nan
    assert!(is_undefined(VALUE_AUTO));
    assert!(is_undefined(VALUE_UNDEFINED));
    assert!(is_undefined(f32::NAN));
    assert!(!is_undefined(0.0));
    assert!(is_defined(0.0));
    assert!(!is_defined(f32::NAN));
}

/// **D17 · 已决定"不做"**：`flex-basis`（以及所有长度字段）**不支持百分比**。
///
/// ## 现状
///
/// 全仓**没有任何百分比表示方式**：`FlexStyle` 的长度字段都是裸 `f32`，
/// `view.rs` 的 DSL 也没有 `width("50%")` 之类的入口。
/// `flex_basis: 50` 只会是 **50 像素**。
///
/// ## 为什么不实现（与 `align-content: Baseline` 同批决策）
///
/// 1. **不是补一个分支，而是加一个能力**：需要引入长度单位表示
///    （`enum Length { Px(f32), Percent(f32), Auto, Undefined }`），
///    它会贯穿 `style_eq`（`style.rs:84` 逐字段比较）、DSL、写回 ——
///    是一次结构性改动，**不是 20 行**。
/// 2. **当前无真实用例**：仓库内的 UI 都能用 `flex_grow` 表达
///    （`flex_basis: 0` + `flex_grow: 1` 即"均分"）。
/// 3. `FlexStyle` 已经有 `content_width` / `content_height: Option<f32>`
///    这类字段，**加百分比字段**会让"同一语义有两种表示"，比统一改成
///    `Length` 枚举更难维护。
///
/// ## 将来若要实现
///
/// 按 `refactor-plan` 的建议**统一**引入 `Length` 枚举（而不是加
/// `flex_basis_percent: Option<f32>` 这类并行字段），并同步处理
/// `style_eq`、DSL、以及上面那条哨兵测试。
#[test]
fn percent_lengths_are_not_supported_by_design() {
    // 本测试不验证运行时行为（当前没有百分比入口可测），
    // 它的作用是**把"不做"这个决策写进代码**，避免后人误以为已实现。
    let style = FlexStyle::default();
    // `flex_basis` 是裸 f32：写 50 就是 50 像素，不存在"50%"的表示
    assert!(
        style.flex_basis.is_nan() || style.flex_basis == 50.0,
        "`flex_basis` 是裸 f32（默认未定义），没有百分比语义"
    );
}
