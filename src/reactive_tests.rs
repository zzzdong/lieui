use super::*;

#[test]
fn set_marks_all_windows_and_take_clears() {
    let rt = Runtime::new();
    let a = WindowId::new(1);
    let b = WindowId::new(2);
    rt.register_window(a);
    rt.register_window(b);

    let s = Signal::new(&rt, 0);
    s.set(1);

    assert!(rt.take_dirty(a).contains(Dirty::VIEW));
    assert!(rt.take_dirty(b).contains(Dirty::VIEW));
    // 取走即清空
    assert!(rt.take_dirty(a).is_empty());
    assert!(rt.peek_dirty(b).is_empty());
}

// ── 主题模式 ──

#[test]
fn system_mode_follows_the_os_theme() {
    use crate::theme::{Theme, ThemeMode};
    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);
    let _ = rt.take_dirty(w);

    // 预设模式：直接应用对应 token 集
    rt.set_theme_mode(ThemeMode::Dark);
    assert_eq!(rt.theme(), Theme::dark());
    assert_eq!(rt.theme_mode(), ThemeMode::Dark);
    assert!(rt.take_dirty(w).contains(Dirty::VIEW), "换主题 ⇒ 重跑 view");

    // System：先按已上报的 OS 状态（默认浅色）
    rt.set_theme_mode(ThemeMode::System);
    assert_eq!(rt.theme(), Theme::light());

    // OS 转深色 ⇒ 自动跟随
    rt.set_system_dark(true);
    assert_eq!(rt.theme(), Theme::dark());
    assert!(rt.take_dirty(w).contains(Dirty::VIEW));

    // OS 转回浅色 ⇒ 再跟随
    rt.set_system_dark(false);
    assert_eq!(rt.theme(), Theme::light());

    // 切到 Custom 后，OS 变化不再影响生效主题
    rt.set_theme_mode(ThemeMode::Dark);
    rt.set_theme(Theme::light());
    assert_eq!(rt.theme_mode(), ThemeMode::Custom);
    rt.set_system_dark(true);
    assert_eq!(rt.theme(), Theme::light(), "自定义主题不受系统影响");
}

#[test]
fn system_mode_picks_up_an_already_reported_os_state() {
    use crate::theme::{Theme, ThemeMode};
    let rt = Runtime::new();
    // 先上报 OS 深色（平台层在窗口创建前/后都可能上报）
    rt.set_system_dark(true);
    assert_eq!(rt.theme(), Theme::light(), "非 System 模式不跟随");
    // 再切 System ⇒ 立即用上报值
    rt.set_theme_mode(ThemeMode::System);
    assert_eq!(rt.theme(), Theme::dark());
}

#[test]
fn update_writes_and_marks() {
    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);

    let s = Signal::new(&rt, 1);
    s.update(|v| *v += 41);
    assert_eq!(s.get(), 42);
    assert!(rt.take_dirty(w).contains(Dirty::VIEW));
}

#[test]
fn batching_is_free_one_flag_for_n_sets() {
    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);

    let a = Signal::new(&rt, 0);
    let b = Signal::new(&rt, 0);
    // 一次"事件"里改 N 个 signal
    a.set(1);
    b.set(2);
    a.update(|v| *v += 1);

    // 脏标志就一位，帧循环只消费一次视图重跑
    assert_eq!(rt.take_dirty(w), Dirty::VIEW);
}

#[test]
fn signal_is_clone_even_if_t_is_not() {
    struct NotClone(u32);
    let rt = Runtime::new();
    let s = Signal::new(&rt, NotClone(1));
    let s2 = s.clone();
    s2.set(NotClone(2));
    assert_eq!(s.with(|v| v.0), 2);
}

#[test]
fn with_avoids_clone() {
    let rt = Runtime::new();
    let s = Signal::new(&rt, vec![1, 2, 3]);
    assert_eq!(s.with(|v| v.len()), 3);
}

#[test]
fn take_resets_to_default() {
    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);
    let s: Signal<String> = Signal::new(&rt, "hi".into());
    assert_eq!(s.take(), "hi");
    assert_eq!(s.with(|v| v.clone()), "");
    assert!(rt.take_dirty(w).contains(Dirty::VIEW));
}

#[test]
fn unregistered_window_gets_nothing_and_does_not_panic() {
    let rt = Runtime::new();
    let w = WindowId::new(7);
    rt.register_window(w);
    let s = Signal::new(&rt, 0);
    s.set(1);
    rt.unregister_window(w);

    assert!(rt.take_dirty(w).is_empty());
    rt.mark_all(Dirty::PAINT); // 没有窗口，也不应 panic
    assert!(rt.take_dirty(w).is_empty());
}

#[test]
fn register_is_idempotent() {
    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);
    rt.register_window(w);
    assert_eq!(rt.windows(), vec![w]);
}

/// 回归（D8 下半）：`assert_not_in_view` 必须 **always-on**。
///
/// 这条测试此前带 `#[cfg(debug_assertions)]` —— **测试本身只在 debug 下存在**，
/// 这正是 D8 的证据：`assert_not_in_view` 被 `cfg!` 包着，release 下整条保护消失
/// ⇒ 用户在 `view()` 里 `set` 变成"每帧 view → set → 再 view"的**永久满帧自激**：
/// 100% CPU、不报错、无日志。
#[test]
#[should_panic(expected = "不能在 view() 内调用")]
fn set_inside_view_panics() {
    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);
    let s = Signal::new(&rt, 0);

    let _guard = rt.begin_view(w); // 模拟帧循环里的 view() 阶段
    s.set(1); // 应当 panic（debug 与 release 都一样）
}

#[test]
fn get_inside_view_is_fine_and_no_dirty_is_marked() {
    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);
    let s = Signal::new(&rt, 7);

    let _guard = rt.begin_view(w);
    assert_eq!(s.get(), 7);
    drop(_guard);

    // 只读不置脏
    assert!(rt.take_dirty(w).is_empty());
}

/// 回归（D8 上半）：`view()` 值守必须是 **RAII**。
///
/// bug 表现：`begin_view()` / `end_view()` 是手写成对调用，而 `view()` 是**用户代码**，
/// 它 panic 时 `end_view()` 永远不会执行 ⇒ `in_view` 永久停在 `Some(..)`
/// ⇒ 此后该窗口所有 `Signal::set` 都被 `assert_not_in_view` 拦下，Runtime 被永久毒化。
#[test]
fn view_guard_recovers_after_panic() {
    let rt = Runtime::new();
    let w = WindowId::new(1);
    rt.register_window(w);
    let s = Signal::new(&rt, 1u32);

    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {})); // 静音预期的 panic 输出
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = rt.begin_view(w);
        panic!("模拟用户 view() 里panic");
    }));
    std::panic::set_hook(prev);

    assert!(panicked.is_err(), "应当确实panic 了");
    assert!(
        !rt.is_in_view(),
        "panic 后 in_view 必须被守卫释放，否则 Runtime 被永久毒化"
    );
    // 关键：Runtime 仍可用
    s.set(2);
    assert_eq!(s.get(), 2);
}

#[test]
fn act_calls_the_method() {
    use std::cell::Cell;
    struct Vm {
        hits: Cell<u32>,
    }
    impl Vm {
        fn inc(&self) {
            self.hits.set(self.hits.get() + 1);
        }
    }
    let vm = Rc::new(Vm { hits: Cell::new(0) });
    let f = act(&vm, Vm::inc);
    f();
    f();
    assert_eq!(vm.hits.get(), 2);
}

#[test]
fn act1_passes_the_argument() {
    use std::cell::Cell;
    struct Vm {
        last: Cell<u32>,
    }
    impl Vm {
        fn pick(&self, id: u32) {
            self.last.set(id);
        }
    }
    let vm = Rc::new(Vm { last: Cell::new(0) });
    let f = act1(&vm, Vm::pick, 42);
    f();
    assert_eq!(vm.last.get(), 42);
}
// ─────────────────── A7 回归（D59） ───────────────────

/// 回归（D59）：注销窗口必须清掉 `window_sizes` 里的尺寸记录。
///
/// bug 表现：`set_window_size`（帧驱动每帧调用）只有 `push` / `find`，
/// **从无删除路径**，而 `unregister_window` 此前只删脏标志表
/// ⇒ **每次开关窗泄漏一条**（`WindowId` + `Size`）。
/// 长时间反复开关窗 ⇒ 稳步增长（且 `window_size()` 的线性查找越来越慢）。
#[test]
fn unregister_window_clears_the_recorded_size() {
    let rt = Runtime::new();
    let a = WindowId::new(1);
    rt.register_window(a);
    rt.set_window_size(a, lieui_geom::Size::new(200.0, 120.0));
    assert_eq!(rt.window_size(a), Some(lieui_geom::Size::new(200.0, 120.0)));

    rt.unregister_window(a);
    assert!(
        rt.window_size(a).is_none(),
        "D59：注销窗口后尺寸记录必须清除，否则每次开关窗泄漏一条"
    );
    // 脏标志表也照旧清空（原有行为不能回退）
    assert!(rt.windows().is_empty());
}

/// 反向：仍注册的窗口尺寸不受影响（确认上一条不是"清空全部"）。
#[test]
fn unregistering_one_window_keeps_the_other_size() {
    let rt = Runtime::new();
    let a = WindowId::new(1);
    let b = WindowId::new(2);
    rt.register_window(a);
    rt.register_window(b);
    rt.set_window_size(a, lieui_geom::Size::new(100.0, 50.0));
    rt.set_window_size(b, lieui_geom::Size::new(300.0, 200.0));

    rt.unregister_window(a);
    assert!(rt.window_size(a).is_none());
    assert_eq!(
        rt.window_size(b),
        Some(lieui_geom::Size::new(300.0, 200.0)),
        "另一个窗口的尺寸不该被动到"
    );
}
