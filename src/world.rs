use std::collections::BTreeMap;

use vello_cpu::RenderContext;

use crate::{
    element::{
        ElementId, ElementRef, IElement,
        world::{ElementNode, ElementTree},
    }, event::{pointer::PointerEvent, world::{EventResult, EventWorld}},
    
};

pub struct View<State> {
    elements: ElementTree,
    event_world: EventWorld<State>,
}

impl<State> View<State> {
    pub fn new() -> Self {
        Self {
            elements: ElementTree::new(),
            event_world: EventWorld::new(),
        }
    }

    pub fn builder() -> Builder<State> {
        Builder::<State>::new()
    }

    pub fn render(&mut self, cx: &mut RenderContext) {
        self.elements.do_paint(cx);
    }

    pub fn request_layout(&mut self, width: f32, height: f32) {
        self.elements.do_layout(width, height);
    }

    pub(crate) fn handle_event(&mut self, event: winit::event::WindowEvent, state: &mut State) {
        self.event_world.dispatch_window_event(&event, &mut self.elements, state);
    }

    pub fn add_element<E: IElement + 'static>(mut self, element: E) -> ElementId {
        let id = self.elements.add_node(element);
        id
    }

    pub fn add_child<E: IElement + 'static>(&mut self, parent: ElementId, child: E) -> ElementId {
        self.elements.add_child(parent, child)
    }

    pub fn add_root<E: IElement + 'static>(&mut self, root: E) -> ElementId {
        self.elements.add_root(root)
    }

    pub fn handle_pointer_pressed(
        &mut self,
        element: ElementId,
        handler: impl FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static,
    ) {
        self.event_world.handle_pointer_pressed(element, handler);
    }

    pub fn handle_pointer_released(
        &mut self,
        element: ElementId,
        handler: impl FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static,
    ) {
        self.event_world.handle_pointer_released(element, handler);
    }
    

    pub fn handle_pointer_clicked(
        &mut self,
        element: ElementId,
        handler: impl FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static,
    ) {
        self.event_world.handle_pointer_clicked(element, handler);
    }
    
    pub fn handle_pointer_entered(
        &mut self,
        element: ElementId,
        handler: impl FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static,
    ) {
        self.event_world.handle_pointer_entered(element, handler);
    }
    

    pub fn handle_pointer_exited(
        &mut self,
        element: ElementId,
        handler: impl FnMut(PointerEvent, &mut State) -> EventResult + Send + 'static,
    ) {
        self.event_world.handle_pointer_exited(element, handler);
    }
    
}

pub struct Builder<State> {
    elements: ElementTree,
    pointer_handler: EventWorld<State>,
}

impl<State> Builder<State> {
    pub fn new() -> Self {
        Self {
            elements: ElementTree::new(),
            pointer_handler: EventWorld::new(),
        }
    }

    pub fn add_element<E: IElement + 'static>(mut self, element: E) -> ElementId {
        let id = self.elements.add_node(element);
        id
    }

    pub fn add_child<E: IElement + 'static>(&mut self, parent: ElementId, child: E) -> ElementId {
        self.elements.add_child(parent, child)
    }

    pub fn add_root<E: IElement + 'static>(&mut self, root: E) -> ElementId {
        self.elements.add_root(root)
    }
}
