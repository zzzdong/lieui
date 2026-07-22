//! 事件回调类型
use crate::core::ElementId;
use crate::event::Event;

pub type EventHandler = Box<dyn FnMut(ElementId, &Event, &mut crate::event::EventContext)>;

#[derive(Default)]
pub struct EventCallbacks {
    pub on_click: Option<Box<dyn FnMut(i32, i32)>>,
}
impl EventCallbacks {
    pub fn new() -> Self {
        Self { on_click: None }
    }
}
