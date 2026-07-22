//! EventManager — 事件管理器
//!
//! 管理焦点/悬停状态和事件分发。
//! 使用 ElementId handle 而非直接引用。

use crate::core::ElementId;
use crate::event::{
    Event, EventContext, EventEffects, EventPhase, HitTestResult, Key, Modifiers, MouseButton,
};
use crate::geometry::Point;

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

    // ---- 清理 ----

    pub fn clear_focused(&mut self) {
        self.focused = None;
    }

    pub fn clear_hovered(&mut self) {
        self.hovered = None;
    }

    pub fn clear_mouse_capture(&mut self) {
        self.mouse_capture = None;
    }

    // ---- 事件处理（存根：真正的分发逻辑在 Runtime 中通过 Layers 完成）----

    /// 处理鼠标按下
    ///
    /// 由 Runtime 调用，传入 hit-test 结果。
    pub fn handle_mouse_down(
        &mut self,
        point: Point,
        button: MouseButton,
        hit: &HitTestResult,
        mut handler: impl FnMut(ElementId, &Event, &mut EventContext),
    ) -> EventEffects {
        self.mouse_down = true;
        let mut effects = EventEffects::default();

        // 处理焦点变化
        if self.focused != Some(hit.target) {
            let focus_effects = self.handle_focus_change(Some(hit.target), &mut handler);
            effects.merge(&focus_effects);
        }

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
        mut handler: impl FnMut(ElementId, &Event, &mut EventContext),
    ) -> EventEffects {
        let mut effects = EventEffects::default();

        if let Some(capture) = self.mouse_capture.take() {
            // 直接分发给捕获的 element
            let mut ctx = EventContext::with_target(capture);
            let event = Event::MouseUp {
                x: point.x,
                y: point.y,
                button,
            };
            handler(capture, &event, &mut ctx);
            effects.merge(&ctx.take_effects());

            // 检查点击
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

    /// 处理鼠标移动
    pub fn handle_mouse_move(
        &mut self,
        point: Point,
        hit: Option<&HitTestResult>,
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
            if let Some(prev) = self.hovered {
                let mut ctx = EventContext::with_target(prev);
                handler(prev, &Event::MouseLeave, &mut ctx);
                effects.merge(&ctx.take_effects());
            }
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
        }

        if let Some(new_id) = new_focus {
            let mut ctx = EventContext::with_target(new_id);
            handler(new_id, &Event::FocusIn, &mut ctx);
            effects.merge(&ctx.take_effects());
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
    // Phase 1: Capture（从根到目标父级）
    for &id in hit.path.iter().rev().skip(1) {
        ctx.set_phase(EventPhase::Capture);
        handler(id, event, ctx);
        if ctx.is_stopped() {
            return;
        }
    }

    // Phase 2: Target
    ctx.set_phase(EventPhase::Target);
    handler(hit.target, event, ctx);
    if ctx.is_stopped() {
        return;
    }

    // Phase 3: Bubble（从目标父级到根）
    for &id in hit.path.iter().rev().skip(1) {
        ctx.set_phase(EventPhase::Bubble);
        handler(id, event, ctx);
        if ctx.is_stopped() {
            return;
        }
    }
}
