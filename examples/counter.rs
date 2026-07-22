//! LieUI v2 计数器 — 展示 State + Reconciler diff

use lieui::geometry::Size;
use lieui::prelude::*;
use lieui::render::VelloRenderer;
use lieui::runtime::Runtime;
use lieui::state::State;
use lieui::view::View;

fn main() {
    let mut runtime = Runtime::new(Size::new(800.0, 600.0));
    let mut renderer = VelloRenderer::new(800, 600);
    let count = State::new(0);

    let vt = Column::new()
        .child(Text::new("Counter").font_size(48.0))
        .child(Text::new(&count.get().to_string()).font_size(72.0))
        .build();
    runtime.submit_view_tree(vt);
    let e = runtime.frame();
    let _px = renderer.render(&e);
    println!("Frame 1: {} elements, tree {}", e.len(), runtime.debug_stats.element_count);

    count.set(5);
    let vt2 = Column::new()
        .child(Text::new("Counter").font_size(48.0))
        .child(Text::new(&count.get().to_string()).font_size(72.0).color(lieui::geometry::Color::RED))
        .child(Text::new(&format!("Count = {}", count.get())).font_size(14.0))
        .build();
    runtime.submit_view_tree(vt2);
    let e2 = runtime.frame();
    let _px2 = renderer.render(&e2);
    println!("Frame 2: {} elements, reconciler {:?}", e2.len(), runtime.debug_stats.reconciler);
    println!("\n=== Counter completed! ===");
}
