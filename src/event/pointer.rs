use std::{collections::HashMap, marker::PhantomData};
use taffy::{Layout as TaffyLayout, NodeId, TaffyTree};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::keyboard::ModifiersState;

use super::types::{PointerEvent, PointerEventType, PointerState};
use crate::element::world::ElementTree;
use crate::element::{ElementId, ElementRef, IElement};

/// 指针事件处理器
pub struct PointerEventHandler<State> {
    /// 指针状态管理器
    pointer_state: PointerState,
    phantom: PhantomData<State>,
}

impl<State> PointerEventHandler<State> {
    pub fn new() -> Self {
        Self {
            pointer_state: PointerState::new(),
            phantom: PhantomData,
        }
    }

    /// 处理窗口事件，转换为指针事件并分发
    pub fn handle_window_event(
        &mut self,
        event: &WindowEvent,
        elements: &mut ElementTree<State>,
        state: &mut State,
    ) {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.handle_cursor_moved(
                    position.x as f32,
                    position.y as f32,
                    elements,
                    state,
                );
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

    /// 处理鼠标移动事件
    fn handle_cursor_moved(
        &mut self,
        x: f32,
        y: f32,
        elements: &mut ElementTree<State>,
        state: &mut State,
    ) {
        // 更新指针位置
        self.pointer_state.update_position(x, y);

        // 查找当前指针位置下的元素
        let target_element = elements.find_element_at_point(x, y);

        // 检测悬停变化并生成enter/exit事件
        self.handle_hover_change(target_element, elements, state);

        // 生成PointerMoved事件
        if let Some(target) = target_element {
            let event =
                PointerEvent::new(PointerEventType::PointerMoved, (x, y)).with_target(target);

            self.dispatch_event(&event, elements, state);
        }
    }

    /// 处理鼠标按键事件
    fn handle_mouse_input(
        &mut self,
        button_state: ElementState,
        button: MouseButton,
        elements: &mut ElementTree<State>,
        state: &mut State,
    ) {
        let (x, y) = self.pointer_state.current_position;
        let target_element = elements.find_element_at_point(x, y);

        match button_state {
            ElementState::Pressed => {
                // 生成PointerPressed事件
                if let Some(target) = target_element {
                    let event = PointerEvent::new(PointerEventType::PointerPressed, (x, y))
                        .with_button(button)
                        .with_target(target);

                    self.dispatch_event(&event, elements, state);

                    // 更新按下状态
                    self.pointer_state.set_pressed(true, Some(target));
                }
            }
            ElementState::Released => {
                // 生成PointerReleased事件
                if let Some(target) = target_element {
                    let event = PointerEvent::new(PointerEventType::PointerReleased, (x, y))
                        .with_button(button)
                        .with_target(target);

                    self.dispatch_event(&event, elements, state);
                }

                // 如果之前有按下的元素，也向其发送release事件
                if let Some(pressed_element) = self.pointer_state.pressed_element {
                    if pressed_element != target_element.unwrap_or(pressed_element) {
                        let event = PointerEvent::new(PointerEventType::PointerReleased, (x, y))
                            .with_button(button)
                            .with_target(pressed_element);

                        self.dispatch_event(&event, elements, state);
                    }
                }

                // 更新按下状态
                self.pointer_state.set_pressed(false, None);
            }
        }
    }

    /// 处理悬停变化，生成enter/exit事件
    fn handle_hover_change(
        &mut self,
        new_hovered: Option<ElementId>,
        elements: &mut ElementTree<State>,
        state: &mut State,
    ) {
        let (x, y) = self.pointer_state.current_position;

        // 更新悬停元素
        self.pointer_state.set_hovered_element(new_hovered);

        // 如果有悬停变化，生成enter/exit事件
        if self.pointer_state.hover_changed() {
            // 处理悬停离开
            if let Some(exited_element) = self.pointer_state.exited_element() {
                let event = PointerEvent::new(PointerEventType::PointerExited, (x, y))
                    .with_target(exited_element);

                self.dispatch_event(&event, elements, state);
            }

            // 处理悬停进入
            if let Some(entered_element) = self.pointer_state.entered_element() {
                let event = PointerEvent::new(PointerEventType::PointerEntered, (x, y))
                    .with_target(entered_element);

                self.dispatch_event(&event, elements, state);
            }
        }
    }

    /// 分发事件到目标元素
    fn dispatch_event(
        &self,
        event: &PointerEvent,
        elements: &mut ElementTree<State>,
        state: &mut State,
    ) {
        if let Some(target_id) = event.target {
            let element_ref = elements.get_mut(target_id);
             {
                let element = &mut element_ref.get_mut().content;

                match event.event_type {
                    PointerEventType::PointerPressed => {
                        element.on_pointer_pressed(event, state);
                    }
                    PointerEventType::PointerReleased => {
                        element.on_pointer_released(event, state);
                    }
                    PointerEventType::PointerMoved => {
                        element.on_pointer_moved(event, state);
                    }
                    PointerEventType::PointerEntered => {
                        element.on_pointer_entered(event, state);
                    }
                    PointerEventType::PointerExited => {
                        element.on_pointer_exited(event, state);
                    }
                    _ => {}
                }
            }
        }
    }

    /// 获取当前指针状态
    pub fn pointer_state(&self) -> &PointerState {
        &self.pointer_state
    }
}

impl<State> Default for PointerEventHandler<State> {
    fn default() -> Self {
        Self::new()
    }
}
