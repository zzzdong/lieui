//! 视觉样式 —— ViewNode 层的绘制属性
//!
//! 与布局属性（FlexStyle）分离，只负责元素的外观呈现。
//! 上层 widget 负责把类 CSS 的样式规则编译成这些确定的内联样式。

use crate::geometry::Color;

/// 阴影规格（对齐 PatternFly 的 box-shadow token）
///
/// 偏移/模糊/扩展均为像素，颜色自带 alpha（用 [`Color::rgba`]）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowSpec {
    /// 水平偏移（px）
    pub offset_x: f32,
    /// 垂直偏移（px）
    pub offset_y: f32,
    /// 高斯模糊半径（px）
    pub blur: f32,
    /// 扩展半径（px，可为负）
    pub spread: f32,
    /// 阴影颜色（含 alpha）
    pub color: Color,
}

/// Div / Canvas 的视觉样式
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaintStyle {
    /// 背景色
    pub background_color: Option<Color>,
    /// 悬停背景色
    pub hover_background: Option<Color>,
    /// 按下背景色
    pub pressed_background: Option<Color>,
    /// 圆角半径
    pub border_radius: f32,
    /// 边框颜色
    pub border_color: Option<Color>,
    /// 边框宽度
    pub border_width: f32,
    /// 是否裁剪子节点内容（用于滚动容器等）
    pub clip_content: bool,
    /// 不透明度（0.0 ~ 1.0）
    pub opacity: f32,
    /// 投影（PatternFly 的 box-shadow token）
    pub shadow: Option<ShadowSpec>,
}

impl PaintStyle {
    pub fn new() -> Self {
        Self {
            opacity: 1.0,
            ..Default::default()
        }
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

    /// 设置投影。传入主题中的 `ShadowTokens`（如 `theme.shadow.sm`）。
    pub fn shadow(mut self, s: ShadowSpec) -> Self {
        self.shadow = Some(s);
        self
    }
}

/// 字重
#[derive(Debug, Clone, Default, PartialEq)]
pub enum FontWeight {
    #[default]
    Normal,
    Medium,
    Bold,
    Weight(u16),
}

impl From<u16> for FontWeight {
    fn from(value: u16) -> Self {
        Self::Weight(value)
    }
}

/// 文本水平对齐
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

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

/// 文本节点的视觉与排版样式
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub font_size: f64,
    pub color: Color,
    pub font_family: String,
    pub font_weight: FontWeight,
    pub line_height: Option<f64>,
    pub max_width: Option<f64>,
    pub text_align: TextAlign,
    /// 是否允许文本换行。默认 true。
    /// 设为 false 时布局和渲染都保持单行，不按容器宽度重新测量。
    pub wrap: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            color: Color::BLACK,
            font_family: "sans-serif".to_string(),
            font_weight: FontWeight::Normal,
            line_height: None,
            max_width: None,
            text_align: TextAlign::Start,
            wrap: true,
        }
    }
}

impl TextStyle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn font_size(mut self, s: f64) -> Self {
        self.font_size = s;
        self
    }

    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }

    pub fn font_family(mut self, f: impl Into<String>) -> Self {
        self.font_family = f.into();
        self
    }

    pub fn font_weight(mut self, w: impl Into<FontWeight>) -> Self {
        self.font_weight = w.into();
        self
    }

    pub fn line_height(mut self, h: f64) -> Self {
        self.line_height = Some(h);
        self
    }

    pub fn max_width(mut self, w: f64) -> Self {
        self.max_width = Some(w);
        self
    }

    pub fn text_align(mut self, a: TextAlign) -> Self {
        self.text_align = a;
        self
    }

    pub fn wrap(mut self, v: bool) -> Self {
        self.wrap = v;
        self
    }
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
