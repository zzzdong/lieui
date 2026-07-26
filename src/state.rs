//! 全局状态更新信号
//!
//! 本模块只负责在单线程内协调“是否需要 rebuild/redraw”。
//! Widget state 的持久化由 `widget::BuildContext` 负责。

use crate::view::node::ViewNode;
use std::cell::{Cell, RefCell};

thread_local! {
    static REBUILD_REQUESTED: Cell<bool> = const { Cell::new(false) };
    static REDRAW_REQUESTED: Cell<bool> = const { Cell::new(false) };
    static PENDING_MODAL: RefCell<Option<Option<ViewNode>>> = const { RefCell::new(None) };
    static PENDING_OVERLAY: RefCell<Option<Option<ViewNode>>> = const { RefCell::new(None) };
}

/// 请求全量重建（builder + layout + render）
pub fn request_rebuild() {
    REBUILD_REQUESTED.with(|r| r.set(true));
}

/// 检查并清除重建标记
pub(crate) fn take_rebuild_requested() -> bool {
    REBUILD_REQUESTED.with(|r| r.replace(false))
}

/// 公开版本：检查并清除重建标记。
/// 供嵌入式驱动（自定义事件循环）与性能测试使用。
pub fn take_rebuild_requested_pub() -> bool {
    take_rebuild_requested()
}

/// 请求仅重绘（不跑 builder/layout，只更新交互状态渲染）
pub fn request_redraw() {
    REDRAW_REQUESTED.with(|r| r.set(true));
}

/// 检查并清除重绘标记
pub(crate) fn take_redraw_requested() -> bool {
    REDRAW_REQUESTED.with(|r| r.replace(false))
}

/// 显示 Modal 层。下一次 rebuild 时会将对应 ViewNode 挂载到 Modal 层。
pub fn show_modal(view: ViewNode) {
    PENDING_MODAL.with(|c| *c.borrow_mut() = Some(Some(view)));
    request_rebuild();
}

/// 隐藏 Modal 层。
pub fn hide_modal() {
    PENDING_MODAL.with(|c| *c.borrow_mut() = Some(None));
    request_rebuild();
}

/// 取走待处理的 Modal 请求（Runtime 内部使用）。
pub(crate) fn take_pending_modal() -> Option<Option<ViewNode>> {
    PENDING_MODAL.with(|c| c.borrow_mut().take())
}

/// 显示 Overlay 层。
pub fn show_overlay(view: ViewNode) {
    PENDING_OVERLAY.with(|c| *c.borrow_mut() = Some(Some(view)));
    request_rebuild();
}

/// 隐藏 Overlay 层。
pub fn hide_overlay() {
    PENDING_OVERLAY.with(|c| *c.borrow_mut() = Some(None));
    request_rebuild();
}

/// 取走待处理的 Overlay 请求（Runtime 内部使用）。
pub(crate) fn take_pending_overlay() -> Option<Option<ViewNode>> {
    PENDING_OVERLAY.with(|c| c.borrow_mut().take())
}

/// 线程局部的共享状态。
///
/// 作为起点，`State<T>` 通过 thread-local 标志触发 rebuild。后续可以迁移到
/// `BuildContext::use_state`，但当前实现与现有示例兼容。
pub struct State<T> {
    inner: std::rc::Rc<std::cell::RefCell<T>>,
}

impl<T> State<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: std::rc::Rc::new(std::cell::RefCell::new(value)),
        }
    }

    pub fn get(&self) -> std::cell::Ref<'_, T> {
        self.inner.borrow()
    }

    pub fn set(&self, value: T) {
        *self.inner.borrow_mut() = value;
        request_rebuild();
    }

    pub fn update<F: FnOnce(&mut T)>(&self, f: F) {
        f(&mut self.inner.borrow_mut());
        request_rebuild();
    }
}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self {
            inner: std::rc::Rc::clone(&self.inner),
        }
    }
}
