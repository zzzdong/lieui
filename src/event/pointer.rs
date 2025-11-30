use std::collections::VecDeque;
use std::{collections::HashMap, marker::PhantomData};
use taffy::{Layout as TaffyLayout, NodeId, TaffyTree};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::keyboard::ModifiersState;

use crate::element::world::ElementTree;
use crate::element::{ElementId, ElementRef, IElement};
use crate::event::world::EventResult;


/// 指针状态管理器
#[derive(Debug, Clone)]
pub struct PointerState {
    /// 当前指针位置
    pub current_position: (f32, f32),
    /// 上一次指针位置
    pub last_position: (f32, f32),
    /// 当前悬停的元素ID
    pub hovered_element: Vec<ElementId>,
    /// 当前按下的元素ID
    pub pressed_element: Vec<ElementId>,
    /// 指针是否按下
    pub is_pressed: bool,
}

impl Default for PointerState {
    fn default() -> Self {
        Self {
            current_position: (0.0, 0.0),
            last_position: (0.0, 0.0),
            hovered_element: Vec::new(),
            pressed_element: Vec::new(),
            is_pressed: false,
        }
    }
}

impl PointerState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 更新指针位置
    pub fn update_position(&mut self, x: f32, y: f32) {
        self.last_position = self.current_position;
        self.current_position = (x, y);
    }
}


/// 指针事件类型，参考Windows UIElement设计
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEventType {
    /// 指针按下（鼠标点击/触摸开始）
    PointerPressed,
    /// 指针释放（鼠标释放/触摸结束）
    PointerReleased,
    /// 指针移动
    PointerMoved,
    /// 指针进入元素边界
    PointerEntered,
    /// 指针离开元素边界
    PointerExited,
    /// 指针捕获丢失
    PointerCaptureLost,
    /// 指针点击（鼠标左键单击/触摸单击）
    PointerClicked,
}

/// 指针事件数据结构
#[derive(Debug, Clone)]
pub struct PointerEvent {
    /// 事件类型
    pub event_type: PointerEventType,
    /// 指针位置（相对窗口坐标）
    pub position: (f32, f32),
    /// 鼠标按钮（仅限鼠标事件）
    pub button: Option<MouseButton>,
    /// 修饰键状态
    pub modifiers: ModifiersState,
    /// 事件目标元素ID
    pub target: Option<ElementId>,
    /// 事件是否已处理
    pub handled: bool,
}

impl PointerEvent {
    pub fn new(event_type: PointerEventType, position: (f32, f32)) -> Self {
        Self {
            event_type,
            position,
            button: None,
            modifiers: ModifiersState::empty(),
            target: None,
            handled: false,
        }
    }

    pub fn with_button(mut self, button: MouseButton) -> Self {
        self.button = Some(button);
        self
    }

    pub fn with_modifiers(mut self, modifiers: ModifiersState) -> Self {
        self.modifiers = modifiers;
        self
    }

    pub fn with_target(mut self, target: ElementId) -> Self {
        self.target = Some(target);
        self
    }
}


pub struct PointerListeners<State> {
    pub on_pointer_pressed: Option<Box<dyn FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static>>,
    pub on_pointer_released: Option<Box<dyn FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static>>,
    pub on_pointer_clicked: Option<Box<dyn FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static>>,
    pub on_pointer_moved: Option<Box<dyn FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static>>,
    pub on_pointer_entered: Option<Box<dyn FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static>>,
    pub on_pointer_exited: Option<Box<dyn FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static>>,
    pub on_pointer_capture_lost: Option<Box<dyn FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static>>,
}

impl<State> PointerListeners<State> {
    pub fn new() -> Self {
        Self {
            on_pointer_pressed: None,
            on_pointer_released: None,
            on_pointer_clicked: None,
            on_pointer_moved: None,
            on_pointer_entered: None,
            on_pointer_exited: None,
            on_pointer_capture_lost: None,
        }
    }
}