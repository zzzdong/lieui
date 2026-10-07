//! 文本**墨迹盒**（ink bounds）——参考 lievisual 的 `text/glyph.rs` + `extract_lines_from_parley`。
//!
//! 为什么需要它：parley 的 `Layout::width/height` 是**行盒**（advance × 行高），行盒里
//! ascent/descent 不对称 ⇒ 可见字形的视觉中心**不在行盒中心**（普通字体约偏 0.1em 高）。
//! 图标字体（Material Icons）行高系数 1.0、墨迹居中于 em，于是"行盒对齐"看起来就是
//! "图标对齐了、文字偏上"。按**墨迹盒**对齐才是视觉正确的做法。
//!
//! 逐字形墨迹盒由 skrifa（parley 内部的同一套字体引擎）直接读字体的 bbox（`GlyphMetrics::bounds`，
//! 不是重建轮廓），按 (字体 blob id, collection index, 字号) 进程级缓存字形盒 map。
//!
//! 坐标系：**相对排版原点、y 向下**（与 lieui 的绘制坐标系一致；skrifa 的 bbox 是 y-up，
//! 这里已翻转）。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use parley::fontique::Blob;
use parley::layout::PositionedLayoutItem;
use skrifa::metrics::GlyphMetrics;
use skrifa::prelude::{FontRef, LocationRef, Size as SkSize};

use crate::TextLayout;

/// 墨迹盒（相对排版原点、y 向下）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InkBounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl InkBounds {
    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    pub fn is_empty(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }

    /// 垂直中心（相对排版原点）
    pub fn center_y(&self) -> f32 {
        (self.top + self.bottom) * 0.5
    }

    fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }
}

/// 逐字形墨迹盒缓存：`(blob id, collection index, 字号 bits) → glyph id → bbox`
type GlyphBox = (f32, f32, f32, f32);
type FontCache = HashMap<(u64, u32, u32), HashMap<u32, Option<GlyphBox>>>;

fn glyph_box_cache() -> &'static Mutex<FontCache> {
    static CACHE: OnceLock<Mutex<FontCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 单个字形的墨迹盒（相对**基线**、y 向下；字号已缩放）。空字形（空格 / .notdef）返回 `None`。
fn glyph_ink_box(blob: &Blob<u8>, index: u32, glyph_id: u32, font_size: f32) -> Option<GlyphBox> {
    let key = (blob.id(), index, font_size.to_bits());
    let mut cache = glyph_box_cache().lock().ok()?;
    let map = cache.entry(key).or_default();
    if let Some(hit) = map.get(&glyph_id) {
        return *hit;
    }

    let computed = FontRef::from_index(blob.data(), index).ok().and_then(|font| {
        GlyphMetrics::new(&font, SkSize::new(font_size), LocationRef::default())
            .bounds(skrifa::GlyphId::new(glyph_id))
            .map(|b| {
                // skrifa 的 bbox 是 y-up（字体约定）⇒ 翻成 y-down（原点在基线）
                (b.x_min, -b.y_max, b.x_max, -b.y_min)
            })
    });
    map.insert(glyph_id, computed);
    computed
}

/// 从**已排版**的 layout 求墨迹盒（全部行取并集）；没有任何可见字形 ⇒ `None`。
///
/// 坐标与 `layout.width()/height()` 同源（相对排版原点、y 向下），所以可以直接和行盒比较、
/// 也可以配合 `TextEngine::measure_text` 的返回值做光学居中。
pub fn ink_bounds(layout: &TextLayout) -> Option<InkBounds> {
    let mut acc: Option<InkBounds> = None;
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(gr) = item else {
                continue;
            };
            let run = gr.run();
            let font = run.font();
            let size = run.font_size();
            for g in gr.positioned_glyphs() {
                let Some((x0, y0, x1, y1)) = glyph_ink_box(&font.data, font.index, g.id, size) else {
                    continue;
                };
                // parley 的 positioned glyph x/y 已在"排版坐标"（y 向下）：y = 基线位置
                let r = InkBounds {
                    left: g.x + x0,
                    top: g.y + y0,
                    right: g.x + x1,
                    bottom: g.y + y1,
                };
                acc = Some(match acc {
                    None => r,
                    Some(a) => a.union(r),
                });
            }
        }
    }
    acc
}
