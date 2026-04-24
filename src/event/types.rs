#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventType {
    MouseMove,
    MouseDown,
    MouseUp,
    MouseEnter,
    MouseLeave,
    Click,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    MouseMove { x: f32, y: f32 },
    MouseDown { button: MouseButton, x: f32, y: f32 },
    MouseUp { button: MouseButton, x: f32, y: f32 },
    MouseEnter,
    MouseLeave,
    Click { button: MouseButton },
}

impl Event {
    pub fn to_type(&self) -> EventType {
        match self {
            Event::MouseMove { .. } => EventType::MouseMove,
            Event::MouseDown { .. } => EventType::MouseDown,
            Event::MouseUp { .. } => EventType::MouseUp,
            Event::MouseEnter => EventType::MouseEnter,
            Event::MouseLeave => EventType::MouseLeave,
            Event::Click { .. } => EventType::Click,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}
