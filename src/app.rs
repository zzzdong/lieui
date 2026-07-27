//! Application — 基于 winit 的窗口化 GUI 应用

use std::cell::RefCell;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, Ime, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::{Window, WindowId};

use crate::event::{EventEffects, HitTestResult, MouseButton};
use crate::geometry::{Point, Size};
use crate::render::renderer::Renderer as _;
use crate::render::VelloRenderer;
use crate::runtime::Runtime;
use crate::state;
use crate::widget::{BuildContext, StateMap, Widget};

/// 当前正在进行的 ScrollView 拖拽滚动状态。
#[derive(Debug, Clone, Copy)]
struct ScrollDragState {
    container_id: crate::core::ElementId,
    mode: ScrollDragMode,
    last_pos: Point,
    /// 滚动条模式下的几何参数
    track_length: f32,
    thumb_size: f32,
    content_length: f32,
    viewport_length: f32,
}

#[derive(Debug, Clone, Copy)]
enum ScrollDragMode {
    /// 按住内容区域拖拽（内容跟随手指/光标移动）。
    Content,
    /// 按住滚动条 thumb 拖拽。
    ScrollbarThumb,
}

pub struct Application<B: Fn(&mut BuildContext) -> Box<dyn Widget>> {
    builder: B,
    runtime: Runtime,
    state: Rc<RefCell<StateMap>>,
    renderer: VelloRenderer,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    window_size: (u32, u32),
    mouse_pos: Point,
    modifiers: winit::keyboard::ModifiersState,
    rendered_once: bool,
    /// 上一次设置给窗口的 IME 状态，用于避免每帧重复调用。
    last_ime_allowed: bool,
    last_ime_cursor: Option<crate::geometry::Rect>,
    /// 启动时需注册到排版引擎的自定义字体文件。
    fonts: Vec<PathBuf>,
    /// ScrollView 拖拽滚动状态。按住左键在可滚动容器内拖拽时生效。
    scroll_drag: Option<ScrollDragState>,
}

static WINDOW_SIZE: AtomicU64 = AtomicU64::new(0);

fn pack_size(w: u32, h: u32) -> u64 {
    ((w as u64) << 32) | (h as u64)
}

fn unpack_size(v: u64) -> (u32, u32) {
    ((v >> 32) as u32, v as u32)
}

fn map_winit_key(key: &WinitKey) -> crate::event::Key {
    use crate::event::Key;
    match key {
        WinitKey::Character(s) => s.chars().next().map(Key::Character).unwrap_or(Key::Unknown),
        WinitKey::Named(named) => match named {
            NamedKey::Enter => Key::Enter,
            NamedKey::Escape => Key::Escape,
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Delete => Key::Delete,
            NamedKey::Tab => Key::Tab,
            NamedKey::Space => Key::Space,
            NamedKey::Home => Key::Home,
            NamedKey::End => Key::End,
            NamedKey::PageUp => Key::PageUp,
            NamedKey::PageDown => Key::PageDown,
            NamedKey::ArrowUp => Key::ArrowUp,
            NamedKey::ArrowDown => Key::ArrowDown,
            NamedKey::ArrowLeft => Key::ArrowLeft,
            NamedKey::ArrowRight => Key::ArrowRight,
            NamedKey::Shift => Key::Shift,
            NamedKey::Control => Key::Ctrl,
            NamedKey::Alt => Key::Alt,
            NamedKey::Meta => Key::Meta,
            _ => Key::Unknown,
        },
        _ => Key::Unknown,
    }
}

fn map_winit_modifiers(m: winit::keyboard::ModifiersState) -> crate::event::Modifiers {
    crate::event::Modifiers {
        shift: m.shift_key(),
        ctrl: m.control_key(),
        alt: m.alt_key(),
        meta: m.super_key(),
    }
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
            modifiers: winit::keyboard::ModifiersState::empty(),
            rendered_once: false,
            last_ime_allowed: false,
            last_ime_cursor: None,
            fonts: Vec::new(),
            scroll_drag: None,
        }
    }

    /// 注册一个自定义字体文件，应用启动（`run`）时加载到排版引擎，
    /// 之后即可在 `TextStyle::font_family` 中使用该字体的 family 名。
    pub fn with_font<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        self.fonts.push(path.as_ref().to_path_buf());
        self
    }

    pub fn run(mut self) {
        // 在事件循环启动前把自定义字体注册进 parley，确保首次排版即可用。
        for f in &self.fonts {
            let families = crate::text::register_font_file(f);
            if !families.is_empty() {
                eprintln!("[lieui] 已注册字体 {:?} -> {:?}", f, families);
            }
        }
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

        let span = crate::perf::Span::start("builder");
        let mut ctx = BuildContext::new(Rc::clone(&self.state));
        let root_widget = (self.builder)(&mut ctx);
        let view_tree = root_widget.build(&mut ctx);
        span.finish();
        let span = crate::perf::Span::start("submit");
        self.runtime.submit_view_tree(view_tree, rebuild_requested);
        span.finish();
        let elements = self.runtime.frame();

        let span = crate::perf::Span::start("raster");
        let pixmap = self.renderer.render(&elements);
        span.finish();
        let span = crate::perf::Span::start("blit");
        self.blit_to_window(pixmap.data());
        span.finish();
        self.update_ime_state();
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
        self.update_ime_state();
    }

    /// 根据当前焦点与布局结果更新 IME 的启用状态及候选窗位置。
    /// 缓存上一次的值，避免每帧重复调用系统 IME API（Windows 上可能引发卡顿）。
    fn update_ime_state(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let em = self.runtime.layers.event_manager.borrow();
        let allowed = em.focused_ime();
        let focused = em.focused();
        let caret_area = focused.map(|id| {
            let tree = &self.runtime.layers.tree;
            let caret_id = tree.find_child_by_key(id, "__ime_caret__");
            caret_id.map_or_else(|| tree.layout(id).rect(), |cid| tree.layout(cid).rect())
        });
        drop(em);

        if self.last_ime_allowed != allowed {
            window.set_ime_allowed(allowed);
            self.last_ime_allowed = allowed;
        }

        if !allowed {
            if self.last_ime_cursor.is_some() {
                self.last_ime_cursor = None;
            }
            return;
        }

        let Some(area) = caret_area else {
            return;
        };
        if self.last_ime_cursor != Some(area) {
            window.set_ime_cursor_area(
                LogicalPosition::new(area.x as f64, area.y as f64),
                LogicalSize::new(area.width as f64, area.height as f64),
            );
            self.last_ime_cursor = Some(area);
        }
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
        // 用切片迭代代替逐下标索引，避免每像素边界检查（debug 下差距显著）。
        let dst = &mut buf[..buf_len];
        for (d, s) in dst[..copy_len].iter_mut().zip(&data[..copy_len]) {
            *d = pack_softbuffer_pixel(*s);
        }
        const CLEAR: u32 = 0x00F0F0F0;
        dst[copy_len..].fill(CLEAR);
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

    // ========== ScrollView 拖拽/滚动条辅助 ==========

    /// 在命中路径中向上查找最近的 overflow_scroll 容器。
    fn find_scroll_container(&self, hit: &HitTestResult) -> Option<crate::core::ElementId> {
        for &id in hit.path.iter().rev() {
            if let Some(node) = self.runtime.layers.tree.get_node_ref(id) {
                if node.layout().overflow_scroll {
                    return Some(id);
                }
            }
        }
        None
    }

    /// 节点是否注册了 MouseDown 监听器。
    fn has_mouse_down_listener(
        tree: &crate::runtime::element::ElementTree,
        id: crate::core::ElementId,
    ) -> bool {
        tree.listeners(id)
            .iter()
            .any(|l| l.event == crate::event::EventType::MouseDown)
    }

    /// 节点是否注册了 Click 监听器。
    fn has_click_listener(
        tree: &crate::runtime::element::ElementTree,
        id: crate::core::ElementId,
    ) -> bool {
        tree.listeners(id)
            .iter()
            .any(|l| l.event == crate::event::EventType::Click)
    }

    /// 计算垂直滚动条轨道矩形（窗口坐标）。当前引擎只渲染垂直滚动条。
    fn scrollbar_track_rect(
        tree: &crate::runtime::element::ElementTree,
        id: crate::core::ElementId,
    ) -> Option<crate::geometry::Rect> {
        let node = tree.get_node_ref(id)?;
        if !node.layout().show_scrollbar {
            return None;
        }
        let layout = tree.layout(id);
        if !layout.overflow_scroll {
            return None;
        }
        let sw = 8.0f32;
        if layout.width < sw || layout.height <= 0.0 {
            return None;
        }
        Some(crate::geometry::Rect::new(
            layout.x + layout.width - sw,
            layout.y,
            sw,
            layout.height,
        ))
    }

    /// 计算垂直滚动条 thumb 的 y 偏移与高度。
    fn scrollbar_thumb_info(
        tree: &crate::runtime::element::ElementTree,
        id: crate::core::ElementId,
    ) -> Option<(f32, f32)> {
        let track = Self::scrollbar_track_rect(tree, id)?;
        let (_, content_h) = tree.content_size(id);
        let viewport_h = tree.layout(id).height;
        if content_h <= viewport_h || viewport_h <= 0.0 {
            return None;
        }
        let (_, scroll_y) = tree.scroll_offset(id);
        let max_scroll = (content_h - viewport_h).max(0.0);
        let thumb_ratio = (viewport_h / content_h).clamp(0.02, 1.0);
        let thumb_size = (track.height * thumb_ratio).max(8.0);
        let thumb_offset = if max_scroll > 0.0 {
            (scroll_y / max_scroll) * (track.height - thumb_size)
        } else {
            0.0
        };
        Some((track.y + thumb_offset, thumb_size))
    }

    /// 在鼠标按下时尝试启动 ScrollView 拖拽滚动或滚动条交互。
    fn try_start_scroll_drag(
        &mut self,
        hit: &HitTestResult,
        button: MouseButton,
        effects: &crate::event::EventEffects,
    ) {
        if button != MouseButton::Left {
            return;
        }
        if effects.propagation_stopped() {
            return;
        }
        let Some(container_id) = self.find_scroll_container(hit) else {
            return;
        };

        let tree = &self.runtime.layers.tree;
        let point = self.mouse_pos;

        // 优先判断是否在引擎渲染的垂直滚动条上。
        if let Some(track) = Self::scrollbar_track_rect(tree, container_id) {
            if track.contains(point) {
                if let Some((thumb_y, thumb_size)) = Self::scrollbar_thumb_info(tree, container_id)
                {
                    let on_thumb = point.y >= thumb_y && point.y < thumb_y + thumb_size;
                    if on_thumb {
                        self.scroll_drag = Some(ScrollDragState {
                            container_id,
                            mode: ScrollDragMode::ScrollbarThumb,
                            last_pos: point,
                            track_length: track.height,
                            thumb_size,
                            content_length: tree.content_size(container_id).1,
                            viewport_length: tree.layout(container_id).height,
                        });
                        self.runtime
                            .layers
                            .event_manager
                            .borrow_mut()
                            .set_mouse_capture(Some(container_id));
                    } else {
                        // 点击轨道空白处：按点击比例跳转。
                        let ratio = ((point.y - track.y) / track.height).clamp(0.0, 1.0);
                        let viewport_h = tree.layout(container_id).height;
                        let content_h = tree.content_size(container_id).1;
                        let max_scroll = (content_h - viewport_h).max(0.0);
                        let new_y = ratio * max_scroll;
                        let (ox, _) = tree.scroll_offset(container_id);
                        self.runtime.scroll_to(container_id, ox, new_y);
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
                return;
            }
        }

        // 内容区拖拽：路径上滚动容器之后的后代节点若注册了 MouseDown 或 Click，
        // 说明子控件需要接管鼠标（Input 选取、Slider 拖柄、Checkbox/Button 等），
        // 此时不启动滚动拖拽，避免捕获导致子控件的 Click 事件无法生成。
        let container_idx = hit
            .path
            .iter()
            .position(|&id| id == container_id)
            .unwrap_or(hit.path.len());
        let descendant_interactive = hit.path.iter().skip(container_idx + 1).any(|&id| {
            Self::has_mouse_down_listener(tree, id) || Self::has_click_listener(tree, id)
        });
        if descendant_interactive {
            return;
        }

        self.scroll_drag = Some(ScrollDragState {
            container_id,
            mode: ScrollDragMode::Content,
            last_pos: point,
            track_length: 0.0,
            thumb_size: 0.0,
            content_length: 0.0,
            viewport_length: 0.0,
        });
        self.runtime
            .layers
            .event_manager
            .borrow_mut()
            .set_mouse_capture(Some(container_id));
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// 处理拖拽滚动中的鼠标移动。
    fn handle_scroll_drag_move(&mut self, point: Point) {
        let Some(mut drag) = self.scroll_drag else {
            return;
        };
        let delta_x = point.x - drag.last_pos.x;
        let delta_y = point.y - drag.last_pos.y;
        if delta_x == 0.0 && delta_y == 0.0 {
            return;
        }
        drag.last_pos = point;
        match drag.mode {
            ScrollDragMode::Content => {
                // 自然滚动：手指/光标向下拖，内容向上滚动。
                self.runtime
                    .scroll_by(drag.container_id, -delta_x, -delta_y);
            }
            ScrollDragMode::ScrollbarThumb => {
                let max_scroll = (drag.content_length - drag.viewport_length).max(0.0);
                let available = (drag.track_length - drag.thumb_size).max(1.0);
                if max_scroll > 0.0 {
                    let scale = max_scroll / available;
                    self.runtime
                        .scroll_by(drag.container_id, 0.0, delta_y * scale);
                }
            }
        }
        self.scroll_drag = Some(drag);
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// 结束拖拽滚动。
    fn end_scroll_drag(&mut self) {
        if self.scroll_drag.is_some() {
            self.scroll_drag = None;
            self.runtime
                .layers
                .event_manager
                .borrow_mut()
                .set_mouse_capture(None);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
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
        ctx.set_event(event.clone());
        // 注入当前节点及其布局矩形，供回调换算局部坐标/请求鼠标捕获。
        ctx.set_current(id, tree.layout(id).rect());

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
            crate::event::Event::ImePreedit { .. } => ET::ImePreedit,
            crate::event::Event::ImeCommit { .. } => ET::ImeCommit,
            crate::event::Event::ImeDisabled => ET::ImeDisabled,
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

                // ScrollView 拖拽滚动优先：捕获期间不再走普通 hover 路径。
                if self.scroll_drag.is_some() {
                    self.handle_scroll_drag_move(self.mouse_pos);
                    return;
                }

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
                        let modifiers = map_winit_modifiers(self.modifiers);
                        let mut em = self.runtime.layers.event_manager.borrow_mut();
                        em.handle_mouse_down(
                            self.mouse_pos,
                            btn,
                            modifiers,
                            &hit,
                            tree,
                            |id, event, ctx| Self::handle_lie_event(tree, id, event, ctx),
                        )
                    };
                    self.apply_event_effects(effects);

                    // 在普通 widget 处理之后尝试启动 ScrollView 拖拽滚动/滚动条交互。
                    // 若子控件已截断事件或注册了 MouseDown，则让子控件优先处理。
                    self.try_start_scroll_drag(&hit, btn, &effects);
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
                // 左键释放时结束 ScrollView 拖拽滚动。注意保持捕获直到 handle_mouse_up
                // 完成，以抑制拖拽期间子控件被误触发的 Click 事件。
                if button == winit::event::MouseButton::Left {
                    self.end_scroll_drag();
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // winit 的 y 方向：macOS 默认自然（手指下移→负）、Windows 默认传统（滚轮下滚→正）。
                // 在非 macOS 上反转 dy 以统一为自然滚动：手指/滚轮方向 = 内容移动方向。
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => (x * 16.0, y * 16.0),
                    winit::event::MouseScrollDelta::PixelDelta(p) => (p.x as f32, p.y as f32),
                };
                // 非 macOS 反转 dy，统一为自然滚动（macOS 的自然方向由系统 / winit 处理）
                let dy = if cfg!(target_os = "macos") { dy } else { -dy };
                let hit = self.build_hit_result();
                if let Some(hit) = &hit {
                    let point = self.mouse_pos;
                    let effects = {
                        let tree = &self.runtime.layers.tree;
                        let mut em = self.runtime.layers.event_manager.borrow_mut();
                        em.handle_wheel(point, dx, dy, hit, |id, event, ctx| {
                            Self::handle_lie_event(tree, id, event, ctx)
                        })
                    };
                    self.apply_event_effects(effects);
                    // 引擎层滚动：命中目标向上最近的 overflow_scroll 容器处理滚轮。
                    self.runtime.handle_wheel_scroll(hit, dx, dy);
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                self.modifiers = m.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let key = map_winit_key(&event.logical_key);
                let modifiers = map_winit_modifiers(self.modifiers);
                let effects = {
                    let tree = &self.runtime.layers.tree;
                    let mut em = self.runtime.layers.event_manager.borrow_mut();
                    match event.state {
                        ElementState::Pressed => {
                            em.handle_key_down(key, modifiers, |id, ev, ctx| {
                                Self::handle_lie_event(tree, id, ev, ctx)
                            })
                        }
                        ElementState::Released => {
                            em.handle_key_up(key, modifiers, |id, ev, ctx| {
                                Self::handle_lie_event(tree, id, ev, ctx)
                            })
                        }
                    }
                };
                self.apply_event_effects(effects);
                if effects.needs_render() || effects.needs_rebuild() {
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }
            WindowEvent::Ime(ime) => {
                let effects = {
                    let tree = &self.runtime.layers.tree;
                    let mut em = self.runtime.layers.event_manager.borrow_mut();
                    match ime {
                        Ime::Preedit(text, cursor) => {
                            let (start, end) =
                                cursor.map_or((None, None), |(s, e)| (Some(s), Some(e)));
                            em.handle_ime_preedit(text, start, end, |id, ev, ctx| {
                                Self::handle_lie_event(tree, id, ev, ctx)
                            })
                        }
                        Ime::Commit(text) => em.handle_ime_commit(text, |id, ev, ctx| {
                            Self::handle_lie_event(tree, id, ev, ctx)
                        }),
                        Ime::Disabled => em.handle_ime_disabled(|id, ev, ctx| {
                            Self::handle_lie_event(tree, id, ev, ctx)
                        }),
                        Ime::Enabled => EventEffects::default(),
                    }
                };
                self.apply_event_effects(effects);
                if effects.needs_render() || effects.needs_rebuild() {
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
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

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // 推进到期动画；活跃时让循环休眠到下一次触发（避免忙等）。
        let (fired, next) = crate::animation::tick(Instant::now());
        if fired {
            state::request_redraw();
        }
        match next {
            Some(t) => el.set_control_flow(ControlFlow::WaitUntil(t)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
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
