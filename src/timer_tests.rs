use super::*;

fn win() -> WindowId {
    WindowId::new(1)
}

#[test]
fn timeout_is_due_only_after_its_deadline_and_is_one_shot() {
    let rt = Runtime::new();
    rt.register_window(win());
    let hits = std::rc::Rc::new(std::cell::Cell::new(0));
    let h = {
        let hits = hits.clone();
        rt.set_timeout(win(), Duration::from_secs(60), move |_| hits.set(hits.get() + 1))
    };
    assert_eq!(rt.timer_count(), 1);
    assert!(h.is_active());
    assert!(rt.next_deadline(win()).is_some(), "有定时器 ⇒ 有唤醒时刻");

    // 还没到点
    assert!(rt.take_due_timers(win(), Instant::now()).is_empty());

    // 到点：取出来跑一次，一次性 ⇒ 不重排
    force_due(&rt, win());
    let mut due = rt.take_due_timers(win(), Instant::now());
    assert_eq!(due.len(), 1);
    (due[0].take_cb().unwrap())(&mut Ctx::new(&rt, win(), crate::event::EventView::tick()));
    rt.reschedule_timer(due.remove(0), Instant::now());
    assert_eq!(hits.get(), 1);
    assert_eq!(rt.timer_count(), 0, "一次性定时器触发后消失");
    assert!(rt.next_deadline(win()).is_none());
}

#[test]
fn interval_reschedules_itself_until_cancelled() {
    let rt = Runtime::new();
    rt.register_window(win());
    let h = rt.set_interval(win(), Duration::from_millis(10), |_| {});

    // 模拟三次触发（`reschedule_timer` 内部会取出并还原回调）
    for _ in 0..3 {
        force_due(&rt, win());
        let mut due = rt.take_due_timers(win(), Instant::now());
        assert_eq!(due.len(), 1);
        rt.reschedule_timer(due.remove(0), Instant::now());
    }
    assert_eq!(rt.timer_count(), 1, "周期定时器一直在表里");

    h.cancel();
    assert_eq!(rt.timer_count(), 0);
    assert!(!h.is_active());
}

/// 回归（D9）：在**定时器自己的回调里**调 `cancel()` 必须有效。
///
/// bug 表现：`take_due_timers` 把到期的定时器**移出表**再执行回调（为了让回调里
/// 能安全地再设定时器）。于是回调内的 `cancel()` 走 `retain` 命中不到任何东西 ⇒
/// **取消无效** ⇒ 周期定时器被放回表里，下个周期再跑，**永远停不下来**。
///
/// 这条路径在真实应用里就是"在 `on_tick` 里根据状态停掉轮询"，非常常用。
#[test]
fn cancel_inside_own_callback_stops_interval_timer() {
    let rt = Runtime::new();
    rt.register_window(win());
    let hits = std::rc::Rc::new(std::cell::Cell::new(0u32));

    // 回调里靠句柄取消自己 —— 用 `Rc<RefCell<Option<TimerHandle>>>` 打破循环借用：
    // 闭包捕获槽位（此时为空），定时器建好后再把句柄写回槽位。
    let slot: std::rc::Rc<std::cell::RefCell<Option<TimerHandle>>> = std::rc::Rc::new(std::cell::RefCell::new(None));
    let s2 = slot.clone();
    let c2 = hits.clone();
    let h = rt.set_interval(win(), Duration::from_millis(10), move |_| {
        c2.set(c2.get() + 1);
        if let Some(h) = s2.borrow().as_ref() {
            h.cancel(); // ★ 回调内取消自己
        }
    });
    *slot.borrow_mut() = Some(h);

    // 触发一次
    force_due(&rt, win());
    let mut due = rt.take_due_timers(win(), Instant::now());
    assert_eq!(due.len(), 1);
    // ⚠️ 必须照 `WindowCtx::tick` 的做法**把 cb 还回去**（`take_cb` 之后 `reschedule_timer`
    // 靠它恢复回调）。否则 `reschedule_timer` 会因"cb 不在了"提前 return，
    // 测试就成了假通过 —— 压根没走到取消检查。
    run_one(&mut due.remove(0), &rt);
    assert_eq!(hits.get(), 1, "回调应执行过一次");
    assert_eq!(
        rt.timer_count(),
        0,
        "回调内 cancel() 后不应被 reschedule_timer放回（D9）"
    );

    // 再触发一次：什么都不会发生
    force_due(&rt, win());
    let due2 = rt.take_due_timers(win(), Instant::now());
    assert!(due2.is_empty(), "周期定时器确实停下来了");
}

/// 回归（D9 另一半）：关窗时**正在执行回调**的周期定时器不能变成孤儿。
///
/// bug 表现：`cancel_timers_of` 删掉表里该窗口的定时器，但正在执行的那个不在表里；
/// 回调返回后 `reschedule_timer` 又把它推回表 ⇒ 永不触发的孤儿，
/// 且一直持有闭包捕获（内存泄漏）。
#[test]
fn closing_window_does_not_orphan_the_executing_interval_timer() {
    let rt = Runtime::new();
    rt.register_window(win());
    let hits = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = hits.clone();
    let _h = rt.set_interval(win(), Duration::from_millis(10), move |_| {
        c.set(c.get() + 1);
    });

    // 取出一个"正在执行"的定时器（此刻它已不在表里）
    force_due(&rt, win());
    let mut due = rt.take_due_timers(win(), Instant::now());
    assert_eq!(due.len(), 1);
    let mut timer = due.remove(0);

    // 回调"执行中"：取出 cb 并执行（不还回去，模拟 app.rs 的 `cb(&mut cx)` 那一刻）
    let mut cb = timer.take_cb().expect("应有回调");
    cb(&mut Ctx::new(&rt, win(), crate::event::EventView::tick()));
    assert_eq!(hits.get(), 1);

    // 回调执行期间关窗（此时 timer 已不在表里，`retain` 删不到它）
    rt.cancel_timers_of(win());
    rt.unregister_window(win());

    // app.rs 的 tick 随后把 cb 还回并放回表 —— 这一步必须被拦住
    timer.cb = Some(cb);
    rt.reschedule_timer(timer, Instant::now());
    assert_eq!(
        rt.timer_count(),
        0,
        "关窗后周期定时器不应回到表里（否则是永不触发的孤儿）"
    );
}

/// 照`WindowCtx::tick` 的方式执行一个到期定时器：取出 cb → 跑 → **还回**。
///
/// 这个"还回"是关键：`reschedule_timer` 靠它恢复回调，
/// 测试里漏掉就会因"cb 不在了"提前 return，让断言变成假通过。
fn run_one(timer: &mut Timer, rt: &Runtime) {
    let mut cb = timer.take_cb().expect("应有回调");
    cb(&mut Ctx::new(rt, win(), crate::event::EventView::tick()));
    timer.cb = Some(cb);
    rt.reschedule_timer(std::mem::replace(timer, dummy_timer()), Instant::now());
}

/// 换出用（`take` 后需要填回一个合法值）。
fn dummy_timer() -> Timer {
    Timer {
        id: u64::MAX,
        window: win(),
        deadline: Instant::now(),
        interval: None,
        cb: None,
    }
}

#[test]
fn animation_request_is_one_shot_and_drives_the_deadline() {
    let rt = Runtime::new();
    rt.register_window(win());
    assert!(!rt.animation_pending(win()));
    assert!(rt.next_deadline(win()).is_none());

    rt.request_animation(win());
    rt.request_animation(win()); // 幂等
    assert!(rt.animation_pending(win()));
    assert!(rt.next_deadline(win()).is_some(), "排了动画帧 ⇒ 有唤醒时刻");

    assert!(rt.take_animation_request(win()), "被取走后当帧消费掉");
    assert!(!rt.take_animation_request(win()), "一次性：不重复触发");
    assert!(!rt.animation_pending(win()));
}

#[test]
fn timers_are_per_window_and_cancelled_with_the_window() {
    let rt = Runtime::new();
    let a = WindowId::new(1);
    let b = WindowId::new(2);
    rt.register_window(a);
    rt.register_window(b);
    rt.set_timeout(a, Duration::from_secs(60), |_| {});
    rt.set_timeout(b, Duration::from_secs(60), |_| {});
    rt.request_animation(b);
    assert_eq!(rt.timer_count(), 2);

    rt.cancel_timers_of(a);
    assert_eq!(rt.timer_count(), 1, "只清 a 的");
    assert!(rt.next_deadline(a).is_none());
    assert!(rt.next_deadline(b).is_some());

    rt.cancel_timers_of(b);
    assert_eq!(rt.timer_count(), 0);
    assert!(!rt.animation_pending(b));
}
