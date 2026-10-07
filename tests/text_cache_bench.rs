// 一次性性能对照：**不是回归测试**（计时断言容易 flaky，跨机器不可比）。
//
// 用途：回答"parley 自己有缓存，我们的 TextCache 还有必要吗"—— 用数据说话。
// 跑法：`cargo test --release --test text_cache_bench -- --nocapture --ignored`
//
// 结论（2026-10-07 实测，release）：
//   miss(真排版) 与 hit(缓存命中) 相差约两个数量级。
// 原因见文件末尾的说明：parley 缓存的是 **shaping 的输入侧**（字体数据 / shaper 实例），
// **不是排版输出** —— 每次 `build()` 仍会跑 `analyze_text` + `shape_text` + 断行 + 对齐。

use std::time::Instant;

use lieui::prelude::Color;
use lieui::render::TextCache;
use lieui_text::TextSpec;

/// 一段需要真实 shaping + 断行的中文文本（接近实际 UI 文案长度）
fn sample_text(tag: &str) -> String {
    format!("{tag}：这是一段用于测量的中文文本，需要触发完整的 shaping、换行与对齐流程。")
}

fn timeit(mut f: impl FnMut()) -> f64 {
    let t0 = Instant::now();
    f();
    t0.elapsed().as_secs_f64()
}

#[test]
#[ignore = "性能对照，非回归测试；手动跑"]
fn text_cache_hit_vs_miss_cost() {
    let spec = TextSpec::default();
    let color = Color::rgba(20, 20, 20, 255);
    const N: usize = 300;
    const MAX_LEN: usize = 180;

    // ── miss：每次内容都不同 ⇒ 必然真排版 ──
    let mut miss_cache = TextCache::new();
    let mut n = 0usize;
    let miss_total = timeit(|| {
        for _ in 0..N {
            // 唯一后缀保证不命中；截断到固定长度以免文本长度影响对比
            let t = sample_text(&format!("#{n}"));
            let t = &t[..t.len().min(MAX_LEN)];
            assert!(!miss_cache.get_or_build_counted(t, &spec, color).1);
            n += 1;
        }
    });

    // ── hit：同一段文本重复查询 ⇒ 全部命中 ──
    let fixed = sample_text("fixed");
    let text = &fixed[..fixed.len().min(MAX_LEN)];
    let mut hit_cache = TextCache::new();
    hit_cache.get_or_build(text, &spec, color); // 预热
    let hit_total = timeit(|| {
        for _ in 0..N {
            assert!(hit_cache.get_or_build_counted(text, &spec, color).1);
        }
    });

    let miss = miss_total / N as f64;
    let hit = hit_total / N as f64;
    println!("文本长度      : {} 字节", text.len());
    println!("miss(真排版)  : {:>9.3} us/次", miss * 1e6);
    println!("hit (缓存命中): {:>9.3} us/次", hit * 1e6);
    println!("比值          : {:.0}x", miss / hit);
    println!(
        "换算 50 个可见文本 / 帧 @60fps：无缓存 {:>6.2} ms/帧，有缓存 {:>6.4} ms/帧",
        miss * 50.0 * 1e3,
        hit * 50.0 * 1e3
    );
}

// ── 为什么 parley 的缓存不能替代这一层 ──
//
// parley 0.11.1 内部确实有缓存，但缓存的是 **shaping 的输入侧**：
//   shape_data_cache:     LruCache<ShapeDataKey,     ShaperData>     （字体数据）
//   shape_instance_cache: LruCache<ShapeInstanceId,  ShaperInstance>  （script/direction 的 shaper）
//   shape_plan_cache:     LruCache<ShapePlanId,      ShapePlan>       （两者组合）
//   fcx.source_cache      （fontique 字体源，prune(128)）
//
// 而 `RangedBuilder::build()` → `build_into_layout()` 每次都会执行：
//   1. `analyze_text`（ICU：Bidi 级别、换行机会、脚本/词边界）
//   2. 逐 style_run 的 `shape_text`（harfrust shaping：字形选择 + 定位）
//   3. `break_all_lines`（断行）
//   4. `align`（对齐）
//
// 也就是说：**字体数据是热的，但排版结果每次都重算。**
// 我们缓存的 `Arc<TextLayout>` 位于更上一层，与 parley 的缓存**互补而非重复**：
//   parley 缓存 ⇒ 第二次排版不必重新解析字体文件
//   TextCache   ⇒ 第二次排版**根本不用发生**
//
// 如果删掉 TextCache，静态 UI（文本不变、每帧重建 Scene）就要每秒做
// `50 可见文本 × 60 帧 = 3000` 次完整排版 —— 上面 `miss` 那一列就是这个代价。
