//! EventManager — 事件管理器
//!
//! 管理焦点/悬停/捕获状态 + 事件分发 + ElementTree 交互状态更新。
//! Application 只需将 winit 事件转发给此管理器，不再直接操作 tree state。

use crate::core::ElementId;
use crate::event::{
    Event, EventContext, EventEffects, EventPhase, HitTestResult, Key, Modifiers, MouseButton,
};
use crate::geometry::Point;
use crate::runtime::element::ElementTree;

/// 事件管理器
pub struct EventManager {
    focused: Option<ElementId>,
    mouse_capture: Option<ElementId>,
    mouse_down: bool,
    hovered: Option<ElementId>,
}

impl EventManager {
    pub fn new() -> Self {
        Self {
            focused: None,
            mouse_capture: None,
            mouse_down: false,
            hovered: None,
        }
    }

    // ---- 查询 ----

    pub fn focused(&self) -> Option<ElementId> {
        self.focused
    }

    pub fn hovered(&self) -> Option<ElementId> {
        self.hovered
    }

    pub fn mouse_capture(&self) -> Option<ElementId> {
        self.mouse_capture
    }

    // ---- 树状态辅助 ----

    fn set_hovered_state(tree: &ElementTree, id: Option<ElementId>, hovered: bool) {
        let Some(id) = id else { return };
        if tree.contains(id) {
            let mut s = tree.state(id);
            s.hovered = hovered;
            tree.set_state(id, s);
        }
    }

    fn set_pressed_state(tree: &ElementTree, id: Option<ElementId>, pressed: bool) {
        let Some(id) = id else { return };
        if tree.contains(id) {
            let mut s = tree.state(id);
            s.pressed = pressed;
            tree.set_state(id, s);
        }
    }

    // ---- 事件处理 ----

    /// 处理鼠标按下
    pub fn handle_mouse_down(
        &mut self,
        point: Point,
        button: MouseButton,
        hit: &HitTestResult,
        tree: &ElementTree,
        mut handler: impl FnMut(ElementId, &Event, &mut EventContext),
    ) -> EventEffects {
        self.mouse_down = true;
        let mut effects = EventEffects::default();

        // 焦点变化
        if self.focused != Some(hit.target) {
            let focus_effects = self.handle_focus_change(Some(hit.target), tree, &mut handler);
            effects.merge(&focus_effects);
        }

        // 设置 pressed 状态
        Self::set_pressed_state(tree, self.hovered, false);
        Self::set_pressed_state(tree, Some(hit.target), true);

        // 三阶段分发
        let mut ctx = EventContext::with_target(hit.target);
        let event = Event::MouseDown {
            x: point.x,
            y: point.y,
            button,
        };
        dispatch_three_phase(&event, hit, &mut handler, &mut ctx);
        effects.merge(&ctx.take_effects());

        effects
    }

    /// 处理鼠标释放
    pub fn handle_mouse_up(
        &mut self,
        point: Point,
        button: MouseButton,
        hit: &HitTestResult,
        tree: &ElementTree,
        mut handler: impl FnMut(ElementId, &Event, &mut EventContext),
    ) -> EventEffects {
        let mut effects = EventEffects::default();

        // 清除 pressed
        Self::set_pressed_state(tree, self.mouse_capture.or(Some(hit.target)), false);

        if let Some(capture) = self.mouse_capture.take() {
            let mut ctx = EventContext::with_target(capture);
            let event = Event::MouseUp {
                x: point.x,
                y: point.y,
                button,
            };
            handler(capture, &event, &mut ctx);
            effects.merge(&ctx.take_effects());

            if hit.target == capture {
                let mut ctx = EventContext::with_target(capture);
                handler(capture, &Event::Click { button }, &mut ctx);
                effects.merge(&ctx.take_effects());
            }
        } else {
            let mut ctx = EventContext::with_target(hit.target);
            dispatch_three_phase(
                &Event::MouseUp {
                    x: point.x,
                    y: point.y,
                    button,
                },
                hit,
                &mut handler,
                &mut ctx,
            );
            effects.merge(&ctx.take_effects());

            let mut ctx = EventContext::with_target(hit.target);
            dispatch_three_phase(&Event::Click { button }, hit, &mut handler, &mut ctx);
            effects.merge(&ctx.take_effects());
        }

        self.mouse_down = false;
        effects
    }

    /// 处理鼠标移动（含 hover 状态管理）
    pub fn handle_mouse_move(
        &mut self,
        point: Point,
        hit: Option<&HitTestResult>,
        tree: &ElementTree,
        mut handler: impl FnMut(ElementId, &Event, &mut EventContext),
    ) -> EventEffects {
        let mut effects = EventEffects::default();

        let event = Event::MouseMove {
            x: point.x,
            y: point.y,
        };

        if let Some(capture) = self.mouse_capture {
            let mut ctx = EventContext::with_target(capture);
            handler(capture, &event, &mut ctx);
            effects.merge(&ctx.take_effects());
        } else if let Some(hit) = hit {
            let mut ctx = EventContext::with_target(hit.target);
            dispatch_three_phase(&event, hit, &mut handler, &mut ctx);
            effects.merge(&ctx.take_effects());
        }

        // 悬停状态变化
        let current_hover = hit.map(|h| h.target);
        if current_hover != self.hovered {
            // 清除旧悬停
            Self::set_hovered_state(tree, self.hovered, false);
            if let Some(prev) = self.hovered {
                let mut ctx = EventContext::with_target(prev);
                handler(prev, &Event::MouseLeave, &mut ctx);
                effects.merge(&ctx.take_effects());
            }
            // 设置新悬停
            Self::set_hovered_state(tree, current_hover, true);
            if let Some(cur) = current_hover {
                let mut ctx = EventContext::with_target(cur);
                handler(cur, &Event::MouseEnter, &mut ctx);
                effects.merge(&ctx.take_effects());
            }
            self.hovered = current_hover;
        }

        effects
    }

    /// 处理鼠标滚轮
    pub fn handle_wheel(
        &mut self,
        point: Point,
        delta_x: f32,
        delta_y: f32,
        hit: &HitTestResult,
        mut handler: impl FnMut(ElementId, &Event, &mut EventContext),
    ) -> EventEffects {
        let mut ctx = EventContext::with_target(hit.target);
        dispatch_three_phase(
            &Event::MouseWheel {
                delta_x,
                delta_y,
                x: point.x,
                y: point.y,
            },
            hit,
            &mut handler,
            &mut ctx,
        );
        ctx.take_effects()
    }

    // ---- 焦点管理 ----

    pub fn handle_focus_change(
        &mut self,
        new_focus: Option<ElementId>,
        tree: &ElementTree,
        handler: &mut impl FnMut(ElementId, &Event, &mut EventContext),
    ) -> EventEffects {
        let old_focus = self.focused;
        let mut effects = EventEffects::default();

        if old_focus == new_focus {
            return effects;
        }

        if let Some(old_id) = old_focus {
            let mut ctx = EventContext::with_target(old_id);
            handler(old_id, &Event::FocusOut, &mut ctx);
            effects.merge(&ctx.take_effects());
            if tree.contains(old_id) {
                let mut s = tree.state(old_id);
                s.focused = false;
                tree.set_state(old_id, s);
            }
        }

        if let Some(new_id) = new_focus {
            let mut ctx = EventContext::with_target(new_id);
            handler(new_id, &Event::FocusIn, &mut ctx);
            effects.merge(&ctx.take_effects());
            if tree.contains(new_id) {
                let mut s = tree.state(new_id);
                s.focused = true;
                tree.set_state(new_id, s);
            }
        }

        self.focused = new_focus;
        effects
    }

    // ---- 键盘事件 ----

    pub fn handle_key_down(
        &mut self,
        key: Key,
        modifiers: Modifiers,
        mut handler: impl FnMut(ElementId, &Event, &mut EventContext),
    ) -> EventEffects {
        if let Some(target) = self.focused {
            let mut ctx = EventContext::with_target(target);
            handler(target, &Event::KeyDown { key, modifiers }, &mut ctx);
            ctx.take_effects()
        } else {
            EventEffects::default()
        }
    }

    pub fn handle_key_up(
        &mut self,
        key: Key,
        modifiers: Modifiers,
        mut handler: impl FnMut(ElementId, &Event, &mut EventContext),
    ) -> EventEffects {
        if let Some(target) = self.focused {
            let mut ctx = EventContext::with_target(target);
            handler(target, &Event::KeyUp { key, modifiers }, &mut ctx);
            ctx.take_effects()
        } else {
            EventEffects::default()
        }
    }
}

impl Default for EventManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---- 三阶段事件分发 ----

pub fn dispatch_three_phase(
    event: &Event,
    hit: &HitTestResult,
    handler: &mut impl FnMut(ElementId, &Event, &mut EventContext),
    ctx: &mut EventContext,
) {
    for &id in hit.path.iter().rev().skip(1) {
        ctx.set_phase(EventPhase::Capture);
        handler(id, event, ctx);
        if ctx.is_stopped() {
            return;
        }
    }

    ctx.set_phase(EventPhase::Target);
    handler(hit.target, event, ctx);
    if ctx.is_stopped() {
        return;
    }

    for &id in hit.path.iter().rev().skip(1) {
        ctx.set_phase(EventPhase::Bubble);
        handler(id, event, ctx);
        if ctx.is_stopped() {
            return;
        }
    }
}
