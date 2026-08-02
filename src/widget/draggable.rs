//! Draggable — 通用拖拽容器
//!
//! 把任意子内容包装为可拖拽节点。拖拽接线完全由引擎完成：
//! 按下左键 → 移动超过阈值 → 合成 `DragStart` / `DragMove` / `DragEnd`
//! 事件并投递给本节点（自动捕获鼠标，指针移出组件后仍持续收到事件）。
//!
//! 拖拽回调通过 `ctx.event()` 读取事件数据，例如：
//!
//! ```ignore
//! Draggable::new(card)
//!     .on_drag_move(|ctx| {
//!         if let Some(Event::DragMove { offset_x, offset_y, .. }) = ctx.event() {
//!             // 用累计偏移移动卡片
//!         }
//!     })
//! ```
//!
//! 注意：拖拽开始后本节点会捕获鼠标并接管后续所有移动/释放事件，
//! 因此不要用 Draggable 包裹 Button/Slider 等自身需要鼠标交互的组件；
//! 需要时用 `.enabled(false)` 关闭拖拽。

use crate::event::EventContext;
use crate::view::node::{Listener, ViewNode};
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;

type DragCallback = Rc<dyn Fn(&mut EventContext)>;

/// 通用拖拽容器。
pub struct Draggable {
    child: Box<dyn Widget>,
    threshold: f32,
    enabled: bool,
    on_drag_start: Option<DragCallback>,
    on_drag_move: Option<DragCallback>,
    on_drag_end: Option<DragCallback>,
}

impl Draggable {
    pub fn new(child: impl Widget + 'static) -> Self {
        Self {
            child: Box::new(child),
            threshold: crate::event::DEFAULT_DRAG_THRESHOLD,
            enabled: true,
            on_drag_start: None,
            on_drag_move: None,
            on_drag_end: None,
        }
    }

    /// 触发 `DragStart` 的移动阈值（像素），默认 3.0。
    pub fn threshold(mut self, px: f32) -> Self {
        self.threshold = px.max(0.0);
        self
    }

    /// 是否启用拖拽。关闭时完全透传子内容（不附加任何监听器）。
    pub fn enabled(mut self, v: bool) -> Self {
        self.enabled = v;
        self
    }

    /// 拖拽开始（移动超过阈值）回调。
    pub fn on_drag_start<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.on_drag_start = Some(Rc::new(f));
        self
    }

    /// 拖拽进行中回调。
    pub fn on_drag_move<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.on_drag_move = Some(Rc::new(f));
        self
    }

    /// 拖拽结束（鼠标释放）回调。
    pub fn on_drag_end<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.on_drag_end = Some(Rc::new(f));
        self
    }
}

impl Widget for Draggable {
    fn key(&self) -> Option<&str> {
        self.child.key()
    }

    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let mut node = ctx.child(0, self.child.as_ref());
        if !self.enabled {
            return node;
        }

        let threshold = self.threshold;
        node.add_listener(
            Listener::on_mouse_down(Rc::new(move |ctx: &mut EventContext| {
                if matches!(
                    ctx.event(),
                    Some(crate::event::Event::MouseDown {
                        button: crate::event::MouseButton::Left,
                        ..
                    })
                ) {
                    ctx.begin_drag_with_threshold(threshold);
                }
            }))
            .builtin(),
        );

        if let Some(cb) = &self.on_drag_start {
            node.add_listener(Listener::on_drag_start(Rc::clone(cb)).builtin());
        }
        if let Some(cb) = &self.on_drag_move {
            node.add_listener(Listener::on_drag_move(Rc::clone(cb)).builtin());
        }
        if let Some(cb) = &self.on_drag_end {
            node.add_listener(Listener::on_drag_end(Rc::clone(cb)).builtin());
        }

        node
    }
}
