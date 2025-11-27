use std::{collections::HashMap, num::NonZeroU32, rc::Rc, time::Instant};

use vello_cpu::{Pixmap, RenderContext, RenderSettings, kurbo::Affine};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::ActiveEventLoop,
    window::{self, Window, WindowAttributes, WindowId},
};

enum RenderState {
    Active {
        window: Rc<Window>,
        surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    },
    Suspended,
}

struct LieWindow {
    attrs: WindowAttributes,
    render_state: RenderState,
    pixmap: Pixmap,
    renderer: RenderContext,
}

impl LieWindow {
    fn new(attrs: WindowAttributes) -> Self {
        Self {
            attrs,
            render_state: RenderState::Suspended,
            pixmap: Pixmap::new(0, 0),
            renderer: RenderContext::new(1, 1),
        }
    }

    fn id(&self) -> Option<WindowId> {
        if let RenderState::Active { window, .. } = &self.render_state {
            Some(window.id())
        } else {
            None
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if matches!(self.render_state, RenderState::Active { .. }) {
            return;
        }

        let window_attrs = self.attrs.clone().with_visible(true).with_active(true);

        let window = Rc::new(event_loop.create_window(window_attrs.clone()).unwrap());

        let inner_size = window.inner_size();

        let context = softbuffer::Context::new(window.clone()).unwrap();
        let surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

        self.attrs = window_attrs.clone().with_inner_size(inner_size);
        self.pixmap
            .resize(inner_size.width as u16, inner_size.height as u16);

        self.render_state = RenderState::Active { window, surface };
        self.renderer = RenderContext::new(inner_size.width as u16, inner_size.height as u16);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let RenderState::Active { window, surface } = &mut self.render_state else {
            return;
        };

        if window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                let width = size.width.max(1);
                let height = size.height.max(1);

                surface
                    .resize(
                        NonZeroU32::new(width).unwrap(),
                        NonZeroU32::new(height).unwrap(),
                    )
                    .unwrap();

                self.pixmap.resize(width as u16, height as u16);
                self.renderer = RenderContext::new_with(
                    width as u16,
                    height as u16,
                    RenderSettings {
                        num_threads: 0,
                        ..Default::default()
                    },
                );

                window.request_redraw();
            }
            // WindowEvent::ModifiersChanged(new_modifiers) => {
            //     self.modifiers = new_modifiers;
            // }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {}
            WindowEvent::MouseInput { state, button, .. } => {}
            WindowEvent::CursorMoved { position, .. } => {}
            WindowEvent::MouseWheel { delta, .. } => {}

            WindowEvent::RedrawRequested => {
                self.renderer.reset();

                self.renderer.flush();
                self.renderer.render_to_pixmap(&mut self.pixmap);

                // Copy pixmap to window surface
                let mut buffer = surface.buffer_mut().unwrap();
                let pixmap_data = self.pixmap.data();

                // Convert RGBA to BGRA/XRGB format expected by softbuffer
                for (buffer_pixel, pixel) in buffer.iter_mut().zip(pixmap_data.iter()) {
                    // softbuffer expects 0RGB format (little-endian: B, G, R, 0)
                    // Our pixmap is premultiplied RGBA
                    *buffer_pixel = u32::from_le_bytes([pixel.b, pixel.g, pixel.r, 0]);
                }

                buffer.present().unwrap();
            }
            _ => {}
        }
    }
}

struct Application {
    windows: HashMap<WindowId, LieWindow>,
}

impl Application {
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
        }
    }

    pub fn create_window(&mut self, attrs: WindowAttributes) -> WindowId {
        let window = LieWindow::new(attrs);
        let window_id = window.id().expect("Window id must be present");

        self.windows.insert(window_id, window);

        window_id
    }

    pub fn run(&mut self) {
        let event_loop = winit::event_loop::EventLoop::new().expect("Failed to create event loop");

        event_loop.run_app(self);
    }
}

impl ApplicationHandler for Application {
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        for window in self.windows.values_mut() {
            window.render_state = RenderState::Suspended;
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        for window in self.windows.values_mut() {
            window.resumed(event_loop);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if let Some(window) = self.windows.get_mut(&window_id) {
            window.window_event(event_loop, window_id, event);
        }
    }
}
