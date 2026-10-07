//! lieui-layout —— Taitank 风格 Flexbox 布局引擎
//!
//! 从 `feature/mvp` 的 `src/layout/` 抽出。相对原版的改动（见 docs/operation-log.md M0）：
//! 1. `crate::layout::x` 路径改为 crate 内 `crate::x`；
//! 2. `crate::geometry::Rect` → `lieui_geom::Rect`；
//! 3. `crate::text::TextEngine` / `crate::view::paint::TextStyle` → `lieui_text::{TextEngine, TextSpec}`；
//! 4. `FlexStyle::scroll_state` 与 `bind_scroll_state` 删除（v3 的滚动状态归 `Kind::Scroll` 节点）；
//! 5. **不包含** `context.rs`：它把旧 `ElementTree` 编译成 `FlexNode`，v3 改为在 M2 写新的适配器。

pub mod box_model;
pub mod constraint;
pub mod flex_line;
pub mod flex_node;
pub mod measurable;
pub mod style;
pub mod types;

pub use box_model::*;
pub use constraint::LayoutConstraint;
pub use flex_line::FlexLine;
pub use flex_node::FlexNode;
pub use measurable::*;
pub use style::FlexStyle;
pub use types::{
    CSSDirection, Dimension, Direction as LayoutDirection, DisplayType, FlexAlign, FlexDirection, FlexWrap, K_AXIS_DIM,
    K_AXIS_END, K_AXIS_START, LayoutAction, LayoutResult, MeasureMode, NodeType as EngineNodeType, PositionType,
    SizeMode, TaitankSize, VALUE_AUTO, VALUE_UNDEFINED, float_is_equal, is_column_direction, is_defined,
    is_reverse_direction, is_row_direction, is_undefined, nan_as_inf,
};
