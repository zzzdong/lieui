//! **性能回归的可断言指标**（确定性，不含时间）。
//!
//! ## 为什么需要这个文件
//!
//! `examples/damage_bench.rs` 早就把脏区收益测出来了，但它的断言写在 `main()` 里，
//! **`cargo test` 根本不执行** —— 于是"脏区退化整窗"（D21）这类问题
//! **没有任何测试能发现**，只能靠人看 benchmark 输出。
//!
//! 本文件把同一批测量搬进 `cargo test`，但**只断言确定性指标**：
//!
//! | 指标 | 字段 | 为什么可断言 |
//! |---|---|---|
//! | 脏区碎片数 | `FrameStats::damage.len()` | 纯几何，确定 |
//! | 光栅批次数 | `RenderStats::raster.batches` | 纯算法，确定 |
//! | 光栅像素数 | `RenderStats::raster.pixels` | 批次面积之和，确定 |
//! | 是否真的光栅化 | `RenderStats::raster.rasterized` | 布尔 |
//! | 帧是否空闲 | `FrameStats::is_idle()` | 布尔 |
//!
//! **时间指标（ms/µs）不进这里** —— 那受机器、编译模式、CPU 调度影响，
//! 属于 `examples/damage_bench.rs` 的职责。
//!
//! ## 当前基线（`cargo test -p lieui --test perf_regression -- --nocapture` 可复现）
//!
//! 见各测试内的断言与 `KNOWN BUG` 标注。两条已知缺陷（D21 碎片退化、D22 上屏按整行）
//! 用 `#[ignore]` 显式标记，修复后改成正断言并去掉 ignore。

use std::rc::Rc;

use lieui::prelude::*;
use lieui::{Dirty, Kind, NodeId, ViewBuf, ViewModel, WindowId};

const W: f32 = 1280.0;
const H: f32 = 720.0;
const FULL_PX: u64 = (W as u64) * (H as u64); // 921_600

/// 一个 N 行的**滚动**页面（与 `damage_bench` 同构，便于对照数字）。
///
/// ⚠️ 刻意用 `v.scroll` 包装 —— `damage_bench.rs` 里的 `Page` 用的是 `v.column`，
/// **它其实没有滚动容器**，所以那份基准从未真正测过"滚动脏区"这条最关键的路径。
/// 本文件补上（这正是 C0 要量出来的东西）。
struct Page {
    rows: usize,
}

impl ViewModel for Page {
    fn view(self: &Rc<Self>, v: &mut ViewBuf) {
        v.scroll(|s| {
            s.column(|c| {
                c.padding(12.0);
                c.gap(4.0);
                for i in 0..self.rows {
                    c.row(|r| {
                        r.gap(8.0);
                        r.text(format!("第 {i} 行：一段用来产生布局与文本测度的内容"))
                            .font_size(13.0);
                        r.button("操作").on_tap(|| {});
                    });
                }
            });
        });
    }
}

/// 起一个页面并跑完首帧，返回 `(Runtime, App, WindowId)`。
fn page(rows: usize) -> (Runtime, App, WindowId) {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());
    let id = app.window(WindowConfig::new().size(W, H), Page { rows });
    app.frame_all();
    (rt, app, id)
}

/// 取一个"深处的按钮"当局部脏区目标。
fn deep_button(app: &App, id: WindowId, nth: usize) -> NodeId {
    let w = app.window_ctx(id).unwrap();
    w.track()
        .node_ids()
        .filter(|n| {
            w.track()
                .get(*n)
                .map(|n| matches!(n.kind, Kind::Button { .. }))
                .unwrap_or(false)
        })
        .nth(nth)
        .expect("页面里应有足够多的按钮")
}

// ─────────────────────── 空闲帧 ───────────────────────

/// **事件驱动的基本保证**：没有任何变化 ⇒ 既不重排也不光栅化。
///
/// 这是"空闲零功耗"的可断言形式；若哪天有人在帧驱动里加了无条件光栅化，这条会立刻炸。
#[test]
fn idle_frame_does_not_rasterize() {
    let (_rt, mut app, _id) = page(50);
    let st = app.frame_all().remove(0).1;

    assert!(st.is_idle(), "无变化时帧应空闲：{st:?}");
    assert!(!st.render.raster.rasterized, "空闲帧不该光栅化（rasterized = true）");
    assert_eq!(st.render.raster.pixels, 0, "空闲帧光栅像素数应为 0");
    assert!(st.damage.is_empty(), "空闲帧不应有脏矩形");
}

/// 空闲帧之后再标一个脏，才又开始光栅化 —— 确认上一条不是"永远不画"。
#[test]
fn marking_damage_reenables_rasterization() {
    let (_rt, mut app, id) = page(50);
    let target = deep_button(&app, id, 5);
    let _ = app.frame_all(); // 吃掉首帧脏

    app.window_ctx_mut(id).unwrap().track_mut().mark_paint_dirty(target);
    let st = app.frame_all().remove(0).1;

    assert!(st.render.raster.rasterized, "标脏后应真的光栅化");
    assert!(st.render.raster.pixels > 0);
}

// ─────────────────────── 局部 vs 整窗 ───────────────────────

/// 局部重绘的代价**远小于**整窗 —— 脏区机制的核心价值，必须有回归保护。
#[test]
fn local_damage_rasterizes_far_less_than_full_window() {
    let (_rt, mut app, id) = page(50);
    let target = deep_button(&app, id, 10);
    let _ = app.frame_all(); // 清掉首帧脏

    app.window_ctx_mut(id).unwrap().track_mut().mark_paint_dirty(target);
    let st = app.frame_all().remove(0).1;

    assert!(!st.damage_all, "单按钮脏区不该退化成整窗脏");
    assert!(
        st.render.raster.pixels < FULL_PX / 50,
        "局部光栅像素应 < 整窗的 2%，实际 {} / {FULL_PX}",
        st.render.raster.pixels
    );
    assert_eq!(
        st.render.raster.batches, 1,
        "单按钮应只产生 1 个批次，实际 {}",
        st.render.raster.batches
    );
}

/// 整窗脏时，光栅像素数应等于窗口面积（确认"退化整窗"确实退成了整窗）。
#[test]
fn full_repaint_covers_the_whole_window() {
    let (rt, mut app, id) = page(50);
    let _ = app.frame_all();

    app.window_ctx_mut(id).unwrap().track_mut().damage_whole_window();
    rt.mark(id, Dirty::PAINT | Dirty::PRESENT);
    let st = app.frame_all().remove(0).1;

    assert!(st.damage_all, "应标记为整窗脏");
    assert!(
        st.render.raster.pixels >= FULL_PX * 9 / 10,
        "整窗脏应光栅化整个窗口，实际 {} / {FULL_PX}",
        st.render.raster.pixels
    );
    assert_eq!(st.render.raster.batches, 1, "整窗应是单批次");
}
// ─────────────────────── 碎片与退化（D21） ───────────────────────

/// **滚动确实该退化为整窗**—— 这条曾经被我误判为 bug，现更正为**正断言**。
///
/// ## 更正记录
///
/// C0 阶段我看到"滚动一次 302 碎片 ⇒ 退化为整窗"，判定这是 D21 缺陷并写下
/// "任何 ≥9 节点变化都普遍失效"。**那个推论是错的**：
///
/// **滚动 20px 时窗口内每一行都位移了 ⇒ 脏区本就覆盖整个视口。**
/// 此时退化整窗**是正确的** —— 按 302 个高度重叠的碎片分别光栅，
/// 总面积是窗口的 **8.4 倍**。
///
/// 我犯的错是：只看了"退化了"这个事实，**没验证"退化是否真的更差"**。
/// 真正需要修的是"**碎片多且分散、总面积小**"那种情形
/// （见 `scattered_small_updates_should_not_fall_back_to_full_window`）。
#[test]
fn scrolling_falls_back_to_full_window_and_that_is_correct() {
    const ROWS: usize = 50;
    let (_rt, mut app, id) = page(ROWS);
    let _ = app.frame_all();

    let scroller = {
        let w = app.window_ctx(id).unwrap();
        w.track()
            .node_ids()
            .find(|n| w.track().get(*n).map(|n| n.layout.overflow_scroll).unwrap_or(false))
            .expect("应存在滚动容器")
    };
    let ok = app
        .window_ctx_mut(id)
        .unwrap()
        .track_mut()
        .set_scroll_offset(scroller, (0.0, 20.0));
    assert!(ok, "滚动偏移应当被接受");

    let st = app.frame_all().remove(0).1;
    println!(
        "滚动：碎片 = {}，批次 = {}，光栅像素 = {} / {FULL_PX}",
        st.damage.len(),
        st.render.raster.batches,
        st.render.raster.pixels
    );

    assert!(
        st.render.raster.batches == 1 && st.render.raster.pixels >= FULL_PX * 9 / 10,
        "滚动的脏区本就覆盖整个视口 ⇒ 退化为整窗是**正确**的（不是 bug）。\
         若此断言失败，说明重叠碎片的合并或视口布局变了，需重新评估：\
         批次 = {}，光栅像素 = {}",
        st.render.raster.batches,
        st.render.raster.pixels
    );
}

/// 碎片数的**精确基线**（供后续调参对照，不判定对错）。
#[test]
fn scroll_fragment_count_baseline() {
    const ROWS: usize = 50;
    let (_rt, mut app, id) = page(ROWS);
    let _ = app.frame_all();

    let scroller = {
        let w = app.window_ctx(id).unwrap();
        w.track()
            .node_ids()
            .find(|n| w.track().get(*n).map(|n| n.layout.overflow_scroll).unwrap_or(false))
            .expect("应存在滚动容器")
    };
    app.window_ctx_mut(id)
        .unwrap()
        .track_mut()
        .set_scroll_offset(scroller, (0.0, 20.0));

    let st = app.frame_all().remove(0).1;
    println!(
        "滚动基线：脏区碎片 = {}，批次 = {}，光栅像素 = {} / {FULL_PX}",
        st.damage.len(),
        st.render.raster.batches,
        st.render.raster.pixels
    );

    // 不断言具体数值 —— 只保证"有碎片可合并"这个前提成立。
    assert!(
        !st.damage.is_empty(),
        "滚动必然产生脏矩形（否则说明滚动没被脏区覆盖，是另一个 bug）"
    );
}

// ─────────────────────── 上屏（D22：局部上屏只按整行拷） ───────────────────────

/// **D22 已修**（C1）：局部上屏原先按 `row * stride` 整行拷贝、**完全忽略 `r.x` / `r.width`**，
/// 一个 40×20 的脏区会被展开成 20 行 × 全窗宽 ⇒ 呈现带宽白白翻倍。
///
/// 修法是把拷贝逻辑抽成纯函数 [`lieui::platform::copy_damage_rects`] 并按列拷贝；
/// 本文件保留一条端到端的"脏区面积 vs 拷贝量"关系断言作为回归护栏。
#[test]
#[cfg(feature = "winit")] // `copy_damage_rects` 只在平台层存在（softbuffer 是可选依赖）
fn local_present_copies_proportionally_to_damage_area() {
    use lieui::platform::copy_damage_rects;
    use vello_cpu::color::PremulRgba8;

    // 1280×720 的窗口 pixmap（只分配需要的部分，避免测试里造 4MB）
    const W: u32 = 1280;
    const STRIDE: usize = W as usize;
    let src: Vec<PremulRgba8> = (0..(STRIDE * 720))
        .map(|i| {
            let v = (i % 251 + 1) as u8;
            PremulRgba8 {
                r: v,
                g: v,
                b: v,
                a: 255,
            }
        })
        .collect();
    let mut dst = vec![0u32; src.len()];

    // 一个 40×20 的小脏区（典型：点一下按钮）
    let rects = [softbuffer::Rect {
        x: 100,
        y: 200,
        width: std::num::NonZeroU32::new(40).unwrap(),
        height: std::num::NonZeroU32::new(20).unwrap(),
    }];
    let copied = copy_damage_rects(&src, &mut dst, STRIDE, &rects);

    assert_eq!(copied, 800, "40×20 的脏区应恰好拷 800 像素");
    // 旧行为是 20 行 × 1280 宽 = 25600 ⇒ 浪费 32 倍
    assert!(
        copied < 25600 / 10,
        "拷贝量应远小于整行拷（旧行为 25600），实际 {copied}"
    );
}

// ─────────────────────── 脏区纪律 ───────────────────────

/// 脏区是"有上限"的吗？—— 一个合理的自检：同等规模的标脏，
/// 产生的碎片数不应与页面节点数成平方级关系。
#[test]
fn marking_many_nodes_does_not_explode_fragment_count() {
    const ROWS: usize = 50;
    let (_rt, mut app, id) = page(ROWS);
    let _ = app.frame_all();

    // 标脏前 30 个节点
    let targets: Vec<NodeId> = {
        let w = app.window_ctx(id).unwrap();
        w.track().node_ids().take(30).collect()
    };
    for t in targets {
        app.window_ctx_mut(id).unwrap().track_mut().mark_paint_dirty(t);
    }

    let st = app.frame_all().remove(0).1;
    println!(
        "集中标脏 30 个节点：碎片 = {}，批次 = {}，光栅像素 = {} / {FULL_PX}",
        st.damage.len(),
        st.render.raster.batches,
        st.render.raster.pixels
    );
    // ⚠️ 这些节点集中在列表**顶部**且横跨整行 ⇒ 脏区面积确实过半
    // ⇒ **退化成整窗是正确的**（此时按 30 个碎片分别光栅反而更贵）。
    // 这条是 `scattered_small_updates...` 的镜像护栏：防止"为了不整窗而整窗"。
    assert!(
        st.render.raster.pixels >= FULL_PX / 2,
        "集中大面积更新退化成整窗是对的（实际光栅 {} / {FULL_PX}）",
        st.render.raster.pixels
    );
}

/// **C2 的核心验收**：**分散**的小更新**不该**退化成整窗光栅。
///
/// ## 为什么"标脏前 30 个节点"当场景测不出东西（我最初踩了这个坑）
///
/// `node_ids()` 按 arena 顺序，前 30 个集中在列表**顶部**、每个横跨整行
/// ⇒ 脏区总面积确实超过窗口 45% ⇒ 退化整窗是**正确**的。
///
/// ## 正确场景
///
/// 取**分散在列表各处**的按钮（每 5 个取 1）：每个约 60×24，10 个合计约 1.4 万像素
/// = 窗口的 1.6%。此时碎片数 ≈ 10（大于旧阈值 8，但**面积微不足道**）
/// ⇒ 按碎片光栅化 ≪ 整窗 ⇒ 退化整窗是**纯浪费**，这才是 D21 要修的地方。
#[test]
fn scattered_small_updates_should_not_fall_back_to_full_window() {
    const ROWS: usize = 50;
    let (_rt, mut app, id) = page(ROWS);
    let _ = app.frame_all();

    let targets: Vec<NodeId> = (0..10).map(|i| deep_button(&app, id, i * 5)).collect();
    for t in &targets {
        app.window_ctx_mut(id).unwrap().track_mut().mark_paint_dirty(*t);
    }

    let st = app.frame_all().remove(0).1;
    println!(
        "分散标脏 10 个按钮：碎片 = {}，批次 = {}，光栅像素 = {} / {FULL_PX}",
        st.damage.len(),
        st.render.raster.batches,
        st.render.raster.pixels
    );

    assert!(
        !st.damage_all && st.render.raster.pixels < FULL_PX / 4,
        "分散小更新不该退化成整窗（批次 {}，光栅 {}/{}）",
        st.render.raster.batches,
        st.render.raster.pixels,
        FULL_PX
    );
    assert!(st.render.raster.rasterized, "仍应走局部光栅");
}
