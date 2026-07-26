//! Input 鼠标选取集成测试（无头）
//!
//! 验证链路：鼠标按下聚焦/定位 → 键入文本 → 双击选词 / 拖拽选择 →
//! rebuild 后视图树中出现 `__sel_*__` 选区高亮节点。

use std::cell::RefCell;
use std::rc::Rc;

use lieui::core::ElementId;
use lieui::event::{
    Event, EventContext, EventPhase, HitTestResult, Key, Modifiers, MouseButton,
};
use lieui::geometry::{Point, Size};
use lieui::runtime::{ElementTree, Runtime};
use lieui::view::node::Callback;
use lieui::widget::{BuildContext, Input, StateMap, Widget};

/// 复刻 Application::handle_lie_event 的分发语义。
fn handle_lie_event(tree: &ElementTree, id: ElementId, event: &Event, ctx: &mut EventContext) {
    ctx.set_event(event.clone());
    ctx.set_current(id, tree.layout(id).rect());
    let event_type = event.to_type();
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

/// DFS 查找 key 以指定前缀开头的节点。
fn find_key_prefix(tree: &ElementTree, id: ElementId, prefix: &str) -> Option<ElementId> {
    if tree
        .get_node_ref(id)
        .and_then(|n| n.key())
        .is_some_and(|k| k.starts_with(prefix))
    {
        return Some(id);
    }
    for cid in tree.children_of(id) {
        if let Some(found) = find_key_prefix(tree, cid, prefix) {
            return Some(found);
        }
    }
    None
}

struct Harness {
    runtime: Runtime,
    state: Rc<RefCell<StateMap>>,
}

impl Harness {
    fn new() -> Self {
        let mut h = Self {
            runtime: Runtime::new(Size::new(400.0, 100.0)),
            state: Rc::new(RefCell::new(StateMap::new())),
        };
        h.rebuild();
        h
    }

    /// 等价 RedrawRequested rebuild 分支：builder → submit → frame。
    fn rebuild(&mut self) {
        let mut ctx = BuildContext::new(Rc::clone(&self.state));
        let vt = Input::new("type here").width(300.0).build(&mut ctx);
        self.runtime.submit_view_tree(vt, true);
        let _ = self.runtime.frame();
    }

    fn hit(&self, p: Point) -> HitTestResult {
        let (_lt, target) = self.runtime.layers.hit_test_top(p).expect("hit");
        let path = self.runtime.layers.path_to(target);
        HitTestResult { target, path }
    }

    fn mouse_down(&mut self, p: Point) {
        let hit = self.hit(p);
        let tree = &self.runtime.layers.tree;
        let mut em = self.runtime.layers.event_manager.borrow_mut();
        em.handle_mouse_down(
            p,
            MouseButton::Left,
            Modifiers::default(),
            &hit,
            tree,
            |id, ev, c| handle_lie_event(tree, id, ev, c),
        );
    }

    fn mouse_up(&mut self, p: Point) {
        let hit = self.hit(p);
        let tree = &self.runtime.layers.tree;
        let mut em = self.runtime.layers.event_manager.borrow_mut();
        em.handle_mouse_up(p, MouseButton::Left, &hit, tree, |id, ev, c| {
            handle_lie_event(tree, id, ev, c)
        });
    }

    /// 拖拽中的移动：hit 传 None，验证鼠标捕获在指针离开控件后仍生效。
    fn mouse_move_captured(&mut self, p: Point) {
        let tree = &self.runtime.layers.tree;
        let mut em = self.runtime.layers.event_manager.borrow_mut();
        em.handle_mouse_move(p, None, tree, |id, ev, c| handle_lie_event(tree, id, ev, c));
    }

    fn type_str(&mut self, s: &str) {
        for ch in s.chars() {
            let tree = &self.runtime.layers.tree;
            let mut em = self.runtime.layers.event_manager.borrow_mut();
            em.handle_key_down(Key::Character(ch), Modifiers::default(), |id, ev, c| {
                handle_lie_event(tree, id, ev, c)
            });
        }
    }

    fn selection_node(&self) -> Option<ElementId> {
        let root = self.runtime.layers.tree.root()?;
        find_key_prefix(&self.runtime.layers.tree, root, "__sel_")
    }
}

#[test]
fn click_focuses_and_double_click_selects_word() {
    let mut h = Harness::new();
    let pad = lieui::theme::current().spacer.sm;
    let p = Point::new(pad + 10.0, 13.0);

    // 单击：聚焦 Input（焦点落到带 FocusIn 监听器的根 Div）
    h.mouse_down(p);
    h.mouse_up(p);
    {
        let em = h.runtime.layers.event_manager.borrow();
        assert!(em.focused().is_some(), "input should be focused");
        assert!(em.mouse_capture().is_none(), "capture released on mouse up");
    }
    h.rebuild();

    // 键入文本
    h.type_str("hello world");
    h.rebuild();
    assert!(
        h.selection_node().is_none(),
        "no selection highlight before selecting"
    );

    // 双击 "hello"（同一位置两次按下，连击计数=2 → 选词）
    h.mouse_down(p);
    h.mouse_up(p);
    h.mouse_down(p);
    h.mouse_up(p);
    h.rebuild();
    assert!(
        h.selection_node().is_some(),
        "double click should create selection highlight"
    );
}

#[test]
fn drag_selects_and_single_click_clears() {
    let mut h = Harness::new();
    let pad = lieui::theme::current().spacer.sm;

    // 聚焦并键入
    let p0 = Point::new(pad + 2.0, 13.0);
    h.mouse_down(p0);
    h.mouse_up(p0);
    h.rebuild();
    h.type_str("hello world");
    h.rebuild();

    // 按下后拖拽（拖到控件外，验证鼠标捕获）
    let start = Point::new(pad + 2.0, 13.0);
    h.mouse_down(start);
    {
        let em = h.runtime.layers.event_manager.borrow();
        assert!(
            em.mouse_capture().is_some(),
            "mouse down on input should capture pointer"
        );
    }
    h.mouse_move_captured(Point::new(pad + 40.0, 13.0));
    h.mouse_move_captured(Point::new(380.0, 60.0));
    // 释放点回到控件内（控件外无命中目标），选区已由拖拽建立。
    h.mouse_up(Point::new(pad + 60.0, 13.0));
    h.rebuild();
    assert!(
        h.selection_node().is_some(),
        "drag should create selection highlight"
    );

    // 间隔超过双击距离阈值的单击：选区collapse，高亮消失
    let p1 = Point::new(pad + 20.0, 13.0);
    h.mouse_down(p1);
    h.mouse_up(p1);
    h.rebuild();
    assert!(
        h.selection_node().is_none(),
        "plain click should collapse selection"
    );
}
