//! Application — 基于 winit 的窗口化 GUI 应用

use std::num::NonZeroU32;
use std::rc::Rc;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::geometry::{Point, Size};
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
    window_size: (u32, u32),
    mouse_pos: Point,
    rendered_once: bool,
    prev_hover: Option<crate::core::ElementId>,
    prev_pressed: Option<crate::core::ElementId>,
}

impl<B: Fn() -> ViewNode + 'static> Application<B> {
    pub fn new(builder: B, viewport: Size) -> Self {
        Self {
            builder,
            runtime: Runtime::new(viewport),
            renderer: VelloRenderer::new(viewport.width as u16, viewport.height as u16),
            window: None,
            surface: None,
            window_size: (viewport.width as u32, viewport.height as u32),
            mouse_pos: Point::zero(),
            rendered_once: false,
            prev_hover: None,
            prev_pressed: None,
        }
    }

    pub fn run(mut self) {
        let el = EventLoop::new().unwrap();
        el.set_control_flow(ControlFlow::Wait);
        let _ = el.run_app(&mut self);
    }

    fn build_and_render(&mut self) {
        let pw = self.renderer.width();
        let ph = self.renderer.height();
        if pw == 0 || ph == 0 {
            return;
        }

        // 同步 viewport 到最新窗口尺寸
        let (ww, wh) = self.window_size;
        self.runtime.set_viewport(Size::new(ww as f32, wh as f32));

        // 清理旧回调用，确保重建时新 Button 注册新回调
        crate::state::clear_callbacks();

        // 1. 构建 View 树 → Runtime
        let view_tree = (self.builder)();
        self.runtime.submit_view_tree(view_tree);
        let elements = self.runtime.frame();

        // 2. VelloRenderer 渲染到 pixmap
        let pixmap = self.renderer.render(&elements);
        let data = pixmap.data(); // &[PremulRgba8]

        // 3. softbuffer 输出到窗口
        if !self.ensure_surface() {
            return;
        }
        let surface = self.surface.as_mut().unwrap();
        let _ = surface.resize(
            NonZeroU32::new(ww.max(1)).unwrap(),
            NonZeroU32::new(wh.max(1)).unwrap(),
        );

        let mut buf = match surface.buffer_mut() {
            Ok(b) => b,
            Err(_) => {
                self.surface = None;
                return;
            }
        };
        let bw = buf.width().get() as usize;
        let bh = buf.height().get() as usize;
        if bw == 0 || bh == 0 {
            return;
        }
        let buf_len = bw * bh;
        let copy_len = buf_len.min(data.len());
        for i in 0..copy_len {
            let p = data[i];
            buf[i] =
                (p.b as u32) | ((p.g as u32) << 8) | ((p.r as u32) << 16) | ((p.a as u32) << 24);
        }
        // 确保新 resize 出来的区域不出现黑色闪烁
        const CLEAR: u32 = 0xFFF0F0F0;
        for i in copy_len..buf_len {
            buf[i] = CLEAR;
        }
        let _ = buf.present();
        self.rendered_once = true;
    }

    fn ensure_surface(&mut self) -> bool {
        if self.surface.is_some() {
            return true;
        }
        let Some(window) = &self.window else {
            return false;
        };
        let Ok(ctx) = softbuffer::Context::new(window.clone()) else {
            return false;
        };
        let Ok(mut surf) = softbuffer::Surface::new(&ctx, window.clone()) else {
            return false;
        };
        let (ww, wh) = self.window_size;
        let _ = surf.resize(
            NonZeroU32::new(ww.max(1)).unwrap(),
            NonZeroU32::new(wh.max(1)).unwrap(),
        );
        self.surface = Some(surf);
        true
    }

    /// 仅重绘视觉层（跳过 builder/layout/reconciliation）
    fn render_visuals(&mut self) {
        let elements = self.runtime.frame_render_only();
        if elements.is_empty() {
            return;
        }
        let pix = self.renderer.render(&elements);
        if let Some(surf) = &mut self.surface {
            let (ww, wh) = self.window_size;
            let _ = surf.resize(
                NonZeroU32::new(ww.max(1)).unwrap(),
                NonZeroU32::new(wh.max(1)).unwrap(),
            );
            let mut buf = match surf.buffer_mut() {
                Ok(b) => b,
                _ => {
                    self.surface = None;
                    return;
                }
            };
            let bw = buf.width().get() as usize;
            let bh = buf.height().get() as usize;
            if bw > 0 && bh > 0 {
                let data = pix.data();
                let buf_len = bw * bh;
                let cl = buf_len.min(data.len());
                for i in 0..cl {
                    let p = data[i];
                    buf[i] = (p.b as u32)
                        | ((p.g as u32) << 8)
                        | ((p.r as u32) << 16)
                        | ((p.a as u32) << 24);
                }
                const CLEAR: u32 = 0xFFF0F0F0;
                for i in cl..buf_len {
                    buf[i] = CLEAR;
                }
                let _ = buf.present();
            }
        }
    }

    fn init_window(&mut self, el: &ActiveEventLoop) {
        let wa = Window::default_attributes()
            .with_title("LieUI v2")
            .with_inner_size(LogicalSize::new(600.0, 400.0));
        let window = Rc::new(el.create_window(wa).unwrap());
        let s = window.inner_size();
        self.window_size = (s.width, s.height);
        self.runtime
            .set_viewport(Size::new(s.width as f32, s.height as f32));
        self.renderer.resize(s.width as u16, s.height as u16);
        self.window = Some(window);
    }

    fn hit_test(&self) -> Option<crate::core::ElementId> {
        self.runtime
            .layers
            .hit_test_top(self.mouse_pos)
            .map(|(_, id)| id)
    }

    fn handle_click(&mut self) {
        let id = match self.hit_test() {
            Some(i) => i,
            None => return,
        };
        // 设置 pressed 状态
        if self.runtime.layers.tree.contains(id) {
            let mut s = self.runtime.layers.tree.state(id);
            s.pressed = true;
            self.runtime.layers.tree.set_state(id, s);
        }
        // 触发回调
        let node = self.runtime.layers.tree.get_node(id);
        if let Some(cb_id) = node.on_click() {
            state::invoke_click(cb_id);
        }
    }

    fn handle_hover(&mut self) {
        let new = self.hit_test();
        if new != self.prev_hover {
            // 旧元素取消 hover
            if let Some(old) = self.prev_hover {
                if self.runtime.layers.tree.contains(old) {
                    let mut s = self.runtime.layers.tree.state(old);
                    s.hovered = false;
                    self.runtime.layers.tree.set_state(old, s);
                }
            }
            // 新元素设置 hover
            if let Some(nid) = new {
                if self.runtime.layers.tree.contains(nid) {
                    let mut s = self.runtime.layers.tree.state(nid);
                    s.hovered = true;
                    self.runtime.layers.tree.set_state(nid, s);
                }
            }
            self.prev_hover = new;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}

impl<B: Fn() -> ViewNode + 'static> ApplicationHandler for Application<B> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_none() {
            self.init_window(el);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _wid: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                if s.width > 0 && s.height > 0 && (s.width, s.height) != self.window_size {
                    self.window_size = (s.width, s.height);
                    self.renderer.resize(s.width as u16, s.height as u16);
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = Point::new(position.x as f32, position.y as f32);
                self.handle_hover();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            } => {
                let id = self.hit_test();
                self.prev_pressed = id;
                self.handle_click();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                ..
            } => {
                // 清除所有元素的 pressed 状态
                if let Some(pid) = self.prev_pressed {
                    if self.runtime.layers.tree.contains(pid) {
                        let mut s = self.runtime.layers.tree.state(pid);
                        s.pressed = false;
                        self.runtime.layers.tree.set_state(pid, s);
                    }
                }
                self.prev_pressed = None;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                let viewport = (
                    self.runtime.viewport.width as u32,
                    self.runtime.viewport.height as u32,
                );
                let size_mismatch = self.window_size != viewport;
                if !self.rendered_once
                    || self.surface.is_none()
                    || state::take_rebuild_requested()
                    || size_mismatch
                {
                    self.build_and_render();
                } else {
                    // hover/pressed/动画等视觉更新
                    self.render_visuals();
                }
            }
            _ => {}
        }
    }
}
