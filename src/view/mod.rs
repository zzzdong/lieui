//! View 模块 — `ViewNode` 数据与相关样式
//!
//! `ViewNode` 是 UI 的纯数据原语枚举；本模块不包含 Widget trait。
//! Widget 相关抽象位于顶层的 `widget` 模块。

pub mod node;
pub mod paint;

pub use node::*;
pub use paint::*;
