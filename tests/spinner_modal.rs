//! **概念边界的编译期证明**：只用公开 API 做"自绘 spinner 的 loading 遮罩"。
//!
//! ## 这个文件要证明什么
//!
//! 库**不需要**内置"忙碌 / 遮罩"概念。它是个**组件**：
//!
//! | 谁的事 | 用什么 |
//! |---|---|
//! | 显示还是消失 | 应用状态 `Signal<bool>` —— **调用者决定** |
//! | 层从哪来 | `ViewBuf::modal_tagged` —— 应用在 `view()` 里声明 |
//! | 每帧重绘自己 | `Ctx::damage_key` + `Ctx::request_animation`（通用机制） |
//! | 最短可见时间 | `Runtime::set_timeout`（通用机制，不是特例 API） |
//! | 长什么样 | 应用的 `CustomNode`（自绘） |
//!
//! 如果这些原语不够用，**这个文件根本编译不过** —— 边界的把关交给编译器。
//!
//! ## 对照：库当前内置的那一套（本文件刻意不用）
//!
//! `Runtime::begin_busy` / `BusyToken` / `BusyItem` / `set_busy_min_visible` /
//! `overlay::BUSY_OVERLAY_TAG` / `WindowConfig::auto_busy_overlay` ——
//! 它们把同一个组件钉进了 `RuntimeInner`、`WindowCtx::frame` 与 `next_wakeup`。

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use lieui::app::{App, ViewModel, erased};
use lieui::prelude::*;
use lieui::{Runtime, WindowId};

/// 遮罩层根的标签 —— **本应用自己定的**（库不再预置 `BUSY_OVERLAY_TAG`）
const SPINNER_LAYER: u64 = 0x5f53_5049_4e4e_4552;
/// spinner 节点的 `Key`（`damage_key` 靠它定位；`on_animation` 里拿不到 `NodeId`）
const SPINNER_KEY: &str = "spinner";

/// 自绘 spinner：相位来自**单调时钟**。
///
/// 挂钟（`SystemTime`）会被 NTP 校时回拨 ⇒ 取模结果倒退 ⇒ spinner 倒转/跳帧。
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

/// 应用状态 —— "**忙碌状态由谁持有**"的答案就是这里：一个普通 `Signal<bool>`。
struct Ui {
    loading: Signal<bool>,
    /// 起始时刻（**最短可见时间**要用；这也是应用状态，不是库的 API）
    started: Cell<Option<Instant>>,
    /// ★ spinner 实例必须**跨帧留着**：`CustomCell` 按 `Rc` 指针判等，
    ///   每帧新建 cell 会被 `align` 当成"换数据"而重建 ⇒ 动画一直在原地重来。
    spinner: CustomCell,
    /// 应用的策略：遮罩至少显示这么久（快活儿也要被看见）
    min_visible: Duration,
}

impl Ui {
    fn new(rt: &Runtime) -> Self {
        let accent = Color::rgba(0x2f, 0x6f, 0xed, 255);
        Self {
            loading: Signal::new(rt, false),
            started: Cell::new(None),
            spinner: custom::cell(Spinner {
                started: Instant::now(),
                color: accent,
            }),
            min_visible: Duration::from_millis(40),
        }
    }

    /// 开始：应用自己开遮罩
    fn begin(&self) {
        self.started.set(Some(Instant::now()));
        self.loading.set(true);
    }

    /// 结束：**调用方决定**；"最短可见时间"用通用的 `set_timeout` 表达。
    ///
    /// 库里原先为这件事有个特例 API（`Runtime::set_busy_min_visible`）——
    /// 它其实就是"到点再置一次状态"。
    fn finish(self: &Rc<Self>, rt: &Runtime, id: WindowId) {
        let Some(t0) = self.started.get() else {
            return;
        };
        let left = self.min_visible.saturating_sub(t0.elapsed());
        if left.is_zero() {
            self.loading.set(false);
            return;
        }
        let me = Rc::clone(self);
        rt.set_timeout(id, left, move |_| me.loading.set(false));
    }
}

impl ViewModel for Ui {
    fn view(self: &Rc<Self>, v: &mut ViewBuf) {
        v.column(|c| {
            c.center();
            c.text("主界面").font_size(20.0);
        });

        // ★ 遮罩 = 一句普通的声明式代码（**库不知道"遮罩"是什么**）
        if self.loading.get() {
            let spinner = self.spinner.clone();
            v.modal_tagged(SPINNER_LAYER, |m| {
                m.center();
                m.container(|card| {
                    card.width(220.0);
                    card.padding(20.0);
                    card.background(Color::WHITE);
                    card.radius(12.0);
                    card.layout(|l| l.flex_shrink = 0.0);
                    card.row(|r| {
                        r.gap(10.0);
                        r.custom(&spinner).key(SPINNER_KEY);
                        r.text("正在处理…");
                    });
                });
            });
        }
    }

    /// RAF：只要遮罩在，就一直要下一帧，并且**只**把自己的节点标脏。
    fn on_animation(self: &Rc<Self>, cx: &mut Ctx, _now: Instant, _dt: Duration) {
        if !self.loading.get() {
            return; // 收起后不再请求 ⇒ 自然停帧（空闲零功耗）
        }
        cx.damage_key(SPINNER_KEY); // ★ 不是 `damage_all`
        cx.request_animation(); // ★ 再要下一帧
    }
}

fn setup() -> (Runtime, App, Rc<Ui>, WindowId) {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());
    let vm = Rc::new(Ui::new(&rt));
    let id = app.window_erased(WindowConfig::new().size(400.0, 300.0), erased(Rc::clone(&vm)));
    app.frame_all();
    (rt, app, vm, id)
}

fn overlay_present(app: &App, id: WindowId) -> bool {
    app.window_ctx(id).unwrap().track().root_by_tag(SPINNER_LAYER).is_some()
}

/// ★★★ 正向：只用公开原语，遮罩"出现 → 每帧只重绘自己 → 由调用方收起"。
#[test]
fn a_loading_overlay_is_built_from_public_primitives_only() {
    let (rt, mut app, vm, id) = setup();
    assert!(!overlay_present(&app, id), "起手没有遮罩");

    // ① 应用开遮罩（普通状态）+ 要第一帧
    vm.begin();
    rt.request_animation(id);
    let st = app.frame_all().remove(0).1;
    assert!(st.view_ran, "状态变了 ⇒ 重跑 view");
    assert!(overlay_present(&app, id), "★ 声明了 Modal 层 ⇒ 层出现");

    // ② 动画帧：**只有 spinner 脏**（这是 `damage_key` 的全部价值）
    let st = app.frame_all().remove(0).1;
    assert!(!st.view_ran, "动画帧不该重跑 view（`damage_key` 只置 PAINT|PRESENT）");
    assert!(!st.damage_all, "★ 不是整窗脏 —— `damage_all` 会让每帧全屏重绘");
    assert!(!st.damage.is_empty(), "spinner 标了脏");
    assert!(st.render.raster.pixels > 0, "确实重绘了");
    // 实测 **324 像素**（= spinner 节点自己的 18×18 矩形），整窗是 400×300 = 120000。
    // `damage_key` 只标**该节点的矩形** ⇒ 自绘必须画在自己的 `rect` 内
    // （画到外面就会留残影 —— 这是这个原语的契约）。
    assert!(
        st.render.raster.pixels < 1_000,
        "★ 只重绘 spinner 自己的 18×18（{} 像素；整窗是 120000）",
        st.render.raster.pixels
    );

    // ③ 调用方收起：**普通状态驱动**，层由 `align` 的 stale 清理删掉
    vm.loading.set(false);
    app.frame_all();
    assert!(!overlay_present(&app, id), "★ 状态没了 ⇒ 层消失");

    // ④ 停帧：`on_animation` 不再请求 ⇒ 回到空闲
    assert!(
        !app.pump(Instant::now()),
        "★ 收起后回到零唤醒（没有动画请求、没有脏标志）"
    );
}

/// ★ **反向**：没人说结束 ⇒ 遮罩必须一直在（且一直在转）。
///
/// 只测上面那条不够 —— 一个"下一帧就把遮罩收掉"的实现也会通过。
#[test]
fn the_overlay_stays_until_the_caller_says_otherwise() {
    let (rt, mut app, vm, id) = setup();

    vm.begin();
    rt.request_animation(id);
    for _ in 0..5 {
        app.frame_all();
    }
    assert!(overlay_present(&app, id), "★ 没人说结束 ⇒ 遮罩一直在");
    assert!(app.pump(Instant::now()), "还在转 ⇒ 仍然要出帧（动画请求未停）");

    // 说结束
    vm.loading.set(false);
    app.frame_all();
    assert!(!overlay_present(&app, id));
}

/// ★ **最短可见时间**是应用策略，用通用机制（`set_timeout`）表达 ——
/// 不需要库为它开一个特例 API。
#[test]
fn the_minimum_visible_time_is_app_policy_via_set_timeout() {
    let (rt, mut app, vm, id) = setup();

    vm.begin();
    rt.request_animation(id);
    app.frame_all();
    assert!(overlay_present(&app, id));

    // 立刻"完成"：因为配了最短可见时间，遮罩**这帧不能收**
    vm.finish(&rt, id);
    app.frame_all();
    assert!(
        overlay_present(&app, id),
        "★ 未到最短可见时间 ⇒ 仍在（快活儿也要被看见一下）"
    );

    // 越过最短可见时间 → 定时器把它收掉（帧驱动）
    let deadline = Instant::now() + Duration::from_secs(2);
    while overlay_present(&app, id) && Instant::now() < deadline {
        app.frame_all();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!overlay_present(&app, id), "★ 到点自己收起（应用策略生效）");
}
