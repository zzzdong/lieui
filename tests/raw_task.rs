//! **投递边界的编译期证明**（集成测试）。
//!
//! ## 这个文件为什么重要
//!
//! `tests/` 目录**只能看到 `lieui` 的公开 API**。所以本文件搭出的
//! "后台工作 + 进度 + 取消 + 结果回传"，是在证明：
//!
//! > **去掉框架的"任务"概念之后，投递原语依然足够。**
//!
//! 缺投递入口的话，这个文件**根本编译不过** —— 边界的把关交给编译器。
//!
//! 遮罩那半（自绘 spinner + 最短可见时间）在 `tests/spinner_modal.rs`。
//!
//! ## 框架提供什么 / 不提供什么
//!
//! | 提供 | 不提供（本文件自己写） |
//! |---|---|
//! | `Runtime::poster()` —— 把数据投回 UI 线程 | 线程模型（这里用 `std::thread`，换线程池/rayon/tokio 都行） |
//! | `Runtime::peek_dirty` / `is_online` —— 观测 | 取消协议（这里一个 `AtomicBool`） |
//! | 通道**不透明**（原样交给 `on_external`） | 进度协议（这里自定义 `Msg::Progress`） |

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use lieui::app::{App, ViewModel, erased};
use lieui::prelude::{ExternalData, Poster, Signal, ViewBuf, WindowConfig};
use lieui::{Runtime, WindowId};

/// 工作线程回传的消息（**调用方自己的类型**，不套框架信封）
enum Msg {
    Progress { done: usize, total: usize },
    Done(String),
}

struct Ui {
    /// 工作线程的结果落到这里
    result: Signal<Option<String>>,
    /// 最后一次收到的进度（应用自管 —— 框架不知道"进度"是什么）
    progress: Signal<Option<(usize, usize)>>,
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
                Msg::Progress { done, total } => self.progress.set(Some((*done, *total))),
                Msg::Done(text) => {
                    self.result.set(Some(text.clone()));
                    self.cancel.store(false, Ordering::SeqCst);
                }
            }
        }
    }
}

/// 调用方自己的"起一个后台工作"助手 —— 这就是原先 `spawn_task` 所在的位置。
///
/// 只用公开 API：`poster()` + `std::thread`。
fn spawn_work(rt: &Runtime, vm: &Rc<Ui>, win: WindowId, total: usize) {
    let poster: Poster = rt.poster();
    // ⚠️ 这里**不**重置 `cancel` —— 它是调用方的状态，何时清由调用方决定。
    //    （第一版在起手处 `store(false)`，把"起手就取消"那条测试的前提抹掉了。）
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

fn setup() -> (Runtime, App, Rc<Ui>, WindowId) {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());
    let vm = Rc::new(Ui {
        result: Signal::new(&rt, None),
        progress: Signal::new(&rt, None),
        cancel: Arc::new(AtomicBool::new(false)),
    });
    let id = app.window_erased(WindowConfig::new().size(400.0, 300.0), erased(Rc::clone(&vm)));
    app.frame_all();
    (rt, app, vm, id)
}

/// ★★★ 正向：只用公开原语跑通"后台工作 → 投递 → 落地"。
#[test]
fn background_work_is_built_entirely_from_public_primitives() {
    let (rt, mut app, vm, id) = setup();

    // 无头：投递落到本地队列，`frame_all` 就是"事件循环"
    assert!(!rt.is_online());

    spawn_work(&rt, &vm, id, 3);

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
    assert_eq!(vm.progress.get(), Some((3, 3)), "★ 进度也是调用方自己的协议");
}

/// ★ **反向**：没投递过 ⇒ 什么都没发生。
///
/// 只测上面那条不够 —— 一个"总是给个默认结果"的实现也会通过。
#[test]
fn nothing_lands_without_a_post() {
    let (_rt, mut app, vm, _id) = setup();
    for _ in 0..5 {
        app.frame_all();
    }
    assert_eq!(vm.result.get(), None);
    assert_eq!(vm.progress.get(), None);
}

/// ★ 取消：调用方的 `AtomicBool` 让工作线程尽快收敛，并把"已取消"投递回来。
#[test]
fn the_caller_can_cancel_its_own_work() {
    let (rt, mut app, vm, id) = setup();

    vm.cancel.store(true, Ordering::SeqCst); // 起手就取消
    spawn_work(&rt, &vm, id, 100);

    let deadline = Instant::now() + Duration::from_secs(5);
    while vm.result.get().is_none() && Instant::now() < deadline {
        app.frame_all();
        std::thread::sleep(Duration::from_millis(2));
    }
    app.frame_all();

    assert!(
        vm.result.get().as_deref().is_some_and(|s| s.starts_with("已取消")),
        "★ 取消是调用方自己的协议，结果照常投递回来（{:?}）",
        vm.result.get()
    );
}
