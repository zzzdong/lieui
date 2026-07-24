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
    /// 真正被按下（pressed）的节点。用于在鼠标释放时清对位置，
    /// 避免「按下 A → 移到 B → 在 B 释放」时 A 残留 pressed 状态。
    pressed_node: Option<ElementId>,
}

impl EventManager {
    pub fn new() -> Self {
        Self {
            focused: None,
            mouse_capture: None,
            mouse_down: false,
            hovered: None,
            pressed_node: None,
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

    /// 解析「交互节点」：从命中目标 `hit.target` 沿祖先链向上，
    /// 找到最近的“交互组件根”（`interactive`）节点。
    ///
    /// 命中测试返回的是最内侧的叶子（如 Button 内的文字/装饰 Div），
    /// 但视觉交互状态（hover/pressed/focus）应作用在最近的 `interactive` 外层节点上，
    /// 否则该类节点的 `tree.state(id)` 永远不会被设置，导致高亮/按下反馈失效。
    /// `interactive` 与是否挂有 listener 解耦：每节点都可有 listener，但只有交互组件
    /// 根才参与视觉状态与点击分发。
    fn resolve_interactive(&self, tree: &ElementTree, hit: &HitTestResult) -> Option<ElementId> {
        // hit.path: root -> ... -> target；从最内侧 target 向上找第一个 interactive 的
        hit.path
            .iter()
            .rev()
            .find(|&&id| tree.get_node_ref(id).map(|n| n.is_interactive()).unwrap_or(false))
            .copied()
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

        // 交互节点（最近的可点击祖先）：视觉状态 + 焦点都应作用于它
        let interactive = self.resolve_interactive(tree, hit);

        // 焦点变化
        if self.focused != interactive {
            let focus_effects = self.handle_focus_change(interactive, tree, &mut handler);
            effects.merge(&focus_effects);
        }

        // 清除此前可能残留的 pressed，并在当前交互节点上设置 pressed
        Self::set_pressed_state(tree, self.pressed_node, false);
        Self::set_pressed_state(tree, interactive, true);
        self.pressed_node = interactive;

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

        // 清除 pressed：优先清捕获节点，否则清真正按下的节点
        // （修复「按下 A → 移到 B → 在 B 释放」时 A 残留 pressed 的问题）
        let pressed = self.mouse_capture.or(self.pressed_node);
        Self::set_pressed_state(tree, pressed, false);
        self.pressed_node = None;

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
            // 拖拽过程中位置变化需要重绘
            effects.request_render();
        } else if let Some(hit) = hit {
            let mut ctx = EventContext::with_target(hit.target);
            dispatch_three_phase(&event, hit, &mut handler, &mut ctx);
            effects.merge(&ctx.take_effects());
        }

        // 悬停状态变化：作用在交互节点（最近的可点击祖先）上
        let current_hover = hit.and_then(|h| self.resolve_interactive(tree, h));
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
            // 悬停目标变化会改变交互节点的视觉状态，需触发重绘
            effects.request_render();
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
    // 捕获阶段：从最外层 root 向 target（不含 target）传播，外层 → 内层
    for &id in hit.path.iter().take(hit.path.len().saturating_sub(1)) {
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

    // 冒泡阶段：从 target 的父节点向 root 传播，内层 → 外层
    for &id in hit.path
        .iter()
        .take(hit.path.len().saturating_sub(1))
        .rev()
    {
        ctx.set_phase(EventPhase::Bubble);
        handler(id, event, ctx);
        if ctx.is_stopped() {
            return;
        }
    }
}
