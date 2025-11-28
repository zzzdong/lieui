pub mod pointer;
pub mod types;

pub use pointer::*;
pub use types::*;

pub struct EventDispatcher<State> {
    pub on_pointer_pressed: Option<Box<dyn Fn(PointerEvent, &mut State) + Send + 'static>>,
    pub on_pointer_released: Option<Box<dyn Fn(PointerEvent, &mut State) + Send + 'static>>,
    pub on_pointer_clicked: Option<Box<dyn Fn(PointerEvent, &mut State) + Send + 'static>>,
    pub on_pointer_moved: Option<Box<dyn Fn(PointerEvent, &mut State) + Send + 'static>>,
    pub on_pointer_entered: Option<Box<dyn Fn(PointerEvent, &mut State) + Send + 'static>>,
    pub on_pointer_exited: Option<Box<dyn Fn(PointerEvent, &mut State) + Send + 'static>>,
    pub on_pointer_capture_lost: Option<Box<dyn Fn(PointerEvent, &mut State) + Send + 'static>>,
}

impl<State> EventDispatcher<State> {
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
