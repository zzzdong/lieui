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
use std::collections::HashSet;

/// 进行中的拖拽状态（由 listener 调用 `ctx.begin_drag()` 开启）。
struct DragState {
    /// 拖拽源：发起拖拽的节点，后续 Drag 事件全部投递给它。
    source: ElementId,
    /// 按下点（窗口坐标）
    start: Point,
    /// 上一次鼠标移动的位置（窗口坐标），用于计算增量
    last: Point,
    /// 触发 DragStart 的移动阈值（像素）
    threshold: f32,
    /// 是否已超过阈值并投递过 DragStart
    started: bool,
    button: MouseButton,
    modifiers: Modifiers,
}

/// 事件管理器
pub struct EventManager {
    focused: Option<ElementId>,
    mouse_capture: Option<ElementId>,
    mouse_down: bool,
    /// 进行中的拖拽（拖拽请求隐含鼠标捕获）。
    drag: Option<DragState>,
    /// 命中最内层目标（可能无 listener）
    hovered: Option<ElementId>,
    /// 当前 hover 路径上所有带 listener 的节点（HTML 式 mouseenter 语义）
    hovered_listeners: Vec<ElementId>,
    /// 真正被按下（pressed）的节点。用于在鼠标释放时清对位置，
    /// 避免「按下 A → 移到 B → 在 B 释放」时 A 残留 pressed 状态。
    pressed_node: Option<ElementId>,
    /// 按下时 hit.path 上所有带 listener 的节点，释放时需清除它们的 pressed 状态
    pressed_listeners: Vec<ElementId>,
    /// 当前焦点元素是否监听 IME 事件。
    focused_ime: bool,
    /// 连击计数状态：上次按下时刻/位置/按钮。
    last_click_at: Option<std::time::Instant>,
    last_click_pos: Point,
    last_click_button: Option<MouseButton>,
    click_count: u8,
}

impl EventManager {
    pub fn new() -> Self {
        Self {
            focused: None,
            mouse_capture: None,
            mouse_down: false,
            drag: None,
            hovered: None,
            hovered_listeners: Vec::new(),
            pressed_node: None,
            pressed_listeners: Vec::new(),
            focused_ime: false,
            last_click_at: None,
            last_click_pos: Point::zero(),
            last_click_button: None,
            click_count: 0,
        }
    }

    // ---- 查询 ----

    pub fn focused(&self) -> Option<ElementId> {
        self.focused
    }

    pub fn focused_ime(&self) -> bool {
        self.focused_ime
    }

    pub fn hovered(&self) -> Option<ElementId> {
        self.hovered
    }

    pub fn mouse_capture(&self) -> Option<ElementId> {
        self.mouse_capture
    }

    /// 由应用层显式设置/清除鼠标捕获（例如 ScrollView 开始/结束拖拽滚动）。
    pub fn set_mouse_capture(&mut self, id: Option<ElementId>) {
        self.mouse_capture = id;
    }

    // ---- 树状态辅助 ----

    fn set_hovered_state(tree: &ElementTree, id: Option<ElementId>, hovered: bool) {
        let Some(id) = id else { return };
        if tree.contains(id) {
            let s = tree.state(id);
            if s.hovered != hovered {
                let mut s = s;
                s.hovered = hovered;
                tree.set_state(id, s);
            }
        }
    }

    fn set_pressed_state(tree: &ElementTree, id: Option<ElementId>, pressed: bool) {
        let Some(id) = id else { return };
        if tree.contains(id) {
            let s = tree.state(id);
            if s.pressed != pressed {
                let mut s = s;
                s.pressed = pressed;
                tree.set_state(id, s);
            }
        }
    }

    /// 从命中目标向上查找最近的「可聚焦」节点。
    /// 可聚焦定义为：注册了 FocusIn 或任意 IME 事件监听器。
    /// 找不到时返回 None，表示点击空白/非可聚焦区域，应清除焦点。
    fn find_focusable_ancestor(tree: &ElementTree, hit: &HitTestResult) -> Option<ElementId> {
        for &id in hit.path.iter().rev() {
            if tree.has_any_listener(id) {
                let listeners = tree.listeners(id);
                if listeners.iter().any(|l| {
                    matches!(
                        l.event,
                        crate::event::EventType::FocusIn
                            | crate::event::EventType::ImePreedit
                            | crate::event::EventType::ImeCommit
                            | crate::event::EventType::ImeDisabled
                    )
                }) {
                    return Some(id);
                }
            }
        }
        None
    }

    // ---- 事件处理 ----

    /// 处理鼠标按下
    pub fn handle_mouse_down(
        &mut self,
        point: Point,
        button: MouseButton,
        modifiers: Modifiers,
        hit: &HitTestResult,
        tree: &ElementTree,
        mut handler: impl FnMut(ElementId, &Event, &mut EventContext),
    ) -> EventEffects {
        // 防御：正常情况下一次拖拽会在 mouse_up 结束，这里兜底清理残留状态。
        self.drag = None;
        self.mouse_down = true;
        let mut effects = EventEffects::default();

        // 连击计数：同按钮、500ms 内、位移 ≤ 4px 时递增（1→2→3→1 循环）。
        let now = std::time::Instant::now();
        let is_multi = self
            .last_click_at
            .is_some_and(|t| now.duration_since(t).as_millis() <= 500)
            && self.last_click_button == Some(button)
            && (point.x - self.last_click_pos.x).abs() <= 4.0
            && (point.y - self.last_click_pos.y).abs() <= 4.0;
        self.click_count = if is_multi {
            self.click_count % 3 + 1
        } else {
            1
        };
        self.last_click_at = Some(now);
        self.last_click_pos = point;
        self.last_click_button = Some(button);

        // 焦点应落到最近的「可聚焦」祖先：优先查找带 FocusIn/IME 监听器的节点，
        // 找不到则清除焦点。这样点击 Input 内的 Text 子节点时，焦点仍归 Input。
        let focus_target = Self::find_focusable_ancestor(tree, hit);
        let target = hit.target;

        // 焦点变化
        if self.focused != focus_target {
            let focus_effects = self.handle_focus_change(focus_target, tree, &mut handler);
            effects.merge(&focus_effects);
        }

        // 清除此前可能残留的 pressed
        Self::set_pressed_state(tree, self.pressed_node, false);
        for &id in &self.pressed_listeners {
            Self::set_pressed_state(tree, Some(id), false);
        }

        // 在当前命中目标以及 hit.path 上所有带 listener 或声明了
        // hover/pressed 视觉样式的节点上设置 pressed。这样 Button 等组件根在
        // 内部子节点被按下时也能正确显示 pressed 反馈，无回调的 IconButton 同样生效。
        Self::set_pressed_state(tree, Some(target), true);
        self.pressed_node = Some(target);
        self.pressed_listeners = hit
            .path
            .iter()
            .copied()
            .filter(|&id| tree.has_any_listener(id) || tree.is_interactive(id))
            .collect();
        for &id in &self.pressed_listeners {
            Self::set_pressed_state(tree, Some(id), true);
        }
        // pressed 视觉状态变化需要重绘，否则按下反馈不会显示。
        if !self.pressed_listeners.is_empty() {
            effects.request_render();
        }

        // 三阶段分发
        let mut ctx = EventContext::with_target(hit.target);
        let event = Event::MouseDown {
            x: point.x,
            y: point.y,
            button,
            modifiers,
            click_count: self.click_count,
        };
        dispatch_three_phase(&event, hit, &mut handler, &mut ctx);
        // 回调可能请求鼠标捕获（拖拽选取等），释放时自动解除。
        if let Some(cap) = ctx.take_capture_request() {
            self.mouse_capture = Some(cap);
        }
        // 拖拽请求：隐含鼠标捕获，后续移动/释放事件全部投递给拖拽源。
        if let Some((source, threshold)) = ctx.take_drag_request() {
            self.mouse_capture = Some(source);
            self.drag = Some(DragState {
                source,
                start: point,
                last: point,
                threshold,
                started: false,
                button,
                modifiers,
            });
        }
        if ctx.is_stopped() {
            effects.stop_propagation();
        }
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

        // 拖拽结束：在 MouseUp 之前向拖拽源投递 DragEnd（仅当真正开始过拖拽）。
        let dragged = self.drag.as_ref().is_some_and(|d| d.started);
        if let Some(drag) = self.drag.take()
            && drag.started
        {
            let mut ctx = EventContext::with_target(drag.source);
            let event = Event::DragEnd {
                x: point.x,
                y: point.y,
                dx: point.x - drag.last.x,
                dy: point.y - drag.last.y,
                offset_x: point.x - drag.start.x,
                offset_y: point.y - drag.start.y,
                button: drag.button,
                modifiers: drag.modifiers,
            };
            handler(drag.source, &event, &mut ctx);
            effects.merge(&ctx.take_effects());
        }

        // 清除 pressed：同时清捕获节点、真正按下的节点以及按下时带 listener 的祖先节点，
        // 避免任一节点残留 pressed。（修复「按下 A → 移到 B → 在 B 释放」时 A 残留 pressed 的问题）
        Self::set_pressed_state(tree, self.mouse_capture, false);
        Self::set_pressed_state(tree, self.pressed_node, false);
        for &id in &self.pressed_listeners {
            Self::set_pressed_state(tree, Some(id), false);
        }
        // 清除 pressed 后同样需要重绘恢复常态视觉。
        if self.pressed_node.is_some() || !self.pressed_listeners.is_empty() {
            effects.request_render();
        }
        self.pressed_node = None;
        self.pressed_listeners.clear();

        if let Some(capture) = self.mouse_capture.take() {
            let mut ctx = EventContext::with_target(capture);
            let event = Event::MouseUp {
                x: point.x,
                y: point.y,
                button,
            };
            handler(capture, &event, &mut ctx);
            effects.merge(&ctx.take_effects());

            // 真实发生拖拽（超过阈值）时抑制 Click，避免「拖拽后又触发点击」。
            if hit.target == capture && !dragged {
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

        if let Some(capture) = self.mouse_capture {
            // 推进拖拽状态：超过阈值后开始合成 Drag 事件。
            let mut drag_started_now = false;
            let mut prev = None;
            if let Some(drag) = &mut self.drag {
                prev = Some(drag.last);
                drag.last = point;
                if !drag.started {
                    let dist = ((point.x - drag.start.x).powi(2)
                        + (point.y - drag.start.y).powi(2))
                    .sqrt();
                    if dist >= drag.threshold {
                        drag.started = true;
                        drag_started_now = true;
                    }
                }
            }

            let event = if let Some(drag) = &self.drag {
                if drag.started {
                    if drag_started_now {
                        Event::DragStart {
                            x: point.x,
                            y: point.y,
                            offset_x: 0.0,
                            offset_y: 0.0,
                            button: drag.button,
                            modifiers: drag.modifiers,
                        }
                    } else {
                        let prev = prev.expect("drag last known");
                        Event::DragMove {
                            x: point.x,
                            y: point.y,
                            dx: point.x - prev.x,
                            dy: point.y - prev.y,
                            offset_x: point.x - drag.start.x,
                            offset_y: point.y - drag.start.y,
                            button: drag.button,
                            modifiers: drag.modifiers,
                        }
                    }
                } else {
                    Event::MouseMove {
                        x: point.x,
                        y: point.y,
                    }
                }
            } else {
                Event::MouseMove {
                    x: point.x,
                    y: point.y,
                }
            };

            let mut ctx = EventContext::with_target(capture);
            handler(capture, &event, &mut ctx);
            effects.merge(&ctx.take_effects());
            // 拖拽过程中位置变化需要重绘
            effects.request_render();
        } else if let Some(hit) = hit {
            let event = Event::MouseMove {
                x: point.x,
                y: point.y,
            };
            let mut ctx = EventContext::with_target(hit.target);
            dispatch_three_phase(&event, hit, &mut handler, &mut ctx);
            effects.merge(&ctx.take_effects());
        }

        // HTML 式悬停状态管理：
        // - 命中最内层目标记录为 self.hovered；
        // - hit.path 上所有带 listener 或声明了 hover/pressed 视觉样式的节点
        //   都会获得 hovered 状态并触发 MouseEnter/MouseLeave。
        // 这样 Button 等组件根在鼠标悬停其内部子节点时也能正确显示 hover 反馈，
        // 且无回调的 IconButton 也能显示 hover 样式。
        let new_target = hit.map(|h| h.target);
        let new_listeners: Vec<ElementId> = hit
            .map(|h| {
                h.path
                    .iter()
                    .copied()
                    .filter(|&id| tree.has_any_listener(id) || tree.is_interactive(id))
                    .collect()
            })
            .unwrap_or_default();

        if new_target != self.hovered {
            Self::set_hovered_state(tree, self.hovered, false);
            Self::set_hovered_state(tree, new_target, true);
            self.hovered = new_target;
            effects.request_render();
        }

        let old_set: HashSet<ElementId> = self.hovered_listeners.iter().copied().collect();
        let new_set: HashSet<ElementId> = new_listeners.iter().copied().collect();

        for &id in &self.hovered_listeners {
            if !new_set.contains(&id) {
                Self::set_hovered_state(tree, Some(id), false);
                let mut ctx = EventContext::with_target(id);
                handler(id, &Event::MouseLeave, &mut ctx);
                effects.merge(&ctx.take_effects());
                effects.request_render();
            }
        }
        for &id in &new_listeners {
            if !old_set.contains(&id) {
                Self::set_hovered_state(tree, Some(id), true);
                let mut ctx = EventContext::with_target(id);
                handler(id, &Event::MouseEnter, &mut ctx);
                effects.merge(&ctx.take_effects());
                effects.request_render();
            }
        }
        self.hovered_listeners = new_listeners;

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

        self.focused_ime = new_focus.is_some_and(|id| tree.has_ime_listener(id));

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

    // ---- IME 事件 ----

    pub fn handle_ime_preedit(
        &mut self,
        text: String,
        cursor_start: Option<usize>,
        cursor_end: Option<usize>,
        mut handler: impl FnMut(ElementId, &Event, &mut EventContext),
    ) -> EventEffects {
        if let Some(target) = self.focused {
            let mut ctx = EventContext::with_target(target);
            handler(
                target,
                &Event::ImePreedit {
                    text,
                    cursor_start,
                    cursor_end,
                },
                &mut ctx,
            );
            ctx.take_effects()
        } else {
            EventEffects::default()
        }
    }

    pub fn handle_ime_commit(
        &mut self,
        text: String,
        mut handler: impl FnMut(ElementId, &Event, &mut EventContext),
    ) -> EventEffects {
        if let Some(target) = self.focused {
            let mut ctx = EventContext::with_target(target);
            handler(target, &Event::ImeCommit { text }, &mut ctx);
            ctx.take_effects()
        } else {
            EventEffects::default()
        }
    }

    pub fn handle_ime_disabled(
        &mut self,
        mut handler: impl FnMut(ElementId, &Event, &mut EventContext),
    ) -> EventEffects {
        if let Some(target) = self.focused {
            let mut ctx = EventContext::with_target(target);
            handler(target, &Event::ImeDisabled, &mut ctx);
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

/// 把事件分发给指定节点上匹配的监听器。
///
/// 同一节点上**内置行为回调**（`ListenerKind::BuiltIn`，组件内部接线，
/// 如 Input 聚焦/输入、Slider 拖拽）**先于用户回调**（`ListenerKind::User`）执行；
/// 因此用户回调调用 `stop_propagation()` 不会阻止同节点已执行的内置行为，
/// 只会停止向其他节点传播。`ctx.set_event` / `ctx.set_current` 由调用方负责。
pub fn dispatch_node_listeners(
    tree: &ElementTree,
    id: ElementId,
    event: &Event,
    ctx: &mut EventContext,
) {
    use crate::view::node::ListenerKind as LK;
    let Some(node) = tree.get_node_ref(id) else {
        return;
    };
    let event_type = event.to_type();
    for pass in [LK::BuiltIn, LK::User] {
        for listener in node.listeners().iter().filter(|l| l.kind == pass) {
            if listener.event != event_type {
                continue;
            }
            match ctx.phase() {
                EventPhase::Capture => {
                    if let crate::view::node::Callback::WithCtx(cb) = &listener.callback {
                        cb(ctx);
                    }
                }
                EventPhase::Target => match &listener.callback {
                    crate::view::node::Callback::Simple(cb) => {
                        cb();
                        ctx.stop_propagation();
                    }
                    crate::view::node::Callback::WithCtx(cb) => {
                        cb(ctx);
                    }
                },
                EventPhase::Bubble => {
                    if let crate::view::node::Callback::Simple(cb) = &listener.callback {
                        cb();
                    }
                }
            }
            if ctx.is_stopped() {
                break;
            }
        }
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
    for &id in hit.path.iter().take(hit.path.len().saturating_sub(1)).rev() {
        ctx.set_phase(EventPhase::Bubble);
        handler(id, event, ctx);
        if ctx.is_stopped() {
            return;
        }
    }
}
