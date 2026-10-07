//! `crate::task` 的单元测试：**两层原语**，与"任务"无关。
//!
//! | 层 | 提供什么 |
//! |---|---|
//! | 投递（[`Poster`] / [`Waker`]） | 非 UI 线程把 `Send` 数据投给 UI 线程并唤醒 |
//! | 遮罩（[`BusyToken`]） | loading 遮罩的挂/收 —— **由调用方决定何时结束** |
//!
//! 框架**不再提供**线程模型 / 取消协议 / 进度协议 / 任务表 —— 那些是应用策略，
//! 调用方用任何并发模型（线程池 / rayon / tokio）自己搭，只把结果投递回来。

use super::*;
use crate::window::WindowId;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
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

/// 事件循环已退出的假唤醒器（投递必须**如实报告失败**）
#[derive(Default)]
struct DeadWaker;

impl Waker for DeadWaker {
    fn wake(&self) -> bool {
        false
    }

    fn post(&self, _window: WindowId, _data: ExternalData) -> bool {
        false
    }
}

fn win() -> WindowId {
    WindowId::new(1)
}

// ───────────────────────── 投递：无头 = 本地队列 ─────────────────────────

#[test]
fn local_queue_round_trips_without_a_platform() {
    let rt = Runtime::new();
    assert!(!rt.is_online(), "默认是本地队列模式");
    assert!(rt.wake(), "无头下唤醒是 no-op 成功");

    let w = win();
    assert!(rt.waker().is_none(), "无平台 ⇒ 拿不到平台唤醒器");

    // 走公开句柄（模拟任务线程投递）
    assert!(rt.poster().post(w, 7u8));
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

    assert!(rt.poster().post(win(), "hi"));
    assert_eq!(tw.take().len(), 1);
    assert!(rt.take_pending_external().is_empty(), "平台模式不落本地队列");
}

/// ★★ 回归（A6 的**结构性**版本）：**先拿句柄、后装平台 waker** 也必须能到达平台。
///
/// ## 原 bug
///
/// 旧设计里工作线程持有的是"spawn 时的 `WakerSlot` **快照**"；平台 waker 是在
/// `run()` 里才注入的 ⇒ 在那之前起的线程**永远**持 `Local` 槽位，投递只会落进
/// 本地队列 —— 而平台层不 drain 它 ⇒ **消息积压、界面毫无反应且零报错**。
/// 当时的补丁是"把平台 waker 事后塞进那个已被共享出去的队列"（retrofit）。
///
/// ## 现在为什么结构上不可能
///
/// [`Poster`] 指向 `Arc<PostHub>` —— **槽位本身是共享的**，`set_waker` 之后
/// 立刻可见。所以这个测试不再验证"补丁生效"，而是验证**句柄不携带快照**。
#[test]
fn a_poster_taken_before_set_waker_still_reaches_the_platform() {
    let rt = Runtime::new();

    // `run()` 之前：业务层就把句柄交给工作线程了
    let early = rt.poster();
    assert!(early.platform_waker().is_none(), "此时还没有平台");

    // 平台起来，注入唤醒器
    let tw = Arc::new(TestWaker::default());
    rt.set_waker(tw.clone());

    // ★ 那个"旧的"句柄必须立刻看到平台，且**带回唤醒**
    assert!(early.post(win(), "从旧句柄投递"));
    assert_eq!(tw.posts.lock().unwrap().len(), 1, "A6：必须到达平台");
    assert!(rt.take_pending_external().is_empty(), "不落本地队列");
    assert!(early.platform_waker().is_some(), "句柄看到的是当前槽位");
}

/// 回归（A6 另一半）：`set_waker` 时**已积压**在本地队列里的消息要被转交平台。
///
/// 否则它们会永远留在队列里：平台模式不再 drain 本地队列，而这些消息又已经
/// 不该由 UI 再消费一次。
#[test]
fn set_waker_forwards_already_queued_messages() {
    let rt = Runtime::new();

    // 平台启动前先投两条（模拟 ViewModel 构造期的同步投递）。
    // ⚠️ **不要在这里 drain**：`take_pending_external` 是"取走"，会把要验证的消息
    // 清掉 —— 那样断言就变成"队列本来就是空的"，测试失去意义。
    let early = rt.poster();
    early.post(win(), "a");
    early.post(win(), "b");

    let tw = Arc::new(TestWaker::default());
    rt.set_waker(tw.clone());

    assert_eq!(
        tw.take().len(),
        2,
        "A6：set_waker 必须把已积压的消息转交平台，否则它们永远滞留"
    );
    assert!(rt.take_pending_external().is_empty(), "转交后本地队列应为空");
}

// ───────────────────────── 投递：非 UI 线程的入口 ─────────────────────────

/// ★★ [`Poster`] —— **非 UI 线程的入口**，不需要任何"任务"概念。
///
/// 用**真线程**验证：`Runtime` 自己是 `!Send`（`Rc` 包着），进不了线程；
/// 但 `rt.poster()` 拿到的句柄可以。
#[test]
fn post_delivers_from_another_thread_without_any_task() {
    let rt = Runtime::new();
    let w = win();

    let poster = rt.poster();
    std::thread::spawn(move || {
        assert!(poster.post(w, String::from("worker 的结果")));
    })
    .join()
    .unwrap();

    let got = rt.take_pending_external();
    assert_eq!(got.len(), 1, "消息落到了本地队列（无头模式）");
    assert_eq!(got[0].0, w, "带着目标窗口");
    assert_eq!(
        got[0].1.downcast_ref::<String>().map(String::as_str),
        Some("worker 的结果")
    );
}

/// ✅ **正向**：事件循环已退出时 `post` 返回 `false`（调用方据此收敛）。
///
/// ★ **反向**对照：上面几条证明"能投进去"，这条证明"投不进去时**会说出来**" ——
/// 只测前者的话，一个永远返回 `true` 的实现也会全绿。
#[test]
fn post_reports_failure_after_the_loop_is_gone() {
    let rt = Runtime::new();
    rt.set_waker(Arc::new(DeadWaker));

    assert!(!rt.poster().post(win(), 1u8), "★ 必须如实报告 false");
    assert!(!rt.wake(), "同上");
}

// ───────────────────────── 遮罩：与任务**无关** ─────────────────────────

/// 快活儿也要看得见：忙碌段在**还没来得及出帧**时就结束 ⇒ 配了最短可见时间后
/// 仍然挂着，到点由帧驱动的 [`Runtime::reap_busy`] 收掉。
///
/// 这里**没有任务**：收尾动作是调用方显式 `finish()` 触发的。
#[test]
fn busy_overlay_is_held_for_the_minimum_visible_time() {
    let rt = Runtime::new();
    let w = win();
    rt.register_window(w);
    assert_eq!(rt.busy_min_visible(), Duration::ZERO, "默认不等待");
    rt.set_busy_min_visible(Duration::from_millis(300));

    let token = rt.begin_busy(w, "正在打开…");
    assert!(rt.is_busy(w));
    // 任务（在这里 = 调用方）瞬间完成
    token.finish();

    assert!(rt.is_busy(w), "遮罩还挂着（没到最短可见时间）");
    assert!(!rt.reap_busy(Instant::now()), "还没到点 ⇒ 不收");
    assert!(
        rt.reap_busy(Instant::now() + Duration::from_millis(400)),
        "过了最短可见时间 ⇒ 收掉"
    );
    assert!(!rt.is_busy(w), "遮罩收起");
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

/// 遮罩上的「取消」按钮**由调用方接管**：框架只存回调并在点击时执行它，
/// 点完**不自动收起**（何时收是调用方的决定）。
#[test]
fn the_overlay_cancel_button_calls_the_callers_callback() {
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
    assert!(rt.is_busy(w), "★ 点取消不自动收起 —— 由调用方决定");

    busy.finish();
    assert!(!rt.is_busy(w));
}

#[test]
fn multiple_busy_items_stack_per_window() {
    let rt = Runtime::new();
    let a = WindowId::new(1);
    let b = WindowId::new(2);
    rt.register_window(a);
    rt.register_window(b);

    let t1 = rt.begin_busy(a, "A 的活儿");
    let t2 = rt.begin_busy(a, "另一个活儿");
    let _t3 = rt.begin_busy(b, "B 的活儿");

    assert_eq!(rt.busy_items(a).len(), 2, "同窗口多个忙碌段各占一项");
    assert_eq!(rt.busy_items(b).len(), 1, "遮罩按窗口隔离");

    t1.finish();
    assert_eq!(rt.busy_items(a).len(), 1, "收一个还剩一个");
    t2.finish();
    assert!(rt.busy_items(a).is_empty());
}

/// 窗口关闭 ⇒ 它的忙碌项被清掉（否则遮罩留在已经没了的窗口上）。
///
/// ★ 这里只剩"遮罩"这一半：`cancel_tasks_of` 的**任务那一半**随"移除 task 概念"
/// 一起删除了 —— 线程归调用方，取消也归调用方。
#[test]
fn closing_a_window_drops_its_busy_items() {
    let rt = Runtime::new();
    let w = win();
    rt.register_window(w);

    let _token = rt.begin_busy(w, "正在加载…");
    assert!(rt.is_busy(w));

    rt.clear_busy_of(w);
    assert!(!rt.is_busy(w), "遮罩项随窗口关闭清掉");
}

/// ★★★ 遮罩**不是任务的一部分**：只用 [`Runtime::begin_busy`] / [`BusyToken`]
/// 就能把它从挂上驱动到收掉 —— 全程没有任何线程、任何任务表。
///
/// 钉住的是**概念边界**：框架提供的是"遮罩作用域"，不是"任务"。
#[test]
fn a_busy_overlay_is_driven_without_any_task() {
    let rt = Runtime::new();
    let w = win();

    assert!(rt.busy_items(w).is_empty(), "起手没有遮罩");

    let t = rt.begin_busy(w, "正在打开…");
    assert!(rt.is_busy(w), "遮罩已挂上");
    assert_eq!(rt.busy_items(w)[0].label, "正在打开…");
    assert_eq!(rt.busy_items(w)[0].ratio(), None, "未报进度 ⇒ 不确定进度");

    // 调用方按**自己的**节奏驱动：文案 / 进度 / 明细 / 取消按钮
    t.set_label("正在合并…");
    t.set_progress(3, 10);
    assert_eq!(rt.busy_items(w)[0].ratio(), Some(0.3));
    t.set_detail("第 3 / 10 个文件 · 正在合并 b.pdf");
    assert_eq!(
        rt.busy_items(w)[0].detail.as_deref(),
        Some("第 3 / 10 个文件 · 正在合并 b.pdf")
    );
    t.cancellable(|| {});
    assert!(rt.busy_items(w)[0].is_cancellable());

    t.finish();
    assert!(!rt.is_busy(w), "遮罩已收");
}

/// ★★ 定时兜底：**没人 `finish()`** 也必须在到点后自己收掉。
///
/// 这就是"遮罩的关闭由**调用方或定时器**决定"里的定时器那条。
/// 用途：什么时候回来没人说得准的活儿（网络请求、外部进程）。
#[test]
fn dismiss_after_is_a_timeout_backstop() {
    let rt = Runtime::new();
    let w = win();
    rt.set_busy_min_visible(Duration::ZERO);

    let t = rt.begin_busy(w, "正在等待外部进程…");
    t.dismiss_after(Duration::from_millis(50));
    assert!(rt.is_busy(w));

    assert!(!rt.reap_busy(Instant::now()), "未到点 ⇒ 不收");
    assert!(rt.is_busy(w));

    assert!(rt.reap_busy(Instant::now() + Duration::from_millis(60)), "到点 ⇒ 收");
    assert!(!rt.is_busy(w), "★ 定时兜底把它收掉了（调用方一直没 finish）");
}

/// ★ 到点时刻**只收紧、不放松**：`dismiss_after` 之后 `finish()` 必须取更早者。
///
/// 反例：若 `end_busy` 直接覆盖 `hide_at`，一个先 `finish()` 的遮罩会被后来那个
/// 60s 的兜底拖住 —— 用户看着一个早就该消失的遮罩。
#[test]
fn finish_tightens_the_deadline_rather_than_loosening_it() {
    let rt = Runtime::new();
    let w = win();
    rt.set_busy_min_visible(Duration::from_millis(30));

    let t = rt.begin_busy(w, "x");
    t.dismiss_after(Duration::from_secs(60)); // 最迟 60s
    t.finish(); // 已经结束 ⇒ 最短可见 30ms 后就该收

    assert!(
        rt.reap_busy(Instant::now() + Duration::from_millis(50)),
        "★ 取更早者：50ms 时已过 30ms 的下限，必须收掉（而不是等 60s 兜底）"
    );
    assert!(!rt.is_busy(w));
}
