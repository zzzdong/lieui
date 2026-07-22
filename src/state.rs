//! 共享状态系统 + 全局回调注册

use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    static REBUILD_REQUESTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static CALLBACKS: RefCell<HashMap<u64, Box<dyn Fn()>>> = RefCell::new(HashMap::new());
    static NEXT_CALLBACK_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// 手动请求重建（用于非 State 触发的场景）
pub fn request_rebuild() { REBUILD_REQUESTED.with(|r| r.set(true)); }

/// 检查并清除重建标记（内部使用）
pub(crate) fn take_rebuild_requested() -> bool {
    REBUILD_REQUESTED.with(|r| r.replace(false))
}

// ---- Callbacks ----

pub fn register_click(f: Box<dyn Fn()>) -> u64 {
    let id = NEXT_CALLBACK_ID.with(|n| { let v = n.get(); n.set(v + 1); v });
    CALLBACKS.with(|c| c.borrow_mut().insert(id, f));
    id
}

pub fn invoke_click(id: u64) {
    CALLBACKS.with(|c| {
        if let Some(f) = c.borrow_mut().remove(&id) {
            f();
        }
    });
}

pub fn clear_callbacks() {
    CALLBACKS.with(|c| c.borrow_mut().clear());
}

// ---- State ----

pub struct State<T> { inner: Rc<RefCell<T>> }
impl<T> State<T> {
    pub fn new(value: T) -> Self { Self { inner: Rc::new(RefCell::new(value)) } }
    pub fn get(&self) -> Ref<'_, T> { self.inner.borrow() }
    pub fn set(&self, value: T) { *self.inner.borrow_mut() = value; REBUILD_REQUESTED.with(|r| r.set(true)); }
    pub fn update<F: FnOnce(&mut T)>(&self, f: F) { f(&mut self.inner.borrow_mut()); REBUILD_REQUESTED.with(|r| r.set(true)); }
}
impl<T> Clone for State<T> {
    fn clone(&self) -> Self { Self { inner: Rc::clone(&self.inner) } }
}
