//! 外部消息队列 — 跨线程事件注入
//!
//! 面向"外部数据源（terminal 的 PTY 读线程、SSH 异步任务、网络等）如何把数据
//! 安全送进 UI 并触发重绘"的问题。
//!
//! 设计要点（对应重构思路）：
//! - **跨线程安全边界**：外部线程只允许发送 `Send` 的事件数据（`ExternalEvent`），
//!   不允许触碰任何 `Rc`/`RefCell`。消费在 UI 线程完成，可自由使用 Rc。
//! - **事件驱动（推）而非定时轮询（拉）**：外部线程投递数据后，通过
//!   [`wake`] 唤醒事件循环（底层 `EventLoopProxy::send_event`），
//!   无需 `Animation` 每帧轮询。
//! - **`ExternalSource::poll`**：UI 线程在事件循环收到唤醒信号后，对所有注册的
//!   外部源逐个 `poll`，把事件分发为 `rebuild` / `redraw`。

use std::any::Any;
use std::sync::{Mutex, OnceLock};

/// 由外部线程（或回调）投递到 UI 线程的事件。
///
/// 刻意不携带 `Rc`/`RefCell`——它必须能安全跨越线程边界（`Send`）。
/// 需要携带任意数据时用 [`ExternalEvent::Data`]（`Box<dyn Any + Send>`）。
#[derive(Debug)]
pub enum ExternalEvent {
    /// 触发一次 rebuild（重建 widget 树）。由 `state::request_rebuild` 消费。
    Rebuild,
    /// 触发一次仅重绘（不跑 builder）—— terminal 高频场景用这个。
    Redraw,
    /// 携带任意数据交给指定 widget 回调。字段：回调标识 + 数据。
    Data(Box<dyn Any + Send>),
}

impl ExternalEvent {
    /// 判断该事件是否请求了一次重绘（rebuild 或 redraw 都算）。
    pub fn needs_redraw(&self) -> bool {
        matches!(self, ExternalEvent::Rebuild | ExternalEvent::Redraw)
    }
}

/// 外部事件源：把外部世界的事件队列桥接到 UI 线程。
///
/// - 该 trait 的实现实例可以由 **外部线程创建**（要求 `Send`），再由 `Application`
///   在 UI 线程持有并 `poll`。
/// - `poll` 在事件循环收到唤醒信号后被调用，从内部队列取出本批事件，逐个交给 `sink`。
/// - 实现方自行持有跨线程安全的内部队列（如 `mpsc::Receiver`、`Arc<Mutex<..>>`）；
///   外部线程投递数据后应调用 [`wake`] 唤醒事件循环。
///
/// 约束说明：`Send` 保证实例可在外部线程构造。`poll` 产出的 [`ExternalEvent`] 也是
/// `Send` 的，因此"外部线程制造事件、UI 线程消费"是安全的。真正访问 UI 状态
/// （如 `Rc`/`RefCell`）的工作发生在 `poll` 的 `sink` 闭包里——该闭包由 UI 线程
/// 的集成方提供，因而可自由使用 Rc。这保持了 lieui 单线程 UI 模型不变。
pub trait ExternalSource: Send + 'static {
    /// 从内部队列取出一批事件并交给 `sink` 消费。
    fn poll(&mut self, sink: &mut dyn FnMut(ExternalEvent));
}

// ============================================================================
// 跨线程唤醒：EventLoopProxy 的单例封装
// ============================================================================

static PROXY: OnceLock<Mutex<Option<winit::event_loop::EventLoopProxy<()>>>> = OnceLock::new();

/// 设置事件循环代理（由 `Application::run` 内部调用）。
pub(crate) fn set_proxy(proxy: winit::event_loop::EventLoopProxy<()>) {
    let cell = PROXY.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap() = Some(proxy);
}

/// 从任意线程唤醒事件循环（外部数据到达时调用）。
///
/// 这是 `ExternalSource` 的外部线程侧配套：投递数据进队列后调用它，
/// 让 UI 线程从 `ControlFlow::Wait` 休眠中醒来并 `poll` 外部源。
///
/// 注意：必须在 `Application::run` 之后才有效；在此之前调用是 no-op。
pub fn wake() {
    if let Some(cell) = PROXY.get() {
        if let Some(proxy) = cell.lock().unwrap().as_ref() {
            let _ = proxy.send_event(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_event_needs_redraw() {
        assert!(ExternalEvent::Rebuild.needs_redraw());
        assert!(ExternalEvent::Redraw.needs_redraw());
        assert!(!ExternalEvent::Data(Box::new(42)).needs_redraw());
    }

    #[test]
    fn wake_before_proxy_is_noop() {
        // 未设置 proxy 前调用不应 panic。
        wake();
    }
}
