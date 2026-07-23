//! Flex 样式属性
//!
//! 参考 Taitank::TaitankStyle

use crate::layout::types::*;

const K_CSS_PROPS_COUNT: usize = 6;

/// Flex 样式
#[derive(Debug, Clone)]
pub struct FlexStyle {
    pub node_type: NodeType,
    pub direction: Direction,
    pub flex_direction: FlexDirection,
    pub justify_content: FlexAlign,
    pub align_content: FlexAlign,
    pub align_items: FlexAlign,
    pub align_self: FlexAlign,
    pub flex_wrap: FlexWrap,
    pub position_type: PositionType,
    pub display_type: DisplayType,
    pub overflow_scroll: bool,

    pub flex_basis: f32,
    pub flex_grow: f32,
    pub flex_shrink: f32,

    // CSS 属性数组
    pub margin: [f32; K_CSS_PROPS_COUNT],
    pub margin_from: [CSSDirection; K_CSS_PROPS_COUNT],
    pub padding: [f32; K_CSS_PROPS_COUNT],
    pub padding_from: [CSSDirection; K_CSS_PROPS_COUNT],
    pub border: [f32; K_CSS_PROPS_COUNT],
    pub border_from: [CSSDirection; K_CSS_PROPS_COUNT],
    pub position: [f32; K_CSS_PROPS_COUNT],

    pub dim: [f32; 2],
    pub min_dim: [f32; 2],
    pub max_dim: [f32; 2],

    /// item 之间的间距（主轴方向）
    pub item_space: f32,
    /// 行之间的间距（交叉轴方向）
    pub line_space: f32,
}

impl Default for FlexStyle {
    fn default() -> Self {
        Self {
            node_type: NodeType::Default,
            direction: Direction::Inherit,
            flex_direction: FlexDirection::Column, // Taitank 默认 Column
            align_self: FlexAlign::Auto,
            align_items: FlexAlign::Stretch,
            align_content: FlexAlign::Start,
            justify_content: FlexAlign::Start,
            position_type: PositionType::Relative,
            display_type: DisplayType::Flex,
            overflow_scroll: false,

            dim: [VALUE_UNDEFINED; 2],
            min_dim: [VALUE_UNDEFINED; 2],
            max_dim: [VALUE_UNDEFINED; 2],

            position: [VALUE_AUTO; K_CSS_PROPS_COUNT],

            margin: [0.0; K_CSS_PROPS_COUNT],
            margin_from: [CSSDirection::None; K_CSS_PROPS_COUNT],
            padding: [0.0; K_CSS_PROPS_COUNT],
            padding_from: [CSSDirection::None; K_CSS_PROPS_COUNT],
            border: [0.0; K_CSS_PROPS_COUNT],
            border_from: [CSSDirection::None; K_CSS_PROPS_COUNT],

            flex_wrap: FlexWrap::NoWrap,
            flex_grow: 0.0,
            flex_shrink: 0.0, // Taitank 默认 0（Web 默认 1）
            flex_basis: VALUE_AUTO,
            item_space: 0.0,
            line_space: 0.0,
        }
    }
}

impl FlexStyle {
    // ---- Dimension ----

    pub fn set_dimension_dim(&mut self, dim: Dimension, value: f32) {
        self.dim[dim as usize] = value;
    }

    pub fn set_dimension_axis(&mut self, axis: FlexDirection, value: f32) {
        self.dim[K_AXIS_DIM[axis as usize] as usize] = value;
    }

    pub fn get_dimension_dim(&self, dim: Dimension) -> f32 {
        self.dim[dim as usize]
    }

    pub fn get_dimension_axis(&self, axis: FlexDirection) -> f32 {
        self.dim[K_AXIS_DIM[axis as usize] as usize]
    }

    pub fn is_dimension_auto(&self, axis: FlexDirection) -> bool {
        is_undefined(self.dim[K_AXIS_DIM[axis as usize] as usize])
    }

    // ---- Margin ----

    fn set_edge(
        values: &mut [f32; K_CSS_PROPS_COUNT],
        from: &mut [CSSDirection; K_CSS_PROPS_COUNT],
        dir: CSSDirection,
        value: f32,
    ) -> bool {
        let mut has_set = false;
        match dir {
            CSSDirection::Start | CSSDirection::End => {
                if !float_is_equal(values[dir as usize], value) {
                    values[dir as usize] = value;
                    from[dir as usize] = dir;
                    has_set = true;
                }
            }
            CSSDirection::Left | CSSDirection::Top | CSSDirection::Right | CSSDirection::Bottom => {
                from[dir as usize] = dir;
                if !float_is_equal(values[dir as usize], value) {
                    values[dir as usize] = value;
                    has_set = true;
                }
            }
            CSSDirection::Horizontal => {
                if from[CSSDirection::Left as usize] != CSSDirection::Left {
                    from[CSSDirection::Left as usize] = CSSDirection::Horizontal;
                    if !float_is_equal(values[CSSDirection::Left as usize], value) {
                        values[CSSDirection::Left as usize] = value;
                        has_set = true;
                    }
                }
                if from[CSSDirection::Right as usize] != CSSDirection::Right {
                    from[CSSDirection::Right as usize] = CSSDirection::Horizontal;
                    if !float_is_equal(values[CSSDirection::Right as usize], value) {
                        values[CSSDirection::Right as usize] = value;
                        has_set = true;
                    }
                }
            }
            CSSDirection::Vertical => {
                if from[CSSDirection::Top as usize] != CSSDirection::Top {
                    from[CSSDirection::Top as usize] = CSSDirection::Vertical;
                    if !float_is_equal(values[CSSDirection::Top as usize], value) {
                        values[CSSDirection::Top as usize] = value;
                        has_set = true;
                    }
                }
                if from[CSSDirection::Bottom as usize] != CSSDirection::Bottom {
                    from[CSSDirection::Bottom as usize] = CSSDirection::Vertical;
                    if !float_is_equal(values[CSSDirection::Bottom as usize], value) {
                        values[CSSDirection::Bottom as usize] = value;
                        has_set = true;
                    }
                }
            }
            CSSDirection::All => {
                for i in CSSDirection::Left as usize..=CSSDirection::Bottom as usize {
                    if from[i] == CSSDirection::None {
                        values[i] = value;
                        from[i] = CSSDirection::All;
                        has_set = true;
                    } else if from[i] == CSSDirection::All && !float_is_equal(values[i], value) {
                        values[i] = value;
                        has_set = true;
                    }
                }
            }
            CSSDirection::None => {}
        }
        has_set
    }

    pub fn set_margin(&mut self, dir: CSSDirection, value: f32) -> bool {
        Self::set_edge(&mut self.margin, &mut self.margin_from, dir, value)
    }

    pub fn set_padding(&mut self, dir: CSSDirection, value: f32) -> bool {
        Self::set_edge(&mut self.padding, &mut self.padding_from, dir, value)
    }

    pub fn set_border(&mut self, dir: CSSDirection, value: f32) -> bool {
        Self::set_edge(&mut self.border, &mut self.border_from, dir, value)
    }

    pub fn set_position(&mut self, dir: CSSDirection, value: f32) -> bool {
        if dir as usize > CSSDirection::End as usize {
            return false;
        }
        if !float_is_equal(self.position[dir as usize], value) {
            self.position[dir as usize] = value;
            return true;
        }
        false
    }

    pub fn get_start_position(&self, axis: FlexDirection) -> f32 {
        if is_row_direction(axis) && is_defined(self.position[CSSDirection::Start as usize]) {
            return self.position[CSSDirection::Start as usize];
        } else if is_defined(self.position[K_AXIS_START[axis as usize] as usize]) {
            return self.position[K_AXIS_START[axis as usize] as usize];
        }
        VALUE_AUTO
    }

    pub fn get_end_position(&self, axis: FlexDirection) -> f32 {
        if is_row_direction(axis) && is_defined(self.position[CSSDirection::End as usize]) {
            return self.position[CSSDirection::End as usize];
        } else if is_defined(self.position[K_AXIS_END[axis as usize] as usize]) {
            return self.position[K_AXIS_END[axis as usize] as usize];
        }
        VALUE_AUTO
    }

    // ---- Border ----

    pub fn get_start_border(&self, axis: FlexDirection) -> f32 {
        if is_row_direction(axis)
            && is_defined(self.border[CSSDirection::Start as usize])
            && self.border_from[CSSDirection::Start as usize] != CSSDirection::None
        {
            return self.border[CSSDirection::Start as usize];
        }
        if is_defined(self.border[K_AXIS_START[axis as usize] as usize]) {
            return self.border[K_AXIS_START[axis as usize] as usize];
        }
        0.0
    }

    pub fn get_end_border(&self, axis: FlexDirection) -> f32 {
        if is_row_direction(axis)
            && is_defined(self.border[CSSDirection::End as usize])
            && self.border_from[CSSDirection::End as usize] != CSSDirection::None
        {
            return self.border[CSSDirection::End as usize];
        }
        if is_defined(self.border[K_AXIS_END[axis as usize] as usize]) {
            return self.border[K_AXIS_END[axis as usize] as usize];
        }
        0.0
    }

    // ---- Padding ----

    pub fn get_start_padding(&self, axis: FlexDirection) -> f32 {
        if is_row_direction(axis)
            && is_defined(self.padding[CSSDirection::Start as usize])
            && self.padding_from[CSSDirection::Start as usize] != CSSDirection::None
        {
            return self.padding[CSSDirection::Start as usize];
        } else if is_defined(self.padding[K_AXIS_START[axis as usize] as usize]) {
            return self.padding[K_AXIS_START[axis as usize] as usize];
        }
        0.0
    }

    pub fn get_end_padding(&self, axis: FlexDirection) -> f32 {
        if is_row_direction(axis)
            && is_defined(self.padding[CSSDirection::End as usize])
            && self.padding_from[CSSDirection::End as usize] != CSSDirection::None
        {
            return self.padding[CSSDirection::End as usize];
        } else if is_defined(self.padding[K_AXIS_END[axis as usize] as usize]) {
            return self.padding[K_AXIS_END[axis as usize] as usize];
        }
        0.0
    }

    // ---- Margin (axis-based) ----

    pub fn get_start_margin(&self, axis: FlexDirection) -> f32 {
        if is_row_direction(axis)
            && is_defined(self.margin[CSSDirection::Start as usize])
            && self.margin_from[CSSDirection::Start as usize] != CSSDirection::None
        {
            return self.margin[CSSDirection::Start as usize];
        }
        if is_defined(self.margin[K_AXIS_START[axis as usize] as usize]) {
            return self.margin[K_AXIS_START[axis as usize] as usize];
        }
        0.0
    }

    pub fn get_end_margin(&self, axis: FlexDirection) -> f32 {
        if is_row_direction(axis)
            && is_defined(self.margin[CSSDirection::End as usize])
            && self.margin_from[CSSDirection::End as usize] != CSSDirection::None
        {
            return self.margin[CSSDirection::End as usize];
        }
        if is_defined(self.margin[K_AXIS_END[axis as usize] as usize]) {
            return self.margin[K_AXIS_END[axis as usize] as usize];
        }
        0.0
    }

    pub fn get_margin(&self, axis: FlexDirection) -> f32 {
        self.get_start_margin(axis) + self.get_end_margin(axis)
    }

    pub fn is_auto_start_margin(&self, axis: FlexDirection) -> bool {
        if is_row_direction(axis)
            && self.margin_from[CSSDirection::Start as usize] != CSSDirection::None
        {
            return is_undefined(self.margin[CSSDirection::Start as usize]);
        }
        is_undefined(self.margin[K_AXIS_START[axis as usize] as usize])
    }

    pub fn is_auto_end_margin(&self, axis: FlexDirection) -> bool {
        if is_row_direction(axis)
            && self.margin_from[CSSDirection::End as usize] != CSSDirection::None
        {
            return is_undefined(self.margin[CSSDirection::End as usize]);
        }
        is_undefined(self.margin[K_AXIS_END[axis as usize] as usize])
    }

    pub fn is_auto_margin(&self, axis: FlexDirection) -> bool {
        self.is_auto_start_margin(axis) || self.is_auto_end_margin(axis)
    }

    // ---- Flex Basis ----

    pub fn get_flex_basis(&self) -> f32 {
        if is_defined(self.flex_basis) {
            self.flex_basis
        } else {
            VALUE_AUTO
        }
    }

    // ---- Overflow ----

    pub fn is_overflow_scroll(&self) -> bool {
        self.overflow_scroll
    }
}

// CSSDirection::Left 作为 from 数组的初始值（"未设置"）

impl FlexStyle {
    /// 创建列方向样式（与 Taitank 默认一致）
    pub fn column() -> Self {
        Self {
            flex_direction: FlexDirection::Column,
            ..Default::default()
        }
    }

    /// 创建行方向样式
    pub fn row() -> Self {
        Self {
            flex_direction: FlexDirection::Row,
            ..Default::default()
        }
    }
}
