//! 共享状态系统 + 全局回调注册

use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::view::node::ViewNode;

use crate::event::EventContext;

thread_local! {
    static REBUILD_REQUESTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static REDRAW_REQUESTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static CALLBACKS: RefCell<HashMap<u64, ClickCallback>> = RefCell::new(HashMap::new());
    static NEXT_CALLBACK_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    static PENDING_MODAL: RefCell<Option<Option<ViewNode>>> = const { RefCell::new(None) };
    static PENDING_OVERLAY: RefCell<Option<Option<ViewNode>>> = const { RefCell::new(None) };
}

/// 点击回调类型
pub enum ClickCallback {
    /// 简单回调：执行后自动停止事件传播
    Simple(Box<dyn Fn()>),
    /// 带事件上下文的回调：由调用方决定是否停止传播
    WithCtx(Box<dyn Fn(&mut EventContext)>),
}

/// 请求全量重建（builder + reconciliation + layout + render）
pub fn request_rebuild() {
    REBUILD_REQUESTED.with(|r| r.set(true));
}

/// 检查并清除重建标记
pub(crate) fn take_rebuild_requested() -> bool {
    REBUILD_REQUESTED.with(|r| r.replace(false))
}

/// 请求仅重绘（不跑 builder/layout，只更新交互状态渲染）
/// 适用于动画、计时器、hover 等视觉变化
pub fn request_redraw() {
    REDRAW_REQUESTED.with(|r| r.set(true));
}

/// 检查并清除重绘标记
#[allow(dead_code)]
pub(crate) fn take_redraw_requested() -> bool {
    REDRAW_REQUESTED.with(|r| r.replace(false))
}

// ---- Callbacks ----

pub fn register_click(f: Box<dyn Fn()>) -> u64 {
    let id = next_callback_id();
    CALLBACKS.with(|c| c.borrow_mut().insert(id, ClickCallback::Simple(f)));
    id
}

pub fn register_click_with_ctx(f: Box<dyn Fn(&mut EventContext)>) -> u64 {
    let id = next_callback_id();
    CALLBACKS.with(|c| c.borrow_mut().insert(id, ClickCallback::WithCtx(f)));
    id
}

fn next_callback_id() -> u64 {
    NEXT_CALLBACK_ID.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    })
}

pub fn invoke_click(id: u64, ctx: &mut EventContext) {
    CALLBACKS.with(|c| {
        if let Some(cb) = c.borrow().get(&id) {
            match cb {
                ClickCallback::Simple(f) => {
                    f();
                    ctx.stop_propagation();
                }
                ClickCallback::WithCtx(f) => {
                    f(ctx);
                }
            }
        }
    });
}

pub fn clear_callbacks() {
    CALLBACKS.with(|c| c.borrow_mut().clear());
}

// ---- Modal / Overlay ----

/// 请求显示 Modal 层（下一次 frame 时生效）
pub fn show_modal(view: ViewNode) {
    PENDING_MODAL.with(|m| *m.borrow_mut() = Some(Some(view)));
    request_rebuild();
}

/// 请求隐藏 Modal 层
pub fn hide_modal() {
    PENDING_MODAL.with(|m| *m.borrow_mut() = Some(None));
    request_rebuild();
}

pub(crate) fn take_pending_modal() -> Option<Option<ViewNode>> {
    PENDING_MODAL.with(|m| m.borrow_mut().take())
}

/// 请求显示 Overlay 层（下一次 frame 时生效）
pub fn show_overlay(view: ViewNode) {
    PENDING_OVERLAY.with(|m| *m.borrow_mut() = Some(Some(view)));
    request_rebuild();
}

/// 请求隐藏 Overlay 层
pub fn hide_overlay() {
    PENDING_OVERLAY.with(|m| *m.borrow_mut() = Some(None));
    request_rebuild();
}

pub(crate) fn take_pending_overlay() -> Option<Option<ViewNode>> {
    PENDING_OVERLAY.with(|m| m.borrow_mut().take())
}

// ---- State ----

pub struct State<T> {
    inner: Rc<RefCell<T>>,
}
impl<T> State<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(RefCell::new(value)),
        }
    }
    pub fn get(&self) -> Ref<'_, T> {
        self.inner.borrow()
    }
    pub fn set(&self, value: T) {
        *self.inner.borrow_mut() = value;
        REBUILD_REQUESTED.with(|r| r.set(true));
    }
    pub fn update<F: FnOnce(&mut T)>(&self, f: F) {
        f(&mut self.inner.borrow_mut());
        REBUILD_REQUESTED.with(|r| r.set(true));
    }
}
impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}
