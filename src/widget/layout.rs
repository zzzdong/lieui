//! LayoutAttr — 通用布局属性（widget 层）
//!
//! 布局属性在多个 widget（Container / Button / Image / Input / Switch / Divider 等）间
//! 高度重复。`LayoutAttr` 集中承载这些「块级布局」属性，并提供统一的 builder 方法与
//! `apply()` 映射到布局引擎的 [`FlexStyle`]，从而消除各 widget 的重复实现。
//!
//! widget 持有 `layout: LayoutAttr`，用 `layout_methods!` 宏生成同名转发方法，
//! 保持链式 API（如 `Container::new().width(100).expand(true)`）。

use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;

/// 通用块级布局属性集合。
#[derive(Debug, Clone, Copy)]
pub struct LayoutAttr {
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_width: Option<f32>,
    pub max_height: Option<f32>,
    /// flex-grow（expand 便捷设为 1.0）。
    pub flex_grow: f32,
    /// flex-shrink（None 表示使用默认）。
    pub flex_shrink: Option<f32>,
    /// 统一外边距。
    pub margin: f32,
    /// 方向性外边距（覆盖统一值）。
    pub margin_l: Option<f32>,
    pub margin_t: Option<f32>,
    pub margin_r: Option<f32>,
    pub margin_b: Option<f32>,
    /// 统一内边距。
    pub padding: f32,
    /// 方向性内边距（覆盖统一值）。
    pub padding_l: Option<f32>,
    pub padding_t: Option<f32>,
    pub padding_r: Option<f32>,
    pub padding_b: Option<f32>,
    /// 主轴对齐。
    pub justify_content: Option<FlexAlign>,
    /// 交叉轴对齐。
    pub align_items: Option<FlexAlign>,
    /// 自身在父容器交叉轴上的对齐。
    pub align_self: Option<FlexAlign>,
    /// 子项间距。
    pub gap: f32,
}

impl Default for LayoutAttr {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutAttr {
    pub fn new() -> Self {
        Self {
            width: None,
            height: None,
            min_width: None,
            min_height: None,
            max_width: None,
            max_height: None,
            flex_grow: 0.0,
            flex_shrink: None,
            margin: 0.0,
            margin_l: None,
            margin_t: None,
            margin_r: None,
            margin_b: None,
            padding: 0.0,
            padding_l: None,
            padding_t: None,
            padding_r: None,
            padding_b: None,
            justify_content: None,
            align_items: None,
            align_self: None,
            gap: 0.0,
        }
    }

    // ---- Builder 方法 ----

    pub fn width(mut self, v: f32) -> Self {
        self.width = Some(v);
        self
    }
    pub fn height(mut self, v: f32) -> Self {
        self.height = Some(v);
        self
    }
    pub fn min_width(mut self, v: f32) -> Self {
        self.min_width = Some(v);
        self
    }
    pub fn min_height(mut self, v: f32) -> Self {
        self.min_height = Some(v);
        self
    }
    pub fn max_width(mut self, v: f32) -> Self {
        self.max_width = Some(v);
        self
    }
    pub fn max_height(mut self, v: f32) -> Self {
        self.max_height = Some(v);
        self
    }
    /// 撑满可用空间（flex-grow = 1）。
    pub fn expand(mut self, v: bool) -> Self {
        self.flex_grow = if v { 1.0 } else { 0.0 };
        self
    }
    pub fn flex_grow(mut self, v: f32) -> Self {
        self.flex_grow = v;
        self
    }
    pub fn flex_shrink(mut self, v: f32) -> Self {
        self.flex_shrink = Some(v);
        self
    }
    pub fn margin(mut self, v: f32) -> Self {
        self.margin = v;
        self
    }
    pub fn margin_left(mut self, v: f32) -> Self {
        self.margin_l = Some(v);
        self
    }
    pub fn margin_top(mut self, v: f32) -> Self {
        self.margin_t = Some(v);
        self
    }
    pub fn margin_right(mut self, v: f32) -> Self {
        self.margin_r = Some(v);
        self
    }
    pub fn margin_bottom(mut self, v: f32) -> Self {
        self.margin_b = Some(v);
        self
    }
    pub fn padding(mut self, v: f32) -> Self {
        self.padding = v;
        self
    }
    pub fn padding_left(mut self, v: f32) -> Self {
        self.padding_l = Some(v);
        self
    }
    pub fn padding_top(mut self, v: f32) -> Self {
        self.padding_t = Some(v);
        self
    }
    pub fn padding_right(mut self, v: f32) -> Self {
        self.padding_r = Some(v);
        self
    }
    pub fn padding_bottom(mut self, v: f32) -> Self {
        self.padding_b = Some(v);
        self
    }
    pub fn justify_content(mut self, a: FlexAlign) -> Self {
        self.justify_content = Some(a);
        self
    }
    pub fn align_items(mut self, a: FlexAlign) -> Self {
        self.align_items = Some(a);
        self
    }
    pub fn align_self(mut self, a: FlexAlign) -> Self {
        self.align_self = Some(a);
        self
    }
    pub fn gap(mut self, v: f32) -> Self {
        self.gap = v;
        self
    }

    /// 把本布局属性应用到一个 FlexStyle 上，返回合并后的 FlexStyle。
    /// `style` 是 widget 的默认样式（如 padding/paint），本属性中的显式值会覆盖它。
    pub fn apply(self, style: FlexStyle) -> FlexStyle {
        let mut s = style;
        if let Some(v) = self.width {
            s = s.width(v);
        }
        if let Some(v) = self.height {
            s = s.height(v);
        }
        if let Some(v) = self.min_width {
            s = s.min_width(v);
        }
        if let Some(v) = self.min_height {
            s = s.min_height(v);
        }
        if let Some(v) = self.max_width {
            s = s.max_width(v);
        }
        if let Some(v) = self.max_height {
            s = s.max_height(v);
        }
        if self.flex_grow != 0.0 {
            s = s.flex_grow(self.flex_grow);
        }
        if let Some(v) = self.flex_shrink {
            s = s.flex_shrink(v);
        }
        if self.margin > 0.0 {
            s = s.margin_all(self.margin);
        }
        if let Some(v) = self.margin_l {
            s = s.margin_left(v);
        }
        if let Some(v) = self.margin_t {
            s = s.margin_top(v);
        }
        if let Some(v) = self.margin_r {
            s = s.margin_right(v);
        }
        if let Some(v) = self.margin_b {
            s = s.margin_bottom(v);
        }
        if self.padding > 0.0 {
            s = s.padding_all(self.padding);
        }
        if let Some(v) = self.padding_l {
            s = s.padding_left(v);
        }
        if let Some(v) = self.padding_t {
            s = s.padding_top(v);
        }
        if let Some(v) = self.padding_r {
            s = s.padding_right(v);
        }
        if let Some(v) = self.padding_b {
            s = s.padding_bottom(v);
        }
        if let Some(a) = self.justify_content {
            s = s.justify_content(a);
        }
        if let Some(a) = self.align_items {
            s = s.align_items(a);
        }
        if let Some(a) = self.align_self {
            s = s.align_self(a);
        }
        if self.gap > 0.0 {
            s = s.gap(self.gap);
        }
        s
    }
}

/// 为持有 `layout: LayoutAttr` 字段的 widget 生成布局方法。
///
/// 用法：`layout_methods! { for WidgetType }`，其中 `WidgetType` 需有一个
/// `layout: LayoutAttr` 字段，且方法返回 `Self`。
///
/// 宏生成两类方法：
/// - `layout()`：用完整 [`LayoutAttr`]（builder 式）设置布局，是推荐方式；
/// - 一组旧链式转发方法（`width`/`height`/`expand`/`flex_shrink`/`margin`/`padding`
///   /`align`/`gap` 等）：保持向后兼容，内部转发到 `layout` 字段。
#[macro_export]
macro_rules! layout_methods {
    (for $ty:ty) => {
        /// 用完整布局属性（builder 式）设置本 widget 的布局。
        pub fn layout(mut self, l: $crate::widget::layout::LayoutAttr) -> Self {
            self.layout = l;
            self
        }
        pub fn width(mut self, v: f32) -> Self {
            self.layout = self.layout.width(v);
            self
        }
        pub fn height(mut self, v: f32) -> Self {
            self.layout = self.layout.height(v);
            self
        }
        pub fn min_width(mut self, v: f32) -> Self {
            self.layout = self.layout.min_width(v);
            self
        }
        pub fn min_height(mut self, v: f32) -> Self {
            self.layout = self.layout.min_height(v);
            self
        }
        pub fn max_width(mut self, v: f32) -> Self {
            self.layout = self.layout.max_width(v);
            self
        }
        pub fn max_height(mut self, v: f32) -> Self {
            self.layout = self.layout.max_height(v);
            self
        }
        pub fn expand(mut self, v: bool) -> Self {
            self.layout = self.layout.expand(v);
            self
        }
        pub fn flex_grow(mut self, v: f32) -> Self {
            self.layout = self.layout.flex_grow(v);
            self
        }
        pub fn flex_shrink(mut self, v: f32) -> Self {
            self.layout = self.layout.flex_shrink(v);
            self
        }
        pub fn margin(mut self, v: f32) -> Self {
            self.layout = self.layout.margin(v);
            self
        }
        pub fn margin_left(mut self, v: f32) -> Self {
            self.layout = self.layout.margin_left(v);
            self
        }
        pub fn margin_top(mut self, v: f32) -> Self {
            self.layout = self.layout.margin_top(v);
            self
        }
        pub fn margin_right(mut self, v: f32) -> Self {
            self.layout = self.layout.margin_right(v);
            self
        }
        pub fn margin_bottom(mut self, v: f32) -> Self {
            self.layout = self.layout.margin_bottom(v);
            self
        }
        pub fn padding(mut self, v: f32) -> Self {
            self.layout = self.layout.padding(v);
            self
        }
        pub fn padding_left(mut self, v: f32) -> Self {
            self.layout = self.layout.padding_left(v);
            self
        }
        pub fn padding_top(mut self, v: f32) -> Self {
            self.layout = self.layout.padding_top(v);
            self
        }
        pub fn padding_right(mut self, v: f32) -> Self {
            self.layout = self.layout.padding_right(v);
            self
        }
        pub fn padding_bottom(mut self, v: f32) -> Self {
            self.layout = self.layout.padding_bottom(v);
            self
        }
        pub fn justify_content(mut self, a: FlexAlign) -> Self {
            self.layout = self.layout.justify_content(a);
            self
        }
        pub fn align_items(mut self, a: FlexAlign) -> Self {
            self.layout = self.layout.align_items(a);
            self
        }
        pub fn align_self(mut self, a: FlexAlign) -> Self {
            self.layout = self.layout.align_self(a);
            self
        }
        pub fn gap(mut self, v: f32) -> Self {
            self.layout = self.layout.gap(v);
            self
        }
    };
}
