//! `crate::post` 的单元测试：**只剩投递原语**。
//!
//! 框架不再提供"任务"（线程模型 / 取消 / 进度 / 任务表）也不再提供"遮罩"
//! （那是个组件，见 `tests/spinner_modal.rs`）。这里只验证**投递**：
//! 无头时落本地队列、有平台时走平台、以及 A6 的**稳定身份**。

use super::*;
use crate::window::WindowId;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::time::Duration;

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
    assert!(rt.waker().is_none(), "无平台 ⇒ 拿不到平台唤醒器");

    let w = win();
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
/// 旧设计里工作线程持有的是"spawn 时的唤醒器**快照**"；平台 waker 是在
/// `run()` 里才注入的 ⇒ 在那之前起的线程**永远**持本地槽位，投递只会落进
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

    // 平台启动前先投两条（模拟构造期的同步投递）。
    // ⚠️ **不要在这里 drain**：`take_pending_external` 是"取走"，会把要验证的
    // 消息清掉 —— 那样断言就变成"队列本来就是空的"，测试失去意义。
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

/// `Runtime::emit`（UI 线程侧的投递面）与 `Poster::post` 是同一条管道。
#[test]
fn runtime_emit_shares_the_same_pipe_as_poster() {
    let rt = Runtime::new();
    let tw = Arc::new(TestWaker::default());
    rt.set_waker(tw.clone());

    assert!(rt.emit(win(), "from-ui-thread"));
    let got = tw.take();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1.downcast_ref::<&str>().copied(), Some("from-ui-thread"));
}

/// `wake()` 是"不带数据"的那一半。
#[test]
fn wake_only_pokes_the_loop_without_payload() {
    let rt = Runtime::new();
    let tw = Arc::new(TestWaker::default());
    rt.set_waker(tw.clone());

    assert!(rt.wake());
    assert_eq!(tw.wakes.load(Ordering::SeqCst), 1);
    assert!(tw.take().is_empty(), "只唤醒，不带载荷");

    // 无头：no-op 但成功（消息会进本地队列）
    let headless = Runtime::new();
    assert!(headless.wake());
    let _ = Duration::from_millis(1);
}
