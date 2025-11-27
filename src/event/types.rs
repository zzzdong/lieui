use crate::element::ElementId;
use winit::event::{ElementState, MouseButton};
use winit::keyboard::ModifiersState;

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

    /// 标记事件为已处理
    pub fn mark_handled(&mut self) {
        self.handled = true;
    }
}

/// 指针状态管理器
#[derive(Debug, Clone)]
pub struct PointerState {
    /// 当前指针位置
    pub current_position: (f32, f32),
    /// 上一次指针位置
    pub last_position: (f32, f32),
    /// 当前悬停的元素ID
    pub hovered_element: Option<ElementId>,
    /// 当前按下的元素ID
    pub pressed_element: Option<ElementId>,
    /// 指针是否按下
    pub is_pressed: bool,
    /// 上一次悬停的元素ID（用于检测enter/exit）
    pub last_hovered_element: Option<ElementId>,
}

impl Default for PointerState {
    fn default() -> Self {
        Self {
            current_position: (0.0, 0.0),
            last_position: (0.0, 0.0),
            hovered_element: None,
            pressed_element: None,
            is_pressed: false,
            last_hovered_element: None,
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

    /// 设置悬停元素
    pub fn set_hovered_element(&mut self, element_id: Option<ElementId>) {
        self.last_hovered_element = self.hovered_element;
        self.hovered_element = element_id;
    }

    /// 设置按下状态
    pub fn set_pressed(&mut self, is_pressed: bool, element_id: Option<ElementId>) {
        self.is_pressed = is_pressed;
        self.pressed_element = element_id;
    }

    /// 检测是否发生了悬停变化
    pub fn hover_changed(&self) -> bool {
        self.hovered_element != self.last_hovered_element
    }

    /// 获取悬停进入的元素（如果有）
    pub fn entered_element(&self) -> Option<ElementId> {
        if self.hovered_element != self.last_hovered_element {
            self.hovered_element
        } else {
            None
        }
    }

    /// 获取悬退离开的元素（如果有）
    pub fn exited_element(&self) -> Option<ElementId> {
        if self.hovered_element != self.last_hovered_element {
            self.last_hovered_element
        } else {
            None
        }
    }
}
