//! 主题系统 —— 以 PatternFly v6 设计令牌（design tokens）为基准。
//!
//! - 调色板取自 PatternFly 的 `palette`（如 blue-100 = `#0066cc`）。
//! - 语义令牌（background / text / border / status）与其对齐。
//! - 间距（spacer）、字号（font）、圆角（radius）、阴影（shadow）
//!   均按 PatternFly 的 token 量表组织，便于 widget 统一引用。

use crate::geometry::Color;
use crate::view::paint::ShadowSpec;

// ───────────────────────────── Token 结构 ─────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct TextTokens {
    pub regular_default: Color,
    pub subtle_default: Color,
    pub link_default: Color,
    pub link_hover: Color,
    /// 品牌主色（用于标题、强调文本）
    pub brand_default: Color,
    pub brand_hover: Color,
    /// 品牌色之上的文本（按钮文字等）
    pub on_brand_default: Color,
}

#[derive(Debug, Clone, Copy)]
pub struct BackgroundTokens {
    /// 卡片 / 表面（白）
    pub primary_default: Color,
    /// 页面底色
    pub secondary_default: Color,
    /// 轨道 / 凹陷（进度条底、开关底）
    pub tertiary_default: Color,
    /// 品牌主色填充背景
    pub brand_default: Color,
    pub brand_hover: Color,
    pub brand_clicked: Color,
    /// 禁用态背景
    pub disabled_default: Color,
}

#[derive(Debug, Clone, Copy)]
pub struct BorderTokens {
    pub default: Color,
    pub strong: Color,
}

#[derive(Debug, Clone, Copy)]
pub struct RadiusTokens {
    pub small: f32,
    pub medium: f32,
    pub large: f32,
}

/// 间距量表（PatternFly spacer 量表，单位 px）
#[derive(Debug, Clone, Copy)]
pub struct SpacerTokens {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
    pub x2l: f32,
    pub x3l: f32,
}

/// 字号量表（PatternFly font-size 量表，单位 px）
#[derive(Debug, Clone, Copy)]
pub struct FontTokens {
    pub xs: f64,
    pub sm: f64,
    pub md: f64,
    pub lg: f64,
    pub xl: f64,
    pub x2l: f64,
    pub x3l: f64,
    pub x4l: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct ShadowTokens {
    pub sm: ShadowSpec,
    pub md: ShadowSpec,
    pub lg: ShadowSpec,
}

#[derive(Debug, Clone, Copy)]
pub struct StatusTokens {
    pub danger: Color,
    pub danger_bg: Color,
    pub success: Color,
    pub success_bg: Color,
    pub warning: Color,
    pub warning_bg: Color,
    pub info: Color,
    pub info_bg: Color,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub text: TextTokens,
    pub background: BackgroundTokens,
    pub border: BorderTokens,
    pub radius: RadiusTokens,
    pub spacer: SpacerTokens,
    pub font: FontTokens,
    pub shadow: ShadowTokens,
    pub status: StatusTokens,
}

// ───────────────────────────── 主题实例 ─────────────────────────────

impl Theme {
    pub fn light() -> Self {
        let shadow_color = Color::rgba(3, 3, 3, 26); // rgba(3,3,3,0.1)
        Theme {
            text: TextTokens {
                regular_default: Color::from_hex("#151515"),
                subtle_default: Color::from_hex("#6a6e73"),
                link_default: Color::from_hex("#0066cc"),
                link_hover: Color::from_hex("#004d99"),
                brand_default: Color::from_hex("#0066cc"),
                brand_hover: Color::from_hex("#004d99"),
                on_brand_default: Color::WHITE,
            },
            background: BackgroundTokens {
                primary_default: Color::from_hex("#ffffff"),
                secondary_default: Color::from_hex("#f2f2f2"),
                tertiary_default: Color::from_hex("#e0e0e0"),
                brand_default: Color::from_hex("#0066cc"),
                brand_hover: Color::from_hex("#004d99"),
                brand_clicked: Color::from_hex("#004d99"),
                disabled_default: Color::from_hex("#d2d2d2"),
            },
            border: BorderTokens {
                default: Color::from_hex("#e0e0e0"),
                strong: Color::from_hex("#c7c7c7"),
            },
            radius: RadiusTokens {
                small: 3.0,
                medium: 6.0,
                large: 12.0,
            },
            spacer: SpacerTokens {
                xs: 4.0,
                sm: 8.0,
                md: 16.0,
                lg: 24.0,
                xl: 32.0,
                x2l: 40.0,
                x3l: 48.0,
            },
            font: FontTokens {
                xs: 12.0,
                sm: 14.0,
                md: 16.0,
                lg: 18.0,
                xl: 24.0,
                x2l: 32.0,
                x3l: 40.0,
                x4l: 48.0,
            },
            shadow: ShadowTokens {
                sm: ShadowSpec {
                    offset_x: 0.0,
                    offset_y: 1.0,
                    blur: 2.0,
                    spread: 0.0,
                    color: shadow_color,
                },
                md: ShadowSpec {
                    offset_x: 0.0,
                    offset_y: 4.0,
                    blur: 8.0,
                    spread: 0.0,
                    color: shadow_color,
                },
                lg: ShadowSpec {
                    offset_x: 0.0,
                    offset_y: 21.0,
                    blur: 32.0,
                    spread: -11.0,
                    color: Color::rgba(3, 3, 3, 30),
                },
            },
            status: StatusTokens {
                danger: Color::from_hex("#c9190b"),
                danger_bg: Color::from_hex("#faeae8"),
                success: Color::from_hex("#3e8635"),
                success_bg: Color::from_hex("#e6f4ea"),
                warning: Color::from_hex("#f0ab00"),
                warning_bg: Color::from_hex("#fdf7d0"),
                info: Color::from_hex("#0066cc"),
                info_bg: Color::from_hex("#e7f1fa"),
            },
        }
    }

    pub fn dark() -> Self {
        let mut t = Self::light();
        t.text.regular_default = Color::from_hex("#f0f0f0");
        t.text.subtle_default = Color::from_hex("#a3a3a3");
        t.text.on_brand_default = Color::WHITE;
        t.background.primary_default = Color::from_hex("#1b1d23");
        t.background.secondary_default = Color::from_hex("#151515");
        t.background.tertiary_default = Color::from_hex("#383838");
        t.background.disabled_default = Color::from_hex("#383838");
        t.border.default = Color::from_hex("#383838");
        t.border.strong = Color::from_hex("#6a6a6a");
        // 暗色下阴影更重
        let dk = Color::rgba(0, 0, 0, 70);
        t.shadow.sm = ShadowSpec { color: dk, ..t.shadow.sm };
        t.shadow.md = ShadowSpec { color: dk, ..t.shadow.md };
        t.shadow.lg = ShadowSpec { color: dk, ..t.shadow.lg };
        t
    }
}

// ───────────────────────────── 全局当前主题 ─────────────────────────────

use std::cell::RefCell;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Light,
    Dark,
}

thread_local! {
    static THEME: RefCell<Theme> = RefCell::new(Theme::light());
    static MODE: RefCell<Mode> = RefCell::new(Mode::Light);
}

/// 取得当前主题快照。
pub fn current() -> Theme {
    THEME.with(|t| *t.borrow())
}

/// 设置当前主题（会覆盖模式）。
pub fn set(theme: Theme) {
    THEME.with(|t| *t.borrow_mut() = theme);
}

/// 设置当前明暗模式（内部用对应的预设主题）。
pub fn set_mode(mode: Mode) {
    MODE.with(|m| *m.borrow_mut() = mode);
    set(match mode {
        Mode::Light => Theme::light(),
        Mode::Dark => Theme::dark(),
    });
}
