//! 布局引擎基础类型定义
//!
//! 参考 Taitank (Tencent) / W3C CSS Flexbox 规范

/// 布局方向（LTR / RTL）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Inherit,
    Ltr,
    Rtl,
}

/// Flex 方向：主轴方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

/// Flex 换行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexWrap {
    NoWrap,
    Wrap,
    WrapReverse,
}

/// Flex 对齐方式（用于 justify-content, align-items, align-self, align-content）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexAlign {
    Auto,
    Start,
    Center,
    End,
    Stretch,
    Baseline,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// 方向边（用于 margin/padding/border/position 方向指定）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CSSDirection {
    Left = 0,
    Top = 1,
    Right = 2,
    Bottom = 3,
    Start = 4,
    End = 5,
    Horizontal = 6,
    Vertical = 7,
    All = 8,
    None = 9,
}

/// 维度（宽/高）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Width = 0,
    Height = 1,
}

/// 定位类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionType {
    Relative,
    Absolute,
}

/// 显示类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayType {
    Flex,
    None,
}

/// 节点类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Default,
    Text,
}

/// 测量模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasureMode {
    Undefined,
    Exactly,
    AtMost,
}

/// 布局动作（用于缓存和分阶段测量）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutAction {
    MeasureWidth = 1,
    MeasureHeight = 2,
    Layout = 3,
}

/// 尺寸
#[derive(Debug, Clone, Copy)]
pub struct TaitankSize {
    pub width: f32,
    pub height: f32,
}

/// 尺寸 + 测量模式
#[derive(Debug, Clone, Copy)]
pub struct SizeMode {
    pub width_measure_mode: MeasureMode,
    pub height_measure_mode: MeasureMode,
}

/// 布局计算结果（参考 Taitank::TaitankLayout）
#[derive(Debug, Clone)]
pub struct LayoutResult {
    /// 位置 [left, top, right, bottom]
    pub position: [f32; 4],
    pub cached_position: [f32; 4],
    /// 尺寸 [width, height]
    pub dim: [f32; 2],
    /// margin [left, right, top, bottom] — 对应 CSS_LEFT=0, CSS_TOP=1, CSS_RIGHT=2, CSS_BOTTOM=3
    /// 注意：Taitank 的 kAxisStart = [CSS_LEFT, CSS_RIGHT, CSS_TOP, CSS_BOTTOM]
    /// 但 position 和 margin 数组按 [CSS_LEFT, CSS_TOP, CSS_RIGHT, CSS_BOTTOM] 存储
    pub margin: [f32; 4],
    pub padding: [f32; 4],
    pub border: [f32; 4],
    pub had_overflow: bool,
    pub direction: Direction,

    /// 弹性基础尺寸
    pub flex_base_size: f32,
    /// 假设主轴外边距框尺寸
    pub hypothetical_main_axis_margin_boxsize: f32,
    /// 假设主轴尺寸
    pub hypothetical_main_axis_size: f32,
}

impl Default for LayoutResult {
    fn default() -> Self {
        Self {
            position: [0.0; 4],
            cached_position: [0.0; 4],
            dim: [0.0; 2],
            margin: [0.0; 4],
            padding: [0.0; 4],
            border: [0.0; 4],
            had_overflow: false,
            direction: Direction::Inherit,
            flex_base_size: 0.0,
            hypothetical_main_axis_margin_boxsize: 0.0,
            hypothetical_main_axis_size: 0.0,
        }
    }
}

/// 轴映射：四个方向对应 FlexDirection 的 ROW, ROW_REVERSE, COLUMN, COLUMN_REVERSE
pub const K_AXIS_START: [CSSDirection; 4] = [
    CSSDirection::Left,
    CSSDirection::Right,
    CSSDirection::Top,
    CSSDirection::Bottom,
];

pub const K_AXIS_END: [CSSDirection; 4] = [
    CSSDirection::Right,
    CSSDirection::Left,
    CSSDirection::Bottom,
    CSSDirection::Top,
];

pub const K_AXIS_DIM: [Dimension; 4] = [
    Dimension::Width,
    Dimension::Width,
    Dimension::Height,
    Dimension::Height,
];

/// 判断是否为 Row 方向
pub fn is_row_direction(dir: FlexDirection) -> bool {
    matches!(dir, FlexDirection::Row | FlexDirection::RowReverse)
}

/// 判断是否为 Column 方向
pub fn is_column_direction(dir: FlexDirection) -> bool {
    matches!(dir, FlexDirection::Column | FlexDirection::ColumnReverse)
}

/// 判断是否为 Reverse 方向
pub fn is_reverse_direction(dir: FlexDirection) -> bool {
    matches!(
        dir,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    )
}

/// NaN 作为"未定义"的值
pub const VALUE_UNDEFINED: f32 = f32::NAN;
pub const VALUE_AUTO: f32 = f32::NAN;

/// 判断值是否已定义（非 NaN）
pub fn is_defined(v: f32) -> bool {
    !v.is_nan()
}

/// 判断值是否未定义
pub fn is_undefined(v: f32) -> bool {
    v.is_nan()
}

/// NaN 转 INF
pub fn nan_as_inf(n: f32) -> f32 {
    if n.is_nan() {
        f32::INFINITY
    } else {
        n
    }
}

/// 浮点数近似相等比较
pub fn float_is_equal(a: f32, b: f32) -> bool {
    if is_undefined(a) {
        return is_undefined(b);
    }
    if is_undefined(b) {
        return is_undefined(a);
    }
    (a - b).abs() < 0.0001
}
