//! Theme — 主题系统
//!
//! 以 PatternFly v6 设计令牌为基础。所有语义化 token 封装在 [`Theme`] 结构体中，
//! 通过 [`current`] 读取当前生效主题、[`set_theme`] 切换主题（会请求整树重建）。
//!
//! 框架在每次重建时由 app 的 `builder` 闭包重新构造组件树，因此 widget 在 `build()`
//! 时从当前主题解析出色值即可——切换主题后下一次重建自然生效，无需改动渲染层。
//!
//! 进阶用法：可用 [`palette`] 中的基础色阶自行构造自定义 [`Theme`]。

use std::cell::RefCell;

use crate::geometry::Color;

/// 基础色阶 —— PatternFly global palette
///
/// 供自定义主题时复用，避免在各处硬编码色值。
pub mod palette {
    use crate::geometry::Color;

    // 品牌蓝（不同档位用于 light / dark）
    pub const BLUE_300: Color = Color::new(0x73, 0xbc, 0xf2); // #73bcf2
    pub const BLUE_400: Color = Color::new(0x00, 0x66, 0xcc); // #0066cc
    pub const BLUE_500: Color = Color::new(0x00, 0x4d, 0x99); // #004d99
    pub const BLUE_600: Color = Color::new(0x00, 0x3d, 0x7a); // #003d7a

    pub const WHITE: Color = Color::new(0xff, 0xff, 0xff);
    pub const BLACK_900: Color = Color::new(0x15, 0x15, 0x15); // #151515
    pub const BLACK_1000: Color = Color::new(0x00, 0x00, 0x00);

    // 中性灰阶（light）
    pub const GRAY_10: Color = Color::new(0xf5, 0xf5, 0xf5); // #f5f5f5
    pub const GRAY_20: Color = Color::new(0xe0, 0xe0, 0xe0); // #e0e0e0
    pub const GRAY_40: Color = Color::new(0xb8, 0xbb, 0xbe); // #b8bbbe
    pub const GRAY_60: Color = Color::new(0x6a, 0x6e, 0x73); // #6a6e73

    // 中性灰阶（dark）
    pub const GRAY_70: Color = Color::new(0x3c, 0x3f, 0x44); // #3c3f44
    pub const GRAY_80: Color = Color::new(0x28, 0x2d, 0x33); // #282d33
    pub const GRAY_90: Color = Color::new(0x1b, 0x1d, 0x23); // #1b1d23
}

/// 文本颜色 token
#[derive(Debug, Clone, Copy)]
pub struct TextTokens {
    /// 品牌文本（默认）
    pub brand_default: Color,
    /// 品牌文本（hover）
    pub brand_hover: Color,
    /// 常规文本（默认）
    pub regular_default: Color,
    /// 次要文本
    pub subtle_default: Color,
    /// 品牌底色之上的文本（反白）
    pub on_brand_default: Color,
}

/// 背景色 token
#[derive(Debug, Clone, Copy)]
pub struct BackgroundTokens {
    /// 主背景（页面/卡片）
    pub primary_default: Color,
    /// 次背景（分区/列表）
    pub secondary_default: Color,
    /// 品牌背景（默认）
    pub brand_default: Color,
    /// 品牌背景（hover）
    pub brand_hover: Color,
    /// 品牌背景（按下）
    pub brand_clicked: Color,
}

/// 边框色 token
#[derive(Debug, Clone, Copy)]
pub struct BorderTokens {
    /// 默认边框
    pub default: Color,
    /// 强调边框
    pub strong: Color,
}

/// 圆角 token
#[derive(Debug, Clone, Copy)]
pub struct RadiusTokens {
    /// 小圆角
    pub small: f32,
    /// 中圆角
    pub medium: f32,
}

/// 间距 token
#[derive(Debug, Clone, Copy)]
pub struct SpacerTokens {
    /// 极小间距 (0.25rem)
    pub xs: f32,
    /// 小间距 (0.5rem)
    pub sm: f32,
    /// 中间距 (1rem)
    pub md: f32,
}

/// 一个完整主题：所有语义化设计令牌的集合
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub text: TextTokens,
    pub background: BackgroundTokens,
    pub border: BorderTokens,
    pub radius: RadiusTokens,
    pub spacer: SpacerTokens,
}

impl Theme {
    /// PatternFly v6 浅色主题
    pub fn light() -> Self {
        Self {
            text: TextTokens {
                brand_default: palette::BLUE_400,
                brand_hover: palette::BLUE_500,
                regular_default: palette::BLACK_900,
                subtle_default: palette::GRAY_60,
                on_brand_default: palette::WHITE,
            },
            background: BackgroundTokens {
                primary_default: palette::WHITE,
                secondary_default: palette::GRAY_10,
                brand_default: palette::BLUE_400,
                brand_hover: palette::BLUE_500,
                brand_clicked: palette::BLUE_600,
            },
            border: BorderTokens {
                default: palette::GRAY_20,
                strong: palette::GRAY_40,
            },
            radius: RadiusTokens {
                small: 3.0,
                medium: 6.0,
            },
            spacer: SpacerTokens {
                xs: 4.0,
                sm: 8.0,
                md: 16.0,
            },
        }
    }

    /// 深色主题
    pub fn dark() -> Self {
        Self {
            text: TextTokens {
                brand_default: palette::BLUE_300,
                brand_hover: palette::BLUE_400,
                regular_default: palette::GRAY_10,
                subtle_default: palette::GRAY_40,
                on_brand_default: palette::BLACK_1000,
            },
            background: BackgroundTokens {
                primary_default: palette::GRAY_90,
                secondary_default: palette::GRAY_80,
                brand_default: palette::BLUE_300,
                brand_hover: palette::BLUE_400,
                brand_clicked: palette::BLUE_500,
            },
            border: BorderTokens {
                default: palette::GRAY_70,
                strong: palette::GRAY_60,
            },
            radius: RadiusTokens {
                small: 3.0,
                medium: 6.0,
            },
            spacer: SpacerTokens {
                xs: 4.0,
                sm: 8.0,
                md: 16.0,
            },
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Theme> = RefCell::new(Theme::light());
}

/// 读取当前生效主题（返回副本，开销极低）
pub fn current() -> Theme {
    CURRENT.with(|t| *t.borrow())
}

/// 切换主题，并请求整树重建使改动立即生效
pub fn set_theme(theme: Theme) {
    CURRENT.with(|t| *t.borrow_mut() = theme);
    crate::state::request_rebuild();
}
