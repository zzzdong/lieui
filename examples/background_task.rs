//! 后台工作 + loading 遮罩 —— **全部由应用实现**。
//!
//! ```text
//! cargo run -p lieui --example background_task
//! ```
//!
//! ## 这个示例在演示一条边界
//!
//! lieui **不提供"任务"概念，也不提供"遮罩"概念** —— 两者都是**应用的**决定：
//!
//! | 谁的事 | 用什么 |
//! |---|---|
//! | 线程 / 取消 / 进度 | 应用自己（这里 `std::thread` + `AtomicBool` + 自定义消息） |
//! | 遮罩显示还是消失 | 应用状态 `Signal<bool>` —— **调用者决定** |
//! | 层从哪来 | `ViewBuf::modal_tagged` —— 在 `view()` 里声明 |
//! | 每帧重绘自己 | `Ctx::damage_key` + `Ctx::request_animation`（通用机制） |
//! | 最短可见时间 | `Ctx::set_timeout`（通用机制，不是特例 API） |
//! | 长什么样 | 本文件的 `Spinner`（自绘 `CustomNode`） |
//!
//! 库只给那些**通用**能力。想换线程池 / rayon / tokio，只改 [`Demo::start`] 一段。
//!
//! ## 与旧 API 的对照（那一套已从库里移除）
//!
//! | 旧（库内置） | 现在（本文件） |
//! |---|---|
//! | `cx.spawn_task_busy(label, \|ctx\| ..)` | `std::thread::spawn` + `poster.post` |
//! | `ctx.progress(done, total)` | 自定义 `Msg::Progress` |
//! | `BusyToken` / `begin_busy` | `Signal<bool>` + `view()` 里的 `if` |
//! | `set_busy_min_visible` | `set_timeout` |
//! | 库内置的 spinner + 卡片 | 本文件的 `Spinner` + 卡片 DSL |
//!
//! 关键机制不变：`Signal` / `Runtime` / `Ctx` 都是 `!Send`，工作线程只能
//! **投递 + 唤醒**；管道由库接好（`platform::run` 注入唤醒器，
//! `WindowCtx::external` 在 UI 线程落地）。

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use lieui::prelude::*;

/// 遮罩层根的标签（**应用自己定的**）
const BUSY_LAYER: u64 = 0x6261_636b_6772_6f75; // "backgrou"
/// spinner 节点的 `Key`（`damage_key` 靠它定位；`on_animation` 里拿不到 `NodeId`）
const SPINNER_KEY: &str = "spinner";

/// 自绘 spinner：相位取自**单调时钟**（挂钟会被 NTP 校时回拨 ⇒ 倒转/跳帧）。
struct Spinner {
    started: Instant,
    color: Color,
}

impl CustomNode for Spinner {
    fn intrinsic_size(&self) -> Size {
        Size::new(18.0, 18.0)
    }

    fn draw(&self, out: &mut Scene, rect: Rect, transform: Affine) {
        const N: usize = 3;
        const CYCLE_MS: u128 = 300;
        let gap = 3.0_f32;
        let dot = ((rect.width - gap * (N as f32 - 1.0)) / N as f32)
            .min(rect.height)
            .max(1.0);
        let total = N as f32 * dot + (N as f32 - 1.0) * gap;
        let x0 = rect.x + (rect.width - total) * 0.5;
        let y = rect.y + (rect.height - dot) * 0.5;
        let phase = (self.started.elapsed().as_millis() % (CYCLE_MS * N as u128)) as f32 / CYCLE_MS as f32;

        for i in 0..N {
            let delta = (phase - i as f32).rem_euclid(N as f32);
            let k = (1.0_f32 - delta).clamp(0.0, 1.0);
            let alpha = (70.0 + 185.0 * k) as u8;
            custom::fill_rect(
                out,
                Rect::new(x0 + i as f32 * (dot + gap), y, dot, dot),
                dot * 0.5,
                Color::rgba(self.color.r, self.color.g, self.color.b, alpha),
                transform,
            );
        }
    }
}

/// 工作线程回传的**调用方自己的消息**
enum Msg {
    Progress { done: usize, total: usize },
    Done(Report),
    Panicked,
}

/// 一段"耗时计算"的返回值（`Send` ⇒ 可以穿过线程边界）
struct Report {
    primes: usize,
    elapsed_ms: u128,
    cancelled: bool,
}

/// 遮罩至少显示这么久（**应用策略**：快活儿也要被看见一下）
const MIN_VISIBLE: Duration = Duration::from_millis(400);

struct Demo {
    report: Signal<String>,
    done: Signal<u32>,
    /// ★ 遮罩"由谁持有"的答案：就是这里 —— 一个普通 `Signal<bool>`
    loading: Signal<bool>,
    /// 进度（也是应用状态）
    progress: Signal<Option<(usize, usize)>>,
    /// 起始时刻（算最短可见时间；应用状态）
    started: Cell<Option<Instant>>,
    /// ★ spinner 实例必须**跨帧留着**：`CustomCell` 按 `Rc` 指针判等，
    ///   每帧新建会被 `align` 当成"换数据"而重建 ⇒ 动画一直在原地重来。
    spinner: CustomCell,
    /// ★ 取消协议也是调用方的（库不再提供 `CancelToken`）
    cancel: Arc<AtomicBool>,
}

impl Demo {
    /// 三段全是应用的东西：线程、取消、遮罩。
    fn start(&self, cx: &mut Ctx) {
        let win = cx.window();
        let poster = cx.poster();

        // ① 遮罩：**调用者决定显示**
        self.cancel.store(false, Ordering::SeqCst);
        self.started.set(Some(Instant::now()));
        self.progress.set(None);
        self.loading.set(true);

        // ② 线程：调用方自己选并发模型（线程池 / rayon / tokio 都行）
        let cancel = Arc::clone(&self.cancel);
        std::thread::spawn(move || {
            // panic 兜底也归调用方：不 catch 的话遮罩就永远挂着了
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let started = Instant::now();
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

    /// 收起遮罩：**调用者决定**；"最短可见时间"用通用的 `set_timeout` 表达。
    ///
    /// 库里原先为这件事有个特例 API（`Runtime::set_busy_min_visible`）——
    /// 它其实就是"到点再置一次状态"。
    fn finish(self: &Rc<Self>, cx: &mut Ctx) {
        let Some(t0) = self.started.get() else {
            return;
        };
        let left = MIN_VISIBLE.saturating_sub(t0.elapsed());
        if left.is_zero() {
            self.loading.set(false);
            return;
        }
        let me = Rc::clone(self);
        cx.set_timeout(left, move |_| me.loading.set(false));
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
            c.text("后台工作 + loading 遮罩（全部由应用实现）").font_size(18.0);
            c.text("库只提供投递 / 定时 / 层 / 自绘这些通用能力")
                .font_size(12.0)
                .color(Color::rgba(0x66, 0x66, 0x66, 255));
            c.text(format!("完成次数：{}", self.done.get())).font_size(13.0);
            c.text(self.report.get()).font_size(13.0);
            c.button("跑一个耗时任务").on_tap_with({
                let me = Rc::clone(self);
                move |cx| me.start(cx)
            });
        });

        // ★ 遮罩 = 一句普通的声明式代码（**库不知道"遮罩"是什么**）
        if self.loading.get() {
            let spinner = self.spinner.clone();
            let progress = self.progress.get();
            v.modal_tagged(BUSY_LAYER, |m| {
                m.center();
                m.container(|card| {
                    card.width(300.0);
                    card.padding(20.0);
                    card.background(Color::WHITE);
                    card.radius(12.0);
                    card.layout(|l| l.flex_shrink = 0.0);
                    card.row(|r| {
                        r.gap(10.0);
                        r.custom(&spinner).key(SPINNER_KEY);
                        r.column(|col| {
                            col.gap(4.0);
                            col.text("正在统计 2000 万以内的质数…");
                            col.text(match progress {
                                Some((d, t)) => format!("{d} / {t} 段"),
                                None => "准备中…".to_string(),
                            })
                            .font_size(12.0)
                            .color(Color::rgba(0x66, 0x66, 0x66, 255));
                        });
                    });
                });
            });
        }
    }

    /// 消息落地：**只有一层 downcast**（没有框架信封）
    fn on_external(self: &Rc<Self>, cx: &mut Ctx, data: ExternalData) {
        let Some(msg) = data.downcast::<Msg>() else {
            return;
        };
        match msg {
            Msg::Progress { done, total } => self.progress.set(Some((done, total))),
            Msg::Done(r) => {
                self.report.set(format!(
                    "上批结果：{} 个质数，耗时 {} ms{}",
                    r.primes,
                    r.elapsed_ms,
                    if r.cancelled { "（被取消）" } else { "" }
                ));
                self.done.update(|d| *d += 1);
                self.cancel.store(false, Ordering::SeqCst);
                // ★ 遮罩由**调用方**收：库连"有这么一个工作"都不知道
                self.finish(cx);
            }
            Msg::Panicked => {
                self.report.set("工作线程崩了（调用方兜底）".to_string());
                self.finish(cx);
            }
        }
    }

    /// RAF：遮罩在就一直要下一帧，并且**只**把自己的节点标脏。
    fn on_animation(self: &Rc<Self>, cx: &mut Ctx, _now: Instant, _dt: Duration) {
        if !self.loading.get() {
            return; // 收起后不再请求 ⇒ 自然停帧（空闲零功耗）
        }
        cx.damage_key(SPINNER_KEY); // ★ 不是 `damage_all`
        cx.request_animation();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());

    let accent = rt.theme().accent;
    let vm = Rc::new(Demo {
        report: Signal::new(&rt, "还没跑过".to_string()),
        done: Signal::new(&rt, 0),
        loading: Signal::new(&rt, false),
        progress: Signal::new(&rt, None),
        started: Cell::new(None),
        spinner: custom::cell(Spinner {
            started: Instant::now(),
            color: accent,
        }),
        cancel: Arc::new(AtomicBool::new(false)),
    });

    let _id = app.window_erased(
        WindowConfig::new()
            .title("lieui · 后台工作（应用自实现）")
            .size(640.0, 400.0),
        lieui::app::erased(Rc::clone(&vm)),
    );

    // `run()` 就够了：唤醒器由平台层注入；**没有任务表要维护**。
    app.run()
}
