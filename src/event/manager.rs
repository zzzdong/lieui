// src/event/manager.rs
//! 事件管理器
//!
//! 改进：
//! - 单次 hit-test，消除重复命中测试
//! - 完整三阶段事件传播：Capture → Target → Bubble
//! - 简化 API，减少参数传递

use crate::core::WidgetId;
use crate::core::layers::{LayerType, Layers};
use crate::event::{Event, EventContext, EventEffects, Key, Modifiers, MouseButton};
use crate::geometry::Point;
use crate::layout::LayoutNode;

/// 事件传播阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPhase {
    /// 捕获阶段：从根到目标父级
    Capture,
    /// 目标阶段：目标元素本身
    Target,
    /// 冒泡阶段：从目标父级到根
    Bubble,
}

/// 命中测试结果（缓存，避免重复计算）
#[derive(Debug, Clone)]
pub struct HitTestResult {
    /// 命中的目标 widget
    pub target: WidgetId,
    /// 从根到目标的路径
    pub path: Vec<WidgetId>,
}

/// 事件管理器
///
/// 管理焦点状态和事件分发逻辑
pub struct EventManager {
    /// 当前焦点 widget
    focused: Option<WidgetId>,
    /// 当前捕获鼠标事件的 widget（拖拽时使用）
    mouse_capture: Option<WidgetId>,
    /// 鼠标是否按下
    mouse_down: bool,
    /// 当前悬停的 widget
    hovered: Option<WidgetId>,
}

impl EventManager {
    /// 创建新的事件管理器
    pub fn new() -> Self {
        Self {
            focused: None,
            mouse_capture: None,
            mouse_down: false,
            hovered: None,
        }
    }

    /// 获取当前焦点 widget
    pub fn focused(&self) -> Option<WidgetId> {
        self.focused
    }

    /// 获取当前悬停的 widget
    pub fn hovered(&self) -> Option<WidgetId> {
        self.hovered
    }

    /// 获取鼠标捕获 widget
    pub fn mouse_capture(&self) -> Option<WidgetId> {
        self.mouse_capture
    }

    // ========================================================================
    // 鼠标事件处理
    // ========================================================================

    /// 处理鼠标按下事件
    pub fn handle_mouse_down(
        &mut self,
        point: Point,
        button: MouseButton,
        layout_root: &LayoutNode,
        layers: &Layers,
        current_layer: LayerType,
    ) -> EventEffects {
        self.mouse_down = true;
        let mut effects = EventEffects::default();

        // 单次 hit-test
        let hit_result = Self::hit_test_with_path(point, layout_root, layers);

        if let Some(hit) = &hit_result {
            // 处理焦点变化
            if self.focused != Some(hit.target) {
                let focus_effects = self.handle_focus_change(Some(hit.target), layers);
                effects.merge(&focus_effects);
            }

            // 检查是否能捕获鼠标
            if Self::widget_can_focus(layers, hit.target) {
                self.mouse_capture = Some(hit.target);
            }

            // 三阶段分发（使用缓存的 hit 结果）
            let ctx = EventContext::new(layers, current_layer);
            let event = Event::MouseDown {
                x: point.x,
                y: point.y,
                button,
            };
            self.dispatch_three_phase(&event, hit, layers, &ctx);
            effects.merge(&ctx.take_effects());
        } else {
            // 点击空白处，清除焦点
            let focus_effects = self.handle_focus_change(None, layers);
            effects.merge(&focus_effects);
        }

        effects
    }

    /// 处理鼠标释放事件
    pub fn handle_mouse_up(
        &mut self,
        point: Point,
        button: MouseButton,
        layout_root: &LayoutNode,
        layers: &Layers,
        current_layer: LayerType,
    ) -> EventEffects {
        self.mouse_down = false;
        let mut effects = EventEffects::default();

        let event = Event::MouseUp {
            x: point.x,
            y: point.y,
            button,
        };

        // 如果有捕获，直接分发给捕获的 widget
        if let Some(capture) = self.mouse_capture.take() {
            let ctx = EventContext::new(layers, current_layer);
            Self::invoke(capture, &event, layers, &ctx);
            effects.merge(&ctx.take_effects());

            // 检查是否是有效的点击（释放位置仍在捕获的 widget 内）
            let hit_result = Self::hit_test_with_path(point, layout_root, layers);
            if let Some(hit) = &hit_result
                && hit.target == capture
            {
                // 发送 Click 事件
                let click_event = Event::Click { button };
                let ctx = EventContext::new(layers, current_layer);
                Self::invoke(capture, &click_event, layers, &ctx);
                effects.merge(&ctx.take_effects());
            }
        } else {
            // 单次 hit-test
            let hit_result = Self::hit_test_with_path(point, layout_root, layers);

            if let Some(hit) = &hit_result {
                let ctx = EventContext::new(layers, current_layer);
                self.dispatch_three_phase(&event, hit, layers, &ctx);
                effects.merge(&ctx.take_effects());

                // 发送 Click 事件
                let click_event = Event::Click { button };
                let ctx = EventContext::new(layers, current_layer);
                self.dispatch_three_phase(&click_event, hit, layers, &ctx);
                effects.merge(&ctx.take_effects());
            }
        }

        effects
    }

    /// 处理鼠标移动事件
    pub fn handle_mouse_move(
        &mut self,
        point: Point,
        layout_root: &LayoutNode,
        layers: &Layers,
        current_layer: LayerType,
    ) -> EventEffects {
        let event = Event::MouseMove {
            x: point.x,
            y: point.y,
        };
        let mut effects = EventEffects::default();

        // 如果有鼠标捕获，直接分发给捕获的 widget
        if let Some(capture) = self.mouse_capture {
            let ctx = EventContext::new(layers, current_layer);
            Self::invoke(capture, &event, layers, &ctx);
            effects.merge(&ctx.take_effects());
        } else {
            // 单次 hit-test
            let hit_result = Self::hit_test_with_path(point, layout_root, layers);
            if let Some(hit) = &hit_result {
                let ctx = EventContext::new(layers, current_layer);
                self.dispatch_three_phase(&event, hit, layers, &ctx);
                effects.merge(&ctx.take_effects());
            }
        }

        // 处理悬停状态变化（使用单次 hit-test）
        let hover_effects = self.handle_hover_change(point, layout_root, layers);
        effects.merge(&hover_effects);

        effects
    }

    /// 处理鼠标滚轮事件
    pub fn handle_wheel(
        &mut self,
        point: Point,
        delta_x: f32,
        delta_y: f32,
        layout_root: &LayoutNode,
        layers: &Layers,
        current_layer: LayerType,
    ) -> EventEffects {
        let event = Event::MouseWheel {
            delta_x,
            delta_y,
            x: point.x,
            y: point.y,
        };

        // 单次 hit-test
        let hit_result = Self::hit_test_with_path(point, layout_root, layers);
        if let Some(hit) = &hit_result {
            let ctx = EventContext::new(layers, current_layer);
            self.dispatch_three_phase(&event, hit, layers, &ctx);
            return ctx.take_effects();
        }

        EventEffects::default()
    }

    // ========================================================================
    // 焦点管理
    // ========================================================================

    /// 设置焦点到指定 widget
    pub fn handle_focus_change(
        &mut self,
        new_focus: Option<WidgetId>,
        layers: &Layers,
    ) -> EventEffects {
        let old_focus = self.focused;
        let mut effects = EventEffects::default();

        if old_focus == new_focus {
            return effects;
        }

        // 焦点离开旧元素
        if let Some(old_id) = old_focus {
            let ctx = EventContext::new(layers, LayerType::Base);
            Self::invoke(old_id, &Event::FocusOut, layers, &ctx);
            effects.merge(&ctx.take_effects());
        }

        // 焦点进入新元素
        if let Some(new_id) = new_focus {
            let ctx = EventContext::new(layers, LayerType::Base);
            Self::invoke(new_id, &Event::FocusIn, layers, &ctx);
            effects.merge(&ctx.take_effects());
        }

        self.focused = new_focus;
        effects
    }

    /// 处理窗口失焦（清除焦点状态）
    pub fn handle_window_unfocus(&mut self, layers: &Layers) -> EventEffects {
        let mut effects = EventEffects::default();
        if let Some(focused) = self.focused.take() {
            let ctx = EventContext::new(layers, LayerType::Base);
            Self::invoke(focused, &Event::FocusOut, layers, &ctx);
            effects.merge(&ctx.take_effects());
        }
        effects
    }

    // ========================================================================
    // 键盘事件处理
    // ========================================================================

    /// 处理键盘按下
    pub fn handle_key_down(
        &mut self,
        key: Key,
        modifiers: Modifiers,
        layers: &Layers,
    ) -> EventEffects {
        self.dispatch_to_focused(&Event::KeyDown { key, modifiers }, layers)
    }

    /// 处理键盘释放
    pub fn handle_key_up(
        &mut self,
        key: Key,
        modifiers: Modifiers,
        layers: &Layers,
    ) -> EventEffects {
        self.dispatch_to_focused(&Event::KeyUp { key, modifiers }, layers)
    }

    // ========================================================================
    // IME 事件处理
    // ========================================================================

    /// 处理 IME 预编辑事件
    pub fn handle_ime_preedit(
        &mut self,
        text: String,
        cursor_start: Option<usize>,
        cursor_end: Option<usize>,
        layers: &Layers,
    ) -> EventEffects {
        self.dispatch_to_focused(
            &Event::ImePreedit {
                text,
                cursor_start,
                cursor_end,
            },
            layers,
        )
    }

    /// 处理 IME 提交事件
    pub fn handle_ime_commit(&mut self, text: String, layers: &Layers) -> EventEffects {
        self.dispatch_to_focused(&Event::ImeCommit { text }, layers)
    }

    /// 处理 IME 禁用事件
    pub fn handle_ime_disabled(&mut self, layers: &Layers) -> EventEffects {
        self.dispatch_to_focused(&Event::ImeDisabled, layers)
    }

    // ========================================================================
    // 内部：命中测试
    // ========================================================================

    /// 单次 hit-test，同时计算路径（避免重复）
    fn hit_test_with_path(
        point: Point,
        layout_root: &LayoutNode,
        layers: &Layers,
    ) -> Option<HitTestResult> {
        let target = layout_root.hit_test(point)?;
        let path = layers.path_to(target);
        Some(HitTestResult { target, path })
    }

    // ========================================================================
    // 内部：事件分发
    // ========================================================================

    /// 三阶段事件分发：Capture → Target → Bubble
    ///
    /// 使用缓存的 HitTestResult，避免重复 hit-test
    fn dispatch_three_phase(
        &self,
        event: &Event,
        hit: &HitTestResult,
        layers: &Layers,
        ctx: &EventContext<'_>,
    ) {
        let target = hit.target;
        let path = &hit.path;

        // Phase 1: Capture（从根到目标父级）
        // 路径: [Root, ..., Parent, Target]
        // Capture 需要遍历 Root → ... → Parent（不包括 Target）
        for &id in path.iter().rev().skip(1) {
            ctx.set_phase(EventPhase::Capture);
            Self::invoke(id, event, layers, ctx);
            if ctx.is_stopped() {
                return;
            }
        }

        // Phase 2: Target
        ctx.set_phase(EventPhase::Target);
        Self::invoke(target, event, layers, ctx);
        if ctx.is_stopped() {
            return;
        }

        // Phase 3: Bubble（从目标父级到根）
        for &id in path.iter().rev().skip(1) {
            ctx.set_phase(EventPhase::Bubble);
            Self::invoke(id, event, layers, ctx);
            if ctx.is_stopped() {
                return;
            }
        }
    }

    /// 向焦点元素分发事件（三阶段）
    fn dispatch_to_focused(&self, event: &Event, layers: &Layers) -> EventEffects {
        let mut effects = EventEffects::default();
        if let Some(target) = self.focused {
            let path = layers.path_to(target);
            let ctx = EventContext::new(layers, LayerType::Base);
            self.dispatch_three_phase_with_path(event, &path, target, layers, &ctx);
            effects.merge(&ctx.take_effects());
        }
        effects
    }

    /// 三阶段分发（已知路径）
    fn dispatch_three_phase_with_path(
        &self,
        event: &Event,
        path: &[WidgetId],
        target: WidgetId,
        layers: &Layers,
        ctx: &EventContext<'_>,
    ) {
        // Phase 1: Capture
        for &id in path.iter().rev().skip(1) {
            ctx.set_phase(EventPhase::Capture);
            Self::invoke(id, event, layers, ctx);
            if ctx.is_stopped() {
                return;
            }
        }

        // Phase 2: Target
        ctx.set_phase(EventPhase::Target);
        Self::invoke(target, event, layers, ctx);
        if ctx.is_stopped() {
            return;
        }

        // Phase 3: Bubble
        for &id in path.iter().rev().skip(1) {
            ctx.set_phase(EventPhase::Bubble);
            Self::invoke(id, event, layers, ctx);
            if ctx.is_stopped() {
                return;
            }
        }
    }

    /// 调用单个 widget 的事件处理
    fn invoke(id: WidgetId, event: &Event, layers: &Layers, ctx: &EventContext<'_>) {
        if let Some(mut widget) = layers.get_widget_mut(id) {
            let _ = widget.handle_event(event, ctx);
        }
    }

    /// 检查 widget 是否可以接受焦点
    fn widget_can_focus(layers: &Layers, id: WidgetId) -> bool {
        layers
            .get_widget(id)
            .map(|w| w.can_focus())
            .unwrap_or(false)
    }

    /// 处理悬停状态变化
    fn handle_hover_change(
        &mut self,
        point: Point,
        layout_root: &LayoutNode,
        layers: &Layers,
    ) -> EventEffects {
        // 单次 hit-test
        let current_hover = layout_root.hit_test(point);
        let prev_hover = self.hovered;
        let mut effects = EventEffects::default();

        if current_hover == prev_hover {
            return effects;
        }

        // 发送 MouseLeave 事件给之前悬停的 widget
        if let Some(prev_id) = prev_hover {
            let ctx = EventContext::new(layers, LayerType::Base);
            Self::invoke(prev_id, &Event::MouseLeave, layers, &ctx);
            effects.merge(&ctx.take_effects());
        }

        // 发送 MouseEnter 事件给当前悬停的 widget
        if let Some(current_id) = current_hover {
            let ctx = EventContext::new(layers, LayerType::Base);
            Self::invoke(current_id, &Event::MouseEnter, layers, &ctx);
            effects.merge(&ctx.take_effects());
        }

        self.hovered = current_hover;
        effects
    }
}

impl Default for EventManager {
    fn default() -> Self {
        Self::new()
    }
}
