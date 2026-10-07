//! 跨线程投递原语（框架级）。
//!
//! ## 为什么模块叫 `post`
//!
//! 这里两个公开类型的核心动作都叫 `post`（[`Poster::post`] / [`Waker::post`]），
//! 而"**投递 + 唤醒**"就是本模块的全部职责 —— 名字就是职责。
//!
//! 它**曾经**叫 `task`，装着 `spawn_task` / `TaskCtx` / `TaskHandle` / 任务表。
//! 那些被移除之后，名字成了历史遗留的谎：模块里一个"任务"也没有。
//!
//! ## 这个模块**不提供**"任务"，也**不提供**"遮罩"
//!
//! | 曾经的 | 为什么移除 |
//! |---|---|
//! | `spawn_task` / `TaskCtx` / `TaskHandle` / `CancelToken` / `TaskEvent` / 任务表 | **线程模型是应用的决定**：线程池 / rayon / tokio / 自己的 reactor —— 框架替不了（当初硬编码 `std::thread::spawn` 就是替错了）。取消与进度同样是调用方的协议；而一旦框架拥有消息协议，`WindowCtx::external` 就得先认一遍"这条是不是我自己发的"，一条本该**不透明**的通道就被污染了 |
//! | `BusyItem` / `BusyToken` / `begin_busy` / `reap_busy` / `set_busy_min_visible` / `overlay` 模块 | **遮罩是个组件，不是库概念**：状态归应用（`Signal<bool>`），层由应用在 `view()` 里声明，动画走通用的 `request_animation` + [`crate::Ctx::damage_key`]。"最短可见时间"就是一次 `set_timeout` —— 不值得为它开一个特例 API。而它住在核心的代价是：`RuntimeInner` 3 个字段、`Runtime` 13 个方法、`WindowCtx::frame` 每帧收尾、`next_wakeup` 被迫借 `&Runtime`（唯一用途就是问"有没有遮罩"） |
//!
//! ## 本模块提供的原语
//!
//! | 原语 | 作用 | 为什么属于 UI 库 |
//! |---|---|---|
//! | [`Poster`]（[`Runtime::poster`]） | 非 UI 线程把 `Send` 数据投递回 UI 线程并唤醒 | 它要接**事件循环**（winit 的 `EventLoopProxy` / 无头时的本地队列），只有窗口库知道怎么接 |
//!
//! ```text
//! UI 线程                                  工作线程（调用方自己的）
//! ────────                                 ──────────────────────
//! let poster = rt.poster();   ← 可 Send 的句柄 ──→  move 进线程
//!                                                  poster.post(win, MyMsg::Progress(3, 9));
//!                                                  poster.post(win, MyMsg::Done(summary));
//!                          ↓ 收到 Done（UI 线程）
//! ViewModel::on_external: data.downcast::<MyMsg>()  ⇒  改自己的状态
//! ```
//!
//! 遮罩之类的东西**建立在它之上**，但归应用 —— 可跑的参考实现见
//! `tests/spinner_modal.rs`（只用公开 API）与 `examples/background_task.rs`。
//!
//! ## 平台无关（可无头测试）
//!
//! [`Waker`] 是一个 trait：winit 平台用 [`crate::platform::RepaintHandle`] 实现它；
//! 没有平台时（无头测试、`App::frame_all` 驱动）投递退化成**本地队列**
//! （[`Runtime::take_pending_external`] 取出来手动喂给 `WindowCtx::external`）。
//! 测试因此可以跑通"工作线程 → 投递 → 落地"的完整闭环，不需要真窗口。
//!
//! ## A6：句柄的**稳定身份**
//!
//! [`Poster`] 会在 [`Runtime::set_waker`] **之前**就被 clone 出去（构造期起的线程）。
//! 若句柄存的是"当时的唤醒器快照"，平台 waker 后装入它就**永远看不见**
//! ⇒ 消息积压、无人消费、**界面毫无反应且零报错**。
//!
//! 所以句柄指向 `Arc<PostHub>`：**槽位本身是共享的**，装入立刻可见 ——
//! 这类问题从"打补丁"变成"结构上不可能"。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::app::ExternalData;
use crate::reactive::Runtime;
use crate::window::WindowId;

// ───────────────────────── Waker ─────────────────────────

/// 「唤醒 UI 线程」的平台无关句柄：`Send + Sync` ⇒ 可以 move 进工作线程。
///
/// 实现者：winit 平台的 `RepaintHandle`（`EventLoopProxy`）。
pub trait Waker: Send + Sync + 'static {
    /// 只唤醒（跑一帧，不带数据）。返回 `false` = 事件循环已退出。
    fn wake(&self) -> bool;

    /// 投递数据到某个窗口并唤醒。返回 `false` = 事件循环已退出（调用方应尽快收敛）。
    fn post(&self, window: WindowId, data: ExternalData) -> bool;
}

/// 无平台时的本地队列（`Mutex` ⇒ 可被工作线程直接用）。
#[derive(Default)]
struct LocalQueue {
    items: Mutex<VecDeque<(WindowId, ExternalData)>>,
}

impl LocalQueue {
    fn push(&self, window: WindowId, data: ExternalData) {
        self.items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back((window, data));
    }

    fn take(&self) -> Vec<(WindowId, ExternalData)> {
        self.items.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect()
    }
}

/// 投递中心：**本地队列 + 后装入的平台唤醒器**，共享给所有 [`Poster`]。
///
/// 把这两样放进**同一个 `Arc`** 是 A6 的**结构性修复**（见 [`Poster`] 的文档）：
/// 句柄 clone 出去之后，平台 waker 后装入也**立刻对它们可见** ——
/// 不再需要"任务持有唤醒器快照 + 事后 retrofit"那套补丁。
#[derive(Default)]
struct PostHub {
    queue: LocalQueue,
    platform: Mutex<Option<Arc<dyn Waker>>>,
}

/// **跨线程投递句柄**：`Send + Sync + Clone` —— 非 UI 线程的入口。
///
/// ## 为什么需要它（而不是直接用 `Runtime`）
///
/// [`Runtime`] 是 `Rc<RuntimeInner>` ⇒ **`!Send`**，工作线程根本拿不到它。
/// 此前"从别的线程投递数据"只能靠框架给的 `TaskCtx` 才做得到 —— 于是框架
/// **替调用方定了线程模型**（`std::thread::spawn` 硬编码）、取消协议、进度协议，
/// 还把消息信封塞进了这条本该**不透明**的通道（`WindowCtx::external` 得先认
/// 一遍是不是框架自己发的）。
///
/// `Poster` 公开之后，调用方可以用**任何**并发模型（线程池 / rayon / tokio /
/// 自己的 reactor）投递结果，框架**不需要知道"任务"这回事**。
///
/// ## 稳定身份：A6 的结构性修复
///
/// 句柄会在 `Runtime::set_waker` **之前**就 clone 出去（调用方自己起的线程、
/// ViewModel 构造期的预热）。若句柄里存的是"当时的唤醒器快照"，平台 waker
/// 后装入它就**永远看不见** ⇒ 消息积压、无人消费、**界面毫无反应且零报错**。
///
/// 所以句柄指向 `Arc<PostHub>` —— **槽位本身是共享的**，装入立刻可见。
///
/// ```ignore
/// // UI 线程：把句柄交给工作线程（`Runtime` 自己进不了线程）
/// let poster = rt.poster();
/// std::thread::spawn(move || {
///     let report = heavy_work();
///     poster.post(win, report);       // ← 只需要这一句
/// });
/// ```
#[derive(Clone)]
pub struct Poster(Arc<PostHub>);

impl Default for Poster {
    fn default() -> Self {
        Self(Arc::new(PostHub::default()))
    }
}

impl Poster {
    /// 只唤醒（不带数据）。
    ///
    /// 无头模式下是 no-op（返回 `true`）—— 消息已入本地队列，由
    /// [`Runtime::take_pending_external`] 消费。
    pub fn wake(&self) -> bool {
        match self.platform_waker() {
            Some(w) => w.wake(),
            None => true,
        }
    }

    /// **投递一条 `Send` 数据到某窗口并唤醒 UI 线程**（非 UI 线程的入口）。
    ///
    /// 返回 `false` = 事件循环已退出（调用方应尽快收敛，别再往这里投）。
    /// 落地侧：UI 线程在 `WindowCtx::external` 里交给 `ViewModel::on_external`，
    /// 用 [`ExternalData::downcast`] 取回自己的类型。
    pub fn post<T: Send + 'static>(&self, window: WindowId, msg: T) -> bool {
        self.post_external(window, ExternalData::new(msg))
    }

    /// [`Self::post`] 的低层形式：已构造好的 [`ExternalData`]。
    pub fn post_external(&self, window: WindowId, data: ExternalData) -> bool {
        // 有平台唤醒器就直接交给它（由它投递并唤醒）。
        // `post` 消耗 `data`，所以不能"失败后退回本地队列"—— 它返回 `false`
        // 的语义本来就是"事件循环已退出，调用方尽快收敛"，丢弃是正确行为。
        //
        // ★ 先把 `Arc` clone 出来再调用：`Waker::post` 是**外部实现**，
        //   持锁调它有重入死锁风险。
        match self.platform_waker() {
            Some(w) => w.post(window, data),
            None => {
                self.0.queue.push(window, data);
                true
            }
        }
    }

    /// 平台唤醒器（无头 / 未进入事件循环时是 `None`）
    pub fn platform_waker(&self) -> Option<Arc<dyn Waker>> {
        self.0.platform.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 取走本地队列（无头 / 平台切换的窗口期；有平台之后通常为空）
    pub(crate) fn take_local(&self) -> Vec<(WindowId, ExternalData)> {
        self.0.queue.take()
    }

    /// 装入平台唤醒器，并把**已积压**的消息转交（否则它们留在队列里没人要）。
    ///
    /// 返回 `true` = 转交了积压消息（它们已由平台接管）。
    pub(crate) fn attach_platform(&self, waker: Arc<dyn Waker>) -> bool {
        *self.0.platform.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&waker));
        let pending = self.0.queue.take();
        if pending.is_empty() {
            return false;
        }
        for (win, data) in pending {
            waker.post(win, data);
        }
        true
    }
}

// ───────────────────────── Runtime 扩展 ─────────────────────────

impl Runtime {
    // ── 唤醒器 ──

    /// 注入平台唤醒器（`platform::run` 在进入事件循环时调用；此后所有投递走平台）。
    ///
    /// ★ 装入的是**共享槽位**（[`Poster`] 里的 `Arc<PostHub>`），所以
    /// **在这之前就 clone 出去的投递句柄也会立刻看见平台 waker** ——
    /// 这就是 A6（"`run()` 之前起的线程消息积压、界面毫无反应且零报错"）的
    /// **结构性修复**：不再有"句柄持有旧快照"这回事。
    ///
    /// 顺带把**已积压**的消息转交给平台（否则它们留在本地队列里没人要）。
    pub fn set_waker(&self, waker: Arc<dyn Waker>) {
        self.inner.poster.attach_platform(waker);
    }

    /// 当前平台唤醒器（无头 / 未进入事件循环时是 `None`）
    pub fn waker(&self) -> Option<Arc<dyn Waker>> {
        self.inner.poster.platform_waker()
    }

    /// 事件循环是否在线（有平台唤醒器）
    pub fn is_online(&self) -> bool {
        self.waker().is_some()
    }

    /// 拿一份**可 `Send` 的投递句柄** —— 交给工作线程用。
    ///
    /// ★ 这是"跨线程通信"的入口，**不需要任何任务概念**：
    ///
    /// ```ignore
    /// let poster = rt.poster();               // UI 线程
    /// std::thread::spawn(move || {            // `Runtime` 自己是 `!Send`，进不了线程
    ///     poster.post(win, heavy_work());     // 投递 + 唤醒
    /// });
    /// ```
    ///
    /// 句柄可以任意 `clone`（`Arc`），并且**先 clone、后装平台 waker** 也有效。
    pub fn poster(&self) -> Poster {
        self.inner.poster.clone()
    }

    /// 唤醒 UI 线程跑一帧（"我改了点东西，来看看"）。
    ///
    /// 无头模式下是 no-op（返回 `true`）——数据仍会进本地队列，由
    /// [`Self::take_pending_external`] 消费。
    pub fn wake(&self) -> bool {
        self.inner.poster.wake()
    }

    /// 取走本地投递队列（无头 / 测试用；有平台唤醒器时通常为空）。
    ///
    /// 典型用法：`for (win, data) in rt.take_pending_external() { app.window_ctx_mut(win)?.external(&rt, data) }`
    pub fn take_pending_external(&self) -> Vec<(WindowId, ExternalData)> {
        self.inner.poster.take_local()
    }
}

#[cfg(test)]
#[path = "post_tests.rs"]
mod tests;
