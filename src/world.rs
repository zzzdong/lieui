use vello_cpu::RenderContext;

use crate::{element::world::ElementTree, event::PointerEventHandler};

pub struct View<State> {
    elements: ElementTree<State>,
    pointer_handler: PointerEventHandler<State>,
}

impl<State> View<State> {
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

    pub fn render(&mut self, cx: &mut RenderContext) {
        self.elements.do_paint(cx);
    }

    pub fn request_layout(&mut self, width: f32, height: f32) {
        self.elements
            .do_layout(width, height);
    }
}
