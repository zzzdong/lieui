//! 事件管理器
//!
//! 统一管理事件状态（hover/press/focus）和事件分发

use crate::core::WidgetId;
use crate::event::{Event, EventResult, Key, Modifiers, MouseButton, Propagation};
use crate::geometry::Point;
use crate::layout::LayoutNode;
use crate::widget::WidgetTree;

/// 事件管理器
///
/// 统一管理悬停、按下、焦点等UI状态，并负责事件分发
pub struct EventManager {
    hovered: Option<WidgetId>,
    pressed: Option<WidgetId>,
    focused: Option<WidgetId>,
}

impl EventManager {
    /// 创建新的事件管理器
    pub fn new() -> Self {
        Self {
            hovered: None,
            pressed: None,
            focused: None,
        }
    }

    /// 获取当前悬停的 widget
    pub fn hovered(&self) -> Option<WidgetId> {
        self.hovered
    }

    /// 获取当前按下的 widget
    pub fn pressed(&self) -> Option<WidgetId> {
        self.pressed
    }

    /// 获取当前焦点的 widget
    pub fn focused(&self) -> Option<WidgetId> {
        self.focused
    }

    /// 处理鼠标移动
    ///
    /// 生成 MouseLeave/MouseEnter/MouseMove 事件
    pub fn handle_mouse_move(&mut self, point: Point, layout_root: &LayoutNode, tree: &WidgetTree) {
        let new_target = layout_root.hit_test(point);
        let old_target = self.hovered;

        // 鼠标离开旧元素
        if old_target != new_target {
            if let Some(old) = old_target {
                self.dispatch_to_target(old, &Event::MouseLeave, tree);
            }
            if let Some(new) = new_target {
                self.dispatch_to_target(new, &Event::MouseEnter, tree);
            }
        }

        // 鼠标移动事件
        if let Some(_new) = new_target {
            self.dispatch(
                &Event::MouseMove {
                    x: point.x,
                    y: point.y,
                },
                point,
                layout_root,
                tree,
            );
        }

        self.hovered = new_target;
    }

    /// 处理鼠标按下
    pub fn handle_mouse_down(
        &mut self,
        button: MouseButton,
        point: Point,
        layout_root: &LayoutNode,
        tree: &WidgetTree,
    ) {
        if let Some(target) = layout_root.hit_test(point) {
            self.pressed = Some(target);
            self.dispatch(
                &Event::MouseDown {
                    button,
                    x: point.x,
                    y: point.y,
                },
                point,
                layout_root,
                tree,
            );
        }
    }

    /// 处理鼠标释放
    pub fn handle_mouse_up(
        &mut self,
        button: MouseButton,
        point: Point,
        layout_root: &LayoutNode,
        tree: &WidgetTree,
    ) {
        let pressed = self.pressed;
        let current = layout_root.hit_test(point);

        // 发送 MouseUp 事件
        if let Some(target) = current {
            self.dispatch(
                &Event::MouseUp {
                    button,
                    x: point.x,
                    y: point.y,
                },
                point,
                layout_root,
                tree,
            );

            // 如果按下和释放是同一个元素，触发 Click
            if pressed == Some(target) {
                self.dispatch(&Event::Click { button }, point, layout_root, tree);
            }
        }

        self.pressed = None;
    }

    /// 处理鼠标滚轮
    pub fn handle_mouse_wheel(
        &mut self,
        delta_x: f32,
        delta_y: f32,
        point: Point,
        layout_root: &LayoutNode,
        tree: &WidgetTree,
    ) {
        if layout_root.hit_test(point).is_some() {
            self.dispatch(
                &Event::MouseWheel {
                    delta_x,
                    delta_y,
                    x: point.x,
                    y: point.y,
                },
                point,
                layout_root,
                tree,
            );
        }
    }

    /// 设置焦点到指定 widget
    pub fn handle_focus_change(&mut self, new_focus: Option<WidgetId>, tree: &WidgetTree) {
        let old_focus = self.focused;

        if old_focus == new_focus {
            return;
        }

        // 焦点离开旧元素
        if let Some(old_id) = old_focus {
            self.dispatch_to_target(old_id, &Event::FocusOut, tree);
        }

        // 焦点进入新元素
        if let Some(new_id) = new_focus {
            self.dispatch_to_target(new_id, &Event::FocusIn, tree);
        }

        self.focused = new_focus;
    }

    /// 处理窗口失焦（清除焦点状态）
    pub fn handle_window_unfocus(&mut self, tree: &WidgetTree) {
        if let Some(focused) = self.focused.take() {
            self.dispatch_to_target(focused, &Event::FocusOut, tree);
        }
    }

    /// 处理键盘按下
    pub fn handle_key_down(&mut self, key: Key, modifiers: Modifiers, tree: &WidgetTree) {
        if let Some(target) = self.focused {
            let path = tree.path_to(target);
            self.dispatch_along_path(&Event::KeyDown { key, modifiers }, &path, target, tree);
        }
    }

    /// 处理键盘释放
    pub fn handle_key_up(&mut self, key: Key, modifiers: Modifiers, tree: &WidgetTree) {
        if let Some(target) = self.focused {
            let path = tree.path_to(target);
            self.dispatch_along_path(&Event::KeyUp { key, modifiers }, &path, target, tree);
        }
    }

    /// 分发事件（目标阶段 + 冒泡）
    fn dispatch(
        &mut self,
        event: &Event,
        point: Point,
        layout_root: &LayoutNode,
        tree: &WidgetTree,
    ) {
        let Some(target) = layout_root.hit_test(point) else {
            return;
        };

        let path = tree.path_to(target);
        self.dispatch_along_path(event, &path, target, tree);
    }

    /// 沿路径分发事件
    ///
    /// 先目标阶段，然后冒泡到祖先
    fn dispatch_along_path(
        &mut self,
        event: &Event,
        path: &[WidgetId],
        target: WidgetId,
        tree: &WidgetTree,
    ) {
        // 目标阶段
        if self.invoke_widget(target, event, tree) == EventResult::Stop {
            return;
        }

        // 冒泡阶段（从目标父级到根）
        let target_idx = path.iter().position(|&id| id == target);
        if let Some(idx) = target_idx {
            for &id in path.iter().take(idx).rev() {
                if self.invoke_widget(id, event, tree) == EventResult::Stop {
                    return;
                }
            }
        }
    }

    /// 直接分发事件到指定目标（不冒泡）
    fn dispatch_to_target(&mut self, target: WidgetId, event: &Event, tree: &WidgetTree) {
        self.invoke_widget(target, event, tree);
    }

    /// 调用单个 widget 的事件处理
    fn invoke_widget(&mut self, id: WidgetId, event: &Event, tree: &WidgetTree) -> EventResult {
        if let Some(mut widget) = tree.get_widget(id) {
            let mut propagation = Propagation::new();
            return widget.handle_event(event, &mut propagation);
        }
        EventResult::Continue
    }
}

impl Default for EventManager {
    fn default() -> Self {
        Self::new()
    }
}
