//! 图标支持：**Material Icons 图标字体 + 名称 → 码点查表**。
//!
//! 图标的本质是"图标字体里的一个字符"，所以整套实现只有两件事：
//! 1. 把字体字节注册进文本引擎（[`ensure_icon_font`]，幂等，首次使用时自动调用）；
//! 2. 图标名 → 码点字符（[`icon_char`]，查内嵌 codepoints 表）。
//!
//! 之后的测度 / 布局 / 绘制 / hover 变色**全部复用文本管线**——
//! `view.icon(name)` 只是"一个设置了图标字体的 `Kind::Text`"，
//! 不引入任何新的绘制原语或布局分支。
//!
//! 字体与 codepoints 表内嵌在 crate 里（`src/assets/`），开箱即用、发布自包含。

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use lieui_text::TextSpec;

/// 图标字体的 family 名（真实值以注册结果为准，用 [`icon_font_family`] 取）
pub const ICON_FONT_FAMILY: &str = "Material Icons";

/// 未知图标名的占位字形（□）
pub const ICON_FALLBACK: char = '□';

static FONT_BYTES: &[u8] = include_bytes!("assets/MaterialIcons-Regular.ttf");
static CODEPOINT_SRC: &str = include_str!("assets/MaterialIcons-Regular.codepoints");

/// 图标名 → Unicode 码点（首次访问时解析内嵌的 codepoints 表）。
pub fn codepoints() -> &'static HashMap<&'static str, u32> {
    static MAP: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();
    MAP.get_or_init(|| {
        CODEPOINT_SRC
            .lines()
            .filter_map(|line| {
                let (name, hex) = line.split_once(' ')?;
                u32::from_str_radix(hex.trim(), 16).ok().map(|cp| (name, cp))
            })
            .collect()
    })
}

/// 按名称取图标字符（未知名称返回 [`ICON_FALLBACK`]）。
pub fn icon_char(name: &str) -> char {
    codepoints()
        .get(name)
        .and_then(|cp| char::from_u32(*cp))
        .unwrap_or(ICON_FALLBACK)
}

thread_local! {
    /// 注册后的真实 family 名（`None` = 尚未注册）。
    /// 字体上下文是 thread_local 的，注册也按线程走。
    static ICON_FAMILY: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// 确保图标字体已注册进文本引擎（幂等）。`view::icon*` 会自动调用。
pub fn ensure_icon_font() {
    ICON_FAMILY.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            return;
        }
        let names = lieui_text::register_font_bytes(FONT_BYTES.to_vec());
        *slot = Some(names.into_iter().find(|n| n == ICON_FONT_FAMILY).unwrap_or_else(|| {
            eprintln!("[lieui] 图标字体注册结果不含 {ICON_FONT_FAMILY:?}，图标将显示为占位字形");
            ICON_FONT_FAMILY.to_string()
        }));
    });
}

/// 图标字体 family 名（首次调用会自动注册字体）。
pub fn icon_font_family() -> String {
    ensure_icon_font();
    ICON_FAMILY.with(|f| f.borrow().clone().unwrap_or_else(|| ICON_FONT_FAMILY.to_string()))
}

/// 图标文本的排版规格：图标字体 + 不换行。
pub fn icon_spec(font_size: f64) -> TextSpec {
    TextSpec::new(font_size).font_family(icon_font_family()).wrap(false)
}

#[cfg(test)]
#[path = "icon_tests.rs"]
mod tests;
