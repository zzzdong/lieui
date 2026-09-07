//! 属性键 —— 编译期常量，携带类型标签与行为标志

use std::marker::PhantomData;

use super::value::{Color, Dimension, PropValue, SharedString, TypeTag};

bitflags::bitflags! {
    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    pub struct PropFlags: u8 {
        /// 沿树继承（字号、前景色、字体族…）
        const INHERITABLE   = 1 << 0;
        /// 变化需触发重排
        const AFFECT_LAYOUT = 1 << 1;
        /// 变化需触发重绘
        const AFFECT_PAINT  = 1 << 2;
    }
}

/// 类型 → TypeTag 的编译期映射
pub trait PropValueKind: Clone + 'static {
    const TAG: TypeTag;
}
impl PropValueKind for f32 {
    const TAG: TypeTag = TypeTag::F32;
}
impl PropValueKind for i32 {
    const TAG: TypeTag = TypeTag::I32;
}
impl PropValueKind for u32 {
    const TAG: TypeTag = TypeTag::U32;
}
impl PropValueKind for bool {
    const TAG: TypeTag = TypeTag::Bool;
}
impl PropValueKind for Color {
    const TAG: TypeTag = TypeTag::Color;
}
impl PropValueKind for SharedString {
    const TAG: TypeTag = TypeTag::Str;
}
impl PropValueKind for Dimension {
    const TAG: TypeTag = TypeTag::Dim;
}

/// 类型化属性键
pub struct PropKey<T> {
    slot: u16,
    tag: TypeTag,
    flags: PropFlags,
    _ty: PhantomData<T>,
}

// 手写实现：`derive` 会因 `PhantomData<T>` 给 T 加上多余约束
impl<T> Clone for PropKey<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for PropKey<T> {}
impl<T> PartialEq for PropKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.slot == other.slot && self.tag == other.tag && self.flags == other.flags
    }
}
impl<T> Eq for PropKey<T> {}
impl<T> std::fmt::Debug for PropKey<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropKey")
            .field("slot", &self.slot)
            .field("tag", &self.tag)
            .field("flags", &self.flags)
            .finish()
    }
}

impl<T: PropValueKind> PropKey<T> {
    /// 编译期常量构造。所有内置属性的 key 都是 `pub const`。
    pub const fn new(slot: u16, flags: PropFlags) -> Self {
        Self {
            slot,
            tag: T::TAG,
            flags,
            _ty: PhantomData,
        }
    }

    #[inline]
    pub const fn slot(&self) -> u16 {
        self.slot
    }
    #[inline]
    pub const fn flags(&self) -> PropFlags {
        self.flags
    }
    #[inline]
    pub const fn tag(&self) -> TypeTag {
        self.tag
    }
    #[inline]
    pub const fn is_inheritable(&self) -> bool {
        self.flags.contains(PropFlags::INHERITABLE)
    }
    #[inline]
    pub const fn id(&self) -> PropKeyId {
        PropKeyId {
            slot: self.slot,
            tag: self.tag,
        }
    }
}

/// 无类型版本：effect / 元信息表 / 注册表用
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct PropKeyId {
    pub slot: u16,
    pub tag: TypeTag,
}

impl<T: PropValueKind> From<PropKey<T>> for PropKeyId {
    fn from(k: PropKey<T>) -> Self {
        k.id()
    }
}

/// 取出 `PropValue` 中对应 `tag` 的值（类型不匹配返回 `None`）
pub fn value_of_tag(v: &PropValue, tag: TypeTag) -> Option<PropValue> {
    let ok = matches!(
        (v, tag),
        (PropValue::F32(_), TypeTag::F32)
            | (PropValue::I32(_), TypeTag::I32)
            | (PropValue::U32(_), TypeTag::U32)
            | (PropValue::Bool(_), TypeTag::Bool)
            | (PropValue::Color(_), TypeTag::Color)
            | (PropValue::Str(_), TypeTag::Str)
            | (PropValue::Dim(_), TypeTag::Dim)
    );
    ok.then(|| v.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn const_key_carries_tag_and_flags() {
        const W: PropKey<f32> = PropKey::new(1, PropFlags::AFFECT_LAYOUT);
        assert_eq!(W.slot(), 1);
        assert_eq!(W.tag(), TypeTag::F32);
        assert!(W.flags().contains(PropFlags::AFFECT_LAYOUT));
        assert!(!W.is_inheritable());
    }
}
