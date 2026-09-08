//! perf_surface — SharedSurface 脏区刷新基准
//!
//! 对比"小脏区增量刷新"与"整屏刷新"的每帧成本，用于验收脏区合屏的收益。
//!
//! 用法：
//! ```text
//! cargo run --release --example perf_surface            # 小脏区（每次 1200x20）
//! cargo run --release --example perf_surface -- full     # 整屏脏区（每次 1200x840）
//! LIEUI_PERF=1 cargo run --release --example perf_surface  # 额外输出 lieui 分阶段耗时
//! ```
//!
//! 关注指标：
//! - `avg_frame`：动画回调的平均间隔（越接近 16ms 表示越跟得上 60FPS）。
//! - `avg_surface_write`：组件自己写像素 + 标记脏区的耗时。
//! - 两个模式的 `avg_frame` 差值即为"脏区合屏 + 部分上屏"省下的开销。

use lieui::animation::Animation;
use lieui::geometry::Rect;
use lieui::layout::types::FlexAlign;
use lieui::prelude::*;
use lieui::render::surface::SharedSurface;
use std::rc::Rc;
use std::time::{Duration, Instant};

const SURF_W: u32 = 1200;
const SURF_H: u32 = 840;
/// 小脏区高度（约 1 行终端文本）。
const ROW_H: u32 = 20;
/// 每多少帧输出一次统计。
const REPORT_EVERY: u32 = 120;

fn main() {
    let full = std::env::args().nth(1).as_deref() == Some("full");
    let mode = if full { "full" } else { "small" };

    let surface: Rc<SharedSurface> = SharedSurface::new(SURF_W, SURF_H);
    // 初始背景：不透明浅灰（模拟终端底色）。
    {
        let mut buf = surface.lock_buffer();
        for px in buf.chunks_exact_mut(4) {
            px.copy_from_slice(&[240, 240, 240, 255]);
        }
    }

    let anim_surface = Rc::clone(&surface);
    let mut frame: u32 = 0;
    let mut write_total = Duration::ZERO;
    let mut since = Instant::now();

    let _anim = Animation::new(Duration::from_millis(16), move || {
        let t0 = Instant::now();
        let row = (frame * ROW_H) % SURF_H;
        let (y0, h) = if full { (0u32, SURF_H) } else { (row, ROW_H) };
        {
            let mut buf = anim_surface.lock_buffer();
            let w = SURF_W as usize;
            for py in y0..(y0 + h).min(SURF_H) {
                let base = py as usize * w * 4;
                for px in 0..w {
                    let o = base + px * 4;
                    // 棋盘格图案，确保每次写入的像素真的变化。
                    let c = if (px / 8 + py as usize / 8 + frame as usize).is_multiple_of(2) {
                        30u8
                    } else {
                        210u8
                    };
                    buf[o..o + 4].copy_from_slice(&[c, c, c, 255]);
                }
            }
        }
        anim_surface.damage(Rect::new(0.0, y0 as f32, SURF_W as f32, h as f32));
        write_total += t0.elapsed();
        frame += 1;

        if frame.is_multiple_of(REPORT_EVERY) {
            let elapsed = since.elapsed();
            println!(
                "[{mode}] frames={} avg_frame={:.2}ms avg_surface_write={:.2}ms damage={}x{}",
                frame,
                elapsed.as_secs_f64() * 1000.0 / REPORT_EVERY as f64,
                write_total.as_secs_f64() * 1000.0 / REPORT_EVERY as f64,
                SURF_W,
                h
            );
            write_total = Duration::ZERO;
            since = Instant::now();
        }

        // 只请求重绘（不 rebuild）：走"仅合屏 + 部分上屏"路径。
        lieui::state::request_redraw();
    });

    let app = Application::new(WindowConfig::new().size(1240.0, 900.0), move |_ctx| {
        Box::new(
            Column::new()
                .expand(true)
                .justify_content(FlexAlign::Center)
                .align_items(FlexAlign::Center)
                .spacing(8.0)
                .child(Text::new("perf_surface — dirty-region compositing").font_size(16.0))
                .child(SharedSurfaceView::new(Rc::clone(&surface))),
        )
    });
    app.run();
}
