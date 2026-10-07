//! 后台任务与跨线程通信（框架级）。
//!
//! ## 为什么需要
//!
//! GUI 的耗时活儿（读盘、解析、网络、压缩……）不能占着 UI 线程跑，否则窗口僵死。
//! 但 `Signal` / `Runtime` / `Ctx` 全是 `!Send`（`Rc` + `RefCell`），后台线程碰不得。
//! 唯一安全的姿势是**投递 + 唤醒**：线程把 `Send` 数据交给 [`Waker`]，UI 线程在
//! 帧循环里落地。
//!
//! 本模块把这条链路做完整：
//!
//! ```text
//! UI 线程                                 工作线程
//! ────────                                ────────
//! rt.spawn_task_busy(win, "正在导出…", |ctx| {   ← 起线程 + 挂 loading 遮罩
//!                                             for i in 0..n {
//!                                                 if ctx.is_cancelled() { break }
//!                                                 ctx.progress(i, n);      → post
//!                                             }
//!                                             Ok(summary)                  ← 返回值
//!                                         })
//!                                             ↓ 完成时框架自动投递
//! WindowCtx::external → on_task_message       TaskEvent { id, payload }
//!   ├─ 收起遮罩 / 清任务表（框架收尾）
//!   └─ 继续交给 ViewModel::on_external（用户 downcast 自己的类型）
//! ```
//!
//! ## 三层 API（按需要选）
//!
//! | 想要 | 用 |
//! |---|---|
//! | 只要"随时唤醒/投递" | [`Runtime::waker`] / [`Runtime::wake`] |
//! | 起个后台任务，完成回传结果 | [`Runtime::spawn_task`] |
//! | 再要一个 loading 遮罩 | [`Runtime::spawn_task_busy`] 或 [`Runtime::begin_busy`] |
//!
//! ## 平台无关（可无头测试）
//!
//! [`Waker`] 是一个 trait：winit 平台用 [`crate::platform::RepaintHandle`] 实现它；
//! 没有平台时（无头测试、`App::frame_all` 驱动）`Runtime` 退化成**本地队列**
//! （[`Runtime::take_pending_external`] 取出来手动喂给 `WindowCtx::external`）。
//! 测试因此可以跑通"任务 → 投递 → 落地"的完整闭环，不需要真窗口。

use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::app::ExternalData;
use crate::reactive::{Dirty, Runtime};
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

/// 无平台时的本地队列（`Arc<Mutex<..>>` ⇒ 可直接被工作线程使用）。
#[derive(Default)]
pub(crate) struct LocalQueue {
    items: Mutex<VecDeque<(WindowId, ExternalData)>>,
    /// 平台 waker 的转发目标（[`Runtime::set_waker`] 注入）。
    ///
    /// ## 为什么需要它（A6）
    ///
    /// `TaskCtx` 持有的是 **spawn 时**的 `WakerSlot` **快照**，而平台 waker 是
    /// 在 `run()` 里才注入的 ⇒ **在 `run()` 之前 spawn 的任务**（典型：ViewModel
    /// 构造期起预加载 / 预热）永远持有 `Local` 槽位。
    ///
    /// 光靠"平台 tick 里 drain 本地队列"只能保证消息**不丢**，但它**唤不醒**事件循环
    /// （`Local` 的 `wake()` 是 no-op）⇒ 任务完成后要等到下一次真实输入才被处理。
    ///
    /// 所以 `set_waker` 会把平台 waker **装进这个已被共享出去的队列**里，
    /// 于是这些"拿着旧快照的任务"也能把消息**与唤醒**转发给平台。
    forward: Mutex<Option<Arc<dyn Waker>>>,
}

impl LocalQueue {
    fn push(&self, window: WindowId, data: ExternalData) {
        // 有平台 waker 就直接交给它（顺带由它唤醒 UI）。
        // `post` 消耗 `data`，所以不能"失败后退回本地队列"—— 而 `post` 返回 `false`
        // 的语义本来就是"事件循环已退出，调用方尽快收敛"，丢弃是正确行为。
        let fwd = self.forward.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(w) = fwd {
            let _ = w.post(window, data);
            return;
        }
        self.items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back((window, data));
    }

    fn take(&self) -> Vec<(WindowId, ExternalData)> {
        self.items.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect()
    }

    /// 装上平台 waker 并**转走**已积压的消息（否则它们永远留在队列里）。
    ///
    /// 返回 `true` 表示成功转交（这些消息已由平台接管，不需要 UI 再消费）。
    fn attach_platform(&self, waker: Arc<dyn Waker>) -> bool {
        let pending = {
            let mut slot = self.forward.lock().unwrap_or_else(|e| e.into_inner());
            *slot = Some(Arc::clone(&waker));
            self.take()
        };
        if pending.is_empty() {
            return false;
        }
        for (win, data) in pending {
            waker.post(win, data);
        }
        true
    }
}

/// 唤醒器槽位：平台注入的，或本地队列（无头）。
#[derive(Clone)]
pub(crate) enum WakerSlot {
    Local(Arc<LocalQueue>),
    Platform(Arc<dyn Waker>),
}

impl Default for WakerSlot {
    fn default() -> Self {
        Self::Local(Arc::new(LocalQueue::default()))
    }
}

impl WakerSlot {
    pub(crate) fn wake(&self) -> bool {
        match self {
            // 本地模式没有事件循环可唤醒：投递后由 `take_pending_external` 消费
            WakerSlot::Local(_) => true,
            WakerSlot::Platform(w) => w.wake(),
        }
    }

    pub(crate) fn post(&self, window: WindowId, data: ExternalData) -> bool {
        match self {
            WakerSlot::Local(q) => {
                q.push(window, data);
                true
            }
            WakerSlot::Platform(w) => w.post(window, data),
        }
    }
}

// ───────────────────────── 取消 ─────────────────────────

/// 取消令牌（`Clone + Send`）：任务内部（或它调用的库）轮询用。
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// 置为已取消（幂等）
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl std::fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CancelToken({})", self.is_cancelled())
    }
}

// ───────────────────────── 任务句柄（工作线程侧）─────────────────────────

/// 任务内部句柄：投递消息 / 上报进度 / 查取消。`Send + Clone` ⇒ 可以再分给子线程。
#[derive(Clone)]
pub struct TaskCtx {
    id: u64,
    window: WindowId,
    waker: WakerSlot,
    cancel: CancelToken,
}

impl TaskCtx {
    /// 任务 id（与 [`TaskEvent::id`] 对应）
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn window(&self) -> WindowId {
        self.window
    }

    /// 投递任意 `Send` 消息给 UI 线程：`ViewModel::on_external` 里 `downcast` 取。
    ///
    /// 返回 `false` = UI 线程已退出（事件循环结束）⇒ 任务应尽快收敛。
    pub fn post<T: Send + 'static>(&self, msg: T) -> bool {
        self.waker.post(self.window, ExternalData::new(msg))
    }

    /// 上报进度：框架**直接消费**（更新该任务的 loading 遮罩），不会传给 `on_external`。
    ///
    /// 任务没挂遮罩时是 no-op（仍返回唤醒是否成功）。
    pub fn progress(&self, done: usize, total: usize) -> bool {
        self.waker.post(
            self.window,
            ExternalData::new(TaskProgress {
                id: self.id,
                done,
                total,
                detail: None,
            }),
        )
    }

    /// 上报进度 + **明细文案**（一条消息、一次唤醒）。
    ///
    /// 遮罩会把 `detail` 显示在进度条下方（替代默认的 `done / total`），适合
    /// "第 2 / 3 个文件 · 正在合并 b.pdf" 这类人话；`done`/`total` 可以比"文件数"
    /// 更细（例如把每个文件拆成读盘/解析/合并三格），进度条因此**在文件内部也会走**。
    pub fn progress_with(&self, done: usize, total: usize, detail: impl Into<String>) -> bool {
        self.waker.post(
            self.window,
            ExternalData::new(TaskProgress {
                id: self.id,
                done,
                total,
                detail: Some(detail.into()),
            }),
        )
    }

    /// 是否已被取消（用户点了取消按钮 / 窗口关闭 / 应用退出）
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// 取消令牌：传给任务内部更深的库（它们不认识 `TaskCtx`）
    pub fn cancel_token(&self) -> CancelToken {
        self.cancel.clone()
    }

    /// 只唤醒 UI 线程跑一帧（不带数据）
    pub fn wake(&self) -> bool {
        self.waker.wake()
    }
}

impl std::fmt::Debug for TaskCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskCtx")
            .field("id", &self.id)
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

// ───────────────────────── 任务句柄（UI 线程侧）─────────────────────────

/// UI 线程拿到的任务句柄（`!Send`：持有 `Runtime`）。
pub struct TaskHandle {
    rt: Runtime,
    id: u64,
    cancel: CancelToken,
    done: Arc<AtomicBool>,
}

impl TaskHandle {
    pub fn id(&self) -> u64 {
        self.id
    }

    /// 请求取消：任务里 `TaskCtx::is_cancelled()` 会变 `true`（协作式，不强杀线程）。
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// 任务体是否已跑完（不含"结果已落地"）
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// 该任务是否还挂在运行时（完成后由框架移除）
    pub fn is_running(&self) -> bool {
        self.rt.inner.tasks.borrow().iter().any(|t| t.id == self.id)
    }
}

impl std::fmt::Debug for TaskHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskHandle")
            .field("id", &self.id)
            .field("done", &self.is_done())
            .finish_non_exhaustive()
    }
}

// ───────────────────────── 框架内部消息 ─────────────────────────

/// 任务完成事件：框架先做收尾（清任务表 + 收遮罩），**再**把它交给 `ViewModel::on_external`。
///
/// 用户在 `on_external` 里按自己的返回类型取载荷：
///
/// ```ignore
/// fn on_external(self: &Rc<Self>, _cx: &mut Ctx, data: ExternalData) {
///     if let Some(ev) = data.downcast::<TaskEvent>() {
///         if let Some(r) = ev.payload.downcast::<MyReport>() {
///             self.report.set(r);
///         }
///     }
/// }
/// ```
pub struct TaskEvent {
    /// 任务 id（见 [`TaskCtx::id`] / [`TaskHandle::id`]）
    pub id: u64,
    /// 任务闭包返回值的类型擦除载荷
    pub payload: ExternalData,
}

impl std::fmt::Debug for TaskEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TaskEvent {{ id: {} }}", self.id)
    }
}

/// 任务**崩了**（工作线程 panic）：框架照常收尾（清任务表 + 收遮罩），
/// 也会把它交给 `on_external`（用户想记日志就 `downcast::<TaskFailed>()`）。
///
/// 没有这条兜底的话，panic 的任务会让遮罩永远挂在屏幕上（任务永远不会"完成"）。
pub struct TaskFailed {
    /// 任务 id
    pub id: u64,
}

impl std::fmt::Debug for TaskFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TaskFailed {{ id: {} }}", self.id)
    }
}

/// 进度消息（框架消费，不交给用户）
pub(crate) struct TaskProgress {
    pub(crate) id: u64,
    pub(crate) done: usize,
    pub(crate) total: usize,
    /// `Some` ⇒ 同时更新遮罩的明细文案（见 `TaskCtx::progress_with`）
    pub(crate) detail: Option<String>,
}

// ───────────────────────── 运行时内部状态 ─────────────────────────

/// 任务表条目（UI 线程侧）
pub(crate) struct TaskRecord {
    pub(crate) id: u64,
    pub(crate) window: WindowId,
    pub(crate) cancel: CancelToken,
    /// 该任务挂的遮罩项（完成时自动收起）
    pub(crate) busy: Option<u64>,
}

/// 一个「忙碌」项 = loading 遮罩上的一行（同窗口多项时取最新的文案，并标出还有几个）。
///
/// `Runtime::busy_items` 返回它的快照 ⇒ 用户可以**自己渲染遮罩**（框架的默认遮罩
/// 见 `crate::overlay`）。
pub struct BusyItem {
    pub id: u64,
    pub window: WindowId,
    /// 文案（`BusyToken::set_label` 可改）
    pub label: String,
    /// 进度明细（`Some` ⇒ 遮罩显示它，而不是默认的 `done / total`；
    /// 见 `TaskCtx::progress_with` / `BusyToken::set_detail`）
    pub detail: Option<String>,
    /// 确定进度 `(done, total)`；`None` = 不确定（画动画）
    pub progress: Option<(usize, usize)>,
    /// 点「取消」时的回调（UI 线程执行）
    pub cancel: Option<Rc<dyn Fn()>>,
    /// 这一项是什么时候挂上的（最短可见时间从它算起）
    pub(crate) since: Instant,
    /// `Some(t)` ⇒ 忙碌段已结束，但为了"最短可见时间"留到 `t` 再收（见
    /// [`Runtime::set_busy_min_visible`]）
    pub(crate) hide_at: Option<Instant>,
}

impl Clone for BusyItem {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            window: self.window,
            label: self.label.clone(),
            detail: self.detail.clone(),
            progress: self.progress,
            cancel: self.cancel.clone(),
            since: self.since,
            hide_at: self.hide_at,
        }
    }
}

impl BusyItem {
    /// 进度比例（`None` = 不确定进度 ⇒ 画动画）
    pub fn ratio(&self) -> Option<f32> {
        self.progress.map(|(d, t)| {
            if t == 0 {
                0.0
            } else {
                (d as f32 / t as f32).clamp(0.0, 1.0)
            }
        })
    }

    pub fn is_cancellable(&self) -> bool {
        self.cancel.is_some()
    }
}

impl std::fmt::Debug for BusyItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BusyItem")
            .field("id", &self.id)
            .field("window", &self.window)
            .field("label", &self.label)
            .field("detail", &self.detail)
            .field("progress", &self.progress)
            .field("cancellable", &self.is_cancellable())
            .finish()
    }
}

// ───────────────────────── Runtime 扩展 ─────────────────────────

impl Runtime {
    // ── 唤醒器 ──

    /// 注入平台唤醒器（`platform::run` 在进入事件循环时调用；此后所有投递走平台）。
    ///
    /// ★ 同时把平台 waker **装进旧的本地队列**（A6）：`TaskCtx` 持有的是 spawn 时的
    /// `WakerSlot` **快照**，所以在 `run()` **之前** spawn 的任务（典型：ViewModel 构造期
    /// 起预加载）手里的槽位永远是 `Local`。不给它装转发器的话，这些任务完成后
    /// 既不会唤醒事件循环、消息也只在无人 drain 的队列里堆积 ⇒ **界面毫无反应且零报错**。
    ///
    /// 装转发器时顺带把**已积压**的消息转交给平台（否则它们留在队列里没人要）。
    pub fn set_waker(&self, waker: Arc<dyn Waker>) {
        let old = std::mem::replace(
            &mut *self.inner.waker.borrow_mut(),
            WakerSlot::Platform(Arc::clone(&waker)),
        );
        if let WakerSlot::Local(q) = old {
            q.attach_platform(waker);
        }
    }

    /// 当前平台唤醒器（无头 / 未进入事件循环时是 `None`）
    pub fn waker(&self) -> Option<Arc<dyn Waker>> {
        match &*self.inner.waker.borrow() {
            WakerSlot::Local(_) => None,
            WakerSlot::Platform(w) => Some(Arc::clone(w)),
        }
    }

    /// 事件循环是否在线（有平台唤醒器）
    pub fn is_online(&self) -> bool {
        self.waker().is_some()
    }

    /// 唤醒 UI 线程跑一帧（"我改了点东西，来看看"）。
    ///
    /// 无头模式下是 no-op（返回 `true`）——数据仍会进本地队列，由
    /// [`Self::take_pending_external`] 消费。
    pub fn wake(&self) -> bool {
        self.inner.waker.borrow().wake()
    }

    /// 取走本地投递队列（无头 / 测试用；有平台唤醒器时恒为空）。
    ///
    /// 典型用法：`for (win, data) in rt.take_pending_external() { app.window_ctx_mut(win)?.external(&rt, data) }`
    pub fn take_pending_external(&self) -> Vec<(WindowId, ExternalData)> {
        match &*self.inner.waker.borrow() {
            WakerSlot::Local(q) => q.take(),
            WakerSlot::Platform(_) => Vec::new(),
        }
    }

    // ── 任务 ──

    /// 起一个后台任务：`work` 在工作线程跑，返回值 `T` 由框架投递给
    /// `ViewModel::on_external`（包在 [`TaskEvent`] 里）。
    ///
    /// 任务结束（成功或 panic 之外的路径）时框架清理任务表；窗口关闭会**自动取消**
    /// 该窗口的所有任务（`TaskCtx::is_cancelled()` 变 true）。
    ///
    /// ```ignore
    /// let h = rt.spawn_task(win, |ctx| {
    ///     ctx.post(FormatStarted);
    ///     Ok::<_, ()>(42)
    /// });
    /// ```
    pub fn spawn_task<T, F>(&self, window: WindowId, work: F) -> TaskHandle
    where
        T: Send + 'static,
        F: FnOnce(TaskCtx) -> T + Send + 'static,
    {
        self.spawn_task_inner(window, None, work)
    }

    /// 同 [`Self::spawn_task`]，但**同时挂一个 loading 遮罩**（`label` 为文案）；
    /// 任务完成时遮罩自动收起（无需手动 `BusyToken`）。
    ///
    /// 任务里用 [`TaskCtx::progress`] 上报进度即可驱动遮罩上的进度条。
    pub fn spawn_task_busy<T, F>(&self, window: WindowId, label: impl Into<String>, work: F) -> TaskHandle
    where
        T: Send + 'static,
        F: FnOnce(TaskCtx) -> T + Send + 'static,
    {
        self.spawn_task_inner(window, Some(label.into()), work)
    }

    fn spawn_task_inner<T, F>(&self, window: WindowId, busy_label: Option<String>, work: F) -> TaskHandle
    where
        T: Send + 'static,
        F: FnOnce(TaskCtx) -> T + Send + 'static,
    {
        let id = {
            let n = self.inner.next_task_id.get() + 1;
            self.inner.next_task_id.set(n);
            n
        };
        let cancel = CancelToken::new();
        let done = Arc::new(AtomicBool::new(false));
        let waker = self.inner.waker.borrow().clone();

        // 挂遮罩（若有）：忙碌项由任务表持有 ⇒ 完成时自动收起
        let busy = busy_label.map(|label| {
            let bid = {
                let n = self.inner.next_task_id.get() + 1;
                self.inner.next_task_id.set(n);
                n
            };
            // 遮罩自带的「取消」按钮 = 取消这个任务（协作式：任务里轮询
            // `TaskCtx::is_cancelled`，看到就尽快收敛）
            let cancel_from_ui = cancel.clone();
            self.inner.busy.borrow_mut().push(BusyItem {
                id: bid,
                window,
                label,
                detail: None,
                progress: None,
                cancel: Some(Rc::new(move || cancel_from_ui.cancel())),
                since: Instant::now(),
                hide_at: None,
            });
            bid
        });

        self.inner.tasks.borrow_mut().push(TaskRecord {
            id,
            window,
            cancel: cancel.clone(),
            busy,
        });
        self.mark(window, Dirty::VIEW | Dirty::PRESENT);

        let ctx = TaskCtx {
            id,
            window,
            waker: waker.clone(),
            cancel: cancel.clone(),
        };
        let done_flag = Arc::clone(&done);
        std::thread::spawn(move || {
            // panic 兜底：任务体崩了也要走"收尾"这条唯一路径（否则遮罩永远挂在屏幕上）
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(ctx)));
            done_flag.store(true, Ordering::SeqCst);
            let msg = match result {
                Ok(v) => ExternalData::new(TaskEvent {
                    id,
                    payload: ExternalData::new(v),
                }),
                Err(_) => ExternalData::new(TaskFailed { id }),
            };
            let _ = waker.post(window, msg);
        });

        TaskHandle {
            rt: self.clone(),
            id,
            cancel,
            done,
        }
    }

    /// 取消某窗口的全部任务（窗口关闭 / 应用退出时调用）
    pub(crate) fn cancel_tasks_of(&self, window: WindowId) {
        let mut tasks = self.inner.tasks.borrow_mut();
        for t in tasks.iter() {
            if t.window == window {
                t.cancel.cancel();
            }
        }
        tasks.retain(|t| t.window != window);
        self.inner.busy.borrow_mut().retain(|b| b.window != window);
    }

    /// 某窗口是否还有后台任务在跑
    pub fn has_tasks(&self, window: WindowId) -> bool {
        self.inner.tasks.borrow().iter().any(|t| t.window == window)
    }

    // ── 忙碌（loading 遮罩）──

    /// 设 loading 遮罩的**最短可见时间**（默认 `ZERO` = 不等待）。
    ///
    /// 几毫秒就干完的活儿（小文件读盘、内存里的合并）常常**来不及出一帧**：忙碌项挂上后
    /// 立刻又被撤下，用户什么都看不到。设成 300~400ms 就能保证这类快活儿也给出可见反馈
    /// —— 遮罩至少显示这么久，到点由帧驱动（`WindowCtx::frame` → [`Runtime::reap_busy`]）收掉。
    ///
    /// 代价：这段"尾巴"期间遮罩仍在（Modal 语义 ⇒ 还会挡住输入）；任务本身早已结束，
    /// 结果也已落到状态里。
    pub fn set_busy_min_visible(&self, d: std::time::Duration) {
        self.inner.busy_min_visible.set(d);
    }

    /// 当前的最短可见时间
    pub fn busy_min_visible(&self) -> std::time::Duration {
        self.inner.busy_min_visible.get()
    }

    /// 开始一个「忙碌」段：窗口出现 loading 遮罩，返回的 [`BusyToken`] **drop 时自动收起**。
    ///
    /// 不挂遮罩的纯任务用 [`Self::spawn_task`]；这里适合"不是任务但也要挡一下"的场景
    /// （例如主线程里的一段同步重活、异步对话框等待）。
    pub fn begin_busy(&self, window: WindowId, label: impl Into<String>) -> BusyToken {
        let id = {
            let n = self.inner.next_task_id.get() + 1;
            self.inner.next_task_id.set(n);
            n
        };
        self.inner.busy.borrow_mut().push(BusyItem {
            id,
            window,
            label: label.into(),
            detail: None,
            progress: None,
            cancel: None,
            since: Instant::now(),
            hide_at: None,
        });
        self.mark(window, Dirty::VIEW | Dirty::PRESENT);
        BusyToken {
            rt: self.clone(),
            window,
            id,
            finished: false,
        }
    }

    /// 当前窗口的忙碌项（渲染遮罩用；空 = 没有遮罩）
    pub fn busy_items(&self, window: WindowId) -> Vec<BusyItem> {
        self.inner
            .busy
            .borrow()
            .iter()
            .filter(|b| b.window == window)
            .cloned()
            .collect()
    }

    pub fn is_busy(&self, window: WindowId) -> bool {
        self.inner.busy.borrow().iter().any(|b| b.window == window)
    }

    pub(crate) fn end_busy(&self, id: u64) {
        let hidden = {
            let mut items = self.inner.busy.borrow_mut();
            let Some(pos) = items.iter().position(|b| b.id == id) else {
                return;
            };
            let min = self.inner.busy_min_visible.get();
            let hide_at = items[pos].since + min;
            // 一瞬间就结束的忙碌段（几毫秒的任务）常常**来不及出帧**就被撤下 ——
            // 用户什么也看不到。配了最短可见时间就先挂住，到点由 `reap_busy` 收。
            if min > Duration::ZERO && Instant::now() < hide_at {
                items[pos].hide_at = Some(hide_at);
                return;
            }
            items.remove(pos)
        };
        // 让该窗口重跑 view：遮罩层随之消失（层消失本身会整窗脏）
        self.mark(hidden.window, Dirty::VIEW | Dirty::PRESENT);
    }

    /// 收掉"最短可见时间已过"的忙碌项（帧驱动每帧开头调用）。返回是否收掉了东西。
    ///
    /// 只在配了 [`Runtime::set_busy_min_visible`] 时才可能有待收项。
    pub fn reap_busy(&self, now: Instant) -> bool {
        let mut removed: Vec<WindowId> = Vec::new();
        {
            let mut items = self.inner.busy.borrow_mut();
            let mut i = 0;
            while i < items.len() {
                if items[i].hide_at.is_some_and(|t| t <= now) {
                    removed.push(items.remove(i).window);
                } else {
                    i += 1;
                }
            }
        }
        for w in &removed {
            self.mark(*w, Dirty::VIEW | Dirty::PRESENT);
        }
        !removed.is_empty()
    }

    pub(crate) fn set_busy_label(&self, id: u64, label: String) {
        let mut items = self.inner.busy.borrow_mut();
        let Some(b) = items.iter_mut().find(|b| b.id == id) else {
            return;
        };
        if b.label == label {
            return;
        }
        b.label = label;
        let window = b.window;
        drop(items);
        self.mark(window, Dirty::VIEW);
    }

    pub(crate) fn set_busy_progress(&self, id: u64, done: usize, total: usize) {
        let mut items = self.inner.busy.borrow_mut();
        let Some(b) = items.iter_mut().find(|b| b.id == id) else {
            return;
        };
        if b.progress == Some((done, total)) {
            return;
        }
        b.progress = Some((done, total));
        let window = b.window;
        drop(items);
        self.mark(window, Dirty::VIEW);
    }

    pub(crate) fn set_busy_detail(&self, id: u64, detail: Option<String>) {
        let mut items = self.inner.busy.borrow_mut();
        let Some(b) = items.iter_mut().find(|b| b.id == id) else {
            return;
        };
        if b.detail == detail {
            return;
        }
        b.detail = detail;
        let window = b.window;
        drop(items);
        self.mark(window, Dirty::VIEW);
    }

    pub(crate) fn set_busy_cancel(&self, id: u64, f: Rc<dyn Fn()>) {
        let mut items = self.inner.busy.borrow_mut();
        if let Some(b) = items.iter_mut().find(|b| b.id == id) {
            b.cancel = Some(f);
        }
    }

    /// 根据任务 id 找回它挂的忙碌项（进度消息 / 取消按钮用）
    pub(crate) fn busy_id_of_task(&self, task: u64) -> Option<u64> {
        self.inner
            .tasks
            .borrow()
            .iter()
            .find(|t| t.id == task)
            .and_then(|t| t.busy)
    }
}

/// UI 线程侧的框架消息处理。返回 `true` = 框架**完全消费**了这条消息
/// （不再交给 `ViewModel::on_external`）。
///
/// - [`TaskProgress`] ⇒ 更新对应遮罩进度 ⇒ `true`
/// - [`TaskEvent`] ⇒ 清任务表 + 收遮罩，但**继续**交给用户（让它取载荷）⇒ `false`
pub(crate) fn on_task_message(rt: &Runtime, window: WindowId, data: &ExternalData) -> bool {
    if let Some(p) = data.downcast_ref::<TaskProgress>() {
        if let Some(bid) = rt.busy_id_of_task(p.id) {
            rt.set_busy_progress(bid, p.done, p.total);
            if let Some(d) = &p.detail {
                rt.set_busy_detail(bid, Some(d.clone()));
            }
        }
        return true;
    }
    let finished = data
        .downcast_ref::<TaskEvent>()
        .map(|ev| ev.id)
        .or_else(|| data.downcast_ref::<TaskFailed>().map(|ev| ev.id));
    if let Some(id) = finished {
        let rec = {
            let mut tasks = rt.inner.tasks.borrow_mut();
            let idx = tasks.iter().position(|t| t.id == id);
            idx.map(|i| tasks.remove(i))
        };
        if let Some(rec) = rec
            && let Some(bid) = rec.busy
        {
            rt.end_busy(bid);
        }
        rt.mark(window, Dirty::VIEW | Dirty::PRESENT);
        return false; // 交给用户（TaskEvent 带载荷，TaskFailed 可记日志）
    }
    false
}

// ───────────────────────── BusyToken ─────────────────────────

/// 「忙碌」段的 RAII 句柄：`drop` ⇒ 遮罩消失。`!Send`（留在 UI 线程）。
///
/// ```ignore
/// let busy = rt.begin_busy(win, "正在导出…");
/// busy.set_progress(0, 100);
/// // …干完活（或 Drop / finish()）
/// ```
pub struct BusyToken {
    rt: Runtime,
    window: WindowId,
    id: u64,
    finished: bool,
}

impl BusyToken {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn window(&self) -> WindowId {
        self.window
    }

    /// 改文案（会触发一次 `view()` 重跑刷新遮罩）
    pub fn set_label(&self, label: impl Into<String>) {
        self.rt.set_busy_label(self.id, label.into());
    }

    /// 设置确定进度（画进度条 + `done / total`）
    pub fn set_progress(&self, done: usize, total: usize) {
        self.rt.set_busy_progress(self.id, done, total);
    }

    /// 设置进度明细（显示在进度条下方，替代默认的 `done / total`）
    pub fn set_detail(&self, detail: impl Into<String>) {
        self.rt.set_busy_detail(self.id, Some(detail.into()));
    }

    /// 挂「取消」按钮：点击时执行 `f`（通常在里面 `TaskHandle::cancel()` 或置 `CancelToken`）。
    ///
    /// 遮罩上的按钮文案固定为「取消」；点击后遮罩**不会**自动消失——由 `f` 里的
    /// 逻辑决定何时 `finish()` / `drop`（任务侧看到取消后自行收敛）。
    pub fn cancellable(&self, f: impl Fn() + 'static) {
        self.rt.set_busy_cancel(self.id, Rc::new(f));
    }

    /// 提前结束（等价于 drop，但语义显式）
    pub fn finish(mut self) {
        self.rt.end_busy(self.id);
        self.finished = true;
    }
}

impl Drop for BusyToken {
    fn drop(&mut self) {
        if !self.finished {
            self.rt.end_busy(self.id);
        }
    }
}

impl std::fmt::Debug for BusyToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BusyToken")
            .field("id", &self.id)
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::WindowId;
    use std::sync::atomic::AtomicU32;
    use std::time::{Duration, Instant};

    /// 记录调用的假唤醒器（无头测试用）
    #[derive(Default)]
    struct TestWaker {
        wakes: AtomicU32,
        posts: Mutex<Vec<(WindowId, ExternalData)>>,
    }

    impl TestWaker {
        fn take(&self) -> Vec<(WindowId, ExternalData)> {
            self.posts.lock().unwrap().drain(..).collect()
        }
    }

    impl Waker for TestWaker {
        fn wake(&self) -> bool {
            self.wakes.fetch_add(1, Ordering::SeqCst);
            true
        }

        fn post(&self, window: WindowId, data: ExternalData) -> bool {
            self.posts.lock().unwrap().push((window, data));
            true
        }
    }

    fn win() -> WindowId {
        WindowId::new(1)
    }

    #[test]
    fn local_queue_round_trips_without_a_platform() {
        let rt = Runtime::new();
        assert!(!rt.is_online(), "默认是本地队列模式");
        assert!(rt.wake(), "无头下唤醒是 no-op 成功");

        let w = win();
        let handle = rt.waker();
        assert!(handle.is_none(), "无平台 ⇒ 拿不到平台唤醒器");

        // 直接走本地队列（模拟任务线程投递）
        let slot = rt.inner.waker.borrow().clone();
        assert!(slot.post(w, ExternalData::new(7u8)));
        let got = rt.take_pending_external();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, w);
        assert_eq!(got[0].1.downcast_ref::<u8>(), Some(&7));
    }

    #[test]
    fn platform_waker_receives_posts_and_marks_online() {
        let rt = Runtime::new();
        let tw = Arc::new(TestWaker::default());
        rt.set_waker(tw.clone());

        assert!(rt.is_online());
        assert!(rt.wake());
        assert_eq!(tw.wakes.load(Ordering::SeqCst), 1);

        let slot = rt.inner.waker.borrow().clone();
        assert!(slot.post(win(), ExternalData::new("hi")));
        assert_eq!(tw.take().len(), 1);
        assert!(rt.take_pending_external().is_empty(), "平台模式不落本地队列");
    }

    /// 回归（A6）：**在 `set_waker` 之前** spawn 的任务也必须能把消息与唤醒交给平台。
    ///
    /// bug 表现：`TaskCtx` 持有 spawn 时的 `WakerSlot` **快照**，而平台 waker 是
    /// `run()` 里才注入的 ⇒ 构造期起的预加载任务永远持 `Local` 槽位，`post()`
    /// 落进 `LocalQueue`。而 `LocalQueue` 只被 `App::frame_all` 消费，**平台层从不调它**
    /// ⇒ 消息积压、无人消费、**任务永不回调、界面毫无反应且零报错**。
    ///
    /// 修复分两半，本测试覆盖"转发"那一半（另一半是平台 tick 里的 drain）。
    #[test]
    fn task_spawned_before_set_waker_still_reaches_the_platform() {
        let rt = Runtime::new();

        // ① 在平台 waker 注入**之前**取一个槽位快照 —— 这模拟"run() 之前 spawn 的任务"
        let early = rt.inner.waker.borrow().clone();

        // ② 平台启动：注入 waker（会给旧本地队列装上转发器）
        let tw = Arc::new(TestWaker::default());
        rt.set_waker(tw.clone());

        // ③ 那个"早期任务"完成并投递 —— 消息必须到达平台，而不是躺在本地队列里
        assert!(early.post(win(), ExternalData::new("early")), "旧快照的 post 应成功");
        assert_eq!(
            tw.take().len(),
            1,
            "A6：set_waker 之前 spawn 的任务，其消息必须转发给平台（否则永久丢失）"
        );
        assert!(
            rt.take_pending_external().is_empty(),
            "消息已转交平台，不应滞留在本地队列"
        );
    }

    /// 回归（A6 另一半）：`set_waker` 时**已积压**在本地队列里的消息要被转交平台。
    ///
    /// 否则它们会永远留在队列里：平台模式不再 drain 本地队列（`take_pending_external`
    /// 在 `Platform` 分支返回空），而这些消息又已经不该由 UI 再消费一次。
    #[test]
    fn set_waker_forwards_already_queued_messages() {
        let rt = Runtime::new();

        // 平台启动前先投两条（模拟 ViewModel 构造期的同步投递）。
        // ⚠️ **不要在这里 drain**：`take_pending_external` 是"取走"，会把要验证的消息清掉
        // —— 那样断言就变成"队列本来就是空的"，测试失去意义。
        {
            let slot = rt.inner.waker.borrow().clone();
            slot.post(win(), ExternalData::new("a"));
            slot.post(win(), ExternalData::new("b"));
        }

        // 注入平台 waker ⇒ 积压消息应被转交
        let tw = Arc::new(TestWaker::default());
        rt.set_waker(tw.clone());

        assert_eq!(
            tw.take().len(),
            2,
            "A6：set_waker 必须把已积压的消息转交平台，否则它们永远滞留"
        );
        assert!(rt.take_pending_external().is_empty(), "转交后本地队列应为空");
    }

    #[test]
    fn spawn_task_delivers_a_task_event_with_the_payload() {
        let rt = Runtime::new();
        let tw = Arc::new(TestWaker::default());
        rt.set_waker(tw.clone());
        rt.register_window(win());

        let handle = rt.spawn_task(win(), |ctx| {
            ctx.post("progress-note");
            Ok::<_, ()>(21u32 * 2)
        });
        assert_eq!(handle.id(), 1, "任务 id 自增");
        assert!(rt.has_tasks(win()));

        // 等任务体跑完
        let deadline = Instant::now() + Duration::from_secs(5);
        while !handle.is_done() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(handle.is_done(), "任务应在超时前完成");

        // 投递：中间消息 + 完成事件
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut msgs = Vec::new();
        while msgs.len() < 2 && Instant::now() < deadline {
            msgs.extend(tw.take());
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(msgs.len(), 2, "一条中间消息 + 一条完成事件");

        // 完成事件：框架收尾（清表）但**继续**交给用户
        let (w, done) = msgs.pop().unwrap();
        assert_eq!(w, win());
        assert!(!on_task_message(&rt, w, &done), "TaskEvent 要交给用户");
        assert!(!rt.has_tasks(win()), "任务表已清");
        let ev = done.downcast::<TaskEvent>().expect("是 TaskEvent");
        assert_eq!(ev.id, handle.id());
        assert_eq!(ev.payload.downcast::<Result<u32, ()>>(), Some(Ok(42)));

        // 中间消息：普通数据，框架不消费
        let (_, note) = msgs.pop().unwrap();
        assert!(!on_task_message(&rt, win(), &note));
        assert_eq!(note.downcast::<&str>(), Some("progress-note"));
    }

    #[test]
    fn spawn_task_busy_shows_a_busy_item_and_clears_it_on_completion() {
        let rt = Runtime::new();
        let tw = Arc::new(TestWaker::default());
        rt.set_waker(tw.clone());
        rt.register_window(win());

        let handle = rt.spawn_task_busy(win(), "正在导出…", |ctx| {
            ctx.progress(1, 4);
            "done"
        });
        assert!(rt.is_busy(win()), "任务开始 ⇒ 遮罩出现");
        let items = rt.busy_items(win());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "正在导出…");
        assert_eq!(items[0].ratio(), None, "还没进度 ⇒ 不确定进度");
        assert!(items[0].is_cancellable(), "带遮罩的任务默认可取消");

        // 等消息齐（进度 + 完成）
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut msgs = Vec::new();
        while (msgs.len() < 2 || !handle.is_done()) && Instant::now() < deadline {
            msgs.extend(tw.take());
            std::thread::sleep(Duration::from_millis(2));
        }

        // 进度消息：框架消费并更新遮罩
        let progress = msgs
            .iter()
            .position(|(_, d)| d.downcast_ref::<TaskProgress>().is_some())
            .expect("有进度消息");
        let (_, p) = msgs.remove(progress);
        assert!(on_task_message(&rt, win(), &p), "进度消息被框架完全消费");
        assert_eq!(rt.busy_items(win())[0].ratio(), Some(0.25));

        // 完成：遮罩自动收起
        let (_, done) = msgs.pop().expect("完成事件");
        assert!(!on_task_message(&rt, win(), &done));
        assert!(!rt.is_busy(win()), "任务完成 ⇒ 遮罩自动收起");
    }

    /// `progress_with`：进度与**明细文案**一次上报；遮罩两者都更新
    #[test]
    fn progress_with_carries_a_human_readable_detail_line() {
        let rt = Runtime::new();
        let tw = Arc::new(TestWaker::default());
        rt.set_waker(tw.clone());
        rt.register_window(win());

        let _handle = rt.spawn_task_busy(win(), "正在打开 3 个文件…", |ctx| {
            // 把每个文件拆成三格 ⇒ 进度条在文件内部也会走
            ctx.progress_with(1, 9, "第 1 / 3 个文件 · 正在读取 a.pdf");
            ctx.progress_with(5, 9, "第 2 / 3 个文件 · 正在合并 b.pdf");
            "done"
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut msgs = Vec::new();
        while msgs.len() < 3 && Instant::now() < deadline {
            msgs.extend(tw.take());
            std::thread::sleep(Duration::from_millis(2));
        }

        let mut seen = Vec::new();
        for (_, d) in msgs {
            if d.downcast_ref::<TaskProgress>().is_some() {
                on_task_message(&rt, win(), &d);
                let item = rt.busy_items(win()).into_iter().next().expect("遮罩项在");
                seen.push((item.ratio().map(|r| (r * 100.0).round() as i32), item.detail.clone()));
            }
        }
        assert_eq!(
            seen,
            vec![
                (Some(11), Some("第 1 / 3 个文件 · 正在读取 a.pdf".to_string())),
                (Some(56), Some("第 2 / 3 个文件 · 正在合并 b.pdf".to_string())),
            ],
            "进度条与明细都跟着上报走"
        );
    }

    /// 任务快到"来不及出帧"时遮罩会一闪而过甚至完全看不见 ⇒ `set_busy_min_visible`
    /// 把忙碌项留到最短可见时间；到点由 `reap_busy` 收掉（帧驱动每帧调它）。
    #[test]
    fn busy_overlay_is_held_for_the_minimum_visible_time() {
        let rt = Runtime::new();
        let tw = Arc::new(TestWaker::default());
        rt.set_waker(tw.clone());
        rt.register_window(win());
        assert_eq!(rt.busy_min_visible(), Duration::ZERO, "默认不等待");
        rt.set_busy_min_visible(Duration::from_millis(300));

        let handle = rt.spawn_task_busy(win(), "正在打开…", |_| 0u32);

        // 等任务完成并把完成消息喂回去（= 平台把 External 先于重绘处理掉的那种情形）
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut msgs = Vec::new();
        while msgs.is_empty() && Instant::now() < deadline {
            msgs.extend(tw.take());
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(handle.is_done());
        let (_, done) = msgs.pop().expect("完成事件");
        on_task_message(&rt, win(), &done);

        assert!(!rt.has_tasks(win()), "任务已经结束");
        assert!(rt.is_busy(win()), "但遮罩还挂着（最短可见时间）");

        assert!(!rt.reap_busy(Instant::now()), "还没到点 ⇒ 不收");
        assert!(
            rt.reap_busy(Instant::now() + Duration::from_millis(400)),
            "过了最短可见时间 ⇒ 收掉"
        );
        assert!(!rt.is_busy(win()), "遮罩收起");
        assert!(
            !rt.reap_busy(Instant::now() + Duration::from_secs(1)),
            "已经收干净了（幂等）"
        );
    }

    /// `BusyToken::set_detail`：UI 线程侧的忙碌段也能给明细
    #[test]
    fn busy_token_can_set_a_detail_line() {
        let rt = Runtime::new();
        let w = win();
        rt.register_window(w);
        let token = rt.begin_busy(w, "正在整理…");
        token.set_progress(2, 5);
        token.set_detail("正在写第 2 个分片");
        let item = rt.busy_items(w).into_iter().next().expect("忙碌项");
        assert_eq!(item.ratio(), Some(0.4));
        assert_eq!(item.detail.as_deref(), Some("正在写第 2 个分片"));
        token.finish();
    }

    /// 点遮罩上的「取消」⇒ 任务收到取消（这是 `spawn_task_busy` 的默认接线）
    #[test]
    fn clicking_the_overlay_cancel_button_cancels_the_task() {
        let rt = Runtime::new();
        let tw = Arc::new(TestWaker::default());
        rt.set_waker(tw.clone());
        rt.register_window(win());

        let handle = rt.spawn_task_busy(win(), "正在导出…", |ctx| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !ctx.is_cancelled() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
            ctx.is_cancelled()
        });

        // 模拟遮罩上的按钮点击
        let click = rt.busy_items(win())[0].cancel.clone().expect("有取消按钮");
        click();
        assert!(handle.is_cancelled(), "点击 ⇒ 任务被取消");

        // 任务收敛后遮罩收起
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut msgs = Vec::new();
        while msgs.is_empty() && Instant::now() < deadline {
            msgs.extend(tw.take());
            std::thread::sleep(Duration::from_millis(2));
        }
        let (_, done) = msgs.pop().expect("完成事件");
        on_task_message(&rt, win(), &done);
        assert!(!rt.is_busy(win()));
    }

    /// 任务体 panic：不能让遮罩永远挂着（框架必须收到"失败"并收尾）
    #[test]
    fn a_panicking_task_still_finishes_and_clears_its_overlay() {
        let rt = Runtime::new();
        let tw = Arc::new(TestWaker::default());
        rt.set_waker(tw.clone());
        rt.register_window(win());

        let handle = rt.spawn_task_busy(win(), "正在解析…", |_ctx| -> u32 { panic!("任务体崩了") });
        assert!(rt.is_busy(win()));

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut msgs = Vec::new();
        while msgs.is_empty() && Instant::now() < deadline {
            msgs.extend(tw.take());
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(handle.is_done(), "panic 也要走收尾路径（否则遮罩永远挂着）");

        let (_, failed) = msgs.pop().expect("失败事件");
        assert!(!on_task_message(&rt, win(), &failed), "失败事件也交给用户");
        assert_eq!(
            failed.downcast::<TaskFailed>().map(|f| f.id),
            Some(handle.id()),
            "投递的是 TaskFailed"
        );
        assert!(!rt.is_busy(win()), "失败 ⇒ 遮罩同样收起");
        assert!(!rt.has_tasks(win()), "任务表同样清空");
    }

    #[test]
    fn cancelling_a_task_is_visible_inside_the_worker() {
        let rt = Runtime::new();
        let tw = Arc::new(TestWaker::default());
        rt.set_waker(tw.clone());
        rt.register_window(win());

        let handle = rt.spawn_task(win(), |ctx| {
            // 等外部取消（最多 5s）
            let deadline = Instant::now() + Duration::from_secs(5);
            while !ctx.is_cancelled() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
            ctx.is_cancelled()
        });
        handle.cancel();
        assert!(handle.is_cancelled());

        // 任务应以"已取消"收场
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut msgs = Vec::new();
        while msgs.is_empty() && Instant::now() < deadline {
            msgs.extend(tw.take());
            std::thread::sleep(Duration::from_millis(2));
        }
        let (_, done) = msgs.pop().expect("完成事件");
        on_task_message(&rt, win(), &done);
        let ev = done.downcast::<TaskEvent>().unwrap();
        assert_eq!(ev.payload.downcast::<bool>(), Some(true), "线程里看到了取消");
    }

    #[test]
    fn closing_a_window_cancels_its_tasks_and_drops_its_busy_items() {
        let rt = Runtime::new();
        let tw = Arc::new(TestWaker::default());
        rt.set_waker(tw.clone());
        rt.register_window(win());

        let handle = rt.spawn_task_busy(win(), "正在加载…", |ctx| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !ctx.is_cancelled() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        assert!(rt.is_busy(win()));

        rt.cancel_tasks_of(win());
        assert!(handle.is_cancelled());
        assert!(!rt.has_tasks(win()), "任务表清空");
        assert!(!rt.is_busy(win()), "遮罩项一并清掉");
    }

    #[test]
    fn busy_token_is_raii_and_updates_progress() {
        let rt = Runtime::new();
        let w = win();
        rt.register_window(w);

        {
            let busy = rt.begin_busy(w, "正在合并…");
            assert!(rt.is_busy(w));
            busy.set_label("正在合并 3 个 PDF…");
            busy.set_progress(2, 3);
            let items = rt.busy_items(w);
            assert_eq!(items[0].label, "正在合并 3 个 PDF…");
            assert_eq!(items[0].ratio(), Some(2.0 / 3.0));
            // 变更 ⇒ 标脏（view 重跑刷新遮罩）
            assert!(rt.take_dirty(w).contains(Dirty::VIEW));
        }
        assert!(!rt.is_busy(w), "Drop ⇒ 遮罩收起");
        assert!(rt.take_dirty(w).contains(Dirty::VIEW), "收起也要刷新一次");
    }

    #[test]
    fn busy_token_can_be_cancelled_from_the_overlay() {
        let rt = Runtime::new();
        let w = win();
        rt.register_window(w);
        let hits = Arc::new(AtomicU32::new(0));

        let busy = rt.begin_busy(w, "正在导出…");
        let h = hits.clone();
        busy.cancellable(move || {
            h.fetch_add(1, Ordering::SeqCst);
        });
        let items = rt.busy_items(w);
        assert!(items[0].is_cancellable());

        // 模拟遮罩上的「取消」按钮点击
        let cb = items[0].cancel.clone().unwrap();
        cb();
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        busy.finish();
        assert!(!rt.is_busy(w));
    }

    #[test]
    fn multiple_tasks_stack_busy_items_per_window() {
        let rt = Runtime::new();
        let tw = Arc::new(TestWaker::default());
        rt.set_waker(tw.clone());
        let a = WindowId::new(1);
        let b = WindowId::new(2);
        rt.register_window(a);
        rt.register_window(b);

        let h1 = rt.spawn_task_busy(a, "A 的任务", |_| ());
        let h2 = rt.spawn_task_busy(a, "另一个任务", |_| ());
        let _h3 = rt.spawn_task_busy(b, "B 的任务", |_| ());

        assert_eq!(rt.busy_items(a).len(), 2, "同窗口多个任务各占一项");
        assert_eq!(rt.busy_items(b).len(), 1, "遮罩按窗口隔离");
        assert!(h1.is_running() && h2.is_running());

        // 只把 **a 窗口** 的事件喂回去（b 的留在队列里没被消费 ⇒ 它的忙碌项应保持）
        let deadline = Instant::now() + Duration::from_secs(5);
        while (h1.is_running() || h2.is_running()) && Instant::now() < deadline {
            for (w, d) in tw.take() {
                if w == a {
                    on_task_message(&rt, w, &d);
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(rt.busy_items(a).is_empty(), "两个任务都完成后遮罩收起");
        assert_eq!(rt.busy_items(b).len(), 1, "B 的任务未被消费 ⇒ 遮罩仍在");
    }
}
