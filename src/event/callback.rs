//! 事件回调系统
//!
//! 提供 Widget 内部的事件回调注册机制

use crate::event::{Event, EventContext, EventType};

/// 统一的事件回调类型（Widget 内部使用）
///
/// 回调接收事件和上下文，可通过 ctx 访问 Widget 树、请求副作用、控制传播
pub type EventCallback = Box<dyn FnMut(&Event, &EventContext)>;

/// 用户事件回调类型（简化签名，只接收上下文）
///
/// 用于 Widget 提供给用户的回调 API，如 on_click、on_mouse_enter 等
pub type UserCallback = Box<dyn FnMut(&EventContext)>;

/// 回调存储类型（用于 Widget 内部）
pub type CallbackMap = std::collections::HashMap<EventType, Vec<EventCallback>>;

/// 用户回调存储类型
pub type UserCallbackMap = std::collections::HashMap<EventType, Vec<UserCallback>>;
