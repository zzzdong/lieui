//! 文本排版规格 —— 一次测量/排版的全部输入

use crate::cache::{HashMix, MeasureKey, hash_str};

/// 文本对齐
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

/// 字重（100..=900，步长 100）
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct FontWeight(pub u32);

impl FontWeight {
    pub const THIN: Self = Self(100);
    pub const NORMAL: Self = Self(400);
    pub const MEDIUM: Self = Self(500);
    pub const BOLD: Self = Self(700);
    pub const BLACK: Self = Self(900);
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::NORMAL
    }
}

/// 一次文本测量 / 排版的输入。
///
/// 只含影响排版结果的字段：颜色、下划线等纯绘制属性不参与，避免无谓的缓存失效。
#[derive(Clone, Copy, Debug)]
pub struct TextSpec<'a> {
    pub text: &'a str,
    /// 字体族名；空串表示使用默认字体
    pub family: &'a str,
    pub font_size: f32,
    /// 行高倍数（`1.5` = CSS `line-height: 1.5`）；`<= 0` 表示使用字体度量
    pub line_height: f32,
    pub weight: FontWeight,
    pub italic: bool,
    /// 是否按 `max_width` 换行；false 时始终单行
    pub wrap: bool,
    /// 可用宽度上限
    pub max_width: Option<f32>,
    pub align: TextAlign,
}

impl Default for TextSpec<'_> {
    fn default() -> Self {
        Self {
            text: "",
            family: "",
            font_size: 16.0,
            line_height: 0.0,
            weight: FontWeight::NORMAL,
            italic: false,
            wrap: true,
            max_width: None,
            align: TextAlign::Start,
        }
    }
}

impl<'a> TextSpec<'a> {
    /// 影响排版结果的样式哈希（不含 text 与 align）
    pub fn style_hash(&self) -> u64 {
        let mut m = HashMix::new();
        m.mix_f32(self.font_size);
        m.mix_f32(self.line_height);
        m.mix_u64(self.weight.0 as u64);
        m.mix_bool(self.italic);
        m.mix_str(self.family);
        m.finish()
    }

    /// 换行后的有效最大宽度
    pub fn effective_max_width(&self) -> Option<f32> {
        if self.wrap { self.max_width } else { None }
    }

    pub fn key(&self) -> MeasureKey {
        MeasureKey::new(
            hash_str(self.text),
            self.style_hash(),
            self.wrap,
            self.effective_max_width(),
        )
    }
}
