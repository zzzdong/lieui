use std::time::Instant;
use lieui::layout::layout;
use lieui::prelude::Color;
use lieui::render::Renderer;
use lieui::track::{Kind, Layer, NodeId, Track};
use lieui_geom::Size;

const WINDOW: Size = Size::new(1280.0, 800.0);
const FRAMES: usize = 60;
const ROW_H: f32 = 20.0;

fn scroll_list(n: usize) -> (Track, NodeId) {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.get_mut(root).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
    t.add_root(Layer::Content, None, root);
    let list = t.create(Kind::Box, None);
    {
        let node = t.get_mut(list).unwrap();
        node.layout.dim = [WINDOW.width, WINDOW.height];
        node.paint.clip_content = true;
        node.layout.flex_direction = lieui_layout::FlexDirection::Column;
        node.layout.overflow_scroll = true;
        node.layout.show_scrollbar = true;
    }
    t.append_child(root, list);
    for i in 0..n {
        let row = t.create(Kind::Box, None);
        {
            let node = t.get_mut(row).unwrap();
            node.layout.dim = [WINDOW.width, ROW_H];
            if i < 60 {
                node.paint.background_color = Some(Color::rgba((i % 7) as u8, 90, 160, 255));
            }
        }
        t.append_child(list, row);
        if i % 4 == 0 {
            let txt = t.create(Kind::Text(format!("行 {i}")), None);
            t.append_child(row, txt);
        }
    }
    layout(&mut t, WINDOW);
    (t, list)
}

#[test]
#[ignore = "perf probe; run manually"]
fn scroll_frame_cost_decomposition() {
    println!("window {}x{}, scroll 1px/frame, avg of {FRAMES}", WINDOW.width, WINDOW.height);
    println!();
    println!("{:>7}  {:>10}  {:>10}  {:>10}  {:>11}  {:>14}", "rows", "layout ms", "render ms", "frame ms", "layout%", "nodes/bounds");
    println!("{}", "-".repeat(72));
    for n in [200usize, 1000, 5000, 20000] {
        let (mut t, list) = scroll_list(n);
        for i in 0..5 {
            t.set_scroll_offset(list, (0.0, i as f32));
            layout(&mut t, WINDOW);
        }
        let mut r = Renderer::new(WINDOW, Color::rgba(250, 250, 250, 255));
        for _ in 0..3 { r.render(&t, &[], false); }
        let (mut lay, mut ren) = (0.0f64, 0.0f64);
        let (mut nodes, mut bounds) = (0usize, 0usize);
        for i in 0..FRAMES {
            t.set_scroll_offset(list, (0.0, (i + 10) as f32));
            let t0 = Instant::now();
            let st = layout(&mut t, WINDOW);
            lay += t0.elapsed().as_secs_f64();
            let dmg = t.take_damage().0;
            let t1 = Instant::now();
            r.render(&t, &dmg, false);
            ren += t1.elapsed().as_secs_f64();
            nodes = st.nodes;
            bounds = st.boundaries;
        }
        let lay = lay * 1e3 / FRAMES as f64;
        let ren = ren * 1e3 / FRAMES as f64;
        let frame = lay + ren;
        println!("{:>7}  {:>10.3}  {:>10.3}  {:>10.3}  {:>10.1}%  {:>8} / {}", n, lay, ren, frame, 100.0 * lay / frame, nodes, bounds);
    }
    println!();
    println!("layout% = upper bound of what a compositor could save");
    println!("(offset applied at paint time => no layout at all)");
}