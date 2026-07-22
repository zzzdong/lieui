//! PDFKit 示例 — Viev 树构建 + 状态驱动 + 头跑测试

use lieui::geometry::Size;
use lieui::prelude::*;
use lieui::render::VelloRenderer;
use lieui::runtime::Runtime;
use lieui::state::State;
use lieui::view::View;

fn build_page_ui(count: usize, checked: &[bool], current: usize) -> impl View {
    let mut col = Column::new();
    col = col.child(Text::new(&format!("{} pages loaded, {} selected", count, checked.iter().filter(|&&v| v).count())).font_size(14.0));
    for i in 0..count {
        let label = format!("Page {}  [{}]", i + 1, if checked.get(i).copied().unwrap_or(false) { "✓" } else { " " });
        col = col.child(Text::new(&label).font_size(12.0));
    }
    col = col.child(Text::new(&format!("Page {} / {}", current + 1, count)).font_size(14.0));
    col
}

fn main() {
    let mut runtime = Runtime::new(Size::new(600.0, 500.0));
    let mut renderer = VelloRenderer::new(600, 500);
    let checked = State::new(vec![false; 20]);
    let current = State::new(0usize);

    // 初始：10 pages
    let vt = build_page_ui(10, &*checked.get(), *current.get()).build();
    runtime.submit_view_tree(vt);
    let e = runtime.frame();
    let _px = renderer.render(&e);
    println!("Frame 1: {} elements, tree {}", e.len(), runtime.debug_stats.element_count);

    // 选中 page 5
    checked.update(|v| v[5] = true);
    current.set(5);
    let vt2 = build_page_ui(10, &*checked.get(), *current.get()).build();
    runtime.submit_view_tree(vt2);
    let e2 = runtime.frame();
    let _px2 = renderer.render(&e2);
    println!("Frame 2 (page 6 selected): {} elements, reconciler {:?}", e2.len(), runtime.debug_stats.reconciler);

    println!("\n=== PDFKit example completed! ===");
}
