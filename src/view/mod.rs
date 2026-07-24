//! View 模块 — `ViewNode` 数据与 `View` trait
//!
//! `ViewNode` 是 UI 的纯数据原语枚举；所有组件（Text/Image/Container/Button...）
//! 都实现 `View` trait，由 `build()` 产出 `ViewNode`，统一位于顶层的 `widget` 模块。

pub mod node;

pub use node::*;

/// 所有 widget 均实现 `View`，通过 `build()` 把自身描述转换为原语组合。
///
/// 内置 widget（Text、Button、Column 等）和用户自定义 widget 都统一使用此 trait，
/// 不再需要单独的 `Widget` trait 或 `ViewNode::Custom` 变体。
pub trait View {
    fn build(&self) -> ViewNode;
}

impl<F> View for F
where
    F: Fn() -> ViewNode,
{
    fn build(&self) -> ViewNode {
        (self)()
    }
}

/// 已构建的 ViewNode 也可以作为 View 使用（允许 Container.child(built_node)）
impl View for ViewNode {
    fn build(&self) -> ViewNode {
        self.clone()
    }
}

pub trait ViewExt: View + Sized {
    fn key(self, key: impl Into<String>) -> KeyedView<Self> {
        KeyedView {
            inner: self,
            key: key.into(),
        }
    }
}
impl<T: View> ViewExt for T {}

pub struct KeyedView<T> {
    pub(crate) inner: T,
    pub(crate) key: String,
}
impl<T: View> View for KeyedView<T> {
    fn build(&self) -> ViewNode {
        let mut node = self.inner.build();
        node.set_key(self.key.clone());
        node
    }
}
