//! Flex 样式属性
//!
//! 参考 Taitank::TaitankStyle

use crate::types::*;

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
    /// 是否由引擎在渲染时自动绘制滚动条（thumb 尺寸与位置基于 layout 后的实际视口/内容尺寸计算）。
    pub show_scrollbar: bool,
    /// 显式内容宽度（滚动容器专用）。设定后引擎以此作为内容总宽而非从子节点计算。
    /// 用于 VirtualList 等虚拟化列表（真实内容 >> 窗口子节点）。
    pub content_width: Option<f32>,
    /// 显式内容高度（滚动容器专用）。同上，用于高度方向。
    pub content_height: Option<f32>,

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

impl PartialEq for FlexStyle {
    fn eq(&self, other: &Self) -> bool {
        use crate::types::float_is_equal;
        self.node_type == other.node_type
            && self.direction == other.direction
            && self.flex_direction == other.flex_direction
            && self.justify_content == other.justify_content
            && self.align_content == other.align_content
            && self.align_items == other.align_items
            && self.align_self == other.align_self
            && self.flex_wrap == other.flex_wrap
            && self.position_type == other.position_type
            && self.display_type == other.display_type
            && self.overflow_scroll == other.overflow_scroll
            && self.show_scrollbar == other.show_scrollbar
            && self.content_width == other.content_width
            && self.content_height == other.content_height
            && float_is_equal(self.flex_basis, other.flex_basis)
            && float_is_equal(self.flex_grow, other.flex_grow)
            && float_is_equal(self.flex_shrink, other.flex_shrink)
            && self
                .margin
                .iter()
                .zip(other.margin.iter())
                .all(|(a, b)| float_is_equal(*a, *b))
            && self.margin_from == other.margin_from
            && self
                .padding
                .iter()
                .zip(other.padding.iter())
                .all(|(a, b)| float_is_equal(*a, *b))
            && self.padding_from == other.padding_from
            && self
                .border
                .iter()
                .zip(other.border.iter())
                .all(|(a, b)| float_is_equal(*a, *b))
            && self.border_from == other.border_from
            && self
                .position
                .iter()
                .zip(other.position.iter())
                .all(|(a, b)| float_is_equal(*a, *b))
            && self
                .dim
                .iter()
                .zip(other.dim.iter())
                .all(|(a, b)| float_is_equal(*a, *b))
            && self
                .min_dim
                .iter()
                .zip(other.min_dim.iter())
                .all(|(a, b)| float_is_equal(*a, *b))
            && self
                .max_dim
                .iter()
                .zip(other.max_dim.iter())
                .all(|(a, b)| float_is_equal(*a, *b))
            && float_is_equal(self.item_space, other.item_space)
            && float_is_equal(self.line_space, other.line_space)
    }
}

impl Eq for FlexStyle {}

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
            show_scrollbar: false,
            content_width: None,
            content_height: None,

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
            flex_shrink: 1.0, // CSS 标准默认 1；Taitank 原默认 0 会导致 Row 溢出
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

    /// 设置 flex-basis。常用 `flex_basis(0.0)` 让 flex-grow 节点纯粹按可用空间分配尺寸，
    /// 避免其内容固有尺寸（如图片像素高度）撑大父容器导致布局抖动。
    pub fn flex_basis(mut self, b: f32) -> Self {
        self.flex_basis = b;
        self
    }

    // ---- Overflow ----

    pub fn is_overflow_scroll(&self) -> bool {
        self.overflow_scroll
    }

    /// 标记为可滚动容器：自身按视口尺寸布局，子节点在内容画布上自然排布，
    /// 由布局/渲染/命中测试层统一处理滚动偏移与裁剪。
    pub fn overflow_scroll(mut self) -> Self {
        self.overflow_scroll = true;
        self
    }

    /// 显式设置滚动容器的内容宽度（替代从子节点计算）。
    pub fn content_width(mut self, v: f32) -> Self {
        self.content_width = Some(v);
        self
    }

    /// 显式设置滚动容器的内容高度（替代从子节点计算）。
    pub fn content_height(mut self, v: f32) -> Self {
        self.content_height = Some(v);
        self
    }

    /// 启用/禁用引擎层自动渲染的滚动条（默认关闭）。
    /// 滚动条的 thumb 位置和大小基于 layout 后的实际视口/内容尺寸计算，
    /// 不受 build-time 猜测影响。需要配合 `overflow_scroll` 使用。
    pub fn scrollbar(mut self, show: bool) -> Self {
        self.show_scrollbar = show;
        self
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

    /// 块级容器（单列自上而下排列，不扩展）
    pub fn block() -> Self {
        Self::column()
    }

    pub fn width(mut self, w: f32) -> Self {
        self.dim[Dimension::Width as usize] = w;
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.dim[Dimension::Height as usize] = h;
        self
    }

    pub fn min_width(mut self, w: f32) -> Self {
        self.min_dim[Dimension::Width as usize] = w;
        self
    }

    pub fn min_height(mut self, h: f32) -> Self {
        self.min_dim[Dimension::Height as usize] = h;
        self
    }

    pub fn max_width(mut self, w: f32) -> Self {
        self.max_dim[Dimension::Width as usize] = w;
        self
    }

    pub fn max_height(mut self, h: f32) -> Self {
        self.max_dim[Dimension::Height as usize] = h;
        self
    }

    pub fn padding_all(mut self, v: f32) -> Self {
        self.set_padding(CSSDirection::All, v);
        self
    }

    pub fn padding_left(mut self, v: f32) -> Self {
        self.set_padding(CSSDirection::Left, v);
        self
    }

    pub fn padding_top(mut self, v: f32) -> Self {
        self.set_padding(CSSDirection::Top, v);
        self
    }

    pub fn padding_right(mut self, v: f32) -> Self {
        self.set_padding(CSSDirection::Right, v);
        self
    }

    pub fn padding_bottom(mut self, v: f32) -> Self {
        self.set_padding(CSSDirection::Bottom, v);
        self
    }

    /// 水平方向 padding 之和（Left + Right）。
    pub fn horizontal_padding(&self) -> f32 {
        self.padding[CSSDirection::Left as usize] + self.padding[CSSDirection::Right as usize]
    }

    /// 水平方向 border 之和（Left + Right）。
    pub fn horizontal_border(&self) -> f32 {
        self.border[CSSDirection::Left as usize] + self.border[CSSDirection::Right as usize]
    }

    pub fn margin_all(mut self, v: f32) -> Self {
        self.set_margin(CSSDirection::All, v);
        self
    }

    pub fn margin_left(mut self, v: f32) -> Self {
        self.set_margin(CSSDirection::Left, v);
        self
    }

    pub fn margin_top(mut self, v: f32) -> Self {
        self.set_margin(CSSDirection::Top, v);
        self
    }

    pub fn margin_right(mut self, v: f32) -> Self {
        self.set_margin(CSSDirection::Right, v);
        self
    }

    pub fn margin_bottom(mut self, v: f32) -> Self {
        self.set_margin(CSSDirection::Bottom, v);
        self
    }

    pub fn justify_content(mut self, j: FlexAlign) -> Self {
        self.justify_content = j;
        self
    }

    pub fn align_items(mut self, a: FlexAlign) -> Self {
        self.align_items = a;
        self
    }

    pub fn align_self(mut self, a: FlexAlign) -> Self {
        self.align_self = a;
        self
    }

    pub fn flex_grow(mut self, v: f32) -> Self {
        self.flex_grow = v;
        self
    }

    pub fn flex_shrink(mut self, v: f32) -> Self {
        self.flex_shrink = v;
        self
    }

    pub fn gap(mut self, v: f32) -> Self {
        self.item_space = v;
        self
    }

    pub fn wrap(mut self, w: FlexWrap) -> Self {
        self.flex_wrap = w;
        self
    }

    pub fn absolute(mut self) -> Self {
        self.position_type = PositionType::Absolute;
        self
    }

    pub fn position_left(mut self, v: f32) -> Self {
        self.set_position(CSSDirection::Left, v);
        self
    }

    pub fn position_top(mut self, v: f32) -> Self {
        self.set_position(CSSDirection::Top, v);
        self
    }

    pub fn position_right(mut self, v: f32) -> Self {
        self.set_position(CSSDirection::Right, v);
        self
    }

    pub fn position_bottom(mut self, v: f32) -> Self {
        self.set_position(CSSDirection::Bottom, v);
        self
    }
}
