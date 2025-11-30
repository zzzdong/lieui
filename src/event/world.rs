use std::collections::HashMap;

use winit::event::{ElementState, MouseButton, WindowEvent};

use crate::{
    element::{ElementId, world::ElementTree},
    event::pointer::{PointerEvent, PointerEventType, PointerListeners, PointerState},
};

#[derive(Default)]
pub struct EventResult {
    pub stop_propagation: bool,
    pub prevent_default: bool,
}

/// 指针事件处理器
pub struct EventWorld<State> {
    /// 指针状态管理器
    pointer_state: PointerState,
    pointer_events: Vec<PointerEvent>,
    pointer_listeners: HashMap<ElementId, PointerListeners<State>>,
}

impl<State> EventWorld<State> {
    pub fn new() -> Self {
        Self {
            pointer_state: PointerState::new(),
            pointer_events: Vec::new(),
            pointer_listeners: HashMap::new(),
        }
    }

    pub fn dispatch_window_event(
        &mut self,
        event: &WindowEvent,
        elements: &ElementTree,
        state: &mut State,
    ) {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.handle_cursor_moved(position.x as f32, position.y as f32, elements);
            }
            WindowEvent::MouseInput {
                state: button_state,
                button,
                ..
            } => {
                self.handle_mouse_input(*button_state, *button, elements, state);
            }
            _ => {}
        }
    }

    pub fn handle_pointer_pressed(
        &mut self,
        element: ElementId,
        handler: impl FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static,
    ) {
        let mut listeners = self
            .pointer_listeners
            .entry(element)
            .or_insert(PointerListeners::new());

        listeners.on_pointer_pressed = Some(Box::new(handler));
    }

    pub fn handle_pointer_released(
        &mut self,
        element: ElementId,
        handler: impl FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static,
    ) {
        let mut listeners = self
            .pointer_listeners
            .entry(element)
            .or_insert(PointerListeners::new());

        listeners.on_pointer_released = Some(Box::new(handler));
    }

    pub fn handle_pointer_moved(
        &mut self,
        element: ElementId,
        handler: impl FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static,
    ) {
        let mut listeners = self
            .pointer_listeners
            .entry(element)
            .or_insert(PointerListeners::new());

        listeners.on_pointer_moved = Some(Box::new(handler));
    }

    pub fn handle_pointer_entered(
        &mut self,
        element: ElementId,
        handler: impl FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static,
    ) {
        let mut listeners = self
            .pointer_listeners
            .entry(element)
            .or_insert(PointerListeners::new());

        listeners.on_pointer_entered = Some(Box::new(handler));
    }

    pub fn handle_pointer_exited(
        &mut self,
        element: ElementId,
        handler: impl FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static,
    ) {
        let mut listeners = self
            .pointer_listeners
            .entry(element)
            .or_insert(PointerListeners::new());

        listeners.on_pointer_exited = Some(Box::new(handler));
    }

    pub fn handle_pointer_capture_lost(
        &mut self,
        element: ElementId,
        handler: impl FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static,
    ) {
        let mut listeners = self
            .pointer_listeners
            .entry(element)
            .or_insert(PointerListeners::new());

        listeners.on_pointer_capture_lost = Some(Box::new(handler));
    }

    pub fn handle_pointer_clicked(
        &mut self,
        element: ElementId,
        handler: impl FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static,
    ) {
        let mut listeners = self
            .pointer_listeners
            .entry(element)
            .or_insert(PointerListeners::new());

        listeners.on_pointer_clicked = Some(Box::new(handler));
    }

    /// 处理鼠标移动事件
    fn handle_cursor_moved(&mut self, x: f32, y: f32, elements: &ElementTree) {
        // 更新指针位置
        self.pointer_state.update_position(x, y);

        // 查找当前指针位置下的元素
        let path = elements.collect_element_path(x, y);

        // 检测悬停变化并生成enter/exit事件
        self.handle_hover_change(&path);
    }

    /// 处理鼠标按键事件
    fn handle_mouse_input(
        &mut self,
        button_state: ElementState,
        button: MouseButton,
        elements: &ElementTree,
        state: &mut State,
    ) {
        // 更新指针按下状态
        self.pointer_state.is_pressed = button_state == ElementState::Pressed;

        // 查找当前指针位置下的元素
        let (x, y) = self.pointer_state.current_position;
        let path = elements.collect_element_path(x, y);

        match button_state {
            ElementState::Pressed => {
                self.pointer_state.pressed_element = path.clone();

                for ele in path.iter().rev() {
                    let listener = self.pointer_listeners.get_mut(ele);
                    if let Some(listener) = listener {
                        if let Some(handler) = listener.on_pointer_pressed.as_mut() {
                            let event = PointerEvent::new(PointerEventType::PointerPressed, (x, y))
                                .with_button(button)
                                .with_target(*ele);
                            let res = handler(event, state);
                            if res.stop_propagation {
                                break;
                            }
                        }
                    }
                }
            }
            ElementState::Released => {
                // 生成PointerReleased事件
                for ele in path.iter().rev() {
                    let listeners = self.pointer_listeners.get_mut(ele);
                    if let Some(listeners) = listeners {
                        if let Some(handler) = listeners.on_pointer_released.as_mut() {
                            let event =
                                PointerEvent::new(PointerEventType::PointerReleased, (x, y))
                                    .with_button(button)
                                    .with_target(*ele);
                            let res = handler(event, state);
                            if res.stop_propagation {
                                break;
                            }
                        }
                    }
                }

                // 生成Clicked事件
                for ele in path.iter().rev() {
                    if self.pointer_state.pressed_element.contains(ele) {
                        let listener = self.pointer_listeners.get_mut(ele);
                        if let Some(listener) = listener {
                            if let Some(handler) = listener.on_pointer_clicked.as_mut() {
                                let event = PointerEvent::new(
                                    PointerEventType::PointerClicked,
                                    self.pointer_state.current_position,
                                )
                                .with_button(button)
                                .with_target(*ele);
                                handler(event, state);
                            }
                        }
                    }
                }

                // 清除按下元素记录
                self.pointer_state.pressed_element.clear();
            }
        }
    }

    /// 处理悬停变化，生成enter/exit事件
    fn handle_hover_change(&mut self, elements: &[ElementId]) {
        for ele in self.pointer_state.hovered_element.iter() {
            if !elements.contains(ele) {
                // 离开元素
                let event = PointerEvent::new(
                    PointerEventType::PointerExited,
                    self.pointer_state.current_position,
                )
                .with_target(*ele);
                self.pointer_events.push(event);
            }
        }

        for ele in elements.iter() {
            if !self.pointer_state.hovered_element.contains(ele) {
                // 进入元素
                let event = PointerEvent::new(
                    PointerEventType::PointerEntered,
                    self.pointer_state.current_position,
                )
                .with_target(*ele);
                self.pointer_events.push(event);
            }
        }

        self.pointer_state.hovered_element = elements.to_vec();
    }

    /// 获取当前指针状态
    pub fn pointer_state(&self) -> &PointerState {
        &self.pointer_state
    }
}

impl<State> Default for EventWorld<State> {
    fn default() -> Self {
        Self::new()
    }
}
