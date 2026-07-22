//! Flex 布局样式

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JustifyContent { Start, Center, End, SpaceBetween, SpaceAround, SpaceEvenly }
impl Default for JustifyContent { fn default() -> Self { Self::Start } }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlignItems { Start, Center, End, Stretch }
impl Default for AlignItems { fn default() -> Self { Self::Stretch } }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FlexDirection { Row, Column }

#[derive(Debug, Clone, PartialEq)]
pub struct FlexStyle {
    pub direction: FlexDirection, pub spacing: f32,
    pub justify: JustifyContent, pub align: AlignItems, pub expand: bool,
}
impl FlexStyle {
    pub fn new(d: FlexDirection) -> Self { Self { direction: d, spacing: 0.0, justify: JustifyContent::Start, align: AlignItems::Stretch, expand: false } }
    pub fn column() -> Self { Self::new(FlexDirection::Column) }
    pub fn row() -> Self { Self::new(FlexDirection::Row) }
}
impl Default for FlexStyle { fn default() -> Self { Self::row() } }
