//! 脏区收益基准：整窗重绘 vs 局部脏区重绘 vs 空闲帧。
//!
//! ```text
//! cargo run --release --example damage_bench
//! ```
//!
//! 本机（Windows / release / 1280×720）实测：
//!
//! ```text
//! 节点  151 行  50 | 整窗   4.6ms (921600 px) | 局部  49µs (588 px) | 空闲 0.5µs | 94×
//! 节点  601 行 200 | 整窗   2.6ms (921600 px) | 局部 114µs (588 px) | 空闲 1.6µs | 22×
//! 节点 1801 行 600 | 整窗   7.2ms (921600 px) | 局部 388µs (637 px) | 空闲 5.7µs | 19×
//! ```
//!
//! 两点结论（也是设计取舍的依据）：
//!
//! 1. **光栅化确实被脏区救回来了**：整窗是 ms 级（占 60fps 预算的 15~43%），
//!    小脏区是 µs 级；空闲帧几乎为零（"有脏才画"）。
//! 2. **但局部重绘的耗时随节点数线性增长**（49µs → 388µs）：瓶颈不在光栅化，
//!    而在**每帧全量重建场景 + 剔除**（O(节点数)）。真要再往上走，
//!    该做的是增量场景 / 保留 draw list，而不是继续抠脏区。
//!
//! 想让帧总是正确优先（放弃脏区收益）时用 `WindowConfig::full_repaint(true)`。

use std::rc::Rc;
use std::time::Instant;

use lieui::layout::rect_of;
use lieui::prelude::*;
use lieui::{Dirty, Kind, NodeId};

struct Page {
    rows: usize,
}

impl ViewModel for Page {
    fn view(self: &Rc<Self>, v: &mut ViewBuf) {
        v.column(|c| {
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
    }
}

fn main() {
    const W: f32 = 1280.0;
    const H: f32 = 720.0;
    const ITERS: u32 = 50;

    for rows in [50usize, 200, 600] {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(W, H), Page { rows });
        app.frame_all();

        // 找一个深处的按钮当"局部脏区"目标
        let target: NodeId = {
            let w = app.window_ctx(id).unwrap();
            w.track()
                .node_ids()
                .filter(|n| {
                    w.track()
                        .get(*n)
                        .map(|n| matches!(n.kind, Kind::Button { .. }))
                        .unwrap_or(false)
                })
                .nth(10)
                .expect("至少有 11 个按钮")
        };
        let (tr_w, tr_h) = {
            let r = rect_of(app.window_ctx(id).unwrap().track(), target);
            (r.width, r.height)
        };
        let nodes = app.window_ctx(id).unwrap().track().len();

        // ① 整窗重绘
        let mut full_px = 0u64;
        let t0 = Instant::now();
        for _ in 0..ITERS {
            app.window_ctx_mut(id)
                .unwrap()
                .track_mut()
                .damage_whole_window();
            rt.mark(id, Dirty::PAINT | Dirty::PRESENT);
            let st = app.frame_all();
            full_px = st[0].1.render.raster.pixels;
        }
        let full = t0.elapsed() / ITERS;

        // ② 局部重绘（一个小按钮的矩形）
        let mut part_px = 0u64;
        let mut batches = 0usize;
        let t1 = Instant::now();
        for _ in 0..ITERS {
            app.window_ctx_mut(id)
                .unwrap()
                .track_mut()
                .mark_paint_dirty(target);
            let st = app.frame_all();
            part_px = st[0].1.render.raster.pixels;
            batches = st[0].1.render.raster.batches;
        }
        let part = t1.elapsed() / ITERS;

        // ③ 空闲帧（没有任何脏区 ⇒ 应该几乎为零）
        let t2 = Instant::now();
        for _ in 0..ITERS {
            app.frame_all();
        }
        let idle = t2.elapsed() / ITERS;

        println!(
            "节点 {nodes:4} 行 {rows:3} | 整窗 {full:>9.3?} ({full_px:>8} px) | \
             局部 {part:>9.3?} ({part_px:>7} px, {batches} 批, 目标 {tr_w:.0}×{tr_h:.0}) | \
             空闲 {idle:>9.3?} | 整窗/局部 {:.0}×",
            full.as_secs_f64() / part.as_secs_f64().max(1e-9),
        );
    }

    strategies();
    rasterize_strategies();
}

/// 端到端：同一份脏区、不同批次策略的**实际光栅化耗时**
/// （包含现实现里"每个批次都把整个场景重放一遍"的固定开销）
fn rasterize_strategies() {
    use lieui::render::{
        Rasterizer, SceneBuilder, SceneOptions, damage_batches, damage_batches_bands,
        damage_batches_union,
    };

    const W: f32 = 1280.0;
    const H: f32 = 720.0;
    const ITERS: u32 = 30;

    let rt = Runtime::new();
    let mut app = App::new(rt);
    let id = app.window(WindowConfig::new().size(W, H), Page { rows: 200 });
    app.frame_all();

    // 滚动 16 行（每行整宽 20px）——列表滚动的典型脏区形状
    let damage: Vec<Rect> = (0..16)
        .map(|i| Rect::new(0.0, i as f32 * 44.0, W, 20.0))
        .collect();

    let (scene, ops) = {
        let track = app.window_ctx(id).unwrap().track();
        let opts = SceneOptions {
            window: Size::new(W, H),
            background: Color::WHITE,
            focus_ring: true,
            theme: Theme::light(),
        };
        let scene = SceneBuilder::new().build(track, &opts, &damage, false);
        let ops = scene.len();
        (scene, ops)
    };

    let cases: Vec<(&str, Vec<Rect>)> = vec![
        ("滚动 16 条横带", damage.clone()),
        (
            "分散 4 块（四角）",
            vec![
                Rect::new(20.0, 20.0, 200.0, 30.0),
                Rect::new(1060.0, 20.0, 200.0, 30.0),
                Rect::new(20.0, 660.0, 200.0, 30.0),
                Rect::new(1060.0, 660.0, 200.0, 30.0),
            ],
        ),
        (
            "同列 6 行",
            (0..6)
                .map(|i| Rect::new(40.0, 100.0 + i as f32 * 40.0, 200.0, 30.0))
                .collect(),
        ),
    ];

    println!("\n== 真实光栅化耗时（场景 {ops} 个原语；每策略 30 次平均）==");
    for (case, damage) in cases {
        println!("-- {case}");
        for (name, f) in [
            (
                "精确碎片（现状）",
                damage_batches as fn(Size, &[Rect], bool) -> Vec<Rect>,
            ),
            ("单包围盒", damage_batches_union),
            ("水平行带", damage_batches_bands),
        ] {
            let batches = f(Size::new(W, H), &damage, false);
            let px: f32 = batches.iter().map(|r| r.width * r.height).sum();
            let mut raster = Rasterizer::new(Size::new(W, H));
            raster.rasterize_batches(&scene, &batches); // 预热
            let t = Instant::now();
            for _ in 0..ITERS {
                raster.rasterize_batches(&scene, &batches);
            }
            let d = t.elapsed() / ITERS;
            println!(
                "   {name:<16} {:>3} 批 / {px:>8.0} px | {d:>10.3?}",
                batches.len()
            );
        }
    }
}

/// 三种脏区策略在同一组脏矩形下的批次/像素对比（决定要不要简化）
fn strategies() {
    use lieui::render::{damage_batches, damage_batches_bands, damage_batches_union};

    let size = Size::new(1280.0, 720.0);
    let block = |x: f32, y: f32| Rect::new(x, y, 200.0, 30.0);

    let cases: [(&str, Vec<Rect>); 5] = [
        ("单块（一个按钮）", vec![block(100.0, 200.0)]),
        (
            "分散 4 块（四角各一个）",
            vec![
                block(20.0, 20.0),
                block(1060.0, 20.0),
                block(20.0, 660.0),
                block(1060.0, 660.0),
            ],
        ),
        (
            "同列 6 行（列表多处变化）",
            (0..6).map(|i| block(40.0, 100.0 + i as f32 * 40.0)).collect(),
        ),
        (
            "同行左右两块（状态栏两端）",
            vec![block(20.0, 10.0), block(1000.0, 10.0)],
        ),
        (
            "滚动 16 条横带（每行整宽）",
            (0..16).map(|i| Rect::new(0.0, i as f32 * 44.0, 1280.0, 20.0)).collect(),
        ),
    ];

    println!("\n== 策略对比（1280×720，脏区→批次）==");
    println!(
        "{:<26} {:>14} {:>16} {:>16}",
        "场景", "精确碎片", "单包围盒", "水平行带"
    );
    for (name, damage) in cases {
        let mut row = Vec::new();
        for f in [
            damage_batches as fn(Size, &[Rect], bool) -> Vec<Rect>,
            damage_batches_union,
            damage_batches_bands,
        ] {
            let b = f(size, &damage, false);
            let px: f32 = b.iter().map(|r| r.width * r.height).sum();
            row.push(format!("{} 批 / {:.0} px", b.len(), px));
        }
        println!(
            "{:<26} {:>14} {:>16} {:>16}",
            name, row[0], row[1], row[2]
        );
    }
}
