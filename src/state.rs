//! 共享状态系统 + 全局回调注册

use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    static REBUILD_REQUESTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static REDRAW_REQUESTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static CALLBACKS: RefCell<HashMap<u64, Box<dyn Fn()>>> = RefCell::new(HashMap::new());
    static NEXT_CALLBACK_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// 请求全量重建（builder + reconciliation + layout + render）
pub fn request_rebuild() { REBUILD_REQUESTED.with(|r| r.set(true)); }

/// 检查并清除重建标记
pub(crate) fn take_rebuild_requested() -> bool {
    REBUILD_REQUESTED.with(|r| r.replace(false))
}

/// 请求仅重绘（不跑 builder/layout，只更新交互状态渲染）
/// 适用于动画、计时器、hover 等视觉变化
pub fn request_redraw() { REDRAW_REQUESTED.with(|r| r.set(true)); }

/// 检查并清除重绘标记
pub(crate) fn take_redraw_requested() -> bool {
    REDRAW_REQUESTED.with(|r| r.replace(false))
}

// ---- Callbacks ----

pub fn register_click(f: Box<dyn Fn()>) -> u64 {
    let id = NEXT_CALLBACK_ID.with(|n| { let v = n.get(); n.set(v + 1); v });
    CALLBACKS.with(|c| c.borrow_mut().insert(id, f));
    id
}

pub fn invoke_click(id: u64) {
    CALLBACKS.with(|c| {
        if let Some(f) = c.borrow().get(&id) {
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
