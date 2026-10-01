//! 排版规格 —— 只含"影响整形 / 测度"的字段。
//!
//! 颜色**不在**这里：画笔颜色由绘制层在 `create_text_layout(text, spec, color)` 时给出，
//! 这样测度缓存的键就不会混入纯绘制属性。

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

/// 文本排版规格（内联，不走 CSS 级联）
#[derive(Debug, Clone, PartialEq)]
pub struct TextSpec {
    pub font_size: f64,
    pub font_family: String,
    pub font_weight: FontWeight,
    /// 行高。parley 0.11 无直接 StyleProperty，暂由上层通过额外行间距实现。
    pub line_height: Option<f64>,
    /// 显式最大宽度（用户设置）。布局约束给出的宽度在 `Measurable` 里另行注入。
    pub max_width: Option<f64>,
    pub text_align: TextAlign,
    /// 是否允许换行。默认 true；false 时布局与渲染都保持单行。
    pub wrap: bool,
    /// **按墨迹盒（ink bounds）参与尺寸与居中**，而不是行盒。
    ///
    /// 行盒里 ascent/descent 不对称 ⇒ 可见字形视觉中心偏离行盒中心（普通字体约 0.1em），
    /// 与行高系数 1.0 的图标字体混排时"盒子对齐了、看着没对齐"。打开本项后：
    /// - `measure_text` 的高度 = 墨迹高度；
    /// - 绘制按墨迹上缘对齐节点矩形 ⇒ 可配合 `align_items(Center)` 做光学居中。
    ///
    /// 默认 false（行盒语义，换行/多行排版更稳）。
    pub optical_align: bool,
}

impl Default for TextSpec {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            font_family: "sans-serif".to_string(),
            font_weight: FontWeight::Normal,
            line_height: None,
            max_width: None,
            text_align: TextAlign::Start,
            wrap: true,
            optical_align: false,
        }
    }
}

impl TextSpec {
    pub fn new(font_size: f64) -> Self {
        Self {
            font_size,
            ..Default::default()
        }
    }

    pub fn font_size(mut self, s: f64) -> Self {
        self.font_size = s;
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

    /// 按墨迹盒参与尺寸/居中（见字段说明）
    pub fn optical_align(mut self, v: bool) -> Self {
        self.optical_align = v;
        self
    }
}
