//! perf_gallery — 模拟 Gallery 量级（~70 widgets）的点击响应全链路压测
//!
//! 运行：cargo run --example perf_gallery [--release]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use lieui::core::ElementId;
use lieui::event::{Event, EventContext, EventPhase, HitTestResult, Modifiers, MouseButton};
use lieui::geometry::{Color, Point, Size};
use lieui::layout::types::FlexAlign;
use lieui::prelude::*;
use lieui::runtime::ElementTree;
use lieui::state::State;
use lieui::view::node::Callback;
use lieui::widget::{BuildContext, StateMap, Widget};

const CLICKS: usize = 10;

fn build_gallery_page(
    count: &State<i32>,
    checked: &State<bool>,
    text1: &State<String>,
) -> Box<dyn Widget> {
    let t = lieui::theme::current();
    Box::new(
        ScrollView::expand()
            .scrollbar(true)
            .child(
                Column::new()
                    .spacing(16.0)
                    .align_items(FlexAlign::Stretch)
                    // --- Button 区块 ---
                    .child(
                        Container::new()
                            .padding(t.spacer.md)
                            .border_radius(t.radius.medium)
                            .background(t.background.primary_default)
                            .border(1.0, t.border.default)
                            .child(
                                Column::new().spacing(12.0).align_items(FlexAlign::Start)
                                    .child(Text::new("Button Variants").font_size(18.0))
                                    .child(Row::new().spacing(12.0)
                                        .child(Button::new("Primary"))
                                        .child(Button::new("Secondary"))
                                        .child(Button::new("Tertiary")))
                                    .child(Row::new().spacing(12.0)
                                        .child(Button::new("Danger"))
                                        .child(Button::new("Counter").on_click({
                                            let c = count.clone();
                                            move || c.update(|v| *v += 1)
                                        }))
                                        .child(Text::new(format!("Count: {}", *count.get())).font_size(14.0))),
                            ),
                    )
                    // --- Checkbox 区块 ---
                    .child(
                        Container::new()
                            .padding(t.spacer.md)
                            .border_radius(t.radius.medium)
                            .background(t.background.primary_default)
                            .border(1.0, t.border.default)
                            .child(
                                Column::new().spacing(12.0).align_items(FlexAlign::Start)
                                    .child(Text::new("Checkbox").font_size(18.0))
                                    .child(Checkbox::new(*checked.get())
                                        .label(if *checked.get() { "Checked" } else { "Unchecked" })
                                        .on_click({
                                            let c = checked.clone();
                                            move || c.update(|v| *v = !*v)
                                        }))
                                    .child(Checkbox::new(true).label("Always on")),
                            ),
                    )
                    // --- Input 区块 ---
                    .child(
                        Container::new()
                            .padding(t.spacer.md)
                            .border_radius(t.radius.medium)
                            .background(t.background.primary_default)
                            .border(1.0, t.border.default)
                            .child(
                                Column::new().spacing(12.0).align_items(FlexAlign::Start)
                                    .child(Text::new("Input").font_size(18.0))
                                    .child(Input::new("Type here...").width(300.0).on_change({
                                        let s = text1.clone();
                                        move |txt| s.set(txt)
                                    }))
                                    .child(Text::new(format!("Value: {}", text1.get())).font_size(13.0)),
                            ),
                    )
                    // --- 文本 & 分隔线 ---
                    .child(
                        Container::new()
                            .padding(t.spacer.md)
                            .border_radius(t.radius.medium)
                            .background(t.background.primary_default)
                            .border(1.0, t.border.default)
                            .child(
                                Column::new().spacing(12.0).align_items(FlexAlign::Start)
                                    .child(Text::new("Text & Divider").font_size(18.0))
                                    .child(Text::new("Multi-line wrapped text demo. This shows how text handles wrapping within a fixed width container.").font_size(14.0).max_width(480.0))
                                    .child(lieui::widget::Divider::new())
                                    .child(Text::new("Colored text").color(Color::new(0, 128, 0)).font_size(14.0)),
                            ),
                    )
                    // --- 布局演示 ---
                    .child(
                        Container::new()
                            .padding(t.spacer.md)
                            .border_radius(t.radius.medium)
                            .background(t.background.primary_default)
                            .border(1.0, t.border.default)
                            .child(
                                Column::new().spacing(12.0).align_items(FlexAlign::Start)
                                    .child(Text::new("Flex Layout").font_size(18.0))
                                    .child(Row::new().spacing(8.0)
                                        .child(Container::new().width(60.0).height(60.0).background(Color::new(255,100,100)).border_radius(4.0))
                                        .child(Container::new().width(60.0).height(60.0).background(Color::new(100,255,100)).border_radius(4.0))
                                        .child(Container::new().width(60.0).height(60.0).background(Color::new(100,100,255)).border_radius(4.0))),
                            ),
                    ),
            ),
    )
}

fn handle_lie_event(tree: &ElementTree, id: ElementId, event: &Event, ctx: &mut EventContext) {
    ctx.set_event(event.clone());
    ctx.set_current(id, tree.layout(id).rect());
    use lieui::event::EventType as ET;
    let event_type: ET = match event {
        Event::Click { .. } => ET::Click,
        Event::MouseDown { .. } => ET::MouseDown,
        Event::MouseUp { .. } => ET::MouseUp,
        _ => return,
    };
    let Some(node) = tree.get_node_ref(id) else {
        return;
    };
    for listener in node.listeners() {
        if listener.event != event_type {
            continue;
        }
        match ctx.phase() {
            EventPhase::Capture => {
                if let Callback::WithCtx(cb) = &listener.callback {
                    cb(ctx);
                }
            }
            EventPhase::Target => match &listener.callback {
                Callback::Simple(cb) => {
                    cb();
                    ctx.stop_propagation();
                }
                Callback::WithCtx(cb) => cb(ctx),
            },
            EventPhase::Bubble => {
                if let Callback::Simple(cb) = &listener.callback {
                    cb();
                }
            }
        }
        if ctx.is_stopped() {
            break;
        }
    }
}

fn find_first_clickable(tree: &ElementTree, id: ElementId) -> Option<ElementId> {
    if tree
        .get_node_ref(id)
        .is_some_and(|n| !n.click_listeners().is_empty())
    {
        return Some(id);
    }
    for cid in tree.children_ref(id) {
        if let Some(found) = find_first_clickable(tree, *cid) {
            return Some(found);
        }
    }
    None
}

fn main() {
    std::env::set_var("LIEUI_PERF", "1");

    let viewport = Size::new(900.0, 720.0); // 与 gallery 一致
    let count = State::new(0i32);
    let checked = State::new(false);
    let text1 = State::new(String::new());
    let state: Rc<RefCell<StateMap>> = Rc::new(RefCell::new(StateMap::new()));

    let mut runtime = Runtime::new(viewport);
    let mut renderer = VelloRenderer::new(viewport.width as u16, viewport.height as u16);

    // ---- 首帧 ----
    let t0 = Instant::now();
    let mut ctx = BuildContext::new(Rc::clone(&state));
    let view_tree = build_gallery_page(&count, &checked, &text1).build(&mut ctx);
    runtime.submit_view_tree(view_tree, true);
    let elements = runtime.frame();
    let _ = renderer.render(&elements);
    eprintln!(
        "== first frame total: {:.1}ms elements={} tree-nodes={}\n",
        t0.elapsed().as_secs_f64() * 1e3,
        elements.len(),
        runtime.debug_stats.element_count
    );

    // 找第一个 clickable 元素 (Button::Counter)
    let base_root = runtime.layers.tree.root().expect("root");
    let cb_id = find_first_clickable(&runtime.layers.tree, base_root).expect("clickable");
    let rect = runtime.layers.tree.layout(cb_id).rect();
    let click_pos = Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);

    // ---- 点击循环 ----
    let mut totals = Vec::new();
    for i in 0..CLICKS {
        eprintln!("---- click #{} ----", i);
        let t = Instant::now();

        // 事件分发
        let t_ev = Instant::now();
        let (_lt, target) = runtime.layers.hit_test_top(click_pos).expect("hit");
        let path = runtime.layers.path_to(target);
        let hit = HitTestResult { target, path };
        {
            let tree = &runtime.layers.tree;
            let mut em = runtime.layers.event_manager.borrow_mut();
            em.handle_mouse_down(
                click_pos,
                MouseButton::Left,
                Modifiers::default(),
                &hit,
                tree,
                |id, ev, c| handle_lie_event(tree, id, ev, c),
            );
            em.handle_mouse_up(click_pos, MouseButton::Left, &hit, tree, |id, ev, c| {
                handle_lie_event(tree, id, ev, c)
            });
        }
        eprintln!(
            "[perf] hit+dispatch    {:>8.1}us",
            t_ev.elapsed().as_secs_f64() * 1e6
        );

        // Builder
        assert!(
            lieui::state::take_rebuild_requested_pub(),
            "click should trigger rebuild"
        );
        let t_b = Instant::now();
        let mut ctx = BuildContext::new(Rc::clone(&state));
        let view_tree = build_gallery_page(&count, &checked, &text1).build(&mut ctx);
        eprintln!(
            "[perf] builder         {:>8.1}us",
            t_b.elapsed().as_secs_f64() * 1e6
        );

        // Submit
        let t_s = Instant::now();
        runtime.submit_view_tree(view_tree, true);
        eprintln!(
            "[perf] submit          {:>8.1}us",
            t_s.elapsed().as_secs_f64() * 1e6
        );

        // Frame (reconcile + layout + render-tree)
        let elements = runtime.frame();

        // Raster
        let t_r = Instant::now();
        let pix = renderer.render(&elements);
        eprintln!(
            "[perf] raster          {:>8.1}us",
            t_r.elapsed().as_secs_f64() * 1e6
        );
        std::hint::black_box(pix.data());

        let total_ms = t.elapsed().as_secs_f64() * 1e3;
        eprintln!("== click->frame: {:.2}ms\n", total_ms);
        totals.push(total_ms);
    }

    totals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let avg: f64 = totals.iter().sum::<f64>() / totals.len() as f64;
    eprintln!(
        "==== {} clicks: avg={:.2}ms min={:.2}ms max={:.2}ms ====",
        CLICKS,
        avg,
        totals[0],
        totals[totals.len() - 1]
    );
}
