//! 视觉样式（**内联**，无选择器 / 无继承 / 无属性表）。
//!
//! 从 `feature/mvp` 的 `src/view/paint.rs` 拆出：
//! - **排版规格**（`font_size` / `font_family` / `font_weight` / `line_height` / `max_width` /
//!   `text_align` / `wrap`）搬进了 `lieui-text::TextSpec` —— 布局引擎要按它测度；
//! - 这里是**绘制层**的样式：`TextStyle = TextSpec + 颜色（含 hover/pressed 变体）`。
//!
//! `Node` 侧三个字段并列（`layout` / `paint` / `text`），不做聚合类型 —— 见设计文档 §3.2。

use lieui_geom::Color;

pub use lieui_text::{FontWeight, TextAlign, TextSpec};

// ───────────────────────── 阴影 ─────────────────────────

/// 阴影规格（对齐 PatternFly 的 box-shadow token）
///
/// 偏移 / 模糊 / 扩展均为像素，颜色自带 alpha（用 [`Color::rgba`]）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowSpec {
    pub offset_x: f32,
    pub offset_y: f32,
    /// 高斯模糊半径（px）
    pub blur: f32,
    /// 扩展半径（px，可为负）
    pub spread: f32,
    /// 阴影颜色（含 alpha）
    pub color: Color,
}

impl ShadowSpec {
    pub const fn new(offset_x: f32, offset_y: f32, blur: f32, spread: f32, color: Color) -> Self {
        Self {
            offset_x,
            offset_y,
            blur,
            spread,
            color,
        }
    }
}

// ───────────────────── 容器视觉样式 ─────────────────────

/// 容器（`Kind::Box` 及其它带背景的组件）的视觉样式
#[derive(Debug, Clone, PartialEq)]
pub struct PaintStyle {
    pub background_color: Option<Color>,
    /// 悬停背景色（配合节点的 `interaction.pointer_over`）
    pub hover_background: Option<Color>,
    /// 按下背景色（配合节点的 `interaction.pressed`）
    pub pressed_background: Option<Color>,
    pub border_radius: f32,
    pub border_color: Option<Color>,
    pub border_width: f32,
    /// 是否裁剪子节点内容（滚动容器等）
    pub clip_content: bool,
    /// 不透明度（0.0 ~ 1.0）
    pub opacity: f32,
    pub shadow: Option<ShadowSpec>,
}

impl Default for PaintStyle {
    fn default() -> Self {
        Self {
            background_color: None,
            hover_background: None,
            pressed_background: None,
            border_radius: 0.0,
            border_color: None,
            border_width: 0.0,
            clip_content: false,
            opacity: 1.0,
            shadow: None,
        }
    }
}

impl PaintStyle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn background(mut self, c: Color) -> Self {
        self.background_color = Some(c);
        self
    }

    pub fn hover_background(mut self, c: Color) -> Self {
        self.hover_background = Some(c);
        self
    }

    pub fn pressed_background(mut self, c: Color) -> Self {
        self.pressed_background = Some(c);
        self
    }

    pub fn radius(mut self, r: f32) -> Self {
        self.border_radius = r;
        self
    }

    pub fn border(mut self, width: f32, color: Color) -> Self {
        self.border_width = width;
        self.border_color = Some(color);
        self
    }

    pub fn opacity(mut self, o: f32) -> Self {
        self.opacity = o.clamp(0.0, 1.0);
        self
    }

    pub fn clip(mut self, v: bool) -> Self {
        self.clip_content = v;
        self
    }

    pub fn shadow(mut self, s: ShadowSpec) -> Self {
        self.shadow = Some(s);
        self
    }

    /// 节点声明了交互视觉吗（决定它是否参与 hover/pressed 状态管理）
    pub fn is_interactive(&self) -> bool {
        self.hover_background.is_some() || self.pressed_background.is_some()
    }
}

// ───────────────────── 文本视觉样式 ─────────────────────

/// 文本节点的视觉 + 排版样式。
///
/// 排版字段全在 `spec` 里（布局引擎按 `spec` 测度，绘制层把 `color` 传给 parley 当画笔）。
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub spec: TextSpec,
    pub color: Color,
    /// 悬停时颜色（`None` 表示保持 `color`）—— 图标/文本随按钮 hover 变色用
    pub hover_color: Option<Color>,
    /// 按下时颜色（`None` 表示保持 `color`）
    pub pressed_color: Option<Color>,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            spec: TextSpec::default(),
            color: Color::BLACK,
            hover_color: None,
            pressed_color: None,
        }
    }
}

impl TextStyle {
    pub fn new() -> Self {
        Self::default()
    }

    // ── 排版（转发到 spec）──

    pub fn font_size(mut self, s: f64) -> Self {
        self.spec.font_size = s;
        self
    }

    pub fn font_family(mut self, f: impl Into<String>) -> Self {
        self.spec.font_family = f.into();
        self
    }

    pub fn font_weight(mut self, w: impl Into<FontWeight>) -> Self {
        self.spec.font_weight = w.into();
        self
    }

    pub fn line_height(mut self, h: f64) -> Self {
        self.spec.line_height = Some(h);
        self
    }

    pub fn max_width(mut self, w: f64) -> Self {
        self.spec.max_width = Some(w);
        self
    }

    pub fn text_align(mut self, a: TextAlign) -> Self {
        self.spec.text_align = a;
        self
    }

    pub fn wrap(mut self, v: bool) -> Self {
        self.spec.wrap = v;
        self
    }

    // ── 颜色 ──

    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }

    pub fn hover_color(mut self, c: Color) -> Self {
        self.hover_color = Some(c);
        self
    }

    pub fn pressed_color(mut self, c: Color) -> Self {
        self.pressed_color = Some(c);
        self
    }

    /// 节点声明了交互视觉吗（决定它是否参与 hover/pressed 状态管理）
    pub fn is_interactive(&self) -> bool {
        self.hover_color.is_some() || self.pressed_color.is_some()
    }
}

// ───────────────────── 图片视觉样式 ─────────────────────

/// 图片填充模式
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum ImageFit {
    /// 拉伸填满容器
    #[default]
    Fill,
    /// 等比缩放，完整显示
    Contain,
    /// 等比缩放，覆盖容器（可能裁剪）
    Cover,
    /// 保持原始尺寸
    None,
}

/// 图片节点的视觉样式与原始尺寸
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageStyle {
    pub width: u32,
    pub height: u32,
    pub opacity: f32,
    pub fit: ImageFit,
    pub border_radius: f32,
}

impl Default for ImageStyle {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            opacity: 1.0,
            fit: ImageFit::Fill,
            border_radius: 0.0,
        }
    }
}

impl ImageStyle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn size(mut self, w: u32, h: u32) -> Self {
        self.width = w;
        self.height = h;
        self
    }

    pub fn opacity(mut self, o: f32) -> Self {
        self.opacity = o.clamp(0.0, 1.0);
        self
    }

    pub fn fit(mut self, f: ImageFit) -> Self {
        self.fit = f;
        self
    }

    pub fn radius(mut self, r: f32) -> Self {
        self.border_radius = r;
        self
    }
}

#[cfg(test)]
#[path = "style_tests.rs"]
mod tests;
