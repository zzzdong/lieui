//! lieui-layout —— Taitank 风格的 Flexbox 布局引擎
//!
//! 设计约束：
//! - **只做 Flex**，不支持 Grid / 多列 / 表格布局（见设计 §3.3 的裁剪决策）。
//! - **树无关**：引擎通过 `LayoutTree` 取样式与测量叶子，不依赖 `lieui-core` 的节点树。
//! - **无外部布局依赖**：不引入 taffy / yoga，引擎本体零依赖，编译快、行为可预测。
//!
//! 一个布局 pass 的流程：
//! 1. `build_flex` 递归构造 `FlexNode` 树（复用 `main` 分支移植的 Taitank 引擎）；
//! 2. `FlexNode::layout` 求解；
//! 3. `write_layout` 写成绝对坐标的 `ComputedLayout`。

pub mod box_model;
pub mod engine;
pub mod flex_line;
pub mod flex_node;
pub mod measurable;
pub mod measure;
pub mod style;
pub mod types;

pub use box_model::{ComputedLayout, EdgeInsets, IntrinsicSize, LayoutConstraint, Rect};
pub use engine::{LayoutEngine, LayoutOutput, LayoutStats, viewport_avail};
pub use flex_line::{FlexLine, FlexSign};
pub use flex_node::FlexNode;
pub use measurable::{EmptyMeasure, FixedMeasure, Measurable};
pub use measure::LayoutTree;
pub use style::FlexStyle;
pub use types::{
    CSSDirection, Dimension, Direction as LayoutDirection, DisplayType, FlexAlign, FlexDirection,
    FlexWrap, K_AXIS_DIM, K_AXIS_END, K_AXIS_START, LayoutAction, LayoutResult, MeasureMode,
    NodeType as EngineNodeType, PositionType, SizeMode, TaitankSize, VALUE_AUTO, VALUE_UNDEFINED,
    float_is_equal, is_column_direction, is_defined, is_reverse_direction, is_row_direction,
    is_undefined, nan_as_inf,
};
