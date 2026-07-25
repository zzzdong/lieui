//! Application — 基于 winit 的窗口化 GUI 应用

use std::cell::RefCell;
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
use crate::widget::{BuildContext, StateMap, Widget};

pub struct Application<B: Fn(&mut BuildContext) -> Box<dyn Widget>> {
    builder: B,
    runtime: Runtime,
    state: Rc<RefCell<StateMap>>,
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

/// 将 `PremulRgba8` 打包为 softbuffer 所需的 `0RGB` u32 像素。
/// softbuffer 0.4 要求每个 u32 的最高 8 位为 0，其余按 R、G、B 从高到低排列。
pub(crate) fn pack_softbuffer_pixel(p: vello_cpu::color::PremulRgba8) -> u32 {
    (p.b as u32) | ((p.g as u32) << 8) | ((p.r as u32) << 16)
}

pub fn set_window_size(w: u32, h: u32) {
    WINDOW_SIZE.store(pack_size(w, h), Ordering::Relaxed);
}

/// 获取当前窗口大小。如果尚未初始化，返回 Size::zero()。
pub fn window_size() -> Size {
    let (w, h) = unpack_size(WINDOW_SIZE.load(Ordering::Relaxed));
    Size::new(w as f32, h as f32)
}

impl<B: Fn(&mut BuildContext) -> Box<dyn Widget> + 'static> Application<B> {
    pub fn new(builder: B, viewport: Size) -> Self {
        set_window_size(viewport.width as u32, viewport.height as u32);
        Self {
            builder,
            runtime: Runtime::new(viewport),
            state: Rc::new(RefCell::new(StateMap::new())),
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

    fn build_and_render(&mut self, viewport_changed: bool, rebuild_requested: bool) {
        let pw = self.renderer.width();
        let ph = self.renderer.height();
        if pw == 0 || ph == 0 {
            return;
        }

        if viewport_changed {
            let (ww, wh) = self.window_size;
            self.runtime.set_viewport(Size::new(ww as f32, wh as f32));
        }

        let mut ctx = BuildContext::new(Rc::clone(&self.state));
        let root_widget = (self.builder)(&mut ctx);
        let view_tree = root_widget.build(&mut ctx);
        self.runtime.submit_view_tree(view_tree, rebuild_requested);
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
            buf[i] = pack_softbuffer_pixel(data[i]);
        }
        const CLEAR: u32 = 0x00F0F0F0;
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

    /// 通用事件回调：直接调用节点上附着的 `ViewListener`。
    ///
    /// HTML 式事件模型：任何挂有 listener 的节点都参与分发，事件按捕获 → 目标 → 冒泡
    /// 顺序传播，每层 listener 自行决定是否调用 `ctx.stop_propagation()`。
    ///
    /// 阶段语义：
    /// - 捕获 Capture：只有 `Callback::WithCtx` 触发，使父节点可在到达 target 前拦截。
    /// - 目标 Target：`Simple` 和 `WithCtx` 都触发。`Simple` 自动 `stop_propagation()`。
    /// - 冒泡 Bubble：只有 `Callback::Simple` 触发，`WithCtx` 已在捕获阶段处理过。
    ///
    /// 只处理关注的事件类型，按 `Listener.event` 类型匹配分派。
    fn handle_lie_event(
        tree: &crate::runtime::element::ElementTree,
        id: crate::core::ElementId,
        event: &crate::event::Event,
        ctx: &mut crate::event::EventContext,
    ) {
        // 提取事件类型，非关注的事件跳过。
        use crate::event::EventType as ET;
        let event_type: ET = match event {
            crate::event::Event::Click { .. } => ET::Click,
            crate::event::Event::MouseDown { .. } => ET::MouseDown,
            crate::event::Event::MouseUp { .. } => ET::MouseUp,
            crate::event::Event::MouseMove { .. } => ET::MouseMove,
            crate::event::Event::MouseWheel { .. } => ET::MouseWheel,
            crate::event::Event::MouseEnter => ET::MouseEnter,
            crate::event::Event::MouseLeave => ET::MouseLeave,
            crate::event::Event::KeyDown { .. } => ET::KeyDown,
            crate::event::Event::KeyUp { .. } => ET::KeyUp,
            crate::event::Event::FocusIn => ET::FocusIn,
            crate::event::Event::FocusOut => ET::FocusOut,
            _ => return,
        };
        let Some(node) = tree.get_node_ref(id) else {
            return;
        };
        for listener in node.listeners() {
            if listener.event != event_type {
                continue;
            }
            match ctx.phase() {
                crate::event::EventPhase::Capture => {
                    if let crate::view::node::Callback::WithCtx(cb) = &listener.callback {
                        cb(ctx);
                    }
                }
                crate::event::EventPhase::Target => match &listener.callback {
                    crate::view::node::Callback::Simple(cb) => {
                        cb();
                        ctx.stop_propagation();
                    }
                    crate::view::node::Callback::WithCtx(cb) => {
                        cb(ctx);
                    }
                },
                crate::event::EventPhase::Bubble => {
                    if let crate::view::node::Callback::Simple(cb) = &listener.callback {
                        cb();
                    }
                }
            }
            if ctx.is_stopped() {
                break;
            }
        }
    }
}

impl<B: Fn(&mut BuildContext) -> Box<dyn Widget> + 'static> ApplicationHandler for Application<B> {
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
                let rebuild_requested = state::take_rebuild_requested();
                if !self.rendered_once
                    || self.surface.is_none()
                    || rebuild_requested
                    || size_mismatch
                {
                    self.build_and_render(size_mismatch, rebuild_requested);
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

#[cfg(test)]
mod tests {
    use super::pack_softbuffer_pixel;
    use vello_cpu::color::PremulRgba8;

    #[test]
    fn pack_softbuffer_obeys_0rgb_format() {
        // softbuffer 0.4 要求 u32 像素格式为 0x00RRGGBB（小端内存中按 B、G、R、0 排列）
        let red = PremulRgba8::from_u8_array([255, 0, 0, 255]);
        assert_eq!(pack_softbuffer_pixel(red), 0x00FF0000);

        let green = PremulRgba8::from_u8_array([0, 255, 0, 255]);
        assert_eq!(pack_softbuffer_pixel(green), 0x0000FF00);

        let blue = PremulRgba8::from_u8_array([0, 0, 255, 255]);
        assert_eq!(pack_softbuffer_pixel(blue), 0x000000FF);

        let white = PremulRgba8::from_u8_array([255, 255, 255, 255]);
        assert_eq!(pack_softbuffer_pixel(white), 0x00FFFFFF);

        // 最高 8 位必须为 0，避免把 alpha 错误地写入颜色通道
        let semi = PremulRgba8::from_u8_array([128, 64, 32, 128]);
        assert_eq!(pack_softbuffer_pixel(semi), 0x00804020);
    }
}
