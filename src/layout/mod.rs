//! Layout 模块入口
//!
//! 包含两套布局系统：
//! 1. 新引擎 (types, style, flex_node, flex_line) — 参考 Taitank 的独立 Flexbox 引擎
//! 2. 旧 API (box_model, constraint, context, flex, measurable, node) — 与 ElementTree 集成的桥接层

pub mod box_model;
pub mod constraint;
pub mod context;
pub mod flex;
pub mod flex_line;
pub mod flex_node;
pub mod measurable;
pub mod node;
pub mod style;
pub mod types;

// 旧 API: 向后兼容
pub use box_model::IntrinsicSize;
pub use box_model::*;
pub use constraint::LayoutConstraint;
pub use context::LayoutContext;
pub use flex::*;
pub use measurable::*;
pub use node::LayoutNode;

// 新引擎：独立 Flexbox 引擎（仅导出无冲突的类型）
pub use flex_line::FlexLine;
pub use flex_node::FlexNode;
pub use style::FlexStyle as NewFlexStyle; // 避免与 flex::FlexStyle 冲突
pub use types::{
    float_is_equal, is_column_direction, is_defined, is_reverse_direction, is_row_direction,
    is_undefined, nan_as_inf, Dimension, Direction as LayoutDirection, DisplayType, FlexAlign,
    FlexWrap as NewFlexWrap, LayoutAction, LayoutResult, MeasureMode, NodeType as EngineNodeType,
    SizeMode, TaitankSize, K_AXIS_DIM, K_AXIS_END, K_AXIS_START, VALUE_AUTO, VALUE_UNDEFINED,
};
