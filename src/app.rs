//! Application — 基于 winit 的窗口化 GUI 应用

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::event::{EventEffects, HitTestResult, MouseButton};
use crate::geometry::{Point, Size};
use crate::render::renderer::Renderer as _;
use crate::render::VelloRenderer;
use crate::runtime::Runtime;
use crate::state;
use crate::view::node::{ClickCallbackRef, ViewNode};

pub struct Application<B: Fn() -> ViewNode> {
    builder: B,
    runtime: Runtime,
    renderer: VelloRenderer,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    window_size: (u32, u32),
    mouse_pos: Point,
    rendered_once: bool,
}

static WINDOW_SIZE: AtomicU64 = AtomicU64::new(0);

fn pack_size(w: u32, h: u32) -> u64 {
    ((w as u64) << 32) | (h as u64)
}

fn unpack_size(v: u64) -> (u32, u32) {
    ((v >> 32) as u32, v as u32)
}

pub fn set_window_size(w: u32, h: u32) {
    WINDOW_SIZE.store(pack_size(w, h), Ordering::Relaxed);
}

/// 获取当前窗口大小。如果尚未初始化，返回 Size::zero()。
pub fn window_size() -> Size {
    let (w, h) = unpack_size(WINDOW_SIZE.load(Ordering::Relaxed));
    Size::new(w as f32, h as f32)
}

impl<B: Fn() -> ViewNode + 'static> Application<B> {
    pub fn new(builder: B, viewport: Size) -> Self {
        set_window_size(viewport.width as u32, viewport.height as u32);
        Self {
            builder,
            runtime: Runtime::new(viewport),
            renderer: VelloRenderer::new(viewport.width as u16, viewport.height as u16),
            window: None,
            surface: None,
            window_size: (viewport.width as u32, viewport.height as u32),
            mouse_pos: Point::zero(),
            rendered_once: false,
        }
    }

    pub fn run(mut self) {
        let el = EventLoop::new().unwrap();
        el.set_control_flow(ControlFlow::Wait);
        let _ = el.run_app(&mut self);
    }

    // ========== 构建/渲染管线 ==========

    fn build_and_render(&mut self) {
        let pw = self.renderer.width();
        let ph = self.renderer.height();
        if pw == 0 || ph == 0 {
            return;
        }

        let (ww, wh) = self.window_size;
        self.runtime.set_viewport(Size::new(ww as f32, wh as f32));

        crate::state::clear_callbacks();

        let view_tree = (self.builder)();
        self.runtime.submit_view_tree(view_tree);
        let elements = self.runtime.frame();

        let pixmap = self.renderer.render(&elements);
        self.blit_to_window(pixmap.data());
        self.rendered_once = true;
    }

    /// 应用事件回调产生的副作用：重建 / 重排 / 重绘。
    /// 修复此前 `EventEffects` 被调用方直接丢弃、导致
    /// `EventContext::request_rebuild/request_layout/request_render` 成为空操作的问题。
    fn apply_event_effects(&mut self, effects: EventEffects) {
        if effects.needs_rebuild() {
            state::request_rebuild();
        }
        if effects.needs_layout() {
            self.runtime.request_layout();
        }
        if effects.needs_render() {
            self.runtime.request_render();
        }
    }

    fn render_visuals(&mut self) {
        let elements = self.runtime.frame_visual_update();
        if elements.is_empty() {
            return;
        }
        let pix = self.renderer.render(&elements);
        self.blit_to_window(pix.data());
    }

    fn blit_to_window(&mut self, data: &[vello_cpu::color::PremulRgba8]) {
        if !self.ensure_surface() {
            return;
        }
        let surface = self.surface.as_mut().unwrap();
        let (ww, wh) = self.window_size;
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
        const CLEAR: u32 = 0xFFF0F0F0;
        for i in copy_len..buf_len {
            buf[i] = CLEAR;
        }
        let _ = buf.present();
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

    fn init_window(&mut self, el: &ActiveEventLoop) {
        let (ww, wh) = self.window_size;
        let wa = Window::default_attributes()
            .with_title("LieUI v2")
            .with_inner_size(LogicalSize::new(ww as f64, wh as f64));
        let window = Rc::new(el.create_window(wa).unwrap());
        let s = window.inner_size();
        self.window_size = (s.width, s.height);
        set_window_size(s.width, s.height);
        self.runtime
            .set_viewport(Size::new(s.width as f32, s.height as f32));
        self.renderer.resize(s.width as u16, s.height as u16);
        self.window = Some(window);
    }

    // ========== 事件处理 ==========

    fn build_hit_result(&self) -> Option<HitTestResult> {
        let (_lt, target) = self.runtime.layers.hit_test_top(self.mouse_pos)?;
        let path = self.runtime.layers.path_to(target);
        Some(HitTestResult { target, path })
    }

    /// 通用事件回调：拦截 Click / MouseDown / MouseUp 并触发全局回调
    ///
    /// HTML 式事件模型：任何挂有 listener 的节点都参与分发，事件按捕获 → 目标 → 冒泡
    /// 顺序传播，每层 listener 自行决定是否调用 `ctx.stop_propagation()`。
    ///
    /// 阶段语义：
    /// - 捕获 Capture：仅运行 `WithCtx` 回调，使父节点可在到达 target 前拦截/停止传播；
    ///   `Simple` 只在 Target/Bubble 触发，避免被祖先误吞或嵌套组件重复触发。
    /// - 目标 Target：触发回调（Simple 自动 stop；WithCtx 由用户决定）。
    /// - 冒泡 Bubble：WithCtx 已在捕获阶段触发过，避免重复；仅 `Simple` 触发。
    fn handle_lie_event(
        tree: &crate::runtime::element::ElementTree,
        id: crate::core::ElementId,
        event: &crate::event::Event,
        ctx: &mut crate::event::EventContext,
    ) {
        // Click / MouseDown / MouseUp 均按 HTML 事件模型分发：
        // - Capture：仅 WithCtx，允许祖先拦截；
        // - Target：触发所有回调；
        // - Bubble：仅 Simple（WithCtx 已在 Capture 触发，避免重复）。
        match event {
            crate::event::Event::Click { .. }
            | crate::event::Event::MouseDown { .. }
            | crate::event::Event::MouseUp { .. } => {
                let has_listener = tree.get_node_ref(id).and_then(|n| n.listener()).is_some();
                if !has_listener {
                    return;
                }
                match ctx.phase() {
                    crate::event::EventPhase::Capture => {
                        if let Some(cb) = tree.get_node_ref(id).and_then(|n| n.listener()) {
                            if let ClickCallbackRef::WithCtx(_) = cb {
                                state::invoke_click(cb.id(), ctx);
                            }
                        }
                    }
                    crate::event::EventPhase::Target => {
                        if let Some(cb_id) = tree.on_click(id) {
                            state::invoke_click(cb_id, ctx);
                        }
                    }
                    crate::event::EventPhase::Bubble => {
                        if let Some(cb) = tree.get_node_ref(id).and_then(|n| n.listener()) {
                            if let ClickCallbackRef::Simple(_) = cb {
                                state::invoke_click(cb.id(), ctx);
                            }
                        }
                    }
                }
            }
            _ => {}
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
                    set_window_size(s.width, s.height);
                    self.renderer.resize(s.width as u16, s.height as u16);
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = Point::new(position.x as f32, position.y as f32);
                let hit = self.build_hit_result();
                let effects = {
                    let tree = &self.runtime.layers.tree;
                    let mut em = self.runtime.layers.event_manager.borrow_mut();
                    em.handle_mouse_move(self.mouse_pos, hit.as_ref(), tree, |id, event, ctx| {
                        Self::handle_lie_event(tree, id, event, ctx)
                    })
                };
                // 仅当 hover/capture 真正变化时才重绘，避免鼠标在静态 UI 上
                // 移动也每帧全量重建渲染树（debug 下严重卡顿主因）。
                let needs_redraw = effects.needs_render();
                self.apply_event_effects(effects);
                if needs_redraw {
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => {
                if let Some(hit) = self.build_hit_result() {
                    let btn = if button == winit::event::MouseButton::Left {
                        MouseButton::Left
                    } else if button == winit::event::MouseButton::Right {
                        MouseButton::Right
                    } else {
                        MouseButton::Middle
                    };
                    let effects = {
                        let tree = &self.runtime.layers.tree;
                        let mut em = self.runtime.layers.event_manager.borrow_mut();
                        em.handle_mouse_down(self.mouse_pos, btn, &hit, tree, |id, event, ctx| {
                            Self::handle_lie_event(tree, id, event, ctx)
                        })
                    };
                    self.apply_event_effects(effects);
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button,
                ..
            } => {
                if let Some(hit) = self.build_hit_result() {
                    let btn = if button == winit::event::MouseButton::Left {
                        MouseButton::Left
                    } else {
                        MouseButton::Right
                    };
                    let effects = {
                        let tree = &self.runtime.layers.tree;
                        let mut em = self.runtime.layers.event_manager.borrow_mut();
                        em.handle_mouse_up(self.mouse_pos, btn, &hit, tree, |id, event, ctx| {
                            Self::handle_lie_event(tree, id, event, ctx)
                        })
                    };
                    self.apply_event_effects(effects);
                }
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
                    self.render_visuals();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        // 允许应用代码在事件循环空闲时通过 `state::request_redraw()` 触发重绘
        // （动画、计时器、外部线程修改等场景）
        if state::take_redraw_requested() {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}
