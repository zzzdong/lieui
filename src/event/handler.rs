use crate::core::WidgetId;
use crate::event::{Event, EventDispatcher, Key, Modifiers, MouseButton};
use crate::geometry::Point;
use crate::layout::LayoutNode;

/// 事件处理器 - 管理事件状态和分发
pub struct EventHandler {
    dispatcher: EventDispatcher,
}

impl EventHandler {
    pub fn new() -> Self {
        Self {
            dispatcher: EventDispatcher::new(),
        }
    }

    /// 获取当前悬停的 widget
    pub fn hovered(&self) -> Option<WidgetId> {
        self.dispatcher.hovered()
    }

    /// 获取当前按下的 widget
    pub fn pressed(&self) -> Option<WidgetId> {
        self.dispatcher.pressed()
    }

    /// 获取当前焦点的 widget
    pub fn focused(&self) -> Option<WidgetId> {
        self.dispatcher.focused()
    }

    /// 处理鼠标移动，返回需要触发的事件列表 (widget_id, event)
    pub fn handle_mouse_move(
        &mut self,
        point: Point,
        layout_root: &LayoutNode,
    ) -> Vec<(WidgetId, Event)> {
        let mut events = Vec::new();
        let new_hovered = self.dispatcher.hit_test(point, layout_root);
        let old_hovered = self.dispatcher.hovered();

        // 鼠标离开旧元素
        if let Some(old_id) = old_hovered {
            if new_hovered != Some(old_id) {
                events.push((old_id, Event::MouseLeave));
            }
        }

        // 鼠标进入新元素
        if let Some(new_id) = new_hovered {
            if old_hovered != Some(new_id) {
                events.push((new_id, Event::MouseEnter));
            }
            // 鼠标移动事件
            events.push((
                new_id,
                Event::MouseMove {
                    x: point.x,
                    y: point.y,
                },
            ));
        }

        self.dispatcher.set_hovered(new_hovered);
        events
    }

    /// 处理鼠标按下，返回需要触发的事件列表
    pub fn handle_mouse_down(
        &mut self,
        point: Point,
        button: MouseButton,
        layout_root: &LayoutNode,
    ) -> Vec<(WidgetId, Event)> {
        let mut events = Vec::new();

        if let Some(target) = self.dispatcher.hit_test(point, layout_root) {
            self.dispatcher.set_pressed(Some(target));
            events.push((
                target,
                Event::MouseDown {
                    button,
                    x: point.x,
                    y: point.y,
                },
            ));
        }

        events
    }

    /// 处理鼠标释放，返回需要触发的事件列表
    pub fn handle_mouse_up(
        &mut self,
        point: Point,
        button: MouseButton,
        layout_root: &LayoutNode,
    ) -> Vec<(WidgetId, Event)> {
        let mut events = Vec::new();
        let pressed = self.dispatcher.pressed();
        let current = self.dispatcher.hit_test(point, layout_root);

        // 发送 MouseUp 事件
        if let Some(target) = current {
            events.push((
                target,
                Event::MouseUp {
                    button,
                    x: point.x,
                    y: point.y,
                },
            ));

            // 如果按下和释放是同一个元素，触发 Click
            if pressed == Some(target) {
                events.push((target, Event::Click { button }));
            }
        }

        self.dispatcher.set_pressed(None);
        events
    }

    /// 处理鼠标滚轮，返回需要触发的事件列表
    pub fn handle_mouse_wheel(
        &mut self,
        delta_x: f32,
        delta_y: f32,
        point: Point,
        layout_root: &LayoutNode,
    ) -> Vec<(WidgetId, Event)> {
        let mut events = Vec::new();

        if let Some(target) = self.dispatcher.hit_test(point, layout_root) {
            events.push((
                target,
                Event::MouseWheel {
                    delta_x,
                    delta_y,
                    x: point.x,
                    y: point.y,
                },
            ));
        }

        events
    }

    /// 处理键盘按下，返回需要触发的事件列表
    pub fn handle_key_down(&mut self, key: Key, modifiers: Modifiers) -> Vec<(WidgetId, Event)> {
        let mut events = Vec::new();

        if let Some(target) = self.dispatcher.focused() {
            events.push((target, Event::KeyDown { key, modifiers }));
        }

        events
    }

    /// 处理键盘释放，返回需要触发的事件列表
    pub fn handle_key_up(&mut self, key: Key, modifiers: Modifiers) -> Vec<(WidgetId, Event)> {
        let mut events = Vec::new();

        if let Some(target) = self.dispatcher.focused() {
            events.push((target, Event::KeyUp { key, modifiers }));
        }

        events
    }

    /// 设置焦点到指定 widget，返回焦点变化事件列表
    pub fn handle_focus_change(&mut self, new_focus: Option<WidgetId>) -> Vec<(WidgetId, Event)> {
        let mut events = Vec::new();
        let old_focus = self.dispatcher.focused();

        if old_focus == new_focus {
            return events;
        }

        if let Some(old_id) = old_focus {
            events.push((old_id, Event::FocusOut));
        }

        if let Some(new_id) = new_focus {
            events.push((new_id, Event::FocusIn));
        }

        self.dispatcher.set_focused(new_focus);
        events
    }
}
