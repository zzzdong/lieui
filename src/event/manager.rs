// src/event/manager.rs
//! 事件管理器
//!
//! 负责事件分发、焦点管理和事件传播控制。
//! 支持捕获阶段、目标阶段和冒泡阶段的事件处理。

use crate::core::WidgetId;
use crate::event::{Event, EventContext, EventEffects, Key, Modifiers, MouseButton};
use crate::geometry::Point;
use crate::layout::LayoutNode;
use crate::widget::WidgetTree;

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

    /// 处理鼠标按下事件
    pub fn handle_mouse_down(
        &mut self,
        point: Point,
        button: MouseButton,
        layout_root: &LayoutNode,
        tree: &WidgetTree,
    ) -> EventEffects {
        self.mouse_down = true;
        let mut effects = EventEffects::default();

        // 命中测试找到目标
        if let Some(target) = layout_root.hit_test(point) {
            // 处理焦点变化
            if self.focused != Some(target) {
                let focus_effects = self.handle_focus_change(Some(target), tree);
                effects.merge(&focus_effects);
            }

            // 检查是否能捕获鼠标
            if let Some(widget) = tree.get_widget(target)
                && widget.can_focus() {
                    self.mouse_capture = Some(target);
                }

            // 创建 EventContext 并分发事件
            let ctx = EventContext::new(tree);
            let event = Event::MouseDown {
                x: point.x,
                y: point.y,
                button,
            };
            Self::dispatch(&event, point, layout_root, tree, &ctx);
            effects.merge(&ctx.take_effects());
        } else {
            // 点击空白处，清除焦点
            let focus_effects = self.handle_focus_change(None, tree);
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
        tree: &WidgetTree,
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
            let ctx = EventContext::new(tree);
            Self::invoke(capture, &event, tree, &ctx);
            effects.merge(&ctx.take_effects());

            // 检查是否是有效的点击（释放位置仍在捕获的 widget 内）
            if let Some(current_target) = layout_root.hit_test(point)
                && current_target == capture {
                    // 发送 Click 事件
                    let click_event = Event::Click { button };
                    let ctx = EventContext::new(tree);
                    Self::invoke(capture, &click_event, tree, &ctx);
                    effects.merge(&ctx.take_effects());
                }
        } else {
            let ctx = EventContext::new(tree);
            Self::dispatch(&event, point, layout_root, tree, &ctx);
            effects.merge(&ctx.take_effects());

            // 检查是否是有效的点击
            if let Some(target) = layout_root.hit_test(point) {
                let click_event = Event::Click { button };
                let ctx = EventContext::new(tree);
                Self::invoke(target, &click_event, tree, &ctx);
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
        tree: &WidgetTree,
    ) -> EventEffects {
        let event = Event::MouseMove {
            x: point.x,
            y: point.y,
        };
        let mut effects = EventEffects::default();

        // 如果有鼠标捕获，直接分发给捕获的 widget
        if let Some(capture) = self.mouse_capture {
            let ctx = EventContext::new(tree);
            Self::invoke(capture, &event, tree, &ctx);
            effects.merge(&ctx.take_effects());
        } else {
            let ctx = EventContext::new(tree);
            Self::dispatch(&event, point, layout_root, tree, &ctx);
            effects.merge(&ctx.take_effects());
        }

        // 处理悬停状态变化
        let hover_effects = self.handle_hover_change(point, layout_root, tree);
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
        tree: &WidgetTree,
    ) -> EventEffects {
        let event = Event::MouseWheel {
            delta_x,
            delta_y,
            x: point.x,
            y: point.y,
        };
        let ctx = EventContext::new(tree);
        Self::dispatch(&event, point, layout_root, tree, &ctx);
        ctx.take_effects()
    }

    /// 设置焦点到指定 widget
    pub fn handle_focus_change(&mut self, new_focus: Option<WidgetId>, tree: &WidgetTree) -> EventEffects {
        let old_focus = self.focused;
        let mut effects = EventEffects::default();

        if old_focus == new_focus {
            return effects;
        }

        // 焦点离开旧元素
        if let Some(old_id) = old_focus {
            let ctx = EventContext::new(tree);
            Self::invoke(old_id, &Event::FocusOut, tree, &ctx);
            effects.merge(&ctx.take_effects());
        }

        // 焦点进入新元素
        if let Some(new_id) = new_focus {
            let ctx = EventContext::new(tree);
            Self::invoke(new_id, &Event::FocusIn, tree, &ctx);
            effects.merge(&ctx.take_effects());
        }

        self.focused = new_focus;
        effects
    }

    /// 处理窗口失焦（清除焦点状态）
    pub fn handle_window_unfocus(&mut self, tree: &WidgetTree) -> EventEffects {
        let mut effects = EventEffects::default();
        if let Some(focused) = self.focused.take() {
            let ctx = EventContext::new(tree);
            Self::invoke(focused, &Event::FocusOut, tree, &ctx);
            effects.merge(&ctx.take_effects());
        }
        effects
    }

    /// 处理键盘按下
    pub fn handle_key_down(&mut self, key: Key, modifiers: Modifiers, tree: &WidgetTree) -> EventEffects {
        let mut effects = EventEffects::default();
        if let Some(target) = self.focused {
            let path = tree.path_to(target);
            let ctx = EventContext::new(tree);
            Self::dispatch_along_path(&Event::KeyDown { key, modifiers }, &path, target, tree, &ctx);
            effects.merge(&ctx.take_effects());
        }
        effects
    }

    /// 处理键盘释放
    pub fn handle_key_up(&mut self, key: Key, modifiers: Modifiers, tree: &WidgetTree) -> EventEffects {
        let mut effects = EventEffects::default();
        if let Some(target) = self.focused {
            let path = tree.path_to(target);
            let ctx = EventContext::new(tree);
            Self::dispatch_along_path(&Event::KeyUp { key, modifiers }, &path, target, tree, &ctx);
            effects.merge(&ctx.take_effects());
        }
        effects
    }

    /// 处理 IME 预编辑事件
    pub fn handle_ime_preedit(
        &mut self,
        text: String,
        cursor_start: Option<usize>,
        cursor_end: Option<usize>,
        tree: &WidgetTree,
    ) -> EventEffects {
        let mut effects = EventEffects::default();
        if let Some(target) = self.focused {
            let path = tree.path_to(target);
            let ctx = EventContext::new(tree);
            Self::dispatch_along_path(
                &Event::ImePreedit {
                    text,
                    cursor_start,
                    cursor_end,
                },
                &path,
                target,
                tree,
                &ctx,
            );
            effects.merge(&ctx.take_effects());
        }
        effects
    }

    /// 处理 IME 提交事件
    pub fn handle_ime_commit(&mut self, text: String, tree: &WidgetTree) -> EventEffects {
        let mut effects = EventEffects::default();
        if let Some(target) = self.focused {
            let path = tree.path_to(target);
            let ctx = EventContext::new(tree);
            Self::dispatch_along_path(&Event::ImeCommit { text }, &path, target, tree, &ctx);
            effects.merge(&ctx.take_effects());
        }
        effects
    }

    /// 处理 IME 禁用事件
    pub fn handle_ime_disabled(&mut self, tree: &WidgetTree) -> EventEffects {
        let mut effects = EventEffects::default();
        if let Some(target) = self.focused {
            let path = tree.path_to(target);
            let ctx = EventContext::new(tree);
            Self::dispatch_along_path(&Event::ImeDisabled, &path, target, tree, &ctx);
            effects.merge(&ctx.take_effects());
        }
        effects
    }

    /// 分发事件（目标阶段 + 冒泡）
    fn dispatch(
        event: &Event,
        point: Point,
        layout_root: &LayoutNode,
        tree: &WidgetTree,
        ctx: &EventContext<'_>,
    ) {
        let Some(target) = layout_root.hit_test(point) else {
            return;
        };

        let path = tree.path_to(target);
        Self::dispatch_along_path(event, &path, target, tree, ctx);
    }

    /// 沿路径分发事件
    ///
    /// 先目标阶段，然后冒泡到祖先
    fn dispatch_along_path(
        event: &Event,
        path: &[WidgetId],
        target: WidgetId,
        tree: &WidgetTree,
        ctx: &EventContext<'_>,
    ) {
        // 目标阶段
        Self::invoke(target, event, tree, ctx);
        if ctx.is_stopped() {
            return;
        }

        // 冒泡阶段（从目标父级到根）
        let target_idx = path.iter().position(|&id| id == target);
        if let Some(idx) = target_idx {
            for &id in path.iter().take(idx).rev() {
                Self::invoke(id, event, tree, ctx);
                if ctx.is_stopped() {
                    return;
                }
            }
        }
    }

    /// 调用单个 widget 的事件处理
    fn invoke(id: WidgetId, event: &Event, tree: &WidgetTree, ctx: &EventContext<'_>) {
        if let Some(mut widget) = tree.get_widget(id) {
            let _ = widget.handle_event(event, ctx);
        }
    }

    /// 处理悬停状态变化
    fn handle_hover_change(
        &mut self,
        point: Point,
        layout_root: &LayoutNode,
        tree: &WidgetTree,
    ) -> EventEffects {
        let current_hover = layout_root.hit_test(point);
        let prev_hover = self.hovered;
        let mut effects = EventEffects::default();

        // 如果悬停状态没有变化，直接返回
        if current_hover == prev_hover {
            return effects;
        }

        // 发送 MouseLeave 事件给之前悬停的 widget
        if let Some(prev_id) = prev_hover {
            let ctx = EventContext::new(tree);
            Self::invoke(prev_id, &Event::MouseLeave, tree, &ctx);
            effects.merge(&ctx.take_effects());
        }

        // 发送 MouseEnter 事件给当前悬停的 widget
        if let Some(current_id) = current_hover {
            let ctx = EventContext::new(tree);
            Self::invoke(current_id, &Event::MouseEnter, tree, &ctx);
            effects.merge(&ctx.take_effects());
        }

        // 更新悬停状态
        self.hovered = current_hover;
        effects
    }
}

impl Default for EventManager {
    fn default() -> Self {
        Self::new()
    }
}
