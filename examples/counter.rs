//! LieUI v2 示例 — 计数器（Builder 驱动 + State）
//! 展示 ViewTree + Runtime 的完整生命周期

use lieui::geometry::{Color, Size};
use lieui::prelude::*;
use lieui::view::View;

fn main() {
    let viewport = Size::new(800.0, 600.0);
    let mut app = Application::new(viewport);
    let mut renderer = VelloRenderer::new(800, 600);
    let count = State::new(0);

    // 第一次运行
    let elements = app.run_once(|| {
        Column::new()
            .child(Text::new("Counter").font_size(48.0).color(Color::from_rgb8(50, 50, 50)))
            .child(Text::new(&count.get().to_string()).font_size(72.0).color(Color::RED))
            .child(Text::new("(use State::set/update to trigger rebuild)").font_size(14.0))
            .build()
    });
    let pixmap = renderer.render(&elements);
    println!("Frame 1: {} elements, {}x{}", elements.len(), pixmap.width(), pixmap.height());

    // 模拟状态变化
    count.set(5);
    let elements = app.run_once(|| {
        Column::new()
            .child(Text::new("Counter").font_size(48.0))
            .child(Text::new(&count.get().to_string()).font_size(72.0).color(Color::RED))
            .child(Text::new(&format!("Count is {}!", count.get())).font_size(14.0))
            .build()
    });
    let _pixmap = renderer.render(&elements);
    println!("Frame 2: {} elements, debug: {:?}", elements.len(), app.debug_stats());

    println!("\n=== LieUI v2 Counter example completed! ===");
}
