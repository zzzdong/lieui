//! 后台工作 + loading 遮罩 —— **只用原语自己搭**。
//!
//! ```text
//! cargo run -p lieui --example background_task
//! ```
//!
//! ## 这个示例在演示一条**边界**
//!
//! lieui **不提供"任务"概念**：它不管线程、不管取消协议、不管进度协议、
//! 也不管"任务完成时收起遮罩"。它只给两样原语：
//!
//! | 原语 | 作用 |
//! |---|---|
//! | [`Runtime::poster`] → [`Poster`] | 把 `Send` 数据**投递**回 UI 线程并唤醒（`Runtime` 自己是 `!Send`） |
//! | [`Runtime::begin_busy`] → [`BusyToken`] | 挂一个 loading 遮罩，**何时收起由你决定** |
//!
//! 剩下的（起线程 / 取消 / 进度 / 收遮罩）全在这个文件里 —— 也就是"调用方的事"。
//! 想换线程池、换成 tokio、换成 rayon，只改这一段，**框架一行都不用动**。
//!
//! ## 与旧 API 的对照
//!
//! | 旧（框架提供的 `spawn_task_busy`） | 现在（本文件） |
//! |---|---|
//! | 框架 `std::thread::spawn` | 你自己 `std::thread::spawn` |
//! | 框架的 `CancelToken` | 你自己一个 `Arc<AtomicBool>` |
//! | `ctx.progress(done, total)` | 你自己投递一条 `Msg::Progress` |
//! | 载荷套框架的 `TaskEvent` 信封 | 直接投递你自己的 `Msg` |
//! | 框架"看到任务结束"就收遮罩 | **你收到结果时**收遮罩 |
//!
//! 关键机制不变：`Signal` / `Runtime` / `Ctx` 都是 `!Send`，工作线程只能
//! **投递 + 唤醒**；管道由框架接好（`platform::run` 注入唤醒器，
//! `WindowCtx::external` 在 UI 线程落地）。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lieui::prelude::*;

/// 工作线程回传的**调用方自己的消息**（`Send`）
enum Msg {
    /// 进度：驱动遮罩上的进度条（框架不知道"进度"是什么）
    Progress { done: usize, total: usize },
    /// 完成：带结果
    Done(Report),
    /// 任务体崩了（`catch_unwind` 兜底）—— 这是**调用方**的兜底策略
    Panicked,
}

/// 一段"耗时计算"的返回值（`Send` ⇒ 可以穿过线程边界）
struct Report {
    primes: usize,
    elapsed_ms: u128,
    cancelled: bool,
}

struct Demo {
    report: Signal<String>,
    done: Signal<u32>,
    /// ★ 调用方**自己持有**遮罩句柄 ⇒ 收到结果时才收它
    busy: RefCell<Option<BusyToken>>,
    /// ★ 取消协议也是调用方的（框架不再提供 `CancelToken`）
    cancel: Arc<AtomicBool>,
}

impl Demo {
    /// 起一个后台工作：**三段全是调用方的东西** —— 线程、取消、遮罩。
    fn start(&self, cx: &mut Ctx) {
        let win = cx.window();
        let poster = cx.poster();

        // ① 遮罩：谁开谁收。这里同时接上「取消」按钮 —— 点它只是置个标志，
        //    遮罩**不会**自动消失（由 `Msg::Done` 的处理负责收）
        self.cancel.store(false, Ordering::SeqCst);
        let busy = cx.begin_busy("正在统计 2000 万以内的质数…");
        {
            let flag = Arc::clone(&self.cancel);
            busy.cancellable(move || flag.store(true, Ordering::SeqCst));
        }
        *self.busy.borrow_mut() = Some(busy);

        // ② 线程：调用方自己选并发模型（线程池 / rayon / tokio 都行）
        let cancel = Arc::clone(&self.cancel);
        std::thread::spawn(move || {
            // panic 兜底也归调用方：不 catch 的话遮罩就永远挂着了
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let started = std::time::Instant::now();
                let limit = 20_000_000u64;
                let segments = 40u64;
                let mut primes = 0usize;
                let mut n = 2u64;

                for seg in 0..segments {
                    let end = limit * (seg + 1) / segments;
                    while n <= end {
                        if is_prime(n) {
                            primes += 1;
                        }
                        n += 1;
                    }
                    // ③ 进度 = 投递一条消息（`post` 顺带唤醒 UI 线程）
                    let _ = poster.post(
                        win,
                        Msg::Progress {
                            done: seg as usize + 1,
                            total: segments as usize,
                        },
                    );
                    // ④ 取消 = 读自己那个标志，协作式收敛
                    if cancel.load(Ordering::SeqCst) {
                        return Report {
                            primes,
                            elapsed_ms: started.elapsed().as_millis(),
                            cancelled: true,
                        };
                    }
                }
                Report {
                    primes,
                    elapsed_ms: started.elapsed().as_millis(),
                    cancelled: false,
                }
            }));

            let _ = match r {
                Ok(report) => poster.post(win, Msg::Done(report)),
                Err(_) => poster.post(win, Msg::Panicked),
            };
        });
    }
}

fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    let mut i = 2;
    while i * i <= n {
        if n.is_multiple_of(i) {
            return false;
        }
        i += 1;
    }
    true
}

impl ViewModel for Demo {
    fn view(self: &Rc<Self>, v: &mut ViewBuf) {
        v.column(|c| {
            c.center();
            c.gap(12.0);
            c.text("后台工作 + loading 遮罩（纯原语）").font_size(18.0);
            c.text("只用了 Runtime::poster + Runtime::begin_busy")
                .font_size(12.0)
                .color(Color::rgba(0x66, 0x66, 0x66, 255));
            c.text(format!("完成次数：{}", self.done.get())).font_size(13.0);
            c.text(self.report.get()).font_size(13.0);
            c.button("跑一个耗时任务").on_tap_with({
                let me = Rc::clone(self);
                move |cx| me.start(cx)
            });
        });
    }

    /// 消息落地：**只有一层 downcast**（没有框架信封）
    fn on_external(self: &Rc<Self>, _cx: &mut Ctx, data: ExternalData) {
        let Some(msg) = data.downcast::<Msg>() else {
            return;
        };
        match msg {
            Msg::Progress { done, total } => {
                if let Some(b) = self.busy.borrow().as_ref() {
                    b.set_progress(done, total);
                }
            }
            Msg::Done(r) => {
                self.report.set(format!(
                    "上批结果：{} 个质数，耗时 {} ms{}",
                    r.primes,
                    r.elapsed_ms,
                    if r.cancelled { "（被取消）" } else { "" }
                ));
                self.done.update(|d| *d += 1);
                self.cancel.store(false, Ordering::SeqCst);
                // ★ 遮罩由**调用方**收：框架连"有这么一个工作"都不知道
                if let Some(t) = self.busy.borrow_mut().take() {
                    t.finish();
                }
            }
            Msg::Panicked => {
                self.report.set("工作线程崩了（调用方兜底）".to_string());
                if let Some(t) = self.busy.borrow_mut().take() {
                    t.finish();
                }
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());

    let vm = Rc::new(Demo {
        report: Signal::new(&rt, "还没跑过".to_string()),
        done: Signal::new(&rt, 0),
        busy: RefCell::new(None),
        cancel: Arc::new(AtomicBool::new(false)),
    });

    let _id = app.window_erased(
        WindowConfig::new()
            .title("lieui · 后台工作（纯原语）")
            .size(600.0, 380.0),
        lieui::app::erased(Rc::clone(&vm)),
    );

    // `run()` 就够了：唤醒器由平台层注入；**没有任务表要维护**。
    app.run()
}
