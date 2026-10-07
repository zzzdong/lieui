//! **概念边界的编译期证明**（集成测试）。
//!
//! ## 这个文件为什么重要
//!
//! `tests/` 目录**只能看到 `lieui` 的公开 API**。所以本文件搭出的
//! "后台工作 + loading 遮罩 + 进度 + 取消 + 结果回传"，是在证明：
//!
//! > **去掉框架的"任务"概念之后，原语依然足够。**
//!
//! 如果原语不够用（缺投递入口、遮罩句柄不可从外部构造、……），
//! 这个文件**根本编译不过** —— 边界的把关交给了编译器，而不是靠约定。
//!
//! ## 框架提供什么 / 不提供什么
//!
//! | 提供 | 不提供（本文件自己写） |
//! |---|---|
//! | `Runtime::poster()` —— 把数据投回 UI 线程 | 线程模型（这里用 `std::thread`，换线程池/rayon/tokio 都行） |
//! | `Runtime::begin_busy()` —— 挂遮罩 | 取消协议（这里一个 `AtomicBool`） |
//! | `Runtime::is_busy()` / `busy_items()` —— 观测状态 | 进度协议（这里自定义 `Msg::Progress`） |
//! | 框架渲染遮罩 + 输入阻断 + 最短可见时间 | **何时收遮罩**（这里是"收到结果时"） |

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use lieui::app::{App, ViewModel, erased};
use lieui::prelude::{BusyToken, ExternalData, Poster, Signal, ViewBuf, WindowConfig};
use lieui::{Runtime, WindowId};

/// 工作线程回传的消息（**调用方自己的类型**，不套框架信封）
enum Msg {
    Progress { done: usize, total: usize },
    Done(String),
}

struct Ui {
    /// 工作线程的结果落到这里
    result: Signal<Option<String>>,
    /// ★ 调用方自己持有遮罩句柄：收到结果时才收
    busy: RefCell<Option<BusyToken>>,
    /// ★ 取消协议也是调用方的
    cancel: Arc<AtomicBool>,
}

impl ViewModel for Ui {
    fn view(self: &Rc<Self>, v: &mut ViewBuf) {
        v.column(|c| {
            c.text(format!("result={:?}", self.result.get()));
        });
    }

    fn on_external(self: &Rc<Self>, _cx: &mut lieui::Ctx, data: ExternalData) {
        // ★ 一层 downcast：通道是不透明的，没有框架信封要剥
        if let Some(p) = data.downcast_ref::<Msg>() {
            match p {
                Msg::Progress { done, total } => {
                    if let Some(b) = self.busy.borrow().as_ref() {
                        b.set_progress(*done, *total);
                    }
                }
                Msg::Done(text) => {
                    self.result.set(Some(text.clone()));
                    self.cancel.store(false, Ordering::SeqCst);
                    // ★ 遮罩由**调用方**收（框架连"有这么一个工作"都不知道）
                    if let Some(t) = self.busy.borrow_mut().take() {
                        t.finish();
                    }
                }
            }
        }
    }
}

/// 调用方自己的"起一个后台工作"助手 —— 这就是原先 `spawn_task_busy` 所在的位置。
///
/// 只用公开 API：`poster()` + `begin_busy()` + `std::thread`。
fn spawn_work(rt: &Runtime, vm: &Rc<Ui>, win: WindowId, total: usize) {
    let poster: Poster = rt.poster();

    // ① 遮罩：谁开谁收
    vm.cancel.store(false, Ordering::SeqCst);
    let busy = rt.begin_busy(win, "正在处理…");
    {
        let flag = Arc::clone(&vm.cancel);
        busy.cancellable(move || flag.store(true, Ordering::SeqCst));
    }
    *vm.busy.borrow_mut() = Some(busy);

    // ② 线程：调用方自己选并发模型
    let cancel = Arc::clone(&vm.cancel);
    std::thread::spawn(move || {
        for i in 1..=total {
            let _ = poster.post(win, Msg::Progress { done: i, total });
            if cancel.load(Ordering::SeqCst) {
                let _ = poster.post(win, Msg::Done(format!("已取消（{i}/{total}）")));
                return;
            }
        }
        let _ = poster.post(win, Msg::Done(format!("完成 {total} 项")));
    });
}

#[test]
fn background_work_is_built_entirely_from_public_primitives() {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());
    let vm = Rc::new(Ui {
        result: Signal::new(&rt, None),
        busy: RefCell::new(None),
        cancel: Arc::new(AtomicBool::new(false)),
    });
    let id = app.window_erased(WindowConfig::new().size(400.0, 300.0), erased(Rc::clone(&vm)));
    app.frame_all();

    // 无头：投递落到本地队列，`frame_all` 就是"事件循环"
    assert!(!rt.is_online());
    assert!(!rt.is_busy(id), "起手没有遮罩");

    spawn_work(&rt, &vm, id, 3);
    assert!(rt.is_busy(id), "遮罩已挂上（由调用方开的）");
    assert_eq!(rt.busy_items(id).len(), 1);
    assert!(rt.busy_items(id)[0].is_cancellable(), "取消按钮接的是调用方回调");

    // 跑"事件循环"直到结果落地
    let deadline = Instant::now() + Duration::from_secs(5);
    while vm.result.get().is_none() && Instant::now() < deadline {
        app.frame_all();
        std::thread::sleep(Duration::from_millis(2));
    }
    app.frame_all();

    assert_eq!(
        vm.result.get().as_deref(),
        Some("完成 3 项"),
        "★ 载荷原样到达（一层 downcast）"
    );
    assert!(!rt.is_busy(id), "★ 遮罩由调用方收掉了");
    assert!(rt.busy_items(id).is_empty());
}

/// ★ **反向**：遮罩在调用方**没收到结果**之前必须一直挂着。
///
/// 只测上面那条不够 —— 一个"立刻收掉遮罩"的实现也会让它通过。
#[test]
fn the_overlay_stays_until_the_caller_says_otherwise() {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());
    let vm = Rc::new(Ui {
        result: Signal::new(&rt, None),
        busy: RefCell::new(None),
        cancel: Arc::new(AtomicBool::new(false)),
    });
    let id = app.window_erased(WindowConfig::new().size(400.0, 300.0), erased(Rc::clone(&vm)));
    app.frame_all();

    // 只开遮罩，**不**起任何线程、不给任何结果
    let busy = rt.begin_busy(id, "永远等下去…");
    busy.set_progress(1, 10);
    *vm.busy.borrow_mut() = Some(busy);

    for _ in 0..10 {
        app.frame_all();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(rt.is_busy(id), "★ 没人说结束 ⇒ 遮罩一直在");
    assert_eq!(rt.busy_items(id)[0].ratio(), Some(0.1), "进度也在");
}

/// ★ 定时兜底：调用方**永远等不到**结果时，`dismiss_after` 必须自己收掉。
///
/// 这就是"遮罩的关闭由**调用方或定时器**决定"里的定时器那条 ——
/// 没有它，"网络请求永远不回来"就会让遮罩永久挡住 UI。
#[test]
fn dismiss_after_rescues_a_work_that_never_returns() {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());
    let vm = Rc::new(Ui {
        result: Signal::new(&rt, None),
        busy: RefCell::new(None),
        cancel: Arc::new(AtomicBool::new(false)),
    });
    let id = app.window_erased(WindowConfig::new().size(400.0, 300.0), erased(Rc::clone(&vm)));
    app.frame_all();

    let busy = rt.begin_busy(id, "正在等外部进程…");
    busy.dismiss_after(Duration::from_millis(30));
    *vm.busy.borrow_mut() = Some(busy);
    app.frame_all();
    assert!(rt.is_busy(id));

    // 帧驱动：`frame_all` 里会调 `reap_busy`，到点由它收
    let deadline = Instant::now() + Duration::from_secs(2);
    while rt.is_busy(id) && Instant::now() < deadline {
        app.frame_all();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!rt.is_busy(id), "★ 定时兜底收掉了它（调用方一直没 finish）");
}
