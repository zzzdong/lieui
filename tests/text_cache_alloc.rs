//! `TextCache` 命中路径的**堆分配计数**验收。
//!
//! ## 为什么这个测试要单开一个文件
//!
//! 要统计"某段代码有没有分配"，就得接管全局分配器（`#[global_allocator]`），
//! 而它**是整个测试二进制共享的**。计数用 `thread_local` 隔离：
//! 只累加**当前线程**的分配，因此与其它并行测试互不干扰。
//!
//! ## 它钉住的契约
//!
//! `TextCache` 的意义在于"命中比未命中便宜得多"。改造前，命中路径会先
//! `text.to_string()` 构造key 才能查表 —— **每帧每可见文本一次堆分配 + 字节拷贝**。
//! 改造后是"hash 分桶 + 桶内 `memcmp`"，命中路径**零分配**。
//!
//! 这类优化**极易被无声回退**：哪天有人觉得"直接 `HashMap<TextKey, _>` 更简洁"，
//! 把它改回去 —— 编译通过、测试全绿、渲染结果一像素不差 —— 只有这个测试会红。

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use lieui::prelude::Color;
use lieui::render::TextCache;
use lieui_text::TextSpec;

thread_local! {
    /// 当前线程的分配次数
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
    /// 是否处于"计量区间"
    static TRACKING: Cell<bool> = const { Cell::new(false) };
}

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if TRACKING.with(Cell::get) {
            ALLOCS.with(|c| c.set(c.get() + 1));
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        if TRACKING.with(Cell::get) {
            ALLOCS.with(|c| c.set(c.get() + 1));
        }
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// 统计 `f` 执行期间的分配次数。
fn allocs_during<R>(f: impl FnOnce() -> R) -> (R, u64) {
    ALLOCS.with(|c| c.set(0));
    TRACKING.with(|t| t.set(true));
    let r = f();
    TRACKING.with(|t| t.set(false));
    let n = ALLOCS.with(Cell::get);
    (r, n)
}

/// 超过 SSO 阈值（Rust `String` 短串优化约 15 字节）的文本 ——
/// 任何拷贝都必须落到堆分配，才能被计数器看见。
fn long_text(tag: &str) -> String {
    format!("{tag} —— 这是一段刻意写长的文本，用来绕开 SSO，确保任何拷贝都必然分配")
}

/// ★ 核心验收：命中路径**零堆分配**。
#[test]
fn cache_hit_path_does_not_allocate() {
    let mut cache = TextCache::new();
    let spec = TextSpec::default();
    let color = Color::rgba(255, 255, 255, 255);
    let text = long_text("hello");

    // 预热：先 miss 一次，把条目放进缓存
    let (first_layout, first_allocs) = allocs_during(|| cache.get_or_build(&text, &spec, color));
    assert!(
        first_allocs > 0,
        "未命中路径本就该有分配（Arc + key + 排版），实际 {first_allocs}"
    );

    // ★ 被测区间：连续 100 次命中
    const N: usize = 100;
    let ((last, hits), allocs) = allocs_during(|| {
        let mut hits = 0usize;
        let mut last = None;
        for _ in 0..N {
            let (l, counted) = cache.get_or_build_counted(&text, &spec, color);
            if counted {
                hits += 1;
            }
            last = Some(l);
        }
        (last.unwrap(), hits)
    });

    assert_eq!(hits, N, "所有查询都应命中缓存");
    assert_eq!(
        allocs, 0,
        "命中路径有 {allocs} 次堆分配 / {N} 次查询（应恒为 0）——\
         是否有人把两段式 key 改回了 `HashMap<TextKey, _>`？"
    );
    // 结果必须与预热那次是**同一份**排版（`Arc` 共享，不是重建）
    assert!(Arc::ptr_eq(&first_layout, &last), "命中应返回缓存里那一份排版");
}

#[test]
fn distinct_texts_all_miss() {
    let mut cache = TextCache::new();
    let spec = TextSpec::default();
    let color = Color::rgba(10, 20, 30, 255);

    for i in 0..50 {
        let t = long_text(&format!("row-{i}"));
        let (_, hit) = cache.get_or_build_counted(&t, &spec, color);
        assert!(!hit, "每个不同文本都该是首次未命中：{i}");
        let (_, hit2) = cache.get_or_build_counted(&t, &spec, color);
        assert!(hit2, "同一文本第二次必须命中：{i}");
    }
    assert_eq!(cache.len(), 50, "缓存应恰好有 50 条");
}

#[test]
fn different_color_or_spec_is_a_different_entry() {
    let mut cache = TextCache::new();
    let t = long_text("same-text");
    let base = TextSpec::default();

    let a = Color::rgba(255, 0, 0, 255);
    let b = Color::rgba(0, 255, 0, 255);
    let big = TextSpec {
        font_size: base.font_size * 2.0,
        ..Default::default()
    };

    assert!(!cache.get_or_build_counted(&t, &base, a).1, "A 首次 miss");
    assert!(cache.get_or_build_counted(&t, &base, a).1, "A 再命中");
    assert!(!cache.get_or_build_counted(&t, &base, b).1, "颜色不同 ⇒ 另一条");
    assert!(!cache.get_or_build_counted(&t, &big, a).1, "字号不同 ⇒ 另一条");
    assert_eq!(cache.len(), 3, "三条独立条目");
}

/// 超过上限时整表清空 —— 且判定用**条目数**而非桶数。
#[test]
fn overflow_clears_by_entry_count_not_bucket_count() {
    let mut cache = TextCache::new();
    let spec = TextSpec::default();
    let color = Color::rgba(1, 2, 3, 255);

    for i in 0..2048 {
        let t = long_text(&format!("f{i}"));
        cache.get_or_build(&t, &spec, color);
    }
    assert_eq!(cache.len(), 2048, "应恰好到上限且尚未清空");

    let t = long_text("trigger");
    cache.get_or_build(&t, &spec, color);
    assert_eq!(cache.len(), 1, "达到上限时整表清空，本条是唯一幸存者");
}

/// 文本内容是 key 的**唯一真实来源**：hash 相同但内容不同必须仍然 miss。
///
/// 这是两段式 key 的正确性要害 —— 若只信 hash 而不比对内容，
/// 冲突会导致**画出错的文本**（静默的视觉错误）。
#[test]
fn entries_are_matched_by_content_not_just_hash() {
    let mut cache = TextCache::new();
    let spec = TextSpec::default();
    let color = Color::rgba(7, 7, 7, 255);

    let a = long_text("AAAA");
    let b = long_text("BBBB");
    let (la, hit_a) = cache.get_or_build_counted(&a, &spec, color);
    let (lb, hit_b) = cache.get_or_build_counted(&b, &spec, color);
    assert!(!hit_a && !hit_b, "两个不同文本都该是首次未命中");
    assert!(!Arc::ptr_eq(&la, &lb), "两个文本必须是两份独立排版");
    assert_eq!(cache.len(), 2);

    // 反查：各自命中自己，且拿回**同一份**排版
    let (la2, hit1) = cache.get_or_build_counted(&a, &spec, color);
    let (lb2, hit2) = cache.get_or_build_counted(&b, &spec, color);
    assert!(hit1 && hit2, "两个文本都应命中");
    assert!(
        Arc::ptr_eq(&la, &la2) && Arc::ptr_eq(&lb, &lb2),
        "命中必须返回缓存里那一份，而不是重建"
    );
}

#[test]
fn empty_text_is_cacheable() {
    let mut cache = TextCache::new();
    let spec = TextSpec::default();
    let color = Color::rgba(0, 0, 0, 255);
    let (_, hit1) = cache.get_or_build_counted("", &spec, color);
    let (_, hit2) = cache.get_or_build_counted("", &spec, color);
    assert!(!hit1, "空串首次应 miss（它确实有排版结果）");
    assert!(hit2, "空串第二次应命中");
}

#[test]
fn clear_empties_the_cache() {
    let mut cache = TextCache::new();
    let spec = TextSpec::default();
    let color = Color::rgba(9, 9, 9, 255);
    let t = long_text("x");
    cache.get_or_build(&t, &spec, color);
    assert_eq!(cache.len(), 1);
    cache.clear();
    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
    assert!(!cache.get_or_build_counted(&t, &spec, color).1, "清空后应重新 miss");
}

/// 冒烟：计数器本身没被写坏 —— 否则上面"零分配"的断言毫无意义。
#[test]
fn counter_actually_counts() {
    let (_, n) = allocs_during(|| {
        let v: Vec<u8> = Vec::with_capacity(1024);
        v.len()
    });
    assert!(n >= 1, "分配计数器工作正常（应 ≥1，实际 {n}）");
}
