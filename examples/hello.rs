//! LieUI v2 示例 — 构建 ViewTree 并渲染为像素图

use lieui::geometry::Size;
use lieui::prelude::*;
use lieui::render::VelloRenderer;
use lieui::view::View;

fn main() {
    let viewport = Size::new(800.0, 600.0);
    let mut app = Application::new(viewport);
    let mut renderer = VelloRenderer::new(800, 600);

    let view_tree = Column::new()
        .child(Text::new("Hello from LieUI v2!").font_size(32.0).color(lieui::geometry::Color::RED))
        .child(Text::new("This is built with View trait + Runtime reconciler.").font_size(18.0))
        .build();

    println!("ViewTree built! {} children", view_tree.children.len());
    println!("First child content: {:?}", view_tree.children[0].props.get_str("content"));

    let elements = app.run_once(|| view_tree.clone());
    println!("Runtime: {} layered elements", elements.len());
    println!("Debug: {:?}", app.debug_stats());

    let pixmap = renderer.render(&elements);
    println!("Rendered {}x{} pixmap", pixmap.width(), pixmap.height());

    let center_idx = (pixmap.height() as usize / 2) * pixmap.width() as usize + (pixmap.width() as usize / 2);
    let center = pixmap.data()[center_idx];
    println!("Center pixel: RGBA({},{},{},{})", center.r, center.g, center.b, center.a);

    println!("\n=== LieUI v2 Hello example completed! ===");
}
