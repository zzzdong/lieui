//! 事件通道 + 统一时钟：自定义事件 / 定时器 / 逐帧动画。
//!
//! ```text
//! cargo run -p lieui --example event_clock
//! ```
//!
//! 演示三件事：
//!
//! 1. **自定义事件**：`rt.emit(win, ..)`（UI 线程）与 [`Emitter`]（`Send + Clone`，
//!    可挂在业务层或 move 进别的线程）走同一条管道，落在 `ViewModel::on_external`。
//! 2. **定时器**：`cx.set_timeout` / `cx.set_interval` —— UI 线程回调，可以改状态；
//!    平台层按时唤醒（没有定时器时是 `ControlFlow::Wait`，空闲零功耗）。
//! 3. **逐帧动画**：`cx.request_animation()` ⇒ 下一帧 `on_animation(cx, now, dt)`；
//!    想继续就在回调里再请求（经典 RAF 语义），停手即停帧。
//!
//! 注意动画的重绘姿势：自绘节点（[`CustomNode`]）的相位不经过保留树 ⇒ 用
//! `cx.damage_all()`（整窗脏）或 `cx.damage(node)`（精修），光 `request_repaint()`
//! 不会画（没有脏矩形）。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use lieui::prelude::*;

/// 自定义事件（枚举是推荐形态：一处定义，两边 match）
#[derive(Debug)]
enum ClockEvent {
    /// 重置相位
    Reset,
}

/// 自绘的旋转指示器：相位由 `on_animation` 推进，`draw` 读它
struct Dial {
    phase: f32,
}

impl CustomNode for Dial {
    fn intrinsic_size(&self) -> Size {
        Size::new(72.0, 72.0)
    }

    fn draw(&self, out: &mut Scene, rect: Rect, tr: Affine) {
        let center = rect.center();
        let r = rect.width.min(rect.height) * 0.5 - 4.0;
        // 12 根刻度，亮度按相位渐变（"转圈"的错觉来自亮度在跑）
        for i in 0..12 {
            let ang = i as f32 / 12.0 * std::f32::consts::TAU;
            let lit = 1.0 - ((i as f32 / 12.0 - self.phase).rem_euclid(1.0));
            let a = (60.0 + 195.0 * lit) as u8;
            let (s, c) = (ang.sin(), ang.cos());
            let dot = Rect::new(center.x + c * r - 2.0, center.y + s * r - 2.0, 4.0, 4.0);
            custom::fill_rect(out, dot, 2.0, Color::rgba(90, 120, 220, a), tr);
        }
    }
}

struct Demo {
    /// 运行时句柄（建定时器 / 发事件用）
    rt: Runtime,
    /// 自绘节点（`Rc<RefCell<_>>` 共享：框架拿 `CustomCell`，我们改相位）
    dial: Rc<RefCell<Dial>>,
    dial_cell: CustomCell,
    /// 已运行的秒数（定时器驱动 ⇒ 低频，用 Signal 让 view 重跑）
    seconds: Signal<u32>,
    /// 收到的事件（跨线程发射器投递）
    log: Signal<String>,
}

impl Demo {
    /// 业务侧：一个只会"发事件"的发射器（示例里直接同步发一次）
    fn emit_reset(tx: &Emitter<ClockEvent>) {
        tx.emit(ClockEvent::Reset);
    }
}

impl ViewModel for Demo {
    fn view(self: &Rc<Self>, v: &mut ViewBuf) {
        v.column(|c| {
            c.center();
            c.gap(14.0);
            c.text("事件通道 + 统一时钟").font_size(18.0);
            c.custom(&self.dial_cell); // 自绘：动画相位每帧变
            c.text(format!("已运行 {} 秒（interval 驱动）", self.seconds.get()))
                .font_size(13.0);
            c.text(self.log.get())
                .font_size(12.0)
                .color(Color::rgba(0x66, 0x66, 0x66, 255));

            let me = Rc::clone(self);
            c.button("开始 3 秒后重置相位").on_tap(move || {
                let Some(win) = me.rt.windows().first().copied() else {
                    return;
                };
                // 定时器：3 秒后在 UI 线程跑一次（可以改状态 / 发事件 / 起任务）
                me.rt.set_timeout(win, Duration::from_secs(3), move |cx| {
                    cx.emit(ClockEvent::Reset); // 自定义事件回到 on_external
                });
            });
        });
    }

    /// 动画帧：推进自绘相位并**再要一帧**（不请求就停）
    fn on_animation(self: &Rc<Self>, cx: &mut Ctx, _now: Instant, dt: Duration) {
        let mut dial = self.dial.borrow_mut();
        dial.phase = (dial.phase + dt.as_secs_f32() * 0.6).fract();
        cx.damage_all(); // 自绘不走保留树 ⇒ 整窗脏（精修可用 cx.damage(node)）
        cx.request_animation();
    }

    /// 自定义事件落地（`emit` / `Emitter` 都走这里）
    fn on_external(self: &Rc<Self>, cx: &mut Ctx, data: ExternalData) {
        if let Some(ev) = data.downcast::<ClockEvent>() {
            match ev {
                ClockEvent::Reset => {
                    self.dial.borrow_mut().phase = 0.0;
                    self.log.set("收到 ClockEvent::Reset（3 秒定时器发来的）".into());
                    cx.damage_all();
                }
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());

    let dial = Rc::new(RefCell::new(Dial { phase: 0.0 }));
    let vm = Rc::new(Demo {
        rt: rt.clone(),
        dial: Rc::clone(&dial),
        dial_cell: CustomCell::from_rc(Rc::clone(&dial)),
        seconds: Signal::new(&rt, 0),
        log: Signal::new(&rt, "（等待事件）".to_string()),
    });

    let id = app.window_erased(
        WindowConfig::new().title("lieui · 事件与时钟").size(520.0, 420.0),
        lieui::app::erased(Rc::clone(&vm)),
    );

    // 动画从此不必自己算时间：请求一帧就开始跑（见 on_animation）
    rt.request_animation(id);
    // 每秒一次的低频定时器（演示 `set_interval`）
    let vm2 = Rc::clone(&vm);
    rt.set_interval(id, Duration::from_secs(1), move |_| {
        vm2.seconds.update(|s| *s += 1);
    });

    // 业务侧发射器：可以在别的线程里 emit（这里演示跨线程）
    let tx = Emitter::<ClockEvent>::new(&rt, id);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(6));
        Demo::emit_reset(&tx);
    });

    app.run()
}
