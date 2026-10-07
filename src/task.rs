//! 跨线程投递 + loading 遮罩（框架级原语）。
//!
//! ## 这个模块**不提供**"任务"
//!
//! 没有 `spawn_task` / `TaskCtx` / `TaskHandle` / `CancelToken` / `TaskEvent`，
//! 也没有任务表。理由：
//!
//! - **线程模型是应用的决定**：线程池 / rayon / tokio / 自己的 reactor —— 框架
//!   替不了（当初硬编码 `std::thread::spawn` 就是替错了）；
//! - **取消与进度是调用方的协议**：cooperative `AtomicBool`？`CancellationToken`？
//!   这个形状对那个场景未必合适；
//! - **信封会污染通道**：一旦框架拥有消息协议，`WindowCtx::external` 就得先认一遍
//!   "这条是不是我自己发的"，一条本该**不透明**的通道就被污染了。
//!
//! ## 本模块提供的两条原语
//!
//! | 原语 | 作用 | 为什么属于 UI 库 |
//! |---|---|---|
//! | [`Poster`]（[`Runtime::poster`]） | 非 UI 线程把 `Send` 数据投递回 UI 线程并唤醒 | 它要接**事件循环**（winit 的 `EventLoopProxy` / 无头时的本地队列），只有窗口库知道怎么接 |
//! | [`BusyToken`]（[`Runtime::begin_busy`]） | 挂一个 loading 遮罩 | 它是**渲染 + 输入阻断 + 最短可见时间**，全是 UI 层的事 |
//!
//! ```text
//! UI 线程                                      工作线程（调用方自己的）
//! ────────                                     ──────────────────────
//! let poster = rt.poster();        ← 可 Send 的句柄 ──→  move 进线程
//! let busy = rt.begin_busy(win, "正在导出…");          poster.post(win, MyMsg::Progress(3, 9));
//! busy.cancellable(|| flag.store(true));               if flag.load() { … 收敛 … }
//!                                                      poster.post(win, MyMsg::Done(summary));
//!                          ↓ 收到 Done（UI 线程）
//! ViewModel::on_external: data.downcast::<MyMsg>()  ⇒  busy.finish()
//! ```
//!
//! 注意最后一步：**遮罩由调用方收**（它收到自己的结果时收），不是框架
//! "看到任务结束"时收 —— 框架压根不知道有这么一个工作。
//! 也可以配 [`BusyToken::dismiss_after`] 做定时兜底。
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
use std::rc::Rc;
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

// ───────────────────────── 忙碌项（loading 遮罩）─────────────────────────

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
    /// 见 [`BusyToken::set_detail`]）
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
    /// 适合"不是任务、但也要挡一下 UI"的场景（外部进程、等待超时……）。
    /// （例如主线程里的一段同步重活、异步对话框等待）。
    pub fn begin_busy(&self, window: WindowId, label: impl Into<String>) -> BusyToken {
        let id = {
            let n = self.inner.next_busy_id.get() + 1;
            self.inner.next_busy_id.set(n);
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

    /// 窗口关闭时清掉它的忙碌项（遮罩不能留在已经没了的窗口上）。
    ///
    /// 这是 `cancel_tasks_of` 拆分后留下的**唯一一半**：任务那半随"框架不再管任务"
    /// 一起删除（线程归调用方，取消也归调用方），遮罩这半仍是框架自己的状态。
    pub(crate) fn clear_busy_of(&self, window: WindowId) {
        self.inner.busy.borrow_mut().retain(|b| b.window != window);
    }

    pub(crate) fn end_busy(&self, id: u64) {
        let hidden = {
            let mut items = self.inner.busy.borrow_mut();
            let Some(pos) = items.iter().position(|b| b.id == id) else {
                return;
            };
            // `hide_at` 的语义是"**最迟**收掉的时刻"，两个来源取**较早**者：
            //   · 最短可见时间（`since + busy_min_visible`）—— 快活儿也要被看见一下；
            //   · 已有的 `hide_at` —— 可能来自 [`BusyToken::dismiss_after`] 的定时兜底。
            // 收掉时刻只收紧、不放松。
            let min_at = items[pos].since + self.inner.busy_min_visible.get();
            let target = match items[pos].hide_at {
                Some(prev) => prev.min(min_at),
                None => min_at,
            };
            // 一瞬间就结束的忙碌段（几毫秒的任务）常常**来不及出帧**就被撤下 ——
            // 用户什么也看不到。配了最短可见时间就先挂住，到点由 `reap_busy` 收。
            if Instant::now() < target {
                items[pos].hide_at = Some(target);
                return;
            }
            items.remove(pos)
        };
        // 让该窗口重跑 view：遮罩层随之消失（层消失本身会整窗脏）
        self.mark(hidden.window, Dirty::VIEW | Dirty::PRESENT);
    }

    /// 给某个忙碌段设一个**最迟收起时刻**（定时兜底，见 [`BusyToken::dismiss_after`]）。
    ///
    /// 多次调用取**最早**者。不需要标脏：`hide_at` 不影响画面，而"遮罩在"
    /// 本身就保证了 `next_wakeup` 会给定时唤醒 ⇒ `reap_busy` 会跑到点。
    pub(crate) fn set_busy_deadline(&self, id: u64, at: Instant) {
        let mut items = self.inner.busy.borrow_mut();
        let Some(b) = items.iter_mut().find(|b| b.id == id) else {
            return;
        };
        b.hide_at = Some(match b.hide_at {
            Some(prev) => prev.min(at),
            None => at,
        });
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

    /// 挂「取消」按钮：点击时执行 `f`（通常是置调用方自己的取消标志）。
    ///
    /// 遮罩上的按钮文案固定为「取消」；点击后遮罩**不会**自动消失——由 `f` 里的
    /// 逻辑决定何时 `finish()` / `drop`（工作线程看到标志后自行收敛）。
    pub fn cancellable(&self, f: impl Fn() + 'static) {
        self.rt.set_busy_cancel(self.id, Rc::new(f));
    }

    /// **定时兜底**：`after` 之后自动收起，即使调用方忘了 `finish()`。
    ///
    /// 用途：什么时候回来没人说得准的活儿（网络请求、外部进程、等到超时）——
    /// "超过 30 秒就别再挡着 UI 了"。
    ///
    /// ★ 正是"遮罩的关闭由**调用方或定时器**决定、而不是由某个任务决定"这条：
    ///   遮罩的存活期是**调用方声明的作用域**，收法有三种，全在调用方手里 ——
    ///   `finish()`（显式）、`drop`（作用域结束）、`dismiss_after`（定时兜底）。
    ///
    /// 多次调用取**最早**者（到点时刻只收紧、不放松）。
    /// 到点由帧驱动的 [`Runtime::reap_busy`] 收 —— 而"遮罩在"保证了定时唤醒，
    /// 所以它不依赖外部事件（见 [`crate::WindowCtx::needs_frame`]）。
    pub fn dismiss_after(&self, after: Duration) {
        self.rt.set_busy_deadline(self.id, Instant::now() + after);
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
#[path = "task_tests.rs"]
mod tests;
