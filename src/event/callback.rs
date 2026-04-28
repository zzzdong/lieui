//! 事件回调系统
//!
//! 提供 Widget 内部的事件回调注册机制

use crate::event::{Event, EventType, Propagation};

/// 统一的事件回调类型
///
/// 回调接收事件引用和传播控制器，可通过 propagation.stop() 停止事件传播
pub type EventCallback = Box<dyn FnMut(&Event, &mut Propagation)>;

/// 回调存储类型（用于 Widget 内部）
pub type CallbackMap = std::collections::HashMap<EventType, Vec<EventCallback>>;
