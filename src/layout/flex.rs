//! Flex 布局样式

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum JustifyContent {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AlignItems {
    Start,
    Center,
    End,
    #[default]
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FlexDirection {
    Row,
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FlexWrap {
    #[default]
    NoWrap,
    Wrap,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FlexStyle {
    pub direction: FlexDirection,
    pub spacing: f32,
    pub justify: JustifyContent,
    pub align: AlignItems,
    pub expand: bool,
    /// flex-grow 系数（0=不增长，>0 按比例占用剩余空间）
    pub flex_grow: f32,
    /// flex-shrink 系数（1=等比例收缩，0=不收缩）
    pub flex_shrink: f32,
    /// 是否允许换行
    pub wrap: FlexWrap,
}
impl FlexStyle {
    pub fn new(d: FlexDirection) -> Self {
        Self {
            direction: d,
            spacing: 0.0,
            justify: JustifyContent::Start,
            align: AlignItems::Stretch,
            expand: false,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            wrap: FlexWrap::NoWrap,
        }
    }
    pub fn column() -> Self {
        Self::new(FlexDirection::Column)
    }
    pub fn row() -> Self {
        Self::new(FlexDirection::Row)
    }
}
impl Default for FlexStyle {
    fn default() -> Self {
        Self::row()
    }
}
