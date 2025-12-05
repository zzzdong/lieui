use std::{
    collections::HashMap,
    marker::PhantomData,
    num::NonZeroU32,
    rc::Rc,
    time::{Duration, Instant, SystemTime},
};

use vello_cpu::{Pixmap, RenderContext, RenderSettings, kurbo::Affine};
use winit::{
    application::ApplicationHandler,
    dpi::Size,
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, StartCause, WindowEvent},
    event_loop::{self, ActiveEventLoop},
    window::{Window, WindowAttributes, WindowId},
};

use crate::world::View;

enum RenderState {
    Active {
        window: Rc<Window>,
        surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    },
    Suspended,
}

struct LieWindow<State> {
    attrs: WindowAttributes,
    render_state: RenderState,
    pixmap: Pixmap,
    renderer: RenderContext,
    view: View<State>,
    last_modified: Option<SystemTime>,
}

impl<State> LieWindow<State> {
    fn new(attrs: WindowAttributes, view: View<State>) -> Self {
        Self {
            attrs,
            render_state: RenderState::Suspended,
            pixmap: Pixmap::new(0, 0),
            renderer: RenderContext::new(1, 1),
            view,
            last_modified: None,
        }
    }

    fn id(&self) -> Option<WindowId> {
        if let RenderState::Active { window, .. } = &self.render_state {
            Some(window.id())
        } else {
            None
        }
    }

    fn window(&self) -> Option<&Window> {
        if let RenderState::Active { window, .. } = &self.render_state {
            Some(window)
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
        self.view
            .request_layout(inner_size.width as f32, inner_size.height as f32);

        self.render_state = RenderState::Active { window, surface };
        self.renderer = RenderContext::new(inner_size.width as u16, inner_size.height as u16);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
        state: &mut State,
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

                self.view.request_layout(width as f32, height as f32);

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

                self.attrs = self.attrs.clone().with_inner_size(size);

                window.request_redraw();
            }

            WindowEvent::RedrawRequested => {
                self.renderer.reset();

                // reload view from xml
                let last_modified = std::fs::metadata("view.xml").unwrap().modified().unwrap();
                if self.last_modified.is_none() || last_modified > self.last_modified.unwrap() {
                    self.last_modified = Some(last_modified);
                    if let Ok(view) = std::fs::read_to_string("view.xml").and_then(|xml| {
                        View::builder().load_xml(&xml).map_err(|err| {
                            std::io::Error::new(std::io::ErrorKind::Other, err.to_string())
                        })
                    }) {
                        self.view = view.build();
                        if let Some(size) = self.attrs.inner_size {
                            self.view.request_layout(
                                size.to_physical::<u32>(1.0).width as f32,
                                size.to_physical::<u32>(1.0).height as f32,
                            );
                        }
                    }
                }

                self.view.render(&mut self.renderer);

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

                window.pre_present_notify();

                buffer.present().unwrap();
            }
            _ => {
                self.view.handle_event(event, state);
            }
        }
    }
}

enum PrimaryWindow<State> {
    Uninitialized {
        attrs: WindowAttributes,
        view: View<State>,
    },
    Initialized(WindowId),
}

pub struct Application<State> {
    windows: HashMap<WindowId, LieWindow<State>>,
    state: State,
    primary_window: PrimaryWindow<State>,
    last_update: Instant,
}

impl<State> Application<State> {
    pub fn new(view: View<State>, attrs: WindowAttributes, state: State) -> Self {
        Self {
            windows: HashMap::new(),
            state,
            primary_window: PrimaryWindow::<State>::Uninitialized { attrs, view },
            last_update: Instant::now(),
        }
    }

    pub fn create_window(
        &mut self,
        attrs: WindowAttributes,
        view: View<State>,
        event_loop: &ActiveEventLoop,
    ) -> WindowId {
        let mut window = LieWindow::<State>::new(attrs, view);

        window.resumed(event_loop);

        let window_id = window.id().expect("window must be initialized");

        self.windows.insert(window_id, window);

        window_id
    }

    pub fn run(&mut self) {
        let event_loop = winit::event_loop::EventLoop::new().expect("failed to create event loop");

        event_loop.set_control_flow(event_loop::ControlFlow::Poll);

        let _ = event_loop.run_app(self);
    }
}

impl<State> ApplicationHandler for Application<State> {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: winit::event::StartCause) {
        if let StartCause::Init = cause {
            let primary_window = std::mem::replace(
                &mut self.primary_window,
                PrimaryWindow::Initialized(WindowId::dummy()),
            );

            if let PrimaryWindow::Uninitialized { attrs, view } = primary_window {
                let window_id = self.create_window(attrs, view, event_loop);
                self.primary_window = PrimaryWindow::Initialized(window_id);
            }
        }
    }

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
            window.window_event(event_loop, window_id, event, &mut self.state);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now - self.last_update > Duration::from_secs_f32(0.01) {
            self.last_update = now;
            for window in self.windows.values_mut() {
                if let Some(window) = window.window() {
                    window.request_redraw();
                }
            }
        }
    }
}

struct ViewModel {
    pub title: String,
    pub counter: i32,
}

impl ViewModel {
    pub fn new() -> Self {
        Self {
            title: "Hello, world!".to_string(),
            counter: 0,
        }
    }

    pub fn increment(&mut self) {
        self.counter += 1;
    }
}

#[cfg(test)]
mod tests {
    use winit::dpi::PhysicalSize;

    use super::*;
}
