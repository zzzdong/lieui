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
use crate::layout::context::LayoutContext;
use crate::render::renderer::Renderer as _;
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
    rendered_once: bool,
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
            rendered_once: false,
        }
    }

    pub fn run(mut self) {
        let el = EventLoop::new().unwrap();
        el.set_control_flow(ControlFlow::Wait);
        let _ = el.run_app(&mut self);
    }

    fn build_and_render(&mut self) {
        let pw = self.renderer.width() as u16;
        let ph = self.renderer.height() as u16;
        if pw == 0 || ph == 0 { return; }

        // 清理旧回调用，确保重建时新 Button 注册新回调
        crate::state::clear_callbacks();

        // 1. 构建 View 树 → Runtime
        let view_tree = (self.builder)();
        self.runtime.submit_view_tree(view_tree);
        let elements = self.runtime.frame();
        self.runtime.layers.with_layout(LayerType::Base, |layout| {
            self.first_layout = Some(layout.clone());
        });

        // 2. VelloRenderer 渲染到 pixmap
        let pixmap = self.renderer.render(&elements);
        let data = pixmap.data(); // &[PremulRgba8]

        // 3. softbuffer 输出到窗口
        if !self.ensure_surface() { return; }
        let surface = self.surface.as_mut().unwrap();
        let s = self.window.as_ref().unwrap().inner_size();
        let _ = surface.resize(NonZeroU32::new(s.width.max(1)).unwrap(), NonZeroU32::new(s.height.max(1)).unwrap());

        let mut buf = match surface.buffer_mut() {
            Ok(b) => b,
            Err(_) => { self.surface = None; return; }
        };
        let bw = buf.width().get() as usize;
        let bh = buf.height().get() as usize;
        if bw == 0 || bh == 0 { return; }
        let copy_len = (bw * bh).min(data.len());
        for i in 0..copy_len {
            let p = data[i];
            buf[i] = (p.b as u32) | ((p.g as u32) << 8) | ((p.r as u32) << 16) | ((p.a as u32) << 24);
        }
        let _ = buf.present();
        self.rendered_once = true;
    }

    fn ensure_surface(&mut self) -> bool {
        if self.surface.is_some() { return true; }
        let Some(window) = &self.window else { return false; };
        let Ok(ctx) = softbuffer::Context::new(window.clone()) else { return false; };
        let Ok(mut surf) = softbuffer::Surface::new(&ctx, window.clone()) else { return false; };
        let s = window.inner_size();
        let _ = surf.resize(NonZeroU32::new(s.width.max(1)).unwrap(), NonZeroU32::new(s.height.max(1)).unwrap());
        self.surface = Some(surf);
        true
    }

    fn init_window(&mut self, el: &ActiveEventLoop) {
        let wa = Window::default_attributes()
            .with_title("LieUI v2")
            .with_inner_size(LogicalSize::new(600.0, 400.0));
        let window = Rc::new(el.create_window(wa).unwrap());
        let s = window.inner_size();
        self.runtime.set_viewport(Size::new(s.width as f32, s.height as f32));
        self.renderer.resize(s.width as u16, s.height as u16);
        self.window = Some(window);
    }

    fn hit_test(&self) -> Option<crate::core::ElementId> {
        let layout = match &self.first_layout { Some(l) => l, None => return None };
        let root = match &layout.root { Some(r) => r, None => return None };
        Self::deepest_at(root, self.mouse_pos.x, self.mouse_pos.y)
    }

    fn handle_click(&mut self) {
        let id = match self.hit_test() { Some(i) => i, None => return };
        let Some(cb_id) = self.runtime.layers.tree.props(id).and_then(|p| p.get_u32("on_click")) else { return };
        state::invoke_click(cb_id as u64);
    }

    fn handle_hover(&mut self) {
        let id = self.hit_test();
        if id != self.runtime.hovered_id {
            self.runtime.hovered_id = id;
            if let Some(w) = &self.window { w.request_redraw(); }
        }
    }

    fn deepest_at(node: &crate::layout::node::LayoutNode, px: f32, py: f32) -> Option<crate::core::ElementId> {
        if !node.computed.contains(px, py) { return None; }
        for child in node.children.iter().rev() {
            if let Some(id) = Self::deepest_at(child, px, py) { return Some(id); }
        }
        Some(node.id)
    }
}

impl<B: Fn() -> ViewNode + 'static> ApplicationHandler for Application<B> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_none() {
            self.init_window(el);
            if let Some(w) = &self.window { w.request_redraw(); }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _wid: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                if s.width > 0 && s.height > 0 {
                    self.runtime.set_viewport(Size::new(s.width as f32, s.height as f32));
                    self.renderer.resize(s.width as u16, s.height as u16);
                    self.surface = None;
                    if let Some(w) = &self.window { w.request_redraw(); }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = Point::new(position.x as f32, position.y as f32);
                self.handle_hover();
                if let Some(w) = &self.window { w.request_redraw(); }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, .. } => {
                self.runtime.pressed_id = self.hit_test();
                self.handle_click();
                if let Some(w) = &self.window { w.request_redraw(); }
            }
            WindowEvent::MouseInput { state: ElementState::Released, .. } => {
                self.runtime.pressed_id = None;
                if let Some(w) = &self.window { w.request_redraw(); }
            }
            WindowEvent::RedrawRequested => {
                if !self.rendered_once || state::take_rebuild_requested() || self.surface.is_none() {
                    self.build_and_render();
                }
            }
            _ => {}
        }
    }
}
