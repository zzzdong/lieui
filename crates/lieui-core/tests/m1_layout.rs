//! M1 布局里程碑验收测试
//!
//! 对应设计 §7.2「M1 布局」：
//! 1. taitank 集成 + 重排边界 → 改单文本节点不触发整窗重排（计数器断言）
//! 2. parley 两段式测度 → 测度缓存命中率 > 90%
//! 3. 文本布局缓存 → 相同 (text, style, wrap) 不重复测度

use lieui_core::props::keys as K;
use lieui_core::text_engine::{TextService, TextSpec};
use lieui_core::{Dimension, ElementTypeId, NodeId, SharedString, Window, WindowId};

const ROWS: usize = 100;

/// 构造：窗口根（viewport 800x600）
///   ├─ 左栏 scroll：400x600（重排边界），内含 100 行文本
///   └─ 右栏 scroll：400x600（重排边界），内含 100 行文本
struct Fixture {
    win: Window,
    left_rows: Vec<NodeId>,
    right_rows: Vec<NodeId>,
}

fn build_fixture() -> Fixture {
    let mut win = Window::new(WindowId::from_u64(1), (800.0, 600.0));
    let root = win.root();
    win.set_prop(root, K::FLEX_DIRECTION, K::flex_direction::ROW);

    let column = |win: &mut Window| -> (NodeId, Vec<NodeId>) {
        let panel = win.create_node(root, ElementTypeId::BOX, None);
        win.set_prop(panel, K::WIDTH, Dimension::px(400.0));
        win.set_prop(panel, K::HEIGHT, Dimension::px(600.0));
        win.set_prop(panel, K::OVERFLOW_SCROLL, true);
        let col = win.create_node(panel, ElementTypeId::BOX, None);
        let mut rows = Vec::new();
        for i in 0..ROWS {
            let t = win.create_node(col, ElementTypeId::TEXT, None);
            win.set_prop(t, K::TEXT, SharedString::new(format!("row {i}")));
            win.set_prop(t, K::FONT_SIZE, 16.0);
            rows.push(t);
        }
        (panel, rows)
    };

    let (_, left_rows) = column(&mut win);
    let (_, right_rows) = column(&mut win);
    Fixture {
        win,
        left_rows,
        right_rows,
    }
}

#[test]
fn full_window_layout_touches_every_node_once() {
    let mut fx = build_fixture();
    let mut text = TextService::new(4096);
    fx.win.run_layout(&mut text);

    let expected = 1 + 2 * (1 + 1 + ROWS as u32); // root + 2×(panel + col + rows)
    assert_eq!(fx.win.stats().last_pass_nodes, expected);
    assert_eq!(fx.win.stats().passes, 1);
    assert_eq!(fx.win.layout_of(fx.win.root()).unwrap().width, 800.0);
}

/// 验收 1：改单个文本节点只重排其所在的重排边界子树，不整窗重排
#[test]
fn single_text_change_does_not_relayout_window() {
    let mut fx = build_fixture();
    let mut text = TextService::new(4096);
    fx.win.run_layout(&mut text);
    let full = fx.win.stats().last_pass_nodes;
    fx.win.reset_stats();

    // 改左栏中间某一行
    fx.win.set_prop(
        fx.left_rows[ROWS / 2],
        K::TEXT,
        SharedString::new("changed text"),
    );
    assert_eq!(fx.win.pending_boundaries(), 1, "只应登记 1 个重排边界");
    fx.win.run_layout(&mut text);

    let scoped = fx.win.stats().last_pass_nodes;
    assert_eq!(fx.win.stats().passes, 1);
    assert!(
        scoped * 2 < full,
        "改 1 个文本只应重排左栏子树（{scoped}），远小于整窗（{full}）"
    );
    // 右栏布局结果未被重写，仍保持原值
    let r = fx.win.layout_of(fx.right_rows[ROWS / 2]).unwrap();
    assert!(r.width > 0.0 && r.height > 0.0);
}

/// 验收 1b：整窗重排（viewport 变化）才会触及全部节点
#[test]
fn viewport_change_relayouts_whole_window() {
    let mut fx = build_fixture();
    let mut text = TextService::new(4096);
    fx.win.run_layout(&mut text);
    fx.win.reset_stats();

    fx.win.set_viewport(1024.0, 768.0);
    fx.win.run_layout(&mut text);
    assert_eq!(
        fx.win.stats().last_pass_nodes,
        1 + 2 * (1 + 1 + ROWS as u32)
    );
    assert_eq!(fx.win.layout_of(fx.win.root()).unwrap().width, 1024.0);
}

/// 验收 2：测度缓存命中率 > 90%
#[test]
fn measure_cache_hit_rate_exceeds_90_percent() {
    let mut fx = build_fixture();
    let mut text = TextService::new(4096);
    fx.win.run_layout(&mut text);

    let first = text.stats();
    assert!(
        first.measure_calls > first.shapes,
        "同节点会被多次测量，多出的必须命中缓存"
    );

    // 二次整窗重排：所有文本与宽度约束都未变 → 应全部命中
    text.reset_stats();
    fx.win.mark_layout_dirty(fx.win.root());
    fx.win.run_layout(&mut text);

    let second = text.stats();
    assert!(
        second.measure_hit_rate() > 0.9,
        "二次重排测度缓存命中率应 > 90%，实际 {:.1}%（calls={}, hits={}）",
        second.measure_hit_rate() * 100.0,
        second.measure_calls,
        second.measure_hits
    );
    assert_eq!(second.shapes, 0, "二次重排不应重新整形");
}

/// 验收 3：相同 (text, style, wrap) 不重复测度 —— 两段式各只做一次
#[test]
fn identical_text_and_style_are_measured_once() {
    let mut text = TextService::new(1024);
    let spec = TextSpec {
        text: "hello world",
        family: "",
        font_size: 16.0,
        line_height: 0.0,
        weight: lieui_core::text_engine::FontWeight::NORMAL,
        italic: false,
        wrap: true,
        max_width: Some(120.0),
        align: lieui_core::text_engine::TextAlign::Start,
    };

    let size = text.measure(&spec);
    assert_eq!(text.stats().shapes, 1);
    assert!(size.0 > 0.0 && size.1 > 0.0, "文本应测得非零尺寸：{size:?}");

    // 重复测量：命中缓存，不重新整形
    assert_eq!(text.measure(&spec), size);
    assert_eq!(text.stats().shapes, 1);

    // 第二段（align）只执行一次
    {
        let _ = text.layout(&spec);
    }
    assert_eq!(text.stats().aligns, 1);
    {
        let _ = text.layout(&spec);
    }
    assert_eq!(text.stats().aligns, 1, "同一 spec 不应重复 align");

    // 换行宽度变化 → 键不同 → 重新整形
    let mut wide = spec;
    wide.max_width = Some(300.0);
    let _ = text.measure(&wide);
    assert_eq!(text.stats().shapes, 2, "max_width 变化必须重新测量");
}

// ── 布局正确性 ──

#[test]
fn column_stacks_children_by_height() {
    let mut win = Window::new(WindowId::from_u64(2), (400.0, 600.0));
    let root = win.root();
    win.set_prop(root, K::FLEX_DIRECTION, K::flex_direction::COLUMN);

    let a = win.create_node(root, ElementTypeId::BOX, None);
    win.set_prop(a, K::WIDTH, Dimension::px(100.0));
    win.set_prop(a, K::HEIGHT, Dimension::px(50.0));
    let b = win.create_node(root, ElementTypeId::BOX, None);
    win.set_prop(b, K::WIDTH, Dimension::px(100.0));
    win.set_prop(b, K::HEIGHT, Dimension::px(70.0));

    let mut text = TextService::new(256);
    win.run_layout(&mut text);

    let la = win.layout_of(a).unwrap();
    let lb = win.layout_of(b).unwrap();
    assert_eq!((la.x, la.y, la.width, la.height), (0.0, 0.0, 100.0, 50.0));
    assert_eq!((lb.x, lb.y, lb.width, lb.height), (0.0, 50.0, 100.0, 70.0));
}

#[test]
fn row_with_flex_grow_fills_remaining_space() {
    let mut win = Window::new(WindowId::from_u64(3), (400.0, 100.0));
    let root = win.root();
    win.set_prop(root, K::FLEX_DIRECTION, K::flex_direction::ROW);

    let fixed = win.create_node(root, ElementTypeId::BOX, None);
    win.set_prop(fixed, K::WIDTH, Dimension::px(100.0));
    win.set_prop(fixed, K::HEIGHT, Dimension::px(100.0));
    let grow = win.create_node(root, ElementTypeId::BOX, None);
    win.set_prop(grow, K::FLEX_GROW, 1.0);
    win.set_prop(grow, K::HEIGHT, Dimension::px(100.0));

    let mut text = TextService::new(256);
    win.run_layout(&mut text);

    assert_eq!(win.layout_of(fixed).unwrap().width, 100.0);
    assert_eq!(win.layout_of(grow).unwrap().width, 300.0);
    assert_eq!(win.layout_of(grow).unwrap().x, 100.0);
}

#[test]
fn percent_width_resolves_against_parent() {
    let mut win = Window::new(WindowId::from_u64(4), (500.0, 200.0));
    let root = win.root();
    win.set_prop(root, K::FLEX_DIRECTION, K::flex_direction::ROW);
    let child = win.create_node(root, ElementTypeId::BOX, None);
    win.set_prop(child, K::WIDTH, Dimension::percent(50.0));
    win.set_prop(child, K::HEIGHT, Dimension::px(100.0));

    let mut text = TextService::new(256);
    win.run_layout(&mut text);
    assert_eq!(
        win.layout_of(child).unwrap().width,
        250.0,
        "百分比宽度应在第二 pass 修正为父级的 50%"
    );
}

#[test]
fn wrapping_text_grows_height_with_content() {
    // 视口 100px：该串单行约 191px，必然折行
    let mut win = Window::new(WindowId::from_u64(5), (100.0, 400.0));
    let root = win.root();
    let t = win.create_node(root, ElementTypeId::TEXT, None);
    win.set_prop(
        t,
        K::TEXT,
        SharedString::new("a b c d e f g h i j k l m n o p"),
    );
    win.set_prop(t, K::TEXT_WRAP, true);

    let mut text = TextService::new(256);
    win.run_layout(&mut text);

    let l = win.layout_of(t).unwrap();
    assert!(
        l.width <= 100.0 + 0.5,
        "换行后宽度不应超过可用宽度：{}",
        l.width
    );
    assert!(l.height > 30.0, "折行后应为多行：{}", l.height);

    // 关掉换行 → 单行，高度回落到单行行高
    win.set_prop(t, K::TEXT_WRAP, false);
    win.run_layout(&mut text);
    let l2 = win.layout_of(t).unwrap();
    assert!(
        l2.height < l.height,
        "wrap=false 应回到单行：{} vs {}",
        l2.height,
        l.height
    );
}

#[test]
fn scroll_container_computes_content_and_clamps_offset() {
    let mut win = Window::new(WindowId::from_u64(6), (200.0, 100.0));
    let root = win.root();
    let panel = win.create_node(root, ElementTypeId::BOX, None);
    win.set_prop(panel, K::WIDTH, Dimension::px(200.0));
    win.set_prop(panel, K::HEIGHT, Dimension::px(100.0));
    win.set_prop(panel, K::OVERFLOW_SCROLL, true);
    win.set_prop(panel, K::SCROLL_Y, 9999.0);
    let inner = win.create_node(panel, ElementTypeId::BOX, None);
    win.set_prop(inner, K::WIDTH, Dimension::px(200.0));
    win.set_prop(inner, K::HEIGHT, Dimension::px(400.0));

    let mut text = TextService::new(256);
    win.run_layout(&mut text);

    let l = win.layout_of(panel).unwrap();
    assert_eq!((l.width, l.height), (200.0, 100.0));
    assert!(l.overflow_scroll);
    assert!(
        l.content_height >= 400.0,
        "内容高度应覆盖子节点：{}",
        l.content_height
    );
    assert_eq!(l.scroll_y, 300.0, "滚动偏移应被钳制到 content - viewport");
    // 子节点应被整体上移 -scroll_y
    let li = win.layout_of(inner).unwrap();
    assert_eq!(li.y, l.y - 300.0);
}
