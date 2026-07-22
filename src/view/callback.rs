//! CallbackRegistry — 跨 C/S 边界的事件回调桥接
use slotmap::SlotMap;
slotmap::new_key_type! { pub struct CallbackId; }

pub struct CallbackRegistry { callbacks: SlotMap<CallbackId, Box<dyn Fn(&[u8])>> }
impl CallbackRegistry {
    pub fn new() -> Self { Self { callbacks: SlotMap::with_key() } }
    pub fn register<F: Fn(&[u8]) + 'static>(&mut self, f: F) -> CallbackId { self.callbacks.insert(Box::new(f)) }
    pub fn invoke(&self, id: CallbackId, data: &[u8]) { if let Some(f) = self.callbacks.get(id) { f(data); } }
    pub fn clear(&mut self) { self.callbacks.clear(); }
}
impl Default for CallbackRegistry { fn default() -> Self { Self::new() } }

pub fn register_void(registry: &mut CallbackRegistry, f: impl Fn() + 'static) -> CallbackId {
    let wrapper: Box<dyn Fn(&[u8])> = Box::new(move |_: &[u8]| f());
    registry.callbacks.insert(wrapper)
}
