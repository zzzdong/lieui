//! Builder 示例：模拟 pdfkit 的页面选择功能（v2 API）
//! 展示动态 ViewTree 构建 + State 驱动 + 自动重建

use lieui::geometry::{Color, Size};
use lieui::prelude::*;
use lieui::state::State;
use lieui::view::View;
use lieui::render::VelloRenderer;

struct PdfModel {
    page_count: usize,
    current_page: usize,
    selected: Vec<bool>,
}
impl PdfModel { fn new(n: usize) -> Self { Self { page_count: n, current_page: 0, selected: vec![false; n] } } }

fn main() {
    let viewport = Size::new(600.0, 500.0);
    let mut app = Application::new(viewport);
    let mut renderer = VelloRenderer::new(600, 500);
    let model = State::new(PdfModel::new(20));
    let checked_count = State::new(0usize);

    let elements = app.run_once(|| {
        let m = model.get();
        let page_count = m.page_count;
        let current_page = m.current_page;

        // 更新选中计数
        let c = m.selected.iter().filter(|&&v| v).count();
        let cc = checked_count.clone();
        cc.set(c);

        let mut col = Column::new();

        // 工具按钮行
        let mut tool_row = Row::new();
        tool_row = tool_row.child(Text::new(&format!("{} pages loaded", page_count)).font_size(14.0));
        tool_row = tool_row.child(Text::new(&format!("[{} selected]", *checked_count.get())).font_size(14.0).color(Color::RED));
        col = col.child(tool_row);

        // 页面列表
        for i in 0..page_count {
            let checked = m.selected.get(i).copied().unwrap_or(false);
            let label = format!("Page {}  [{}]", i + 1, if checked { "✓" } else { " " });
            col = col.child(Text::new(&label).font_size(12.0));
        }

        // 导航
        let mut nav_row = Row::new();
        let prev_label = if current_page > 0 {
            format!("‹ Prev (page {})", current_page)
        } else {
            "‹ Prev".to_string()
        };
        nav_row = nav_row.child(Text::new(&prev_label).font_size(14.0));
        nav_row = nav_row.child(Text::new(&format!("Page {} / {}", current_page + 1, page_count)).font_size(14.0));
        nav_row = nav_row.child(Text::new("Next ›").font_size(14.0));
        col = col.child(nav_row);

        col.build()
    });

    let pixmap = renderer.render(&elements);
    println!("PDFKit UI: {} elements rendered, {}x{} pixmap", elements.len(), pixmap.width(), pixmap.height());
    println!("Debug: {:?}", app.debug_stats());
    println!("\n=== PDFKit example completed! ===");
}
