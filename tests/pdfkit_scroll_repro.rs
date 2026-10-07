//! 复现 pdfkit sidebar 的滚动问题（滚轮 + 滚动条）。
use lieui::prelude::*;
use std::rc::Rc;

const ITEM_H: f32 = 80.0;
const VIEWPORT_H: f32 = 1400.0;

struct Vm {
    vl: VirtualListState,
    pages: Vec<u32>,
}

impl ViewModel for Vm {
    fn view(self: &Rc<Self>, v: &mut ViewBuf) {
        let pages = self.pages.clone();
        v.container(|side| {
            side.width(220.0);
            side.scroll(|sc| {
                sc.expand(true);
                sc.column(|list| {
                    list.gap(0.0);
                    list.text("页面").font_size(13.0).layout(|l| l.flex_shrink = 0.0);
                    list.virtual_list(
                        &self.vl,
                        &pages,
                        |pn| u64::from(*pn),
                        ITEM_H,
                        VIEWPORT_H,
                        |v, pn| {
                            let pn = *pn;
                            v.container(|row| {
                                row.height(ITEM_H);
                                row.text(format!("page {pn}"));
                            });
                        },
                    );
                });
            });
        });
    }
}

fn setup(pages: usize) -> (Runtime, App, lieui::window::WindowId) {
    let rt = Runtime::new();
    let vm = Vm {
        vl: VirtualListState::new(&rt),
        pages: (1..=pages as u32).collect(),
    };
    let mut app = App::new(rt.clone());
    let id = app.window(WindowConfig::new().size(900.0, 700.0), vm);
    (rt, app, id)
}

#[test]
fn sidebar_scroll_diagnostics() {
    let (rt, mut app, id) = setup(200);
    app.frame_all();

    let (sc, view, content, show_sb) = {
        let w = app.window_ctx(id).unwrap();
        let t = w.track();
        let mut r = None;
        for n in t.node_ids() {
            let nd = t.get(n).unwrap();
            if nd.layout.overflow_scroll {
                r = Some((n, nd.rect(), nd.content_size, nd.layout.show_scrollbar));
            }
        }
        r.expect("scroll container")
    };
    println!("scroll container = {:?}", sc);
    println!("  view    = {:?}", view);
    println!("  content = {:?}", content);
    println!("  max_y   = {}", (content.height - view.height).max(0.0));
    println!("  show_scrollbar = {}", show_sb);

    let path = app.window_ctx(id).unwrap().hit(lieui_geom::Point::new(110.0, 300.0));
    println!("hit at (110,300) = {:?}", path);
    println!("  contains scroll container = {}", path.contains(&sc));

    let out = app.window_ctx_mut(id).unwrap().pointer(
        &rt,
        InputEvent::Wheel {
            pointer: PointerId(0),
            pos: lieui_geom::Point::new(110.0, 300.0),
            delta: (0.0, -100.0),
        },
    );
    println!("out.scrolled = {}", out.scrolled);
    let after = app.window_ctx(id).unwrap().track().scroll_offset(sc);
    println!("offset after wheel = {:?}", after);
    assert!(out.scrolled, "wheel should scroll");
    assert!(after.1 > 0.0, "offset should advance, got {:?}", after);
}
