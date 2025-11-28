use std::collections::BTreeMap;

use vello_cpu::RenderContext;

use crate::{
    element::{
        ElementId, ElementRef, IElement,
        world::{ElementNode, ElementTree},
    },
    event::{EventDispatcher, PointerEvent, PointerEventHandler, PointerEventType},
};

pub struct View<State> {
    elements: ElementTree,
    pointer_handler: PointerEventHandler,
    event_dispatcher: BTreeMap<ElementId, EventDispatcher<State>>,
}

impl<State> View<State> {
    pub fn new() -> Self {
        Self {
            elements: ElementTree::new(),
            pointer_handler: PointerEventHandler::new(),
            event_dispatcher: BTreeMap::new(),
        }
    }

    pub fn builder() -> Builder<State> {
        Builder::new()
    }

    pub fn render(&mut self, cx: &mut RenderContext) {
        self.elements.do_paint(cx);
    }

    pub fn request_layout(&mut self, width: f32, height: f32) {
        self.elements.do_layout(width, height);
    }

    pub(crate) fn handle_event(&mut self, event: winit::event::WindowEvent, state: &mut State) {
        self.pointer_handler
            .handle_window_event(&event, &mut self.elements);

        for event in self.pointer_handler.events().split_off(0) {
            if let Some(target) = event.target {
                if let Some(dispatcher) = self.event_dispatcher.get(&target) {
                    match event.event_type {
                        PointerEventType::PointerPressed => {
                            if let Some(handler) = dispatcher.on_pointer_pressed.as_ref() {
                                handler(event, state);
                            }
                        }
                        PointerEventType::PointerReleased => {
                            if let Some(handler) = dispatcher.on_pointer_released.as_ref() {
                                handler(event, state);
                            }
                        }
                        PointerEventType::PointerClicked => {
                            if let Some(handler) = dispatcher.on_pointer_clicked.as_ref() {
                                handler(event, state);
                            }
                        }
                        PointerEventType::PointerMoved => {
                            if let Some(handler) = dispatcher.on_pointer_moved.as_ref() {
                                handler(event, state);
                            }
                        }
                        PointerEventType::PointerEntered => {
                            if let Some(handler) = dispatcher.on_pointer_entered.as_ref() {
                                handler(event, state);
                            }
                        }
                        PointerEventType::PointerExited => {
                            if let Some(handler) = dispatcher.on_pointer_exited.as_ref() {
                                handler(event, state);
                            }
                        }
                        PointerEventType::PointerCaptureLost => {
                            if let Some(handler) = dispatcher.on_pointer_capture_lost.as_ref() {
                                handler(event, state);
                            }
                        }
                    }
                }
            }
        }

        self.pointer_handler.events().clear();
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
        handler: impl Fn(PointerEvent, &mut State) + Send + 'static,
    ) {
        let dispatcher = self
            .event_dispatcher
            .entry(element)
            .or_insert_with(EventDispatcher::new);
        dispatcher.on_pointer_pressed = Some(Box::new(handler));
    }

    pub fn handle_pointer_released(
        &mut self,
        element: ElementId,
        handler: impl Fn(PointerEvent, &mut State) + Send + 'static,
    ) {
        let dispatcher = self
            .event_dispatcher
            .entry(element)
            .or_insert_with(EventDispatcher::new);
        dispatcher.on_pointer_released = Some(Box::new(handler));
    }
}

pub struct Builder<State> {
    elements: ElementTree,
    pointer_handler: PointerEventHandler,
    event_dispatcher: BTreeMap<ElementId, EventDispatcher<State>>,
}

impl<State> Builder<State> {
    pub fn new() -> Self {
        Self {
            elements: ElementTree::new(),
            pointer_handler: PointerEventHandler::new(),
            event_dispatcher: BTreeMap::new(),
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

    pub fn handle_pointer_pressed(
        mut self,
        element: ElementId,
        handler: impl Fn(PointerEvent, &mut State) + Send + 'static,
    ) {
        let dispatcher = self
            .event_dispatcher
            .entry(element)
            .or_insert_with(EventDispatcher::new);
        dispatcher.on_pointer_pressed = Some(Box::new(handler));
    }

    pub fn handle_pointer_released(
        mut self,
        element: ElementId,
        handler: impl Fn(PointerEvent, &mut State) + Send + 'static,
    ) {
        let dispatcher = self
            .event_dispatcher
            .entry(element)
            .or_insert_with(EventDispatcher::new);
        dispatcher.on_pointer_released = Some(Box::new(handler));
    }

    pub fn build(self) -> View<State> {
        View {
            elements: self.elements,
            pointer_handler: self.pointer_handler,
            event_dispatcher: self.event_dispatcher,
        }
    }
}
