//! 事件类型定义

/// 事件类型标签（用于分类匹配）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventType {
    MouseMove,
    MouseDown,
    MouseUp,
    MouseEnter,
    MouseLeave,
    Click,
    MouseWheel,
    DragStart,
    DragMove,
    DragEnd,
    KeyDown,
    KeyUp,
    FocusIn,
    FocusOut,
    ImePreedit,
    ImeCommit,
    ImeDisabled,
}

/// 事件
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
        /// 按下时的键盘修饰键（Shift+点击扩选等场景需要）
        modifiers: Modifiers,
        /// 连击计数：1=单击 2=双击 3=三击（循环）
        click_count: u8,
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
    /// 拖拽开始：按下左键后移动超过阈值，由 EventManager 合成。
    /// 偏移始终为 0（刚超过阈值，尚未移动）。
    DragStart {
        x: f32,
        y: f32,
        /// 相对按下点的累计偏移（始终为 0）
        offset_x: f32,
        offset_y: f32,
        button: MouseButton,
        /// 按下时的键盘修饰键
        modifiers: Modifiers,
    },
    /// 拖拽进行中（每次鼠标移动合成一次）。
    DragMove {
        x: f32,
        y: f32,
        /// 相对上一个事件的增量
        dx: f32,
        dy: f32,
        /// 相对按下点的累计偏移
        offset_x: f32,
        offset_y: f32,
        button: MouseButton,
        /// 移动时的键盘修饰键
        modifiers: Modifiers,
    },
    /// 拖拽结束（鼠标释放时投递，仅当拖拽真正开始过）。
    DragEnd {
        x: f32,
        y: f32,
        /// 相对上一个事件的增量
        dx: f32,
        dy: f32,
        /// 相对按下点的累计偏移
        offset_x: f32,
        offset_y: f32,
        button: MouseButton,
        /// 释放时的键盘修饰键
        modifiers: Modifiers,
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
    ImePreedit {
        text: String,
        cursor_start: Option<usize>,
        cursor_end: Option<usize>,
    },
    ImeCommit {
        text: String,
    },
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
            Event::DragStart { .. } => EventType::DragStart,
            Event::DragMove { .. } => EventType::DragMove,
            Event::DragEnd { .. } => EventType::DragEnd,
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
    Continue,
    Stop,
}

impl EventResult {
    pub fn is_stopped(&self) -> bool {
        matches!(self, EventResult::Stop)
    }
}
