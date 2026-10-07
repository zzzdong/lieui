use lieui::layout::{layout, prof_reset, prof_snap};
use lieui::track::{Kind, Layer, Track};
use lieui_geom::Size;

const WINDOW: Size = Size::new(1280.0, 800.0);
const FRAMES: usize = 30;
const ROW_H: f32 = 20.0;

fn scroll_list(n: usize, text_every: usize) -> (Track, lieui::track::NodeId) {
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
    }
    t.append_child(root, list);
    for i in 0..n {
        let row = t.create(Kind::Box, None);
        {
            let node = t.get_mut(row).unwrap();
            node.layout.dim = [WINDOW.width, ROW_H];
            node.layout.flex_shrink = 0.0;
        }
        t.append_child(list, row);
        if text_every > 0 && i % text_every == 0 {
            let txt = t.create(Kind::Text(format!("row {i} sample text")), None);
            t.append_child(row, txt);
        }
    }
    layout(&mut t, WINDOW);
    (t, list)
}

fn run(n: usize, te: usize) -> (f64, f64) {
    let (mut t, list) = scroll_list(n, te);
    for i in 0..3 {
        t.set_scroll_offset(list, (0.0, i as f32));
        layout(&mut t, WINDOW);
    }
    prof_reset();
    for i in 0..FRAMES {
        t.set_scroll_offset(list, (0.0, (i + 10) as f32));
        layout(&mut t, WINDOW);
    }
    let (b, l) = prof_snap();
    (b as f64 * 1e-6 / FRAMES as f64, l as f64 * 1e-6 / FRAMES as f64)
}

#[test]
#[ignore = "perf probe; run manually"]
fn measure_cache_cliff_fixed() {
    println!("text every 4 rows. 60fps budget = 16.67 ms/frame");
    println!(
        "{:>7}  {:>8}  {:>10}  {:>10}  {:>10}  {:>11}",
        "rows", "texts", "build ms", "flex ms", "total ms", "us/text"
    );
    println!("{}", "-".repeat(70));
    for n in [10000usize, 16000, 20000, 40000, 80000] {
        let (b, l) = run(n, 4);
        let texts = n.div_ceil(4);
        println!(
            "{:>7}  {:>8}  {:>10.3}  {:>10.3}  {:>10.3}  {:>11.3}",
            n,
            texts,
            b,
            l,
            b + l,
            (l) * 1e3 / texts as f64
        );
    }
    println!();
    println!("BEFORE the fix: 16000 rows -> 9.0ms flex ; 20000 rows -> 47.0ms flex");
    println!("(4096-entry whole-table clear made every frame re-shape everything)");
}
