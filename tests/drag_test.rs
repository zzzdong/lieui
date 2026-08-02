//! 通用拖拽支持集成测试（无头）
//!
//! 验证链路：按下 → 移动超过阈值 → 合成 DragStart/DragMove/DragEnd，
//! 拖拽期间自动捕获鼠标、结束后抑制 Click，以及 Draggable 组件的接线。

use std::cell::RefCell;
use std::rc::Rc;

use lieui::core::ElementId;
use lieui::event::{Event, EventContext, EventType, HitTestResult, Modifiers, MouseButton};
use lieui::geometry::{Point, Size};
use lieui::layout::style::FlexStyle;
use lieui::runtime::{ElementTree, Runtime};
use lieui::view::node::{Listener, NodeType, ViewNode};
use lieui::view::paint::PaintStyle;
use lieui::widget::{BuildContext, Draggable, StateMap, Text, Widget};

/// 复刻 Application::handle_lie_event 的分发语义。
fn handle_lie_event(tree: &ElementTree, id: ElementId, event: &Event, ctx: &mut EventContext) {
    ctx.set_event(event.clone());
    ctx.set_current(id, tree.layout(id).rect());
    lieui::event::dispatch_node_listeners(tree, id, event, ctx);
}

struct Harness {
    runtime: Runtime,
    /// 按时间顺序记录所有进入监听器的事件。
    events: Rc<RefCell<Vec<Event>>>,
}

impl Harness {
    fn new(threshold: f32) -> Self {
        let mut h = Self {
            runtime: Runtime::new(Size::new(400.0, 200.0)),
            events: Rc::new(RefCell::new(Vec::new())),
        };

        // 根 Div：200x100 位于 (0,0)，挂载拖拽所需的全部监听器。
        let rec = Rc::clone(&h.events);
        let on_down = {
            let rec = Rc::clone(&rec);
            Rc::new(move |ctx: &mut EventContext| {
                rec.borrow_mut().push(ctx.event().unwrap().clone());
                ctx.begin_drag_with_threshold(threshold);
            })
        };
        let on_move = {
            let rec = Rc::clone(&rec);
            Rc::new(move |ctx: &mut EventContext| {
                rec.borrow_mut().push(ctx.event().unwrap().clone());
            })
        };
        let on_up = {
            let rec = Rc::clone(&rec);
            Rc::new(move |ctx: &mut EventContext| {
                rec.borrow_mut().push(ctx.event().unwrap().clone());
            })
        };
        let on_drag_start = {
            let rec = Rc::clone(&rec);
            Rc::new(move |ctx: &mut EventContext| {
                rec.borrow_mut().push(ctx.event().unwrap().clone());
            })
        };
        let on_drag_move = {
            let rec = Rc::clone(&rec);
            Rc::new(move |ctx: &mut EventContext| {
                rec.borrow_mut().push(ctx.event().unwrap().clone());
            })
        };
        let on_drag_end = {
            let rec = Rc::clone(&rec);
            Rc::new(move |ctx: &mut EventContext| {
                rec.borrow_mut().push(ctx.event().unwrap().clone());
            })
        };
        let on_click = {
            let rec = Rc::clone(&rec);
            Rc::new(move || {
                rec.borrow_mut().push(Event::Click {
                    button: MouseButton::Left,
                });
            })
        };

        let vt = ViewNode::Div {
            layout: FlexStyle::default().width(200.0).height(100.0),
            paint: PaintStyle::new(),
            key: None,
            children: vec![],
            listeners: vec![
                Listener::on_mouse_down(on_down),
                Listener::on_mouse_move(on_move),
                Listener::on_mouse_up(on_up),
                Listener::on_drag_start(on_drag_start),
                Listener::on_drag_move(on_drag_move),
                Listener::on_drag_end(on_drag_end),
                Listener::on_click(on_click),
            ],
        };
        h.runtime.submit_view_tree(vt, true);
        let _ = h.runtime.frame(winit::window::WindowId::dummy());
        h
    }

    fn hit(&mut self, p: Point) -> HitTestResult {
        let (_, target, _) = self.runtime.layers.hit_test_top(p).expect("hit");
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

    fn mouse_move(&mut self, p: Point) {
        let hit = self.hit(p);
        let tree = &self.runtime.layers.tree;
        let mut em = self.runtime.layers.event_manager.borrow_mut();
        em.handle_mouse_move(p, Some(&hit), tree, |id, ev, c| {
            handle_lie_event(tree, id, ev, c)
        });
    }

    /// 拖拽中的移动：hit 传 None，验证捕获后指针离开组件仍持续收到事件。
    fn mouse_move_outside(&mut self, p: Point) {
        let tree = &self.runtime.layers.tree;
        let mut em = self.runtime.layers.event_manager.borrow_mut();
        em.handle_mouse_move(p, None, tree, |id, ev, c| handle_lie_event(tree, id, ev, c));
    }

    fn mouse_up(&mut self, p: Point) {
        let hit = self.hit(p);
        let tree = &self.runtime.layers.tree;
        let mut em = self.runtime.layers.event_manager.borrow_mut();
        em.handle_mouse_up(p, MouseButton::Left, &hit, tree, |id, ev, c| {
            handle_lie_event(tree, id, ev, c)
        });
    }

    fn recorded(&self) -> Vec<Event> {
        self.events.borrow().clone()
    }
}

fn types(events: &[Event]) -> Vec<EventType> {
    events.iter().map(|e| e.to_type()).collect()
}

#[test]
fn drag_below_threshold_falls_back_to_click() {
    let mut h = Harness::new(3.0);
    let p0 = Point::new(10.0, 10.0);

    h.mouse_down(p0);
    // 移动 1px（低于阈值）：仍投递 MouseMove，不合成拖拽事件。
    h.mouse_move(Point::new(11.0, 10.0));
    h.mouse_up(p0);

    let events = h.recorded();
    let got = types(&events);
    assert!(
        !got.contains(&EventType::DragStart)
            && !got.contains(&EventType::DragMove)
            && !got.contains(&EventType::DragEnd),
        "below threshold must not synthesize drag events, got {:?}",
        got
    );
    assert!(
        got.contains(&EventType::MouseMove) && got.contains(&EventType::Click),
        "below threshold should behave like a click, got {:?}",
        got
    );
}

#[test]
fn drag_above_threshold_synthesizes_drag_sequence() {
    let mut h = Harness::new(3.0);
    h.mouse_down(Point::new(10.0, 10.0));
    // 距离 sqrt(25+4) ≈ 5.4 > 3：触发 DragStart。
    h.mouse_move(Point::new(15.0, 12.0));
    h.mouse_move(Point::new(20.0, 14.0));
    h.mouse_move(Point::new(22.0, 20.0));
    h.mouse_up(Point::new(22.0, 20.0));

    let events = h.recorded();
    let got = types(&events);
    assert_eq!(
        got,
        vec![
            EventType::MouseDown,
            EventType::DragStart,
            EventType::DragMove,
            EventType::DragMove,
            EventType::DragEnd,
            EventType::MouseUp,
        ],
        "unexpected event sequence"
    );
    assert!(
        !got.contains(&EventType::Click),
        "real drag must suppress Click, got {:?}",
        got
    );

    // DragStart 携带触发位置与按下时的修饰键。
    assert_eq!(
        events[1],
        Event::DragStart {
            x: 15.0,
            y: 12.0,
            offset_x: 0.0,
            offset_y: 0.0,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }
    );
    // DragMove 携带相对增量与累计偏移。
    assert_eq!(
        events[2],
        Event::DragMove {
            x: 20.0,
            y: 14.0,
            dx: 5.0,
            dy: 2.0,
            offset_x: 10.0,
            offset_y: 4.0,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }
    );
    // DragEnd 携带释放点的最终偏移；与上次移动同位置时增量为 0。
    assert_eq!(
        events[4],
        Event::DragEnd {
            x: 22.0,
            y: 20.0,
            dx: 0.0,
            dy: 0.0,
            offset_x: 12.0,
            offset_y: 10.0,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }
    );
}

#[test]
fn drag_events_continue_when_pointer_leaves_widget() {
    let mut h = Harness::new(3.0);
    h.mouse_down(Point::new(10.0, 10.0));
    // 超过阈值，触发 DragStart。
    h.mouse_move(Point::new(20.0, 20.0));
    // 指针移出组件（hit=None）：捕获仍生效，继续合成 DragMove。
    h.mouse_move_outside(Point::new(300.0, 150.0));
    h.mouse_move_outside(Point::new(350.0, 170.0));
    // 指针回到组件内释放（组件外无命中目标，真实应用中以此结束拖拽）。
    h.mouse_up(Point::new(30.0, 30.0));

    let events = h.recorded();
    let got = types(&events);
    assert_eq!(
        got,
        vec![
            EventType::MouseDown,
            EventType::DragStart,
            EventType::DragMove,
            EventType::DragMove,
            EventType::DragEnd,
            EventType::MouseUp,
        ]
    );
    assert_eq!(
        events[3],
        Event::DragMove {
            x: 350.0,
            y: 170.0,
            dx: 50.0,
            dy: 20.0,
            offset_x: 340.0,
            offset_y: 160.0,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }
    );
    assert_eq!(
        events[4],
        Event::DragEnd {
            x: 30.0,
            y: 30.0,
            dx: -320.0,
            dy: -140.0,
            offset_x: 20.0,
            offset_y: 20.0,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }
    );
}

#[test]
fn drag_event_types_map_correctly() {
    assert_eq!(
        Event::DragStart {
            x: 0.0,
            y: 0.0,
            offset_x: 0.0,
            offset_y: 0.0,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }
        .to_type(),
        EventType::DragStart
    );
    assert_eq!(
        Event::DragMove {
            x: 0.0,
            y: 0.0,
            dx: 0.0,
            dy: 0.0,
            offset_x: 0.0,
            offset_y: 0.0,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }
        .to_type(),
        EventType::DragMove
    );
    assert_eq!(
        Event::DragEnd {
            x: 0.0,
            y: 0.0,
            dx: 0.0,
            dy: 0.0,
            offset_x: 0.0,
            offset_y: 0.0,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }
        .to_type(),
        EventType::DragEnd
    );
}

#[test]
fn draggable_widget_wires_listeners_and_passthrough() {
    // 启用：监听器注入到子内容根节点，子内容保持原样。
    let mut ctx = BuildContext::new(Rc::new(RefCell::new(StateMap::new())));
    let node = Draggable::new(Text::new("drag me"))
        .on_drag_start(|_| {})
        .on_drag_move(|_| {})
        .on_drag_end(|_| {})
        .build(&mut ctx);
    assert_eq!(
        node.node_type(),
        NodeType::Text,
        "child should pass through"
    );
    let got: Vec<EventType> = node.listeners().iter().map(|l| l.event).collect();
    assert_eq!(
        got,
        vec![
            EventType::MouseDown,
            EventType::DragStart,
            EventType::DragMove,
            EventType::DragEnd,
        ]
    );

    // 禁用：完全透传，不附加任何监听器。
    let mut ctx = BuildContext::new(Rc::new(RefCell::new(StateMap::new())));
    let node = Draggable::new(Text::new("static"))
        .enabled(false)
        .build(&mut ctx);
    assert_eq!(node.node_type(), NodeType::Text);
    assert!(
        node.listeners().is_empty(),
        "disabled draggable adds no listeners"
    );
}
