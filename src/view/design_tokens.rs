//! PatternFly v6 semantic design tokens
//!
//! 取值参考 https://www.patternfly.org/foundations-and-styles/design-tokens/all-design-tokens/
//! 统一用命名常量，避免 widget 中硬编码色值，便于后续主题切换。

/// 基础色阶 —— PatternFly global palette
pub mod palette {
    use crate::geometry::Color;

    pub const BLUE_400: Color = Color::new(0x00, 0x66, 0xcc); // #0066cc
    pub const BLUE_500: Color = Color::new(0x00, 0x4d, 0x99); // #004d99
    pub const BLUE_600: Color = Color::new(0x00, 0x3d, 0x7a); // #003d7a

    pub const WHITE: Color = Color::new(0xff, 0xff, 0xff); // #ffffff
    pub const BLACK_900: Color = Color::new(0x15, 0x15, 0x15); // #151515

    pub const GRAY_10: Color = Color::new(0xf5, 0xf5, 0xf5); // #f5f5f5
    pub const GRAY_20: Color = Color::new(0xe0, 0xe0, 0xe0); // #e0e0e0
    pub const GRAY_40: Color = Color::new(0xb8, 0xbb, 0xbe); // #b8bbbe
    pub const GRAY_60: Color = Color::new(0x6a, 0x6e, 0x73); // #6a6e73
}

/// 语义化 token —— 直接映射 PatternFly semantic tokens
pub mod semantic {
    use crate::geometry::Color;
    use crate::view::design_tokens::palette;

    /// --pf-t--global--text--color--brand--default
    pub const TEXT_BRAND_DEFAULT: Color = palette::BLUE_400;
    /// --pf-t--global--text--color--brand--hover
    pub const TEXT_BRAND_HOVER: Color = palette::BLUE_500;
    /// --pf-t--global--text--color--regular--default
    pub const TEXT_REGULAR_DEFAULT: Color = palette::BLACK_900;
    /// --pf-t--global--text--color--subtle--default
    pub const TEXT_SUBTLE_DEFAULT: Color = palette::GRAY_60;

    /// --pf-t--global--icon--color--on-brand--default
    pub const ICON_ON_BRAND_DEFAULT: Color = palette::WHITE;

    /// --pf-t--global--background--color--primary--default
    pub const BACKGROUND_PRIMARY_DEFAULT: Color = palette::WHITE;
    /// --pf-t--global--background--color--secondary--default
    pub const BACKGROUND_SECONDARY_DEFAULT: Color = palette::GRAY_10;
    /// --pf-t--global--background--color--brand--default
    pub const BACKGROUND_BRAND_DEFAULT: Color = palette::BLUE_400;
    /// --pf-t--global--background--color--brand--hover
    pub const BACKGROUND_BRAND_HOVER: Color = palette::BLUE_500;
    /// --pf-t--global--background--color--brand--clicked
    pub const BACKGROUND_BRAND_CLICKED: Color = palette::BLUE_600;

    /// --pf-t--global--border--color--default
    pub const BORDER_DEFAULT: Color = palette::GRAY_20;
    /// --pf-t--global--border--color--strong
    pub const BORDER_STRONG: Color = palette::GRAY_40;

    /// --pf-t--global--border--radius--small
    pub const BORDER_RADIUS_SMALL: f32 = 3.0;

    /// --pf-t--global--spacer--xs (0.25rem)
    pub const SPACER_XS: f32 = 4.0;
    /// --pf-t--global--spacer--sm (0.5rem)
    pub const SPACER_SM: f32 = 8.0;
    /// --pf-t--global--spacer--md (1rem)
    pub const SPACER_MD: f32 = 16.0;
}
