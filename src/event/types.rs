#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventType {
    MouseMove,
    MouseDown,
    MouseUp,
    MouseEnter,
    MouseLeave,
    Click,
    MouseWheel,
    KeyDown,
    KeyUp,
    FocusIn,
    FocusOut,
    ImePreedit,
    ImeCommit,
    ImeDisabled,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    MouseMove {
        x: f32,
        y: f32,
    },
    MouseDown {
        button: MouseButton,
        x: f32,
        y: f32,
    },
    MouseUp {
        button: MouseButton,
        x: f32,
        y: f32,
    },
    MouseEnter,
    MouseLeave,
    Click {
        button: MouseButton,
    },
    MouseWheel {
        delta_x: f32,
        delta_y: f32,
        x: f32,
        y: f32,
    },
    KeyDown {
        key: Key,
        modifiers: Modifiers,
    },
    KeyUp {
        key: Key,
        modifiers: Modifiers,
    },
    FocusIn,
    FocusOut,
    /// IME 预编辑状态更新
    ImePreedit {
        text: String,
        cursor_start: Option<usize>,
        cursor_end: Option<usize>,
    },
    /// IME 提交最终文本
    ImeCommit {
        text: String,
    },
    /// IME 被禁用
    ImeDisabled,
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
            Event::MouseWheel { .. } => EventType::MouseWheel,
            Event::KeyDown { .. } => EventType::KeyDown,
            Event::KeyUp { .. } => EventType::KeyUp,
            Event::FocusIn => EventType::FocusIn,
            Event::FocusOut => EventType::FocusOut,
            Event::ImePreedit { .. } => EventType::ImePreedit,
            Event::ImeCommit { .. } => EventType::ImeCommit,
            Event::ImeDisabled => EventType::ImeDisabled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Character(char),
    Enter,
    Escape,
    Backspace,
    Delete,
    Tab,
    Space,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Shift,
    Ctrl,
    Alt,
    Meta,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

/// 事件处理结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    /// 继续传播
    Continue,
    /// 停止传播
    Stop,
}

impl EventResult {
    /// 是否停止传播
    pub fn is_stopped(&self) -> bool {
        matches!(self, EventResult::Stop)
    }
}
