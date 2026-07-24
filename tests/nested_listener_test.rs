//! 嵌套 Listener 事件分发测试
//!
//! 验证点击内层 Listener（Checkbox）时只触发内层回调，不会冒泡到外层 Listener。

use lieui::event::{Event, EventManager, MouseButton};
use lieui::geometry::{Point, Size};
use lieui::prelude::*;
use lieui::runtime::Runtime;
use lieui::state;
use lieui::view::node::{ClickCallbackRef, ViewNode};
use lieui::widget::Checkbox;
use lieui::view::View;

struct Clickable<V: View> {
    child: V,
    callback_id: Option<u64>,
}

impl<V: View> Clickable<V> {
    fn new(child: V) -> Self {
        Self {
            child,
            callback_id: None,
        }
    }
    fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.callback_id = Some(state::register_click(Box::new(f)));
        self
    }
}

impl<V: View> View for Clickable<V> {
    fn build(&self) -> ViewNode {
        self.child
            .build()
            .with_listener(self.callback_id.map(ClickCallbackRef::Simple))
    }
}

#[test]
fn nested_click_stops_at_inner_listener() {
    let inner = state::State::new(false);
    let outer = state::State::new(false);

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
        .build();

    let mut rt = Runtime::new(Size::new(400.0, 200.0));
    rt.submit_view_tree(vt);
    let _ = rt.frame();

    // 命中测试：点击 Checkbox 区域（大致在 (8, 8) 附近）
    let hit = rt
        .layers
        .layer_hit_test(lieui::core::layers::LayerType::Base, Point::new(12.0, 12.0))
        .expect("should hit checkbox");
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
                if let Some(cb_id) = rt.layers.tree.on_click(id) {
                    state::invoke_click(cb_id, ctx);
                }
            }
        },
    );

    assert!(*inner.get(), "inner checkbox callback should fire");
    assert!(!*outer.get(), "outer row callback should not fire");
}
