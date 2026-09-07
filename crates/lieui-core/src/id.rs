//! 标识系统 —— 全部为 `Copy + 'static` 的整数句柄
//!
//! 底层统一走 `GenerationalArena`：`remove` 时 generation 自增，
//! 使所有旧句柄在 `get` 时静默返回 `None`，而不是读到被复用的新值。
//! 这是整个框架安全性的基石。

use std::fmt;

/// 节点 ID：低 32 位 index，高 32 位 generation
///
/// 用 u64 是因为节点数量在长列表 + 虚拟化场景下可能很大，且 `NodeId` 需要
/// 跨后端 / 调试工具 / 未来 FFI 传递，统一 64 位更省心。
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u64);

impl NodeId {
    /// 空句柄（永不匹配任何存活节点）
    pub const NULL: NodeId = NodeId(u64::MAX);

    #[inline]
    pub const fn index(self) -> u32 {
        self.0 as u32
    }
    #[inline]
    pub const fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }
    #[inline]
    pub const fn to_u64(self) -> u64 {
        self.0
    }
    #[inline]
    pub const fn from_u64(v: u64) -> Self {
        Self(v)
    }
    #[inline]
    pub const fn from_parts(index: u32, generation: u32) -> Self {
        Self((generation as u64) << 32 | index as u64)
    }
    #[inline]
    pub const fn is_null(self) -> bool {
        self.0 == u64::MAX
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            write!(f, "NodeId(NULL)")
        } else {
            write!(f, "NodeId({}:{})", self.index(), self.generation())
        }
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        <Self as fmt::Debug>::fmt(self, f)
    }
}

/// 声明一批结构相同的 u32 句柄
macro_rules! declare_ids {
    ($( $(#[$meta:meta])* $name:ident : $doc:literal ),* $(,)?) => {
        $(
            $(#[$meta])*
            #[doc = $doc]
            #[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
            pub struct $name(u32);

            impl $name {
                #[inline] pub const fn index(self) -> u32 { self.0 }
                #[inline] pub const fn generation(self) -> u32 { 0 }
                #[inline] pub const fn to_u64(self) -> u64 { self.0 as u64 }
                #[inline] pub const fn from_u64(v: u64) -> Self { Self(v as u32) }
                #[inline] pub const fn from_parts(index: u32, _gen: u32) -> Self { Self(index) }
            }
            impl fmt::Debug for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    write!(f, concat!(stringify!($name), "({})"), self.0)
                }
            }
        )*
    };
}

declare_ids! {
    /// 信号 ID
    SignalId: "",
    /// 派生值 ID
    MemoId: "",
    /// 副作用 ID
    EffectId: "",
    /// 响应式 Owner ID
    OwnerId: "",
    /// 窗口 ID
    WindowId: "",
    /// 事件监听器 ID
    ListenerId: "",
    /// 动画 ID
    AnimId: "",
    /// 模板 ID
    TemplateId: "",
}

/// 控件类型 ID（u16）。由 `lieui-widgets` 分配，内核只做透传。
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementTypeId(u16);

impl ElementTypeId {
    pub const ROOT: Self = Self(0);
    pub const BOX: Self = Self(1);
    pub const TEXT: Self = Self(2);
    /// 用户控件起始编号
    pub const USER_START: u16 = 1024;

    #[inline]
    pub const fn new(v: u16) -> Self {
        Self(v)
    }
    #[inline]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

impl fmt::Debug for ElementTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TypeId({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_roundtrip() {
        let id = NodeId::from_parts(7, 3);
        assert_eq!(id.index(), 7);
        assert_eq!(id.generation(), 3);
        assert_eq!(NodeId::from_u64(id.to_u64()), id);
    }
}
