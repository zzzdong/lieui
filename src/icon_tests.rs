use super::*;
use lieui_text::TextEngine;

#[test]
fn known_names_resolve_to_their_codepoints() {
    assert_eq!(icon_char("close"), '\u{e5cd}');
    assert_eq!(icon_char("settings"), '\u{e8b8}');
    assert_eq!(icon_char("add"), '\u{e145}');
}

#[test]
fn unknown_names_fall_back_to_a_placeholder() {
    assert_eq!(icon_char("no-such-icon-xyz"), ICON_FALLBACK);
    assert_eq!(icon_char(""), ICON_FALLBACK);
}

#[test]
fn codepoint_table_covers_a_useful_subset() {
    for name in ["home", "search", "menu", "delete", "favorite", "star", "arrow_back"] {
        assert!(codepoints().contains_key(name), "{name} 不在表里");
    }
}

#[test]
fn icon_font_registers_and_measures() {
    ensure_icon_font();
    assert_eq!(icon_font_family(), ICON_FONT_FAMILY);
    // 图标字形能测出尺寸（字体真的进了引擎，而不是回退成 tofu）
    let spec = icon_spec(20.0);
    let (w, h) = TextEngine::measure_text(&icon_char("close").to_string(), &spec);
    assert!(w > 0.0 && h > 0.0, "close 测得 {w}×{h}");
    assert!(!spec.wrap);
    // 对齐友好的关键性质：Material Icons 行高系数 = 1.0 ⇒ 行盒是**字号见方的正方形**，
    // 字形墨迹居中于 em。于是 align_items(Center) 按行盒居中 ≈ 按视觉中心居中，
    // 不同字号的图标混排天然对齐（gallery 图标段依赖这一点）。
    assert!(
        (w - 20.0).abs() < 0.5 && (h - 20.0).abs() < 0.5,
        "行盒应≈字号见方：{w}×{h}"
    );
}
