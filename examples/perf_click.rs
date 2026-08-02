//! perf_click — 无头性能压测：模拟 Checkbox 点击 → rebuild → layout → render 全链路
//!
//! 不开窗口，直接驱动 Runtime + VelloRenderer，测量点击一次 checkbox 后
//! 各阶段（builder / submit / reconcile+layout+render-tree / raster）耗时。
//!
//! 运行：cargo run --example perf_click [--release]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use lieui::core::ElementId;
use lieui::event::{Event, EventContext, EventPhase, HitTestResult, Modifiers, MouseButton};
use lieui::geometry::{Point, Size};
use lieui::layout::types::FlexAlign;
use lieui::prelude::*;
use lieui::runtime::ElementTree;
use lieui::state::State;
use lieui::view::node::Callback;
use lieui::widget::{BuildContext, StateMap, Widget};

const COLS: usize = 5;
const ROWS: usize = 20;
const CLICKS: usize = 20;

/// 构建测试 UI：ROWS 行 x COLS 列的 Checkbox 网格 + 标题文本。
fn build_ui(checked: &State<Vec<bool>>) -> Box<dyn Widget> {
    let mut col = Column::new()
        .spacing(4.0)
        .expand(true)
        .align_items(FlexAlign::Start)
        .child(Text::new("Checkbox Grid Benchmark").font_size(20.0));
    for r in 0..ROWS {
        let mut row = Row::new().spacing(8.0).align_items(FlexAlign::Center);
        for c in 0..COLS {
            let idx = r * COLS + c;
            let is_checked = checked.get()[idx];
            let st = checked.clone();
            row = row.child(
                Checkbox::new(is_checked)
                    .label(format!("Item {}", idx))
                    .on_click(move || st.update(|v| v[idx] = !v[idx])),
            );
        }
        col = col.child(row);
    }
    Box::new(col)
}

/// 复刻 Application::handle_lie_event 的分发语义（Capture=WithCtx，Target=双触发，Bubble=Simple）。
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

/// DFS 查找第一个带 Click 监听器的节点（即第一个 Checkbox 根）。
fn find_first_clickable(tree: &ElementTree, id: ElementId) -> Option<ElementId> {
    if tree
        .get_node_ref(id)
        .is_some_and(|n| !n.click_listeners().is_empty())
    {
        return Some(id);
    }
    for cid in tree.children_of(id) {
        if let Some(found) = find_first_clickable(tree, cid) {
            return Some(found);
        }
    }
    None
}

fn main() {
    unsafe { std::env::set_var("LIEUI_PERF", "1") };

    let viewport = Size::new(800.0, 600.0);
    let checked = State::new(vec![false; ROWS * COLS]);
    let state: Rc<RefCell<StateMap>> = Rc::new(RefCell::new(StateMap::new()));

    let mut runtime = Runtime::new(viewport);
    let mut renderer = VelloRenderer::new(viewport.width as u16, viewport.height as u16);

    // ---- 首帧 ----
    let t0 = Instant::now();
    let mut ctx = BuildContext::new(Rc::clone(&state));
    let view_tree = build_ui(&checked).build(&mut ctx);
    runtime.submit_view_tree(view_tree, true);
    let elements = runtime.frame();
    let _ = renderer.render(&elements);
    eprintln!(
        "== first frame total: {:.1}ms, elements={} tree-nodes={}\n",
        t0.elapsed().as_secs_f64() * 1e3,
        elements.len(),
        runtime.debug_stats.element_count
    );

    // 找到第一个 checkbox，取其中心点做命中
    let base_root = runtime.layers.tree.root().expect("root");
    let cb_id = find_first_clickable(&runtime.layers.tree, base_root).expect("checkbox");
    let rect = runtime.layers.tree.layout(cb_id).rect();
    let click_pos = Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);

    // ---- 点击循环 ----
    let mut totals = Vec::new();
    for i in 0..CLICKS {
        eprintln!("---- click #{} ----", i);
        let t = Instant::now();

        // 1. 命中测试 + 事件分发（MouseDown + MouseUp/Click）
        let t_ev = Instant::now();
        let (_, target, _) = runtime.layers.hit_test_top(click_pos).expect("hit");
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
        let ev_us = t_ev.elapsed().as_secs_f64() * 1e6;
        eprintln!("[lieui-perf] hit+dispatch     {:>8.1}us", ev_us);

        // 2. rebuild 管线（等价 RedrawRequested 分支）
        assert!(
            lieui::state::take_rebuild_requested_pub(),
            "click should trigger rebuild"
        );
        let t_b = Instant::now();
        let mut ctx = BuildContext::new(Rc::clone(&state));
        let view_tree = build_ui(&checked).build(&mut ctx);
        eprintln!(
            "[lieui-perf] builder          {:>8.1}us",
            t_b.elapsed().as_secs_f64() * 1e6
        );
        let t_s = Instant::now();
        runtime.submit_view_tree(view_tree, true);
        eprintln!(
            "[lieui-perf] submit           {:>8.1}us",
            t_s.elapsed().as_secs_f64() * 1e6
        );
        let elements = runtime.frame();
        let t_r = Instant::now();
        let pix = renderer.render(&elements);
        eprintln!(
            "[lieui-perf] raster           {:>8.1}us",
            t_r.elapsed().as_secs_f64() * 1e6
        );
        std::hint::black_box(pix.data());

        let total_ms = t.elapsed().as_secs_f64() * 1e3;
        eprintln!("== click->frame total: {:.2}ms\n", total_ms);
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

    // 校验：状态确实翻转过（偶数次点击后 item0 应回到 false）
    let v0 = checked.get()[0];
    eprintln!("item0 checked = {} (clicks={})", v0, CLICKS);
}
