// src/event/context.rs

use crate::core::{ViewContext, WidgetId};
use crate::event::{Event, EventCallbackManager, EventResult, EventType};
use crate::geometry::Point;
use crate::layout::LayoutNode;

/// 事件分发器 - 负责命中测试和事件分发
pub struct EventDispatcher {
    hovered: Option<WidgetId>,
    pressed: Option<WidgetId>,
    focused: Option<WidgetId>,
}

impl EventDispatcher {
    pub fn new() -> Self {
        Self {
            hovered: None,
            pressed: None,
            focused: None,
        }
    }

    /// 命中测试 - 找到指定点下的最深层 widget
    pub fn hit_test(&self, point: Point, layout_root: &LayoutNode) -> Option<WidgetId> {
        Self::hit_test_recursive(point, layout_root)
    }

    fn hit_test_recursive(point: Point, layout_node: &LayoutNode) -> Option<WidgetId> {
        // 先检查子节点（从上层开始）
        for child in layout_node.children.iter().rev() {
            if let Some(id) = Self::hit_test_recursive(point, child) {
                return Some(id);
            }
        }

        // 再检查当前节点
        if let Some(computed) = &layout_node.computed {
            if computed.content_box.contains(point) {
                return Some(layout_node.id);
            }
        }

        None
    }

    /// 获取当前悬停的 widget
    pub fn hovered(&self) -> Option<WidgetId> {
        self.hovered
    }

    /// 获取当前按下的 widget
    pub fn pressed(&self) -> Option<WidgetId> {
        self.pressed
    }

    /// 设置悬停状态
    pub fn set_hovered(&mut self, id: Option<WidgetId>) {
        self.hovered = id;
    }

    /// 设置按下状态
    pub fn set_pressed(&mut self, id: Option<WidgetId>) {
        self.pressed = id;
    }

    /// 获取当前焦点的 widget
    pub fn focused(&self) -> Option<WidgetId> {
        self.focused
    }

    /// 设置焦点状态
    pub fn set_focused(&mut self, id: Option<WidgetId>) {
        self.focused = id;
    }
}

impl Default for EventDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// 事件上下文——封装完整的事件处理流程
pub struct EventContext;

impl EventContext {
    /// 分发事件（入口方法）
    ///
    /// 1. 命中测试找到目标
    /// 2. 构建传播路径
    /// 3. 按阶段传播：捕获 → 目标 → 冒泡
    pub fn dispatch(ctx: &mut ViewContext, event: &Event, point: Point, layout_root: &LayoutNode) {
        let target_id = match Self::hit_test(point, layout_root) {
            Some(id) => id,
            None => return,
        };

        let path = Self::build_path(target_id, layout_root);
        let event_type = event.to_type();

        // 阶段 1: 捕获（从根到目标之前）
        for &id in path.iter().take(path.len().saturating_sub(1)) {
            if Self::invoke_widget(ctx, id, event) == EventResult::Stop {
                return;
            }
            if Self::invoke_user_callbacks(ctx, id, event_type, event) == EventResult::Stop {
                return;
            }
        }

        // 阶段 2: 目标
        if Self::invoke_widget(ctx, target_id, event) == EventResult::Stop {
            return;
        }
        if Self::invoke_user_callbacks(ctx, target_id, event_type, event) == EventResult::Stop {
            return;
        }

        // 阶段 3: 冒泡（从目标父级到根）
        for &id in path.iter().rev().skip(1) {
            if Self::invoke_widget(ctx, id, event) == EventResult::Stop {
                return;
            }
            if Self::invoke_user_callbacks(ctx, id, event_type, event) == EventResult::Stop {
                return;
            }
        }
    }

    /// 命中测试
    fn hit_test(point: Point, layout_root: &LayoutNode) -> Option<WidgetId> {
        layout_root.hit_test(point)
    }

    /// 构建从根到目标的路径
    fn build_path(target_id: WidgetId, layout_root: &LayoutNode) -> Vec<WidgetId> {
        let mut path = Vec::new();
        if Self::find_path(layout_root, target_id, &mut path) {
            path.reverse();
        }
        path
    }

    fn find_path(node: &LayoutNode, target: WidgetId, path: &mut Vec<WidgetId>) -> bool {
        path.push(node.id);
        if node.id == target {
            return true;
        }
        for child in &node.children {
            if Self::find_path(child, target, path) {
                return true;
            }
        }
        path.pop();
        false
    }

    /// 调用 Widget 内部事件处理
    fn invoke_widget(ctx: &mut ViewContext, id: WidgetId, event: &Event) -> EventResult {
        // 调用 widget 的 handle_event 方法
        // 注意：这里我们使用 widget_tree 的方法来避免借用冲突
        let result = ctx.invoke_widget_event(id, event);
        result
    }

    /// 调用用户注册的回调
    fn invoke_user_callbacks(
        ctx: &mut ViewContext,
        id: WidgetId,
        event_type: EventType,
        event: &Event,
    ) -> EventResult {
        let mut callbacks = ctx.take_callbacks(id, event_type);
        let mut result = EventResult::Continue;

        for cb in &mut callbacks {
            match cb(id, event, ctx) {
                EventResult::Stop => {
                    result = EventResult::Stop;
                    break;
                }
                EventResult::PreventDefault => {
                    result = EventResult::PreventDefault;
                    // 继续执行其他回调，但不继续传播
                }
                EventResult::Continue => {}
            }
        }

        result
    }
}
