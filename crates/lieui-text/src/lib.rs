//! lieui-text —— 文本测量与排版（基于 parley）
//!
//! ## 两段式测度（M1 任务 2）
//! parley 的排版天然分两步，本模块把它们拆成两个可缓存的阶段：
//!
//! 1. **measure**：`ranged_builder → build → break_all_lines(max_width)`。
//!    得到尺寸，供布局引擎求解；结果按 `MeasureKey` 缓存。
//! 2. **layout**：在上面那份**已分行的 Layout** 上执行 `align()`。
//!    不再重新整形（shape），只是把每行在行盒内对齐。
//!
//! 关键点：两步共享同一份 `Layout`，换行结果不会被丢弃重算——
//! 这是「测度完还要再排一次」场景下的主要 CPU 节省点。
//!
//! ## 所有权隔离（设计 §3.4）
//! `TextService` 独占持有 `FontContext` 与 `LayoutContext`（parley 需要三重 `&mut`），
//! 不共享给节点树；所有交互按 `MeasureKey` 索引。

pub mod cache;
pub mod spec;

use std::collections::{HashMap, VecDeque};

use parley::{
    Alignment, AlignmentOptions, FontContext, LayoutContext,
    style::{
        FontFamily, FontWeight as ParleyFontWeight, LineHeight as ParleyLineHeight, StyleProperty,
    },
};

pub use cache::{CacheStats, FnvBuildHasher, MeasureKey, NO_MAX_WIDTH};
pub use spec::{FontWeight, TextAlign, TextSpec};

/// 文本画笔。M1 阶段排版不携带颜色——颜色由绘制阶段从属性表取，
/// 这样缓存键里就不会混入纯绘制属性。
pub type Brush = ();

/// parley 排版结果
pub type TextLayout = parley::Layout<Brush>;

/// 文本服务统计
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct TextStats {
    /// measure 调用次数
    pub measure_calls: u64,
    /// 其中命中缓存的次数
    pub measure_hits: u64,
    /// 真正执行整形（shape）的次数
    pub shapes: u64,
    /// 执行 align 的次数（第二段）
    pub aligns: u64,
    /// 因容量淘汰的条目数
    pub evictions: u64,
}

impl TextStats {
    pub fn measure_hit_rate(&self) -> f32 {
        if self.measure_calls == 0 {
            1.0
        } else {
            self.measure_hits as f32 / self.measure_calls as f32
        }
    }
}

struct TextEntry {
    layout: TextLayout,
    size: (f32, f32),
    /// 已对齐的方式；与请求不符时重新 align
    aligned: Option<TextAlign>,
}

/// 文本服务。每个 App 一份（parley 上下文是重资源，必须长期持有）。
pub struct TextService {
    font_cx: FontContext,
    layout_cx: LayoutContext<Brush>,
    entries: HashMap<MeasureKey, TextEntry, FnvBuildHasher>,
    order: VecDeque<MeasureKey>,
    capacity: usize,
    stats: TextStats,
}

impl Default for TextService {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl TextService {
    pub fn new(capacity: usize) -> Self {
        Self {
            font_cx: FontContext::default(),
            layout_cx: LayoutContext::new(),
            entries: HashMap::with_hasher(FnvBuildHasher::default()),
            order: VecDeque::new(),
            capacity: capacity.max(16),
            stats: TextStats::default(),
        }
    }

    // ── 第一段：measure ──

    /// 测量文本尺寸。命中缓存时零成本返回。
    pub fn measure(&mut self, spec: &TextSpec<'_>) -> (f32, f32) {
        self.stats.measure_calls += 1;
        let key = spec.key();
        if let Some(e) = self.entries.get(&key) {
            self.stats.measure_hits += 1;
            return e.size;
        }

        let layout = self.shape(spec);
        let size = (layout.width(), layout.height());
        self.insert(
            key,
            TextEntry {
                layout,
                size,
                aligned: None,
            },
        );
        self.stats.shapes += 1;
        size
    }

    // ── 第二段：layout ──

    /// 取得排版结果（已分行且已对齐）。
    ///
    /// 若该 key 尚未 measure，会先走第一段；否则复用缓存里已分行的 Layout，只补 align。
    pub fn layout(&mut self, spec: &TextSpec<'_>) -> &TextLayout {
        let key = spec.key();
        if !self.entries.contains_key(&key) {
            self.stats.measure_calls += 1;
            let layout = self.shape(spec);
            let size = (layout.width(), layout.height());
            self.insert(
                key,
                TextEntry {
                    layout,
                    size,
                    aligned: None,
                },
            );
            self.stats.shapes += 1;
        }

        let entry = self.entries.get_mut(&key).expect("just inserted");
        if entry.aligned != Some(spec.align) {
            entry
                .layout
                .align(to_alignment(spec.align), AlignmentOptions::default());
            entry.aligned = Some(spec.align);
            self.stats.aligns += 1;
        }
        &self.entries.get(&key).expect("just inserted").layout
    }

    /// 仅取第一段已分行的尺寸（不触发 align），用于布局求解后的尺寸回查
    pub fn peek(&self, spec: &TextSpec<'_>) -> Option<(f32, f32)> {
        self.entries.get(&spec.key()).map(|e| e.size)
    }

    // ── 缓存管理 ──

    fn insert(&mut self, key: MeasureKey, entry: TextEntry) {
        if self.entries.len() >= self.capacity {
            let drop = self.capacity / 4;
            for _ in 0..drop {
                if let Some(k) = self.order.pop_front() {
                    self.entries.remove(&k);
                    self.stats.evictions += 1;
                }
            }
        }
        self.entries.insert(key, entry);
        self.order.push_back(key);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// 字体资源变化（换字体、DPI 变化）后调用
    pub fn invalidate(&mut self) {
        self.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    // ── 统计 ──

    pub fn stats(&self) -> TextStats {
        self.stats
    }
    pub fn reset_stats(&mut self) {
        self.stats = TextStats::default();
    }

    pub fn font_context(&mut self) -> &mut FontContext {
        &mut self.font_cx
    }
    pub fn layout_context(&mut self) -> &mut LayoutContext<Brush> {
        &mut self.layout_cx
    }

    // ── parley 交互 ──

    /// 新建排版并分行（第一段）。不查缓存。
    fn shape(&mut self, spec: &TextSpec<'_>) -> TextLayout {
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, spec.text, 1.0, true);

        builder.push_default(StyleProperty::FontSize(spec.font_size));
        builder.push_default(StyleProperty::FontWeight(ParleyFontWeight::new(
            spec.weight.0 as f32,
        )));
        if spec.line_height > 0.0 {
            builder.push_default(StyleProperty::LineHeight(
                ParleyLineHeight::FontSizeRelative(spec.line_height),
            ));
        }
        if spec.italic {
            builder.push_default(StyleProperty::FontStyle(parley::style::FontStyle::Italic));
        }
        if !spec.family.is_empty() {
            builder.push_default(StyleProperty::FontFamily(FontFamily::named(spec.family)));
        }

        let mut layout = builder.build(spec.text);
        layout.break_all_lines(spec.effective_max_width());
        layout
    }
}

fn to_alignment(a: TextAlign) -> Alignment {
    match a {
        TextAlign::Start => Alignment::Start,
        TextAlign::Center => Alignment::Center,
        TextAlign::End => Alignment::End,
        TextAlign::Justify => Alignment::Justify,
    }
}
