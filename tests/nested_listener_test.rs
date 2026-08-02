//! 嵌套 Listener 事件分发测试
//!
//! 验证点击内层 Listener（Checkbox）时只触发内层回调，不会冒泡到外层 Listener。

use lieui::event::{Event, EventManager, MouseButton};
use lieui::geometry::{Point, Size};
use lieui::prelude::*;
use lieui::runtime::Runtime;
use lieui::view::node::{Callback, Listener, ViewNode};
use lieui::widget::{BuildContext, Checkbox, Widget};
use std::rc::Rc;

struct Clickable<V: Widget> {
    child: V,
    listeners: Vec<Listener>,
}

impl<V: Widget> Clickable<V> {
    fn new(child: V) -> Self {
        Self {
            child,
            listeners: Vec::new(),
        }
    }
    fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click(Rc::new(f)));
        self
    }
}

impl<V: Widget> Widget for Clickable<V> {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let mut node = ctx.child(0, &self.child);
        for l in &self.listeners {
            node.add_listener(l.clone());
        }
        node
    }
}

#[test]
fn nested_click_stops_at_inner_listener() {
    let inner = State::new(false);
    let outer = State::new(false);

    let i = inner.clone();
    let o = outer.clone();
    let vt = Column::new()
        .child(
            Clickable::new(
                Row::new().child(
                    Checkbox::new(false)
                        .label("Toggle")
                        .on_click(move || i.set(true)),
                ),
            )
            .on_click(move || o.set(true)),
        )
        .build_node();

    let mut rt = Runtime::new(Size::new(400.0, 200.0));
    rt.submit_view_tree(vt, false);
    let _ = rt.frame(winit::window::WindowId::dummy());

    // 命中测试：点击 Checkbox 区域（大致在 (8, 8) 附近）
    let hit = rt
        .layers
        .hit_test_top(Point::new(12.0, 12.0))
        .expect("should hit checkbox")
        .1;
    let path = rt.layers.path_to(hit);

    let mut em = EventManager::new();
    let _effects = em.handle_mouse_up(
        Point::new(12.0, 12.0),
        MouseButton::Left,
        &lieui::event::HitTestResult { target: hit, path },
        &rt.layers.tree,
        |id, event, ctx| {
            if let Event::Click { .. } = event {
                if ctx.phase() == lieui::event::EventPhase::Capture {
                    return;
                }
                for l in rt.layers.tree.listeners(id) {
                    if l.event == lieui::event::EventType::Click
                        && let Callback::Simple(cb) = &l.callback
                    {
                        cb();
                        ctx.stop_propagation();
                    }
                }
            }
        },
    );

    assert!(*inner.get(), "inner checkbox callback should fire");
    assert!(!*outer.get(), "outer row callback should not fire");
}
