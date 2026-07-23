//! ElementId — 运行时元素的唯一标识 Handle
//! 基于 SlotMap 的 generational key，类比 Wayland 的 object ID。

slotmap::new_key_type! {
    /// Runtime 侧 Element 的唯一标识符。
    /// 数值型（u64），拷贝成本极低，`Copy + Send + Sync`。
    pub struct ElementId;
}

impl ElementId {
    /// 获取底层 FFI 数值（用于调试/日志/渲染场景）
    pub fn as_ffi(&self) -> u64 {
        self.0.as_ffi()
    }

    /// 从 u64 重建 ElementId
    pub fn from_u64(v: u64) -> Self {
        Self(slotmap::KeyData::from_ffi(v))
    }
}
