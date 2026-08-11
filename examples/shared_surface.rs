//! LieUI — SharedSurface + 进程内 Compositor 演示
//!
//! 演示高频大画布组件的增量更新模式：
//! 组件自持一块 SharedSurface，通过 Animation 定时重画其中一小块脏区，
//! 再由 Compositor 把脏区合成进 backing 并上屏。不同于走完整
//! `builder → reconcile → layout → render-tree → vello 光栅化` 全链路的
//! 普通 widget，SharedSurface 的更新不触发 UI 全量 rebuild。

use lieui::animation::Animation;
use lieui::geometry::{Color, Rect};
use lieui::prelude::*;
use lieui::render::surface::SharedSurface;
use std::rc::Rc;
use std::time::Duration;

fn main() {
    // 自持一块 320x240 的共享像素表面（RGBA8）。
    let surface: Rc<SharedSurface> = SharedSurface::new(320, 240);

    // 供动画闭包使用的表面副本（Rc 克隆，共享同一底层 buffer）。
    let anim_surface = Rc::clone(&surface);

    // 用一个计时动画驱动"每 33ms 在表面画一个小方块并标记脏区"，
    // 模拟 terminal 等高频数据源的增量刷新。
    let mut x: f32 = 0.0;
    let mut going_right = true;
    let _anim = Animation::new(Duration::from_millis(33), move || {
        // 直接写表面 buffer（模拟外部数据线程/回调写入像素）。
        let w = anim_surface.width() as usize;
        let mut buf = anim_surface.lock_buffer();

        // 上一帧画过的小方块区域，先清成背景色（整条 20px 色带）。
        for py in 20..40usize {
            for px in 0..w {
                let o = (py * w + px) * 4;
                buf[o..o + 4].copy_from_slice(&[240, 240, 240, 255]);
            }
        }
        // 当前块（20px 高，40px 宽的水平色块）。
        for py in 20..40usize {
            for px in x as usize..(x as usize + 40).min(w) {
                let o = (py * w + px) * 4;
                buf[o..o + 4].copy_from_slice(&[30, 144, 255, 255]); // dodger blue
            }
        }
        drop(buf);

        // 标记脏区：整条 20px 高的横向色带（widget 级脏区）。
        anim_surface.damage(Rect::new(0.0, 20.0, anim_surface.width() as f32, 20.0));

        // 左右往返移动。
        if going_right {
            x += 6.0;
            if x + 40.0 >= anim_surface.width() as f32 {
                going_right = false;
            }
        } else {
            x -= 6.0;
            if x <= 0.0 {
                going_right = true;
            }
        }
        // 请求一帧仅重绘（走 render-only 路径，不触发全量 rebuild）。
        lieui::state::request_redraw();
    });

    let app = Application::new(WindowConfig::new().size(420.0, 340.0), move |_ctx| {
        Box::new(
            Column::new()
                .expand(true)
                .justify_content(lieui::layout::types::FlexAlign::Center)
                .align_items(lieui::layout::types::FlexAlign::Center)
                .spacing(12.0)
                .child(Text::new("SharedSurface + Compositor").font_size(20.0))
                .child(
                    // 承载共享表面。默认显示尺寸等于 surface 自身尺寸。
                    SharedSurfaceView::new(Rc::clone(&surface)),
                )
                .child(
                    Text::new("blue bar moves via dirty-region compositing")
                        .font_size(12.0)
                        .color(Color::new(128, 128, 128)),
                ),
        )
    });

    app.run();
}
