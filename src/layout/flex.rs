// src/layout/flex.rs
//! Flex 布局支持

/// Flex 方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexDirection {
    /// 水平方向
    Row,
    /// 垂直方向
    Column,
}

/// 主轴对齐方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JustifyContent {
    /// 起始位置对齐
    Start,
    /// 结束位置对齐
    End,
    /// 居中对齐
    Center,
    /// 两端对齐
    SpaceBetween,
    /// 均匀分布
    SpaceAround,
    /// 等间距分布
    SpaceEvenly,
}

/// 交叉轴对齐方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignItems {
    /// 起始位置对齐
    Start,
    /// 结束位置对齐
    End,
    /// 居中对齐
    Center,
    /// 拉伸填满
    Stretch,
}

/// Flex 布局配置
#[derive(Debug, Clone)]
pub struct FlexStyle {
    /// 主轴方向
    pub direction: FlexDirection,
    /// 主轴对齐方式
    pub justify_content: JustifyContent,
    /// 交叉轴对齐方式
    pub align_items: AlignItems,
    /// 子节点间距
    pub gap: f32,
}

impl Default for FlexStyle {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Column,
            justify_content: JustifyContent::Start,
            align_items: AlignItems::Start,
            gap: 0.0,
        }
    }
}

impl FlexStyle {
    /// 创建行布局
    pub fn row() -> Self {
        Self {
            direction: FlexDirection::Row,
            ..Default::default()
        }
    }

    /// 创建列布局
    pub fn column() -> Self {
        Self {
            direction: FlexDirection::Column,
            ..Default::default()
        }
    }

    /// 设置主轴对齐
    pub fn justify(mut self, justify: JustifyContent) -> Self {
        self.justify_content = justify;
        self
    }

    /// 设置交叉轴对齐
    pub fn align(mut self, align: AlignItems) -> Self {
        self.align_items = align;
        self
    }

    /// 设置间距
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }
}
