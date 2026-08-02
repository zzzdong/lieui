//! 文本引擎 — 基于 parley 0.11.0 排版

use std::borrow::Cow;
use std::cell::RefCell;

use parley::{
    Alignment, AlignmentOptions, FontContext, LayoutContext,
    editing::PlainEditor,
    style::{FontFamily, FontFamilyName, FontWeight as ParleyFontWeight, StyleProperty},
};

use crate::geometry::Color;
use crate::view::paint::{FontWeight, TextAlign, TextStyle};

/// 文本布局类型
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

fn apply_text_style(builder: &mut parley::RangedBuilder<Color>, style: &TextStyle) {
    builder.push_default(StyleProperty::FontSize(style.font_size as f32));
    builder.push_default(StyleProperty::Brush(style.color));
    builder.push_default(StyleProperty::FontFamily(FontFamily::named(
        &style.font_family,
    )));

    let pw = match &style.font_weight {
        FontWeight::Normal => ParleyFontWeight::NORMAL,
        FontWeight::Medium => ParleyFontWeight::MEDIUM,
        FontWeight::Bold => ParleyFontWeight::BOLD,
        FontWeight::Weight(w) => ParleyFontWeight::new(*w as f32),
    };
    builder.push_default(StyleProperty::FontWeight(pw));

    // line_height 暂由上层通过额外行间距实现，parley 0.11.0 无直接 StyleProperty。
}

fn map_text_align(a: TextAlign) -> Alignment {
    match a {
        TextAlign::Start => Alignment::Start,
        TextAlign::Center => Alignment::Center,
        TextAlign::End => Alignment::End,
        TextAlign::Justify => Alignment::Justify,
    }
}

/// 创建文本布局
pub fn create_text_layout(text: &str, style: &TextStyle) -> TextLayout {
    with_text_contexts(|fc, lc| {
        let mut builder = lc.ranged_builder(fc, text, 1.0, true);
        apply_text_style(&mut builder, style);
        let mut layout = builder.build(text);
        layout.break_all_lines(style.max_width.map(|w| w as f32));
        layout.align(
            map_text_align(style.text_align),
            AlignmentOptions::default(),
        );
        layout
    })
}

/// 文本引擎
pub struct TextEngine;
impl TextEngine {
    pub fn measure_text(text: &str, style: &TextStyle) -> (f64, f64) {
        // 测量结果缓存：布局引擎每帧会对每个文本节点测量 1~2 次，
        // parley 排版是纯 CPU 大头；同样的 (内容, 样式, 宽度约束) 直接命中。
        let key = measure_cache_key(text, style);
        if let Some(hit) = MEASURE_CACHE.with(|c| c.borrow().get(&key).copied()) {
            return hit;
        }
        let layout = create_text_layout(text, style);
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
}

/// 测量缓存键：内容 + 影响尺寸的样式字段（颜色/对齐不影响测量结果）。
type MeasureKey = (String, String, u64, u16, bool, u64, u64);

thread_local! {
    static MEASURE_CACHE: RefCell<std::collections::HashMap<MeasureKey, (f64, f64)>> =
        RefCell::new(std::collections::HashMap::new());
}

fn measure_cache_key(text: &str, style: &TextStyle) -> MeasureKey {
    let weight = match &style.font_weight {
        FontWeight::Normal => 400u16,
        FontWeight::Medium => 500,
        FontWeight::Bold => 700,
        FontWeight::Weight(w) => *w,
    };
    (
        text.to_owned(),
        style.font_family.clone(),
        style.font_size.to_bits(),
        weight,
        style.wrap,
        style.max_width.unwrap_or(f64::INFINITY).to_bits(),
        style.line_height.unwrap_or(f64::NAN).to_bits(),
    )
}

/// 将 `TextStyle` 应用到 `PlainEditor` 的默认样式。
pub fn apply_plain_editor_style(editor: &mut PlainTextEditor, style: &TextStyle) {
    let styles = editor.edit_styles();
    styles.insert(StyleProperty::FontSize(style.font_size as f32));
    styles.insert(StyleProperty::Brush(style.color));
    // `StyleSet::insert` 要求 `StyleProperty<'static>`，因此把字体名复制为 'static。
    styles.insert(StyleProperty::FontFamily(FontFamily::Single(
        FontFamilyName::Named(Cow::Owned(style.font_family.clone())),
    )));
    let pw = match &style.font_weight {
        FontWeight::Normal => ParleyFontWeight::NORMAL,
        FontWeight::Medium => ParleyFontWeight::MEDIUM,
        FontWeight::Bold => ParleyFontWeight::BOLD,
        FontWeight::Weight(w) => ParleyFontWeight::new(*w as f32),
    };
    styles.insert(StyleProperty::FontWeight(pw));
}

/// 用给定样式创建并配置 `PlainEditor`。
pub fn create_plain_editor(style: &TextStyle) -> PlainTextEditor {
    let mut editor = PlainTextEditor::new(style.font_size as f32);
    apply_plain_editor_style(&mut editor, style);
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
    style: &TextStyle,
    width: Option<f32>,
    cursor_width: f32,
) -> Option<parley::BoundingBox> {
    with_text_contexts(|font_cx, layout_cx| {
        editor.set_width(width);
        editor.set_alignment(map_text_align(style.text_align));
        editor.refresh_layout(font_cx, layout_cx);
        editor.cursor_geometry(cursor_width)
    })
}

/// 配置编辑器宽度、对齐方式并刷新布局，返回布局宽高。
pub fn editor_layout_size(
    editor: &mut PlainTextEditor,
    style: &TextStyle,
    width: Option<f32>,
) -> (f32, f32) {
    with_text_contexts(|font_cx, layout_cx| {
        editor.set_width(width);
        editor.set_alignment(map_text_align(style.text_align));
        editor.refresh_layout(font_cx, layout_cx);
        let layout = editor.layout(font_cx, layout_cx);
        (layout.width(), layout.height())
    })
}

/// 注册自定义字体字节（.ttf / .otf / .woff 等），返回可用在 `TextStyle::font_family`
/// 中的 family 名称列表。
///
/// 内部走 parley 的 `FontContext.collection.register_fonts`，与系统字体共用同一套
/// 解析链路；多次注册同名族不会破坏解析，只是追加数据源。
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
/// 失败（文件不存在/读取错误）时打印告警并返回空列表。
pub fn register_font_file<P: AsRef<std::path::Path>>(path: P) -> Vec<String> {
    match std::fs::read(path.as_ref()) {
        Ok(bytes) => register_font_bytes(bytes),
        Err(e) => {
            eprintln!("[lieui] 加载字体失败 {:?}: {e}", path.as_ref());
            Vec::new()
        }
    }
}
