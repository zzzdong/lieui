//! M4 示例：**真窗口 + 真交互**（winit 事件循环 + softbuffer 局部上屏）。
//!
//! ```text
//! cargo run -p lieui --example window_counter
//! ```
//!
//! 演示四件事：
//! 1. `App::run()` 起事件循环，窗口尺寸/DPI 由 winit 校正；
//! 2. 点击按钮 → 闭包改 `Signal` → 下一帧只重绘那一小块（脏区 CPU 渲染）；
//! 3. `Tab` 切换焦点（框架默认行为，键盘聚焦才画焦点框）；
//! 4. 后台线程通过 `RepaintHandle` 投递数据（`Signal` 是 `!Send`，只能"投递 + 唤醒"）。

use std::rc::Rc;
use std::time::Duration;

use lieui::app::ExternalData;
use lieui::prelude::*;

/// 后台线程投递的数据
struct Tick;

struct Counter {
    count: Signal<i32>,
    volume: Signal<f32>,
    agree: Signal<bool>,
}

impl Counter {
    fn inc(&self) {
        self.count.update(|v| *v += 1);
    }
}

impl ViewModel for Counter {
    fn view(self: &Rc<Self>, v: &mut ViewBuf) {
        v.column(|c| {
            c.center();
            c.gap(12.0);
            c.text("Counter（点按钮 / Tab 切焦点 / 每秒 +1）").font_size(16.0);
            c.text(self.count.get().to_string())
                .font_size(72.0)
                .color(Color::RED);
            c.row(|r| {
                r.gap(12.0);
                r.button("+1")
                    .tab_stop(true)
                    .on_tap(act(self, Self::inc));
            });

            // ── 双向绑定（M5）：拖动/点击会写回 Signal ──
            c.text(format!("音量 {:.1}", self.volume.get())).font_size(14.0);
            c.slider_bound(&self.volume, 0.0, 10.0);
            c.checkbox_bound(&self.agree);
        });
    }

    /// 后台线程投递的数据在这里落到 `Signal`（`Signal` 是 `!Send`，只能这么写）
    fn on_external(self: &Rc<Self>, _cx: &mut Ctx, data: ExternalData) {
        if data.downcast::<Tick>().is_some() {
            self.inc();
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());

    let vm = Rc::new(Counter {
        count: Signal::new(&rt, 0),
        volume: Signal::new(&rt, 3.0),
        agree: Signal::new(&rt, false),
    });
    let id = app.window_erased(
        WindowConfig::new()
            .title("lieui v3 · counter（M4）")
            .size(480.0, 320.0)
            .min_size(320.0, 220.0),
        lieui::app::erased(Rc::clone(&vm)),
    );

    // `RepaintHandle` 必须在事件循环建好之后才能拿到（`EventLoopProxy` 由它创建）
    app.run_with_handle(move |handle| {
        // 后台线程：每秒投递一次数据并唤醒 UI 线程（`RepaintHandle` 是 `Send`）
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(1));
                if !handle.post_external(id, ExternalData::new(Tick)) {
                    break; // 事件循环已退出
                }
            }
        });
    })
}
