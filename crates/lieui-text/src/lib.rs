//! lieui-text —— 文本测度与排版（基于 parley 0.11）
//!
//! 从 `feature/mvp` 的 `src/text/mod.rs` 抽出。相对原版的改动：
//! 1. 不再依赖 `crate::view::paint::TextStyle`，改为本 crate 的 [`TextSpec`]（只含测度/整形字段）；
//! 2. `create_text_layout` / `apply_plain_editor_style` / `create_plain_editor` 增加 `color` 参数
//!    （原实现从 `TextStyle.color` 取，现在由绘制层显式给）；
//! 3. 颜色类型改从 `lieui-geom` 引入；
//! 4. **参考 lievisual 0.2 的文本引擎**（2026-10）：
//!    - **进程级共享字体集合**（`CollectionOptions::shared`）：注册的字体对所有线程（含后启线程）
//!      可见，不再静默回退到系统默认字体；
//!    - **`line_height` 真正生效**：parley 0.11 有 `StyleProperty::LineHeight`（`FontSizeRelative`），
//!      旧注释"parley 无此属性、由上层处理"是错的——现在测度/排版/编辑器三处一致应用；
//!    - **CSS 字体族列表**：`font_family` 支持 `"Segoe UI, sans-serif"` 形式的回退链；
//!    - **字体注册增强**：[`FontSource`]（路径/内存）+ family 名覆盖 + 绑定 generic family
//!      （如把注册字体挂到 `monospace`）。

pub mod ink;
pub mod spec;

use std::borrow::Cow;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use parley::{
    Alignment, AlignmentOptions, FontContext, LayoutContext, editing::PlainEditor,
    style::{FontFamily, FontFamilyName, FontWeight as ParleyFontWeight, LineHeight, StyleProperty},
};

use lieui_geom::Color;

pub use ink::{InkBounds, ink_bounds};
pub use spec::{FontWeight, TextAlign, TextSpec};

/// 文本布局类型（画笔颜色在整形时写入）
pub type TextLayout = parley::Layout<Color>;

/// 单样式文本编辑器类型（用于 Input 等可编辑文本）。
pub type PlainTextEditor = PlainEditor<Color>;

// ─────────────────── 进程级共享字体集合（参考 lievisual） ───────────────────

/// 进程级字体集合（`shared: true` ⇒ 每个线程的 FontContext 克隆共享同一份注册状态）。
fn global_font_collection() -> &'static Mutex<parley::fontique::Collection> {
    static GLOBAL: OnceLock<Mutex<parley::fontique::Collection>> = OnceLock::new();
    GLOBAL.get_or_init(|| {
        Mutex::new(parley::fontique::Collection::new(parley::fontique::CollectionOptions {
            shared: true,
            ..Default::default()
        }))
    })
}

/// 从共享集合构造线程本地的 FontContext。
fn new_font_context() -> FontContext {
    let collection = global_font_collection()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    FontContext {
        collection,
        source_cache: parley::fontique::SourceCache::default(),
    }
}

thread_local! {
    pub static FONT_CONTEXT: RefCell<FontContext> = RefCell::new(new_font_context());
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

/// 解析 CSS `font-family` 列表（如 `"Segoe UI, sans-serif"`）为 parley 字体族；
/// 不是合法列表时退化为单一命名族（与旧行为一致）。
fn font_family_prop(raw: &str) -> FontFamily<'_> {
    let families: Vec<FontFamilyName> = FontFamilyName::parse_css_list(raw)
        .filter_map(Result::ok)
        .collect();
    if families.is_empty() {
        FontFamily::named(raw)
    } else {
        FontFamily::List(families.into())
    }
}

/// 绝对行高 → parley 的字号相对行高（parley 只接受倍率形式）。
fn parley_line_height(lh: f64, font_size: f64) -> LineHeight {
    let factor = (lh / font_size.max(1e-6)) as f32;
    LineHeight::FontSizeRelative(factor)
}

fn apply_text_style(
    builder: &mut parley::RangedBuilder<Color>,
    spec: &TextSpec,
    color: Color,
) {
    builder.push_default(StyleProperty::FontSize(spec.font_size as f32));
    builder.push_default(StyleProperty::Brush(color));
    builder.push_default(StyleProperty::FontFamily(font_family_prop(&spec.font_family)));
    builder.push_default(StyleProperty::FontWeight(parley_weight(&spec.font_weight)));
    if let Some(lh) = spec.line_height {
        builder.push_default(StyleProperty::LineHeight(parley_line_height(
            lh,
            spec.font_size,
        )));
    }
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
    /// 测度：返回 `(宽, 高)`。
    ///
    /// 默认口径是 parley 的**行盒**（advance × 行高）；`spec.optical_align = true` 时
    /// 高度换成**墨迹高度**（见 [`TextSpec::optical_align`]）。
    pub fn measure_text(text: &str, spec: &TextSpec) -> (f64, f64) {
        // 测量结果缓存：布局引擎会对每个文本节点测量 1~2 次，
        // parley 排版是纯 CPU 大头；同样的 (内容, 规格) 直接命中。
        let key = measure_cache_key(text, spec);
        if let Some(hit) = MEASURE_CACHE.with(|c| c.borrow().get(&key).copied()) {
            return hit;
        }
        // 测度不关心颜色，用默认色整形（不影响尺寸）
        let layout = create_text_layout(text, spec, Color::BLACK);
        let w = layout.width() as f64;
        let h = if spec.optical_align {
            ink::ink_bounds(&layout)
                .filter(|b| !b.is_empty())
                .map_or(layout.height() as f64, |b| b.height() as f64)
        } else {
            layout.height() as f64
        };
        let result = (w, h);
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

    /// 文本的墨迹盒（相对排版原点、y 向下）。无可见字形（空串 / 全空白）⇒ `None`。
    ///
    /// 与 [`TextEngine::measure_text`] 同源（同一次排版的坐标系），供**光学对齐**使用：
    /// 绘制时把字形上缘对到节点矩形上缘（`origin.y = rect.y - ink.top`）。
    pub fn ink_bounds(text: &str, spec: &TextSpec) -> Option<InkBounds> {
        if text.is_empty() {
            return None;
        }
        let layout = create_text_layout(text, spec, Color::BLACK);
        ink::ink_bounds(&layout)
    }

    /// 清空测度缓存（主题/字体变更后调用）
    pub fn clear_measure_cache() {
        MEASURE_CACHE.with(|c| c.borrow_mut().clear());
    }
}

/// 测量缓存键：内容 + 影响尺寸的规格字段（颜色/对齐不影响测量结果）
type MeasureKey = (String, String, u64, u16, bool, u64, u64, bool);

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
        spec.optical_align,
    )
}

/// `StyleSet::insert` 要求 `StyleProperty<'static>`：把 CSS 字体族列表复制为 `'static`。
fn static_font_family(raw: &str) -> FontFamily<'static> {
    let families: Vec<FontFamilyName<'static>> = FontFamilyName::parse_css_list(raw)
        .filter_map(Result::ok)
        .map(|f| match f {
            FontFamilyName::Named(c) => FontFamilyName::Named(Cow::Owned(c.into_owned())),
            FontFamilyName::Generic(g) => FontFamilyName::Generic(g),
        })
        .collect();
    if families.is_empty() {
        FontFamily::Single(FontFamilyName::Named(Cow::Owned(raw.to_string())))
    } else {
        FontFamily::List(families.into())
    }
}

/// 将规格与颜色应用到 `PlainEditor` 的默认样式。
pub fn apply_plain_editor_style(editor: &mut PlainTextEditor, spec: &TextSpec, color: Color) {
    let styles = editor.edit_styles();
    styles.insert(StyleProperty::FontSize(spec.font_size as f32));
    styles.insert(StyleProperty::Brush(color));
    // `StyleSet::insert` 要求 `StyleProperty<'static>`，因此把字体名复制为 'static。
    styles.insert(StyleProperty::FontFamily(static_font_family(&spec.font_family)));
    styles.insert(StyleProperty::FontWeight(parley_weight(&spec.font_weight)));
    if let Some(lh) = spec.line_height {
        styles.insert(StyleProperty::LineHeight(parley_line_height(
            lh,
            spec.font_size,
        )));
    }
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

// ─────────────────── 字体注册（参考 lievisual：共享集合 + 覆盖 + generic 绑定） ───────────────────

/// 字体来源（[`register_font_source`] 用）。
#[derive(Debug, Clone)]
pub enum FontSource {
    /// 从文件读取。
    Path(PathBuf),
    /// 内存字节（`include_bytes!` / 网络下载均可）。
    Memory(Vec<u8>),
}

/// 注册自定义字体到**进程级共享集合**（所有线程可见，含后启线程），返回可用 family 名。
///
/// - `family_override`：覆盖字体内嵌的 family 名（同一份字体可以不同名字注册多次）；
/// - `generic_family`：把字体挂到某个 generic family（如 `Monospace`），此后
///   `font_family: "monospace"` 会命中它。
pub fn register_font_source(
    source: FontSource,
    family_override: Option<&str>,
    generic_family: Option<parley::fontique::GenericFamily>,
) -> Result<Vec<String>, String> {
    let blob = match source {
        FontSource::Path(path) => {
            let bytes =
                std::fs::read(&path).map_err(|e| format!("读取字体失败 {path:?}: {e}"))?;
            parley::fontique::Blob::new(std::sync::Arc::new(bytes))
        }
        FontSource::Memory(bytes) => parley::fontique::Blob::new(std::sync::Arc::new(bytes)),
    };
    let override_info = family_override.map(|name| parley::fontique::FontInfoOverride {
        family_name: Some(name),
        ..Default::default()
    });

    let mut collection = global_font_collection()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let registered: Vec<_> = collection.register_fonts(blob, override_info);
    let names: Vec<String> = registered
        .iter()
        .filter_map(|(fid, _)| collection.family_name(*fid).map(str::to_string))
        .collect();
    if let Some(generic) = generic_family {
        let ids = registered.into_iter().map(|(fid, _)| fid);
        collection.append_generic_families(generic, ids);
    }
    drop(collection);
    // 触碰一次线程本地上下文，让 fontique 的懒同步立即生效（而非等下一次查询）
    with_text_contexts(|fc, _| {
        let _ = fc.collection.family_names().count();
    });
    Ok(names)
}

/// 注册自定义字体字节（.ttf / .otf / .woff 等），返回可用的 family 名称列表。
pub fn register_font_bytes(bytes: Vec<u8>) -> Vec<String> {
    register_font_source(FontSource::Memory(bytes), None, None).unwrap_or_default()
}

/// 从字体文件读取并注册，等价于 `register_font_source(FontSource::Path(..), ..)`。
pub fn register_font_file<P: AsRef<std::path::Path>>(path: P) -> Vec<String> {
    match register_font_source(FontSource::Path(path.as_ref().to_path_buf()), None, None) {
        Ok(names) => names,
        Err(e) => {
            eprintln!("[lieui] {e}");
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

    /// 墨迹盒的 characterization：图标字体墨迹**精确居中**于行盒（Δ=0），
    /// 普通字体则偏（拉丁偏上、CJK 略偏下）——这正是"盒子对齐 ≠ 视觉对齐"的量化依据。
    #[test]
    fn ink_bounds_explains_the_optical_offset() {
        // 图标字体（Material Icons：行盒 = 字号见方，墨迹居中）
        let bytes = include_bytes!("../../../src/assets/MaterialIcons-Regular.ttf");
        let names = register_font_bytes(bytes.to_vec());
        let family = names
            .first()
            .cloned()
            .unwrap_or_else(|| "Material Icons".into());
        for size in [16.0f32, 20.0, 24.0, 28.0] {
            let spec = TextSpec::new(f64::from(size))
                .font_family(family.clone())
                .wrap(false);
            let (w, h) = TextEngine::measure_text("\u{e5cd}", &spec);
            let (w, h) = (w as f32, h as f32);
            let ink = TextEngine::ink_bounds("\u{e5cd}", &spec).unwrap();
            assert!((w - size).abs() < 0.5 && (h - size).abs() < 0.5, "行盒见方：{w}×{h}");
            assert!(ink.center_y() - h / 2.0 == 0.0, "图标墨迹中心 = 行盒中心");
        }

        // 普通字体：行盒 ≠ 墨迹中心
        let s13 = TextSpec::new(13.0);
        let (_, box_cjk) = TextEngine::measure_text("不同字号（28/24/20/16）", &s13);
        let box_cjk = box_cjk as f32;
        let ink_cjk = TextEngine::ink_bounds("不同字号（28/24/20/16）", &s13).unwrap();
        assert!(
            ink_cjk.height() < box_cjk,
            "墨迹比行盒矮：{} vs {box_cjk}",
            ink_cjk.height()
        );
        let (_, box_latin) = TextEngine::measure_text("abc", &s13);
        let box_latin = box_latin as f32;
        let ink_latin = TextEngine::ink_bounds("abc", &s13).unwrap();
        assert!(
            ink_latin.center_y() < box_latin / 2.0 - 0.5,
            "拉丁文字墨迹中心明显高于行盒中心：{} vs {}",
            ink_latin.center_y(),
            box_latin / 2.0
        );
    }

    /// `optical_align`：测度高度换成墨迹高度。
    #[test]
    fn optical_align_measures_by_ink() {
        let plain = TextSpec::new(13.0);
        let optical = TextSpec::new(13.0).optical_align(true);
        let text = "不同字号（28/24/20/16），可 .color 变色";
        let (w0, h0) = TextEngine::measure_text(text, &plain);
        let (w1, h1) = TextEngine::measure_text(text, &optical);
        let ink = TextEngine::ink_bounds(text, &plain).unwrap();
        assert_eq!(w0, w1, "宽度口径不变（advance）");
        assert!(h1 < h0, "光学高度小于行盒高度：{h1} < {h0}");
        assert!(
            (h1 - f64::from(ink.height())).abs() < 0.01,
            "光学高度 = 墨迹高度：{h1} vs {}",
            ink.height()
        );
    }

    /// 参考 lievisual：parley 0.11 有 `StyleProperty::LineHeight`（`FontSizeRelative`），
    /// `spec.line_height` 必须真正影响测度（旧实现只是把它放进缓存键，是 no-op）。
    #[test]
    fn line_height_is_applied_to_measurement() {
        let base = TextSpec {
            font_size: 16.0,
            ..Default::default()
        };
        let (_, h0) = TextEngine::measure_text("hello", &base);
        let taller = TextSpec {
            font_size: 16.0,
            line_height: Some(32.0),
            ..Default::default()
        };
        let (_, h1) = TextEngine::measure_text("hello", &taller);
        assert!(h1 > h0, "line_height 应生效：{h0} → {h1}");
        assert!((h1 - 32.0).abs() < 1.0, "单行行高应≈32（2×字号）：{h1}");
    }

    /// `font_family` 支持 CSS 回退链：第一个字体不存在时落到后续 generic family。
    #[test]
    fn font_family_list_is_accepted() {
        let s = TextSpec {
            font_size: 16.0,
            font_family: "NoSuchFontXYZ, sans-serif".to_string(),
            ..Default::default()
        };
        let (w, h) = TextEngine::measure_text("hello", &s);
        assert!(w > 0.0 && h > 0.0, "回退链应能解析出字体：{w}×{h}");
    }

    /// 注册增强：family 名覆盖 + generic family 绑定（共享集合 ⇒ 本线程立即可测）。
    #[test]
    fn register_with_family_override_and_generic_binding() {
        // Material Icons 字体由主 crate 内嵌；这里直接拿仓库里的同一份文件测 API
        let bytes = include_bytes!("../../../src/assets/MaterialIcons-Regular.ttf");
        let names = register_font_source(
            FontSource::Memory(bytes.to_vec()),
            Some("LieTestIcons"),
            Some(parley::fontique::GenericFamily::Fantasy),
        )
        .expect("内存字体注册不应失败");
        assert!(
            names.iter().any(|n| n == "LieTestIcons"),
            "覆盖的 family 名应出现在注册结果里：{names:?}"
        );

        // 覆盖名可直接用于排版（图标字形 1em 见方 ⇒ 宽度≈字号）
        let spec = TextSpec::new(20.0)
            .font_family("LieTestIcons")
            .wrap(false);
        let (w, _) = TextEngine::measure_text("\u{e5cd}", &spec);
        assert!(w > 0.0, "按覆盖名测度图标字形：{w}");
        assert!((w - 20.0).abs() < 2.0, "图标字形 advance≈字号：{w}");

        // generic 绑定：font_family 写 "fantasy" 也应能解析（落到注册的图标字体）
        let s2 = TextSpec::new(14.0).font_family("fantasy");
        let (w2, h2) = TextEngine::measure_text("hello", &s2);
        assert!(w2 > 0.0 && h2 > 0.0, "fantasy 应命中注册字体：{w2}×{h2}");
    }
}
