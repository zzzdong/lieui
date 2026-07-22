//! 共享状态系统
//!
//! `State<T>` 是基于 `Rc<RefCell<T>>` 的共享可变状态，可在多个闭包间廉价 Clone。
//! 状态变化会自动触发页面重建标记。

use std::cell::{Ref, RefCell};
use std::rc::Rc;

thread_local! {
    /// 全局重建请求标记。
    /// 任何 `State::update()` 调用都会设置此标志。
    static REBUILD_REQUESTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 检查并清除重建标记。
pub(crate) fn take_rebuild_requested() -> bool {
    REBUILD_REQUESTED.with(|r| r.replace(false))
}

/// 共享状态句柄
///
/// 内部使用 `Rc<RefCell<T>>`，可廉价克隆并在多个闭包间共享。
/// 状态变化时自动触发页面重建。
pub struct State<T> {
    inner: Rc<RefCell<T>>,
}

impl<T> State<T> {
    /// 创建新的共享状态
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(RefCell::new(value)),
        }
    }

    /// 获取当前值的只读引用
    pub fn get(&self) -> Ref<'_, T> {
        self.inner.borrow()
    }

    /// 设置新值，自动触发重建
    pub fn set(&self, value: T) {
        *self.inner.borrow_mut() = value;
        REBUILD_REQUESTED.with(|r| r.set(true));
    }

    /// 原地修改值，自动触发重建
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
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
