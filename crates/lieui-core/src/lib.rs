//! lieui-core —— 节点树 · 属性 · 布局阶段
//!
//! 里程碑 M1（布局）范围内的内核：
//! - `arena` / `id`：代际索引，悬垂句柄静默失效
//! - `tree`：保留式节点树
//! - `props`：列式属性存储 + 4 级优先层 + 继承路径压缩
//! - `layout`：把树 + 属性适配成 `lieui-layout` 的 `LayoutTree`
//! - `window`：P2 LAYOUT 阶段与重排边界
//!
//! 响应式（M2）、事件（M5）、渲染（P3/P4）尚未接入。

pub mod arena;
pub mod id;
pub mod layout;
pub mod props;
pub mod tree;
pub mod window;

pub use id::{ElementTypeId, NodeId, WindowId};
pub use layout::{LayoutHost, LayoutStore};
pub use props::{
    Color, Dimension, PropFlags, PropKey, PropKeyId, PropValue, PropertyStore, SharedString,
    ValueSource,
};
pub use tree::{ItemKey, Node, NodeFlags, Tree};
pub use window::Window;

pub use lieui_layout as layout_engine;
pub use lieui_text as text_engine;
