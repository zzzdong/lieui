//! 后台任务 + loading 遮罩（跨线程通信的完整链路）。
//!
//! ```text
//! cargo run -p lieui --example background_task
//! ```
//!
//! 演示四件事（**不需要**自己传 `RepaintHandle`，也不需要手写遮罩）：
//!
//! 1. `rt.spawn_task_busy(win, "…", |ctx| { .. })` —— 耗时计算跑在工作线程，UI 不卡；
//!    框架自动挂 loading 遮罩，任务结束（成功 / 失败 / 被取消）遮罩自动收起。
//! 2. `ctx.progress(done, total)` —— 任务内上报进度 ⇒ 遮罩上的进度条实时走。
//! 3. 遮罩上的「取消」按钮默认就接好了 ⇒ 点它等于 `TaskHandle::cancel()`，
//!    任务里轮询 `ctx.is_cancelled()` 协作式收敛。
//! 4. 任务返回值经 `TaskEvent` 回到 UI 线程（`on_external`），落到 `Signal`。
//!
//! 关键机制：`Signal` / `Runtime` / `Ctx` 都是 `!Send`，工作线程只能"投递 + 唤醒"
//! —— 详见 `lieui::task` 的模块文档。管道由框架接好：`platform::run` 把 winit 的
//! `EventLoopProxy` 注入 `Runtime`，`WindowCtx::external` 负责在 UI 线程落地。

use std::rc::Rc;

use lieui::prelude::*;

/// 一段"耗时计算"的返回值（`Send` ⇒ 可以穿过线程边界）
struct Report {
    primes: usize,
    elapsed_ms: u128,
    cancelled: bool,
}

struct Demo {
    report: Signal<String>,
    done: Signal<u32>,
}

impl Demo {
    /// 起一个后台任务（按钮处理里调用）。
    ///
    /// `Ctx::spawn_task_busy` = `Runtime::spawn_task_busy(window, ..)` + 当前窗口 +
    /// 自动挂遮罩；想自己管线程时用 `rt.spawn_task(win, ..)` / `rt.waker()`。
    fn start(&self, cx: &mut Ctx) {
        cx.spawn_task_busy("正在统计 2000 万以内的质数…", |ctx| {
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
                // ① 上报进度：驱动遮罩上的进度条
                ctx.progress(seg as usize + 1, segments as usize);
                // ② 用户点了遮罩上的「取消」⇒ 尽快收敛（协作式）
                if ctx.is_cancelled() {
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
            c.text("后台任务 + loading 遮罩").font_size(18.0);
            c.text("点按钮起一个耗时任务；遮罩上的「取消」可中断它")
                .font_size(12.0)
                .color(Color::rgba(0x66, 0x66, 0x66, 255));
            c.text(format!("完成任务数：{}", self.done.get())).font_size(13.0);
            c.text(self.report.get()).font_size(13.0);
            c.button("跑一个耗时任务").on_tap_with({
                let me = Rc::clone(self);
                move |cx| me.start(cx)
            });
        });
    }

    /// 任务回传（框架**先**收尾：清任务表 + 收遮罩，再把事件交给这里）
    fn on_external(self: &Rc<Self>, _cx: &mut Ctx, data: ExternalData) {
        // 用 `downcast_ref`：一条消息可能是完成事件，也可能是失败事件
        if let Some(ev) = data.downcast_ref::<TaskEvent>() {
            if let Some(r) = ev.payload.downcast_ref::<Report>() {
                self.report.set(format!(
                    "上批结果：{} 个质数，耗时 {} ms{}",
                    r.primes,
                    r.elapsed_ms,
                    if r.cancelled { "（被取消）" } else { "" }
                ));
                self.done.update(|d| *d += 1);
            }
        } else if let Some(f) = data.downcast_ref::<TaskFailed>() {
            // 任务 panic 的兜底事件（这里只记一笔；不处理也不会卡住遮罩）
            self.report.set(format!("任务 {} 崩了（已收尾）", f.id));
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());

    let vm = Rc::new(Demo {
        report: Signal::new(&rt, "还没跑过任务".to_string()),
        done: Signal::new(&rt, 0),
    });

    let _id = app.window_erased(
        WindowConfig::new().title("lieui · 后台任务").size(560.0, 360.0),
        lieui::app::erased(Rc::clone(&vm)),
    );

    // `run()` 就够了：唤醒器由平台层注入（想自己管线程时用 `rt.waker()` /
    // `rt.spawn_task(..)` / `run_with_handle` 都行）。
    app.run()
}
