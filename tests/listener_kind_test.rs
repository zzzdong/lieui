//! 内置回调 vs 用户回调的区分测试（无头）
//!
//! 验证：ListenerKind 标记；同一节点上内置行为回调先于用户回调执行，
//! 且用户回调的 stop_propagation 不会阻止同节点已执行的内置行为。

use std::cell::RefCell;
use std::rc::Rc;

use lieui::core::ElementId;
use lieui::event::{Event, EventContext, HitTestResult, Modifiers, MouseButton};
use lieui::geometry::{Point, Size};
use lieui::layout::style::FlexStyle;
use lieui::runtime::{ElementTree, Runtime};
use lieui::state::State;
use lieui::view::node::{Listener, ListenerKind, ViewNode};
use lieui::view::paint::PaintStyle;
use lieui::widget::{
    BuildContext, Button, Draggable, IconButton, IconName, Slider, StateMap, Switch, Text, Widget,
};

/// 复刻 Application::handle_lie_event 的分发语义（走真实的 dispatch_node_listeners）。
fn handle_lie_event(tree: &ElementTree, id: ElementId, event: &Event, ctx: &mut EventContext) {
    ctx.set_event(event.clone());
    ctx.set_current(id, tree.layout(id).rect());
    lieui::event::dispatch_node_listeners(tree, id, event, ctx);
}

fn new_ctx() -> BuildContext {
    BuildContext::new(Rc::new(RefCell::new(StateMap::new())))
}

#[test]
fn listener_kind_defaults_to_user_and_builtin_marker() {
    let l = Listener::on_click(Rc::new(|| {}));
    assert_eq!(l.kind(), ListenerKind::User);
    let l = Listener::on_mouse_down(Rc::new(|_| {})).builtin();
    assert_eq!(l.kind(), ListenerKind::BuiltIn);
}

#[test]
fn widget_internal_listeners_are_builtin_user_api_is_user() {
    let mut ctx = new_ctx();

    // 用户 API：Button / IconButton 的 on_click → User。
    let node = Button::new("x").on_click(|| {}).build(&mut ctx);
    let kinds: Vec<ListenerKind> = node.listeners().iter().map(|l| l.kind()).collect();
    assert_eq!(kinds, vec![ListenerKind::User]);

    let node = IconButton::new(IconName::Search)
        .on_click(|| {})
        .build(&mut ctx);
    let kinds: Vec<ListenerKind> = node.listeners().iter().map(|l| l.kind()).collect();
    assert_eq!(kinds, vec![ListenerKind::User]);

    // 内置行为：Slider 拖拽、Switch 切换 → BuiltIn。
    let node = Slider::new(State::new(0.5f32)).build(&mut ctx);
    assert!(
        !node.listeners().is_empty()
            && node
                .listeners()
                .iter()
                .all(|l| l.kind() == ListenerKind::BuiltIn),
        "slider listeners should all be builtin"
    );

    let node = Switch::new(State::new(false)).build(&mut ctx);
    let kinds: Vec<ListenerKind> = node.listeners().iter().map(|l| l.kind()).collect();
    assert_eq!(kinds, vec![ListenerKind::BuiltIn]);

    // 内置接线：Draggable 的拖拽监听器（含用户回调转发）都是 BuiltIn。
    let node = Draggable::new(Text::new("d"))
        .on_drag_move(|_| {})
        .build(&mut ctx);
    assert!(
        !node.listeners().is_empty()
            && node
                .listeners()
                .iter()
                .all(|l| l.kind() == ListenerKind::BuiltIn),
        "draggable wiring listeners should all be builtin"
    );
}

#[test]
fn builtin_listeners_run_first_before_user() {
    let order = Rc::new(RefCell::new(Vec::new()));

    // 故意让用户回调排在 vec 前面：分派必须无视插入顺序，内置回调先执行。
    let user_click = {
        let order = Rc::clone(&order);
        Rc::new(move || {
            order.borrow_mut().push("user".to_string());
        })
    };
    let builtin_click = {
        let order = Rc::clone(&order);
        Rc::new(move |_ctx: &mut EventContext| {
            order.borrow_mut().push("builtin".to_string());
        })
    };

    let vt = ViewNode::Div {
        layout: FlexStyle::default().width(100.0).height(50.0),
        paint: PaintStyle::new(),
        key: None,
        children: vec![],
        listeners: vec![
            Listener::on_click(user_click), // User（vec 靠前，Simple 自动 stop）
            Listener::on_click_with_ctx(builtin_click).builtin(), // BuiltIn（vec 靠后）
        ],
    };

    let mut runtime = Runtime::new(Size::new(200.0, 100.0));
    runtime.submit_view_tree(vt, true);
    let _ = runtime.frame();

    let p = Point::new(50.0, 25.0);
    let (_, target, _) = runtime.layers.hit_test_top(p).expect("hit");
    let path = runtime.layers.path_to(target);
    let hit = HitTestResult { target, path };

    {
        let tree = &runtime.layers.tree;
        let mut em = runtime.layers.event_manager.borrow_mut();
        em.handle_mouse_down(
            p,
            MouseButton::Left,
            Modifiers::default(),
            &hit,
            tree,
            |id, ev, c| handle_lie_event(tree, id, ev, c),
        );
        em.handle_mouse_up(p, MouseButton::Left, &hit, tree, |id, ev, c| {
            handle_lie_event(tree, id, ev, c)
        });
    }

    assert_eq!(
        *order.borrow(),
        vec!["builtin".to_string(), "user".to_string()],
        "builtin callback must run before user callback on the same node"
    );
}
