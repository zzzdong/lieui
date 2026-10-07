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
