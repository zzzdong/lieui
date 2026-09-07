//! M1 冒烟示例：构造一棵两栏列表树并跑一次布局，打印前几行的结果。
//!
//! 运行：`cargo run -p lieui --example layout_smoke`

use lieui::prelude::*;

fn main() {
    let mut win = Window::new(WindowId::from_u64(1), (800.0, 600.0));
    let root = win.root();
    win.set_prop(root, props::FLEX_DIRECTION, props::flex_direction::ROW);

    let mut rows = Vec::new();
    for _ in 0..2 {
        let panel = win.create_node(root, ElementTypeId::BOX, None);
        win.set_prop(panel, props::WIDTH, Dimension::px(400.0));
        win.set_prop(panel, props::HEIGHT, Dimension::px(600.0));
        win.set_prop(panel, props::OVERFLOW_SCROLL, true);
        let col = win.create_node(panel, ElementTypeId::BOX, None);
        for i in 0..5 {
            let t = win.create_node(col, ElementTypeId::TEXT, None);
            win.set_prop(t, props::TEXT, SharedString::new(format!("row {i}")));
            rows.push(t);
        }
    }

    let mut text = TextService::new(1024);
    win.run_layout(&mut text);

    for t in rows.iter().take(3) {
        let l = win.layout_of(*t).unwrap();
        println!(
            "{t:?} -> x={:.1} y={:.1} w={:.1} h={:.1}",
            l.x, l.y, l.width, l.height
        );
    }

    // 改一行文本 → 只重排左栏
    win.reset_stats();
    win.set_prop(rows[2], props::TEXT, SharedString::new("changed!"));
    win.run_layout(&mut text);
    println!(
        "改 1 行后本帧参与布局节点数 = {}（整窗 {}）",
        win.stats().last_pass_nodes,
        win.tree_node_count()
    );
    println!(
        "测度缓存命中率 = {:.1}%",
        text.stats().measure_hit_rate() * 100.0
    );
}
