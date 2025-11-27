use crate::{element::world::ElementTree, event::PointerEventHandler};

struct UIWorld<State> {
    elements: ElementTree<State>,
    pointer_handler: PointerEventHandler<State>,
}

impl<State> UIWorld<State> {
    pub fn new() -> Self {
        Self {
            elements: ElementTree::new(),
            pointer_handler: PointerEventHandler::new(),
        }
    }

    pub fn handle_event(&mut self, event: winit::event::WindowEvent, state: &mut State) {
        self.pointer_handler
            .handle_window_event(&event, &mut self.elements, state);
    }
}
