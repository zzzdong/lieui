//! lieui-text —— 文本测度与排版（基于 parley 0.11）
//!
//! 从 `feature/mvp` 的 `src/text/mod.rs` 抽出。相对原版的改动：
//! 1. 不再依赖 `crate::view::paint::TextStyle`，改为本 crate 的 [`TextSpec`]（只含测度/整形字段）；
//! 2. `create_text_layout` / `apply_plain_editor_style` / `create_plain_editor` 增加 `color` 参数
//!    （原实现从 `TextStyle.color` 取，现在由绘制层显式给）；
//! 3. 颜色类型改从 `lieui-geom` 引入。
//!
//! 遗留（M2 处理，见 docs/operation-log.md）：字体/排版上下文与测度缓存仍是 thread_local，
//! v3 目标是收敛成显式的 `TextService` 对象。

pub mod spec;

use std::borrow::Cow;
use std::cell::RefCell;

use parley::{
    Alignment, AlignmentOptions, FontContext, LayoutContext, editing::PlainEditor,
    style::{FontFamily, FontFamilyName, FontWeight as ParleyFontWeight, StyleProperty},
};

use lieui_geom::Color;

pub use spec::{FontWeight, TextAlign, TextSpec};

/// 文本布局类型（画笔颜色在整形时写入）
pub type TextLayout = parley::Layout<Color>;

/// 单样式文本编辑器类型（用于 Input 等可编辑文本）。
pub type PlainTextEditor = PlainEditor<Color>;

thread_local! {
    pub static FONT_CONTEXT: RefCell<FontContext> = RefCell::new(FontContext::default());
    pub static LAYOUT_CONTEXT: RefCell<LayoutContext<Color>> = RefCell::new(LayoutContext::new());
}

/// 同时访问字体和布局上下文
pub fn with_text_contexts<R, F: FnOnce(&mut FontContext, &mut LayoutContext<Color>) -> R>(
    f: F,
) -> R {
    FONT_CONTEXT.with(|fc| LAYOUT_CONTEXT.with(|lc| f(&mut fc.borrow_mut(), &mut lc.borrow_mut())))
}

fn parley_weight(w: &FontWeight) -> ParleyFontWeight {
    match w {
        FontWeight::Normal => ParleyFontWeight::NORMAL,
        FontWeight::Medium => ParleyFontWeight::MEDIUM,
        FontWeight::Bold => ParleyFontWeight::BOLD,
        FontWeight::Weight(w) => ParleyFontWeight::new(*w as f32),
    }
}

fn apply_text_style(
    builder: &mut parley::RangedBuilder<Color>,
    spec: &TextSpec,
    color: Color,
) {
    builder.push_default(StyleProperty::FontSize(spec.font_size as f32));
    builder.push_default(StyleProperty::Brush(color));
    builder.push_default(StyleProperty::FontFamily(FontFamily::named(&spec.font_family)));
    builder.push_default(StyleProperty::FontWeight(parley_weight(&spec.font_weight)));
}

fn map_text_align(a: TextAlign) -> Alignment {
    match a {
        TextAlign::Start => Alignment::Start,
        TextAlign::Center => Alignment::Center,
        TextAlign::End => Alignment::End,
        TextAlign::Justify => Alignment::Justify,
    }
}

/// 创建文本布局（整形 + 换行 + 对齐一次完成）
pub fn create_text_layout(text: &str, spec: &TextSpec, color: Color) -> TextLayout {
    with_text_contexts(|fc, lc| {
        let mut builder = lc.ranged_builder(fc, text, 1.0, true);
        apply_text_style(&mut builder, spec, color);
        let mut layout = builder.build(text);
        layout.break_all_lines(spec.max_width.map(|w| w as f32));
        layout.align(map_text_align(spec.text_align), AlignmentOptions::default());
        layout
    })
}

/// 文本引擎（测度入口）
pub struct TextEngine;

impl TextEngine {
    pub fn measure_text(text: &str, spec: &TextSpec) -> (f64, f64) {
        // 测量结果缓存：布局引擎会对每个文本节点测量 1~2 次，
        // parley 排版是纯 CPU 大头；同样的 (内容, 规格) 直接命中。
        let key = measure_cache_key(text, spec);
        if let Some(hit) = MEASURE_CACHE.with(|c| c.borrow().get(&key).copied()) {
            return hit;
        }
        // 测度不关心颜色，用默认色整形（不影响尺寸）
        let layout = create_text_layout(text, spec, Color::BLACK);
        let result = (layout.width() as f64, layout.height() as f64);
        MEASURE_CACHE.with(|c| {
            let mut c = c.borrow_mut();
            // 简单防膨胀：超限后整体清空（正常 UI 远达不到上限）。
            if c.len() >= 4096 {
                c.clear();
            }
            c.insert(key, result);
        });
        result
    }

    /// 清空测度缓存（主题/字体变更后调用）
    pub fn clear_measure_cache() {
        MEASURE_CACHE.with(|c| c.borrow_mut().clear());
    }
}

/// 测量缓存键：内容 + 影响尺寸的规格字段（颜色/对齐不影响测量结果）
type MeasureKey = (String, String, u64, u16, bool, u64, u64);

thread_local! {
    static MEASURE_CACHE: RefCell<std::collections::HashMap<MeasureKey, (f64, f64)>> =
        RefCell::new(std::collections::HashMap::new());
}

fn measure_cache_key(text: &str, spec: &TextSpec) -> MeasureKey {
    let weight = match &spec.font_weight {
        FontWeight::Normal => 400u16,
        FontWeight::Medium => 500,
        FontWeight::Bold => 700,
        FontWeight::Weight(w) => *w,
    };
    (
        text.to_owned(),
        spec.font_family.clone(),
        spec.font_size.to_bits(),
        weight,
        spec.wrap,
        spec.max_width.unwrap_or(f64::INFINITY).to_bits(),
        spec.line_height.unwrap_or(f64::NAN).to_bits(),
    )
}

/// 将规格与颜色应用到 `PlainEditor` 的默认样式。
pub fn apply_plain_editor_style(editor: &mut PlainTextEditor, spec: &TextSpec, color: Color) {
    let styles = editor.edit_styles();
    styles.insert(StyleProperty::FontSize(spec.font_size as f32));
    styles.insert(StyleProperty::Brush(color));
    // `StyleSet::insert` 要求 `StyleProperty<'static>`，因此把字体名复制为 'static。
    styles.insert(StyleProperty::FontFamily(FontFamily::Single(
        FontFamilyName::Named(Cow::Owned(spec.font_family.clone())),
    )));
    styles.insert(StyleProperty::FontWeight(parley_weight(&spec.font_weight)));
}

/// 用给定规格创建并配置 `PlainEditor`。
pub fn create_plain_editor(spec: &TextSpec, color: Color) -> PlainTextEditor {
    let mut editor = PlainTextEditor::new(spec.font_size as f32);
    apply_plain_editor_style(&mut editor, spec, color);
    editor
}

/// 将任意字节偏移限制到最近的合法 UTF-8 字符边界（向起始位置回退）。
pub fn align_to_utf8_boundary(text: &str, mut idx: usize) -> usize {
    let len = text.len();
    if idx > len {
        idx = len;
    }
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// 配置编辑器宽度、对齐方式并刷新布局，返回光标几何（坐标相对于文本原点）。
pub fn editor_cursor_geometry(
    editor: &mut PlainTextEditor,
    spec: &TextSpec,
    width: Option<f32>,
    cursor_width: f32,
) -> Option<parley::BoundingBox> {
    with_text_contexts(|font_cx, layout_cx| {
        editor.set_width(width);
        editor.set_alignment(map_text_align(spec.text_align));
        editor.refresh_layout(font_cx, layout_cx);
        editor.cursor_geometry(cursor_width)
    })
}

/// 配置编辑器宽度、对齐方式并刷新布局，返回布局宽高。
pub fn editor_layout_size(
    editor: &mut PlainTextEditor,
    spec: &TextSpec,
    width: Option<f32>,
) -> (f32, f32) {
    with_text_contexts(|font_cx, layout_cx| {
        editor.set_width(width);
        editor.set_alignment(map_text_align(spec.text_align));
        editor.refresh_layout(font_cx, layout_cx);
        let layout = editor.layout(font_cx, layout_cx);
        (layout.width(), layout.height())
    })
}

/// 注册自定义字体字节（.ttf / .otf / .woff 等），返回可用的 family 名称列表。
pub fn register_font_bytes(bytes: Vec<u8>) -> Vec<String> {
    FONT_CONTEXT.with(|fc| {
        let mut fc = fc.borrow_mut();
        let blob = parley::fontique::Blob::from(bytes);
        let registered = fc.collection.register_fonts(blob, None);
        registered
            .into_iter()
            .filter_map(|(fid, _)| fc.collection.family_name(fid).map(|s| s.to_string()))
            .collect()
    })
}

/// 从字体文件读取并注册，等价于 `register_font_bytes(std::fs::read(path)?)`。
pub fn register_font_file<P: AsRef<std::path::Path>>(path: P) -> Vec<String> {
    match std::fs::read(path.as_ref()) {
        Ok(bytes) => register_font_bytes(bytes),
        Err(e) => {
            eprintln!("[lieui] 加载字体失败 {:?}: {e}", path.as_ref());
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(max_width: f64) -> TextSpec {
        TextSpec {
            font_size: 16.0,
            max_width: Some(max_width),
            wrap: true,
            ..Default::default()
        }
    }

    #[test]
    fn measures_and_lays_out_latin_text() {
        let s = TextSpec {
            font_size: 16.0,
            ..Default::default()
        };
        let (w, h) = TextEngine::measure_text("hello world", &s);
        assert!(w > 0.0 && h > 0.0, "测得 {w}×{h}");
    }

    #[test]
    fn wraps_long_latin_text() {
        let s = spec(80.0);
        let long = "the quick brown fox jumps over the lazy dog again and again";
        let layout = create_text_layout(long, &s, Color::BLACK);
        let one_line = create_text_layout("w", &s, Color::BLACK).height();
        assert!(
            layout.height() > one_line * 1.5,
            "应在 80px 宽度内换行：{} 行",
            layout.height() / one_line.max(1.0)
        );
    }

    /// 中文：书写系统没有空格，但 **UAX#14 允许在汉字之间断** ⇒ 照常换行（不依赖词典/分词模型）。
    #[test]
    fn wraps_long_chinese_text() {
        let s = spec(96.0);
        let text = "这是一段很长的中文文本，用来验证在没有空格的书写系统里也能正确断行。";
        let layout = create_text_layout(text, &s, Color::BLACK);
        let one_line = create_text_layout("中", &s, Color::BLACK).height();
        let lines = layout.height() / one_line.max(1.0);
        assert!(lines >= 2.0, "中文应在 96px 内断成多行，实际 {lines} 行");
    }

    /// **已知限制（characterization test）**：泰语既没有空格、也没有 UAX#14 断点，
    /// 需要"复杂脚本分词"（`icu_segmenter` 的 LSTM 模型）才能找到断点。
    ///
    /// 实测（2026-10-01）：即便把 `icu_segmenter` 的 `auto`/`lstm` 特性打开
    /// （见 `cargo tree -e features -i icu_segmenter`），parley 的**换行**也不消费该分词器
    /// （它只给 `char`/`word` 分割用），因此泰语不会自动换行 —— 那条额外依赖没有收益，已撤销。
    ///
    /// 变通：在文本里插入零宽空格（`\u{200B}`）或放宽容器宽度。
    /// 这条测试是为了"将来 parley 支持了就会立刻失败，提醒我们删掉这个限制说明"。
    #[test]
    fn thai_text_does_not_wrap_without_explicit_break_opportunities() {
        let s = spec(96.0);
        let text = "ฉันรักการเขียนโปรแกรมและการออกแบบส่วนติดต่อผู้ใช้";
        let layout = create_text_layout(text, &s, Color::BLACK);
        let one_line = create_text_layout("ก", &s, Color::BLACK).height();
        let lines = layout.height() / one_line.max(1.0);
        assert_eq!(lines, 1.0, "当前预期为不换行；若变了说明 parley 已支持复杂脚本断行");

        // 显式插入零宽空格后可以换行（推荐的变通做法）
        let with_zwsp = text.replace("การ", "การ\u{200B}");
        let layout2 = create_text_layout(&with_zwsp, &s, Color::BLACK);
        let lines2 = layout2.height() / one_line.max(1.0);
        assert!(lines2 >= 2.0, "插入零宽空格后应能断行，实际 {lines2} 行");
    }

    #[test]
    fn utf8_boundary_helper() {
        let s = "中文abc";
        assert_eq!(align_to_utf8_boundary(s, 1), 0, "1 落在汉字中间 ⇒ 回退");
        assert_eq!(align_to_utf8_boundary(s, 3), 3);
        assert_eq!(align_to_utf8_boundary(s, 999), s.len());
    }
}
