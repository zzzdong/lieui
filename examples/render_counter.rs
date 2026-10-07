//! M3 无窗口示例：跑一帧完整管线（`view()` → 对齐 → 布局 → 展开 → 光栅化），
//! 打印统计并把 pixmap 导出成 PNG。
//!
//! ```text
//! cargo run -p lieui --example render_counter
//! ```
//!
//! 这是 `docs/architecture-v3.md` §九 counter 例子的"渲染版"：
//! 还没有 winit（M4），所以帧是手动驱动的，但**像素是真的**。

use std::rc::Rc;

use lieui::prelude::*;
use lieui::render::to_vello;

struct Counter {
    count: Signal<i32>,
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
            c.text("Counter").font_size(48.0);
            c.text(self.count.get().to_string()).font_size(72.0).color(Color::RED);
            c.row(|r| {
                r.gap(12.0);
                r.button("+1").on_tap(act(self, Self::inc));
            });
        });
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());
    let vm = Rc::new(Counter {
        count: Signal::new(&rt, 0),
    });
    let id = app.window_erased(
        WindowConfig::new().title("Counter").size(400.0, 300.0),
        lieui::app::erased(Rc::clone(&vm)),
    );

    // ① 首帧：view() → 对齐 → 布局 → 展开 → 光栅化（整窗）
    let first = app.frame_all();
    println!("首帧       : {:?}", first[0].1);
    let (ops, pixels) = (first[0].1.render.ops, first[0].1.rasterized_pixels());
    println!("展开 op    : {ops}，光栅化像素 {pixels}");

    // ② 直接改状态（等价于点了一次按钮），只置脏
    vm.inc();
    println!("set 后脏标志: {:?}", rt.peek_dirty(id));

    // ③ 第二帧：`view()` 重跑 → 对齐只 patch 那一个文本节点 → 只重画它的脏区
    let second = app.frame_all()[0].1.clone();
    println!("第二帧     : {:?}", second);
    println!(
        "  对齐 patch={} created={}，重排边界={}，光栅化 {} 像素（占整窗 {:.1}%）",
        second.align.patched,
        second.align.created,
        second.layout.boundaries,
        second.rasterized_pixels(),
        100.0 * second.rasterized_pixels() as f64 / (400.0 * 300.0),
    );

    // ④ 第三帧：无变化 ⇒ 完全空闲
    let idle = app.frame_all()[0].1.clone();
    println!("空闲帧     : is_idle={}", idle.is_idle());
    assert!(idle.is_idle());

    // ⑤ 导出 PNG（纯 CPU 渲染的可视化证据）
    let ctx = app.window_ctx(id).expect("窗口存在");
    let pix = ctx.pixmap();
    let mut painted = 0usize;
    let mut distinct = std::collections::HashSet::new();
    for p in pix.data() {
        distinct.insert((p.r, p.g, p.b, p.a));
        if !(p.r == 240 && p.g == 240 && p.b == 240) {
            painted += 1;
        }
    }
    println!(
        "pixmap     : {}×{}，非底色像素 {painted}，不同颜色 {} 种",
        pix.width(),
        pix.height(),
        distinct.len()
    );
    let png = pix.clone().into_png()?;
    let path = std::env::temp_dir().join("lieui_counter.png");
    std::fs::write(&path, &png)?;
    println!("已写出     : {}（{} 字节）", path.display(), png.len());

    // 顺带演示渲染层的颜色转换（straight → vello premultiplied）
    let _ = to_vello(Color::RED);
    Ok(())
}
