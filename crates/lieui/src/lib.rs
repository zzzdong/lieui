//! lieui —— 门面 crate
//!
//! M1 阶段只重导出内核（节点树 + 属性 + 布局）。平台 / 渲染 / 响应式随后续里程碑接入。

pub use lieui_core as core;

pub mod prelude {
    pub use lieui_core::props::keys as props;
    pub use lieui_core::text_engine::{FontWeight, TextService, TextSpec};
    pub use lieui_core::{
        Color, Dimension, ElementTypeId, ItemKey, NodeId, SharedString, Window, WindowId,
    };
}
