//! View 模块 — Widget / Primitive 全部实现 `View` trait
//!
//! 层次：
//!   primitives  — 直接映射 ViewNode 的基础块（Text, Container, Column, Row）
//!   widget/     — 由原语组合的高阶组件（Button, Checkbox, Divider）

pub mod design_tokens;
pub mod node;
pub mod primitives;
pub mod widget;

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
