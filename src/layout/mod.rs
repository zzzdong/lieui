//! Layout 模块入口
//!
//! 以 Taitank 风格 FlexNode 引擎作为唯一布局系统。
//! ViewNode 直接提供确定好的 FlexStyle，本模块负责把 ElementTree 转成 FlexNode 树并计算布局。

pub mod box_model;
pub mod constraint;
pub mod context;
pub mod flex_line;
pub mod flex_node;
pub mod measurable;
pub mod style;
pub mod types;

pub use box_model::*;
pub use constraint::LayoutConstraint;
pub use context::LayoutContext;
pub use measurable::*;

// Taitank 风格 Flexbox 引擎
pub use flex_line::FlexLine;
pub use flex_node::FlexNode;
pub use style::FlexStyle;
pub use types::{
    float_is_equal, is_column_direction, is_defined, is_reverse_direction, is_row_direction,
    is_undefined, nan_as_inf, CSSDirection, Dimension, Direction as LayoutDirection, DisplayType,
    FlexAlign, FlexDirection, FlexWrap, LayoutAction, LayoutResult, MeasureMode,
    NodeType as EngineNodeType, PositionType, SizeMode, TaitankSize, K_AXIS_DIM, K_AXIS_END,
    K_AXIS_START, VALUE_AUTO, VALUE_UNDEFINED,
};
