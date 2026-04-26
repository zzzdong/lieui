// src/event/callback.rs

use crate::core::{ViewContext, WidgetId};
use crate::event::{Event, EventResult, EventType};

/// 统一的事件回调类型
///
/// 参数：
/// - id: 当前处理事件的 Widget ID
/// - event: 事件数据
/// - ctx: 可修改 Widget 树的上下文
///
/// 返回 EventResult 控制传播：
/// - Continue: 继续传播
/// - Stop: 停止传播
/// - PreventDefault: 阻止默认行为但继续传播
pub type EventCallback = Box<dyn FnMut(WidgetId, &Event, &mut ViewContext) -> EventResult>;

/// 事件回调管理 trait
pub trait EventCallbackManager {
    /// 注册事件回调
    fn register_callback(&mut self, id: WidgetId, event_type: EventType, callback: EventCallback);

    /// 取出回调（避免借用冲突）
    fn take_callbacks(&mut self, id: WidgetId, event_type: EventType) -> Vec<EventCallback>;
}
