//! 属性默认值表
//!
//! 解析链最后一环：`ANIM → LOCAL → STYLE → 沿树向上 → 主题变量 → 控件类型默认值`。
//! M1 无主题（M4 接入 `lieui-theme`），因此这里就是兜底的常量表。

use super::keys as K;
use super::value::{Color, Dimension, PropValue, SharedString};

/// 可继承属性的槽位列表（用于继承边界判定）
pub const INHERITABLE_SLOTS: &[u16] = &[
    K::FONT_SIZE.slot(),
    K::FONT_FAMILY.slot(),
    K::FONT_WEIGHT.slot(),
    K::LINE_HEIGHT.slot(),
    K::ITALIC.slot(),
    K::FG.slot(),
];

/// 槽位默认值。未列出的槽位返回 `PropValue::None`。
pub fn default_value(slot: u16) -> PropValue {
    match slot {
        // 盒模型
        s if s == K::WIDTH.slot() => PropValue::Dim(Dimension::Auto),
        s if s == K::HEIGHT.slot() => PropValue::Dim(Dimension::Auto),
        s if s == K::MIN_WIDTH.slot()
            || s == K::MIN_HEIGHT.slot()
            || s == K::MAX_WIDTH.slot()
            || s == K::MAX_HEIGHT.slot() =>
        {
            PropValue::None
        }
        s if (K::PADDING_L.slot()..=K::BORDER_B.slot()).contains(&s) => PropValue::F32(0.0),

        // Flex
        s if s == K::FLEX_DIRECTION.slot() => PropValue::U32(K::flex_direction::COLUMN),
        s if s == K::FLEX_WRAP.slot() => PropValue::U32(K::flex_wrap::NO_WRAP),
        s if s == K::JUSTIFY_CONTENT.slot() => PropValue::U32(K::align::START),
        s if s == K::ALIGN_ITEMS.slot() => PropValue::U32(K::align::STRETCH),
        s if s == K::ALIGN_SELF.slot() => PropValue::U32(K::align::AUTO),
        s if s == K::ALIGN_CONTENT.slot() => PropValue::U32(K::align::START),
        s if s == K::FLEX_GROW.slot() => PropValue::F32(0.0),
        // ★ CSS 标准默认 1；Taitank 原默认 0 会让 Row 子节点溢出
        s if s == K::FLEX_SHRINK.slot() => PropValue::F32(1.0),
        s if s == K::GAP.slot() || s == K::LINE_GAP.slot() => PropValue::F32(0.0),
        s if s == K::POSITION_TYPE.slot() => PropValue::U32(K::position_type::RELATIVE),
        s if s == K::DISPLAY.slot() => PropValue::U32(K::display::FLEX),

        // 滚动
        s if s == K::OVERFLOW_SCROLL.slot() => PropValue::Bool(false),
        s if s == K::SCROLL_X.slot() || s == K::SCROLL_Y.slot() => PropValue::F32(0.0),

        // 文本
        s if s == K::TEXT.slot() => PropValue::Str(SharedString::default()),
        s if s == K::FONT_SIZE.slot() => PropValue::F32(16.0),
        s if s == K::FONT_FAMILY.slot() => PropValue::Str(SharedString::default()),
        s if s == K::FONT_WEIGHT.slot() => PropValue::U32(400),
        // 0 = 用字体度量（等效 CSS line-height: normal）
        s if s == K::LINE_HEIGHT.slot() => PropValue::F32(0.0),
        s if s == K::TEXT_ALIGN.slot() => PropValue::U32(K::text_align::START),
        s if s == K::TEXT_WRAP.slot() => PropValue::Bool(true),
        s if s == K::ITALIC.slot() => PropValue::Bool(false),

        // 外观
        s if s == K::FG.slot() => PropValue::Color(Color::BLACK),
        s if s == K::BG.slot() => PropValue::Color(Color::TRANSPARENT),
        s if s == K::OPACITY.slot() => PropValue::F32(1.0),
        s if s == K::RADIUS.slot() => PropValue::F32(0.0),

        // 状态
        s if s == K::VISIBLE.slot() => PropValue::Bool(true),
        s if s == K::DISABLED.slot() => PropValue::Bool(false),

        _ => PropValue::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        assert_eq!(default_value(K::FLEX_SHRINK.slot()), PropValue::F32(1.0));
        assert_eq!(default_value(K::FONT_SIZE.slot()), PropValue::F32(16.0));
        assert_eq!(
            default_value(K::WIDTH.slot()),
            PropValue::Dim(Dimension::Auto)
        );
        assert!(default_value(K::CONTENT_WIDTH.slot()).is_none());
    }
}
