// src/event/context.rs
//! 事件上下文
//!
//! 提供事件处理期间对 Widget 树的安全访问和副作用收集能力

use crate::core::WidgetId;
use crate::event::Propagation;
use crate::widget::{Widget, WidgetTree};
use std::cell::{Ref, RefCell, RefMut};

/// 事件副作用
#[derive(Debug, Default)]
pub struct EventEffects {
    pub needs_render: bool,
    pub needs_layout: bool,
    pub needs_animate: bool,
}

impl EventEffects {
    pub fn request_render(&mut self) {
        self.needs_render = true;
    }

    pub fn request_layout(&mut self) {
        self.needs_layout = true;
    }

    pub fn request_animate(&mut self) {
        self.needs_animate = true;
    }

    pub fn needs_render(&self) -> bool {
        self.needs_render
    }

    pub fn needs_layout(&self) -> bool {
        self.needs_layout
    }

    pub fn needs_animate(&self) -> bool {
        self.needs_animate
    }

    pub fn clear(&mut self) {
        self.needs_render = false;
        self.needs_layout = false;
        self.needs_animate = false;
    }

    /// 合并另一个 EventEffects 的状态
    pub fn merge(&mut self, other: &EventEffects) {
        self.needs_render |= other.needs_render;
        self.needs_layout |= other.needs_layout;
        self.needs_animate |= other.needs_animate;
    }
}

/// 事件上下文，贯穿整个事件处理流程
///
/// 提供：
/// 1. 对 Widget 树的安全访问
/// 2. 副作用收集（渲染、布局、动画）
/// 3. 传播控制
pub struct EventContext<'a> {
    /// Widget 树引用
    tree: &'a WidgetTree,

    /// 副作用收集器
    effects: RefCell<EventEffects>,

    /// 传播控制器
    propagation: RefCell<Propagation>,
}

impl<'a> EventContext<'a> {
    /// 创建新的事件上下文
    pub fn new(tree: &'a WidgetTree) -> Self {
        Self {
            tree,
            effects: RefCell::new(EventEffects::default()),
            propagation: RefCell::new(Propagation::new()),
        }
    }

    // ========== Widget 访问 ==========

    /// 获取 Widget 的不可变引用
    pub fn get<W: Widget + 'static>(&self, id: WidgetId) -> Option<Ref<'_, W>> { 
        self.tree.get::<W>(id)
    }

    /// 获取 Widget 的可变引用
    pub fn get_mut<W: Widget + 'static>(&self, id: WidgetId) -> Option<RefMut<'_, W>> { 
        self.tree.get_mut::<W>(id)
    }

    /// 获取 WidgetTree 引用
    pub fn tree(&self) -> &WidgetTree {
        self.tree
    }

    // ========== 副作用 ==========

    /// 请求重新渲染
    pub fn request_render(&self) {
        self.effects.borrow_mut().request_render();
    }

    /// 请求重新布局
    pub fn request_layout(&self) {
        self.effects.borrow_mut().request_layout();
    }

    /// 请求动画帧
    pub fn request_animate(&self) {
        self.effects.borrow_mut().request_animate();
    }

    /// 提取副作用（会清空当前副作用）
    pub fn take_effects(&self) -> EventEffects {
        std::mem::take(&mut *self.effects.borrow_mut())
    }

    /// 获取副作用的副本（不清空）
    pub fn effects(&self) -> EventEffects {
        let e = self.effects.borrow();
        EventEffects {
            needs_render: e.needs_render,
            needs_layout: e.needs_layout,
            needs_animate: e.needs_animate,
        }
    }

    // ========== 传播控制 ==========

    /// 停止事件传播
    pub fn stop_propagation(&self) {
        self.propagation.borrow_mut().stop();
    }

    /// 是否已停止传播
    pub fn is_stopped(&self) -> bool {
        self.propagation.borrow().is_stopped()
    }

    /// 重置传播状态
    pub fn reset_propagation(&self) {
        *self.propagation.borrow_mut() = Propagation::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Rect, Size};
    use crate::layout::{IntrinsicSize, LayoutNode};
    use crate::render::RenderNode;
    use crate::widget::Widget;

    struct TestWidget {
        value: i32,
    }

    impl TestWidget {
        fn new(value: i32) -> Self {
            Self { value }
        }
    }

    impl Widget for TestWidget {
        crate::impl_widget_any!(TestWidget);

        fn type_name(&self) -> &'static str {
            "TestWidget"
        }

        fn layout(&self, id: WidgetId) -> LayoutNode {
            LayoutNode::new(id).with_intrinsic_size(IntrinsicSize::Fixed(Size::new(100.0, 100.0)))
        }

        fn render(&mut self, _layout: &LayoutNode, _ctx: &crate::prelude::ViewContext) -> RenderNode {
            RenderNode::view(Rect::zero())
        }
    }

    #[test]
    fn test_event_effects() {
        let mut effects = EventEffects::default();
        assert!(!effects.needs_render());
        assert!(!effects.needs_layout());

        effects.request_render();
        assert!(effects.needs_render());

        effects.request_layout();
        assert!(effects.needs_layout());

        effects.clear();
        assert!(!effects.needs_render());
        assert!(!effects.needs_layout());
    }

    #[test]
    fn test_event_effects_merge() {
        let mut effects1 = EventEffects::default();
        effects1.request_render();

        let mut effects2 = EventEffects::default();
        effects2.request_layout();

        effects1.merge(&effects2);
        assert!(effects1.needs_render());
        assert!(effects1.needs_layout());
    }
}
