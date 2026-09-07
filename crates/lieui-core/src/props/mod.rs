//! 属性系统 —— 列式存储 + 4 级优先层 + 继承解析

pub mod defaults;
pub mod key;
pub mod keys;
pub mod store;
pub mod value;

pub use key::{PropFlags, PropKey, PropKeyId, PropValueKind};
pub use store::{PropColumn, PropertyStore, SlotMeta, SlotRef, ValueSource, WriterKind};
pub use value::{Color, Dimension, PropValue, SharedString, TypeTag};
