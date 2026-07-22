//! Application — 基于 winit 的窗口化 GUI 应用

use std::num::NonZeroU32;
use std::rc::Rc;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::core::layers::LayerType;
use crate::geometry::{Point, Size};
use crate::layout::node::LayoutContext;
use crate::render::renderer::Renderer;
use crate::render::VelloRenderer;
use crate::runtime::Runtime;
use crate::state;
use crate::view::node::ViewNode;

pub struct Application<B: Fn() -> ViewNode> {
    builder: B,
    runtime: Runtime,
    renderer: VelloRenderer,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    mouse_pos: Point,
    first_layout: Option<LayoutContext>,
}

impl<B: Fn() -> ViewNode + 'static> Application<B> {
    pub fn new(builder: B, viewport: Size) -> Self {
        Self {
            builder,
            runtime: Runtime::new(viewport),
            renderer: VelloRenderer::new(viewport.width as u16, viewport.height as u16),
            window: None,
            surface: None,
            mouse_pos: Point::zero(),
            first_layout: None,
        }
    }

    pub fn run(mut self) {
        let el = EventLoop::new().unwrap();
        el.set_control_flow(ControlFlow::Wait);
        let _ = el.run_app(&mut self);
    }

    fn build_and_render(&mut self) {
        let view_tree = (self.builder)();
        self.runtime.submit_view_tree(view_tree);
        let elements = self.runtime.frame();

        self.runtime.layers.with_layout(LayerType::Base, |layout| {
            self.first_layout = Some(layout.clone());
        });

        let pw = self.renderer.width();
        let ph = self.renderer.height();
        if pw > 0 && ph > 0 {
            let pixmap = self.renderer.render(&elements);
            self.present(pixmap);
        }
    }

    fn present(&mut self, pixmap: vello_cpu::Pixmap) {
        let Some(surface) = &mut self.surface else { return };
        let mut buf = match surface.buffer_mut() { Ok(b) => b, Err(_) => return };
        let w = buf.width().get();
        let h = buf.height().get();
        if w == 0 || h == 0 { return; }
        let data = pixmap.data();
        for i in 0..(w * h) as usize {
            let p = if i < data.len() { data[i] } else { continue };
            let bgra = (p.b as u32) | ((p.g as u32) << 8) | ((p.r as u32) << 16) | ((p.a as u32) << 24);
            buf[i] = bgra;
        }
        let _ = buf.present();
    }

    fn init_window(&mut self, el: &ActiveEventLoop) {
        let wa = Window::default_attributes()
            .with_title("LieUI v2")
            .with_inner_size(LogicalSize::new(1100.0, 780.0));
        let window = Rc::new(el.create_window(wa).unwrap());
        let ctx = softbuffer::Context::new(window.clone()).unwrap();
        let s = window.inner_size();
        let mut surface = softbuffer::Surface::new(&ctx, window.clone()).unwrap();
        let _ = surface.resize(
            NonZeroU32::new(s.width.max(1)).unwrap(),
            NonZeroU32::new(s.height.max(1)).unwrap(),
        );
        self.window = Some(window);
        self.surface = Some(surface);
        let vp = Size::new(s.width as f32, s.height as f32);
        self.runtime.set_viewport(vp);
        self.renderer = VelloRenderer::new(s.width as u16, s.height as u16);
    }

    fn handle_click(&mut self) {
        let layout = match &self.first_layout { Some(l) => l, None => return };
        let root = match &layout.root { Some(r) => r, None => return };
        let id = Self::find_clicked_element(root, self.mouse_pos.x, self.mouse_pos.y);
        let Some(id) = id else { return };
        let props = self.runtime.layers.tree.props(id);
        let Some(p) = props else { return };
        let Some(cb_id) = p.get_u32("on_click") else { return };
        state::invoke_click(cb_id as u64);
    }

    fn find_clicked_element(node: &crate::layout::node::LayoutNode, px: f32, py: f32) -> Option<crate::core::ElementId> {
        if !node.computed.contains(px, py) { return None; }
        for child in node.children.iter().rev() {
            if let Some(id) = Self::find_clicked_element(child, px, py) { return Some(id); }
        }
        Some(node.id)
    }
}

impl<B: Fn() -> ViewNode + 'static> ApplicationHandler for Application<B> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_none() { self.init_window(el); }
        self.build_and_render();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _wid: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                if s.width > 0 && s.height > 0 {
                    let vp = Size::new(s.width as f32, s.height as f32);
                    self.runtime.set_viewport(vp);
                    self.renderer.resize(s.width as u16, s.height as u16);
                    if let Some(surf) = &mut self.surface {
                        let _ = surf.resize(NonZeroU32::new(s.width).unwrap(), NonZeroU32::new(s.height).unwrap());
                    }
                    self.build_and_render();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = Point::new(position.x as f32, position.y as f32);
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, .. } => {
                self.handle_click();
                if let Some(w) = &self.window { w.request_redraw(); }
            }
            WindowEvent::RedrawRequested => {
                if state::take_rebuild_requested() {
                    self.build_and_render();
                }
            }
            _ => {}
        }
    }
}
