//! 共享状态系统
//!
//! 提供轻量级的 `State<T>`，用于在多个 Widget 或回调之间共享可变状态。
//! 配合 `ViewContext::bind_text` 可自动将状态同步到 Text widget，避免手动传递 `WidgetId`。

use std::cell::{Ref, RefCell};
use std::rc::Rc;

/// 共享状态句柄
///
/// 内部使用 `Rc<RefCell<T>>`，可廉价克隆并在多个闭包间共享。
/// 状态变化后可通过 `ViewContext::bind_text` 自动同步到 Text widget。
pub struct State<T> {
    inner: Rc<RefCell<StateInner<T>>>,
}

struct StateInner<T> {
    value: T,
    listeners: Vec<Rc<dyn Fn()>>,
}

impl<T> State<T> {
    /// 创建新的共享状态
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(RefCell::new(StateInner {
                value,
                listeners: Vec::new(),
            })),
        }
    }

    /// 获取当前值的只读引用
    pub fn get(&self) -> Ref<'_, T> {
        Ref::map(self.inner.borrow(), |inner| &inner.value)
    }

    /// 设置新值
    pub fn set(&self, value: T) {
        self.update(|v| *v = value);
    }

    /// 原地修改值，并在修改后通知所有监听器
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        {
            let mut inner = self.inner.borrow_mut();
            f(&mut inner.value);
        }
        self.notify();
    }

    /// 注册状态变化监听器
    pub fn on_change<F>(&self, callback: F)
    where
        F: Fn() + 'static,
    {
        self.inner.borrow_mut().listeners.push(Rc::new(callback));
    }

    fn notify(&self) {
        // 克隆监听器指针，避免调用期间持有 inner 的 borrow
        let listeners: Vec<_> = self.inner.borrow().listeners.clone();
        for cb in listeners {
            cb();
        }
    }
}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}
