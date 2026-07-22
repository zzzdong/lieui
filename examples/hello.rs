//! LieUI v2 头跑测试 — 构建 ViewTree 并渲染为像素图

use lieui::geometry::Size;
use lieui::prelude::*;
use lieui::render::VelloRenderer;
use lieui::runtime::Runtime;
use lieui::view::View;

fn main() {
    let mut runtime = Runtime::new(Size::new(800.0, 600.0));
    let mut renderer = VelloRenderer::new(800, 600);

    let view_tree = Column::new()
        .child(Text::new("Hello from LieUI v2!").font_size(32.0).color(lieui::geometry::Color::RED))
        .child(Text::new("Runtime + Renderer headless test.").font_size(18.0))
        .build();

    println!("ViewTree built! {} children", view_tree.children.len());

    runtime.submit_view_tree(view_tree);
    let elements = runtime.frame();
    println!("Runtime: {} layered elements", elements.len());
    println!("Debug: {:?}", runtime.debug_stats);

    let pixmap = renderer.render(&elements);
    println!("Rendered {}x{} pixmap", pixmap.width(), pixmap.height());

    let mid = (pixmap.height() as usize / 2) * pixmap.width() as usize + (pixmap.width() as usize / 2);
    let c = pixmap.data()[mid];
    println!("Center pixel: RGBA({},{},{},{})", c.r, c.g, c.b, c.a);

    println!("\n=== LieUI v2 Hello completed! ===");
}
