//! Application — 基于 winit 的多窗口 GUI 应用

use std::cell::RefCell;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, Ime, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::{Window, WindowId, WindowLevel};

use crate::event::{EventEffects, HitTestResult, MouseButton};
use crate::geometry::{Point, Size};
use crate::render::VelloRenderer;
use crate::render::renderer::Renderer as _;
use crate::runtime::Runtime;
use crate::state;
use crate::widget::{BuildContext, StateMap, Widget};
use crate::window::WindowConfig;

// ============================================================================
// 关闭守卫
// ============================================================================

/// 关闭守卫的返回结果，只表达"是否允许现在关闭"，不含任何 UI 语义。
///
/// 关闭守卫是「关闭前的一个操作钩子」，由 `Application` 在窗口收到关闭请求时调用。
/// 它**只应做判定**，绝不应该决定 UI：弹窗、保存对话框等交互完全由集成方自己实现。
///
/// - 返回 [`CloseAction::Allow`]：窗口立即关闭。
/// - 返回 [`CloseAction::Cancel`]：本次关闭被拦截。集成方通常会在此自行弹出一个
///   确认对话框；当用户最终确认退出时，调用守卫回调中提供的「请求真正关闭」函数
///   （见 [`Application::close_guard`]）即可触发真实关闭，而该调用不会再经过守卫。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// 允许立即关闭窗口。
    Allow,
    /// 取消本次关闭。窗口保持打开，由集成方自行处理后续交互。
    Cancel,
}

// ============================================================================
// 内部类型
// ============================================================================

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

/// 窗口 widget 构建函数类型。
type WidgetBuilder = Box<dyn Fn(&mut BuildContext) -> Box<dyn Widget>>;

/// 关闭守卫类型。
///
/// 第一参数为当前窗口的 `Runtime`，集成方可用它做任何关闭前操作（如弹窗）。
/// 第二参数是一个「请求真正关闭」的回调：集成方在自行弹出的确认对话框被用户
/// 确认后调用它即可关闭窗口；该调用绕过守卫，不会再次触发 `CloseAction::Cancel`。
type CloseGuard = Box<dyn Fn(&Runtime, &dyn Fn()) -> CloseAction>;

/// 窗口规格：配置 + 构建函数 + 关闭守卫，在 `Application::new` / `.window()` 时收集。
struct WindowSpec {
    config: WindowConfig,
    builder: WidgetBuilder,
    close_guard: Option<CloseGuard>,
    /// 是否启用 Inspector（CDP，仅 feature `inspector` 生效）
    #[cfg(feature = "inspector")]
    inspector: bool,
}

/// 每个窗口的运行时上下文。
struct WindowContext {
    wid: winit::window::WindowId,
    runtime: Runtime,
    renderer: VelloRenderer,
    /// 进程内脏区合屏器：持有一份与窗口同尺寸的 backing 缓冲，
    /// 负责把 SharedSurface 的脏区合成到 UI 像素之上，支持部分上屏。
    compositor: crate::render::Compositor,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    window_size: (u32, u32),
    mouse_pos: Point,
    modifiers: winit::keyboard::ModifiersState,
    rendered_once: bool,
    /// 上一次设置给窗口的 IME 状态，用于避免每帧重复调用。
    last_ime_allowed: bool,
    last_ime_cursor: Option<crate::geometry::Rect>,
    /// ScrollView 拖拽滚动状态。按住左键在可滚动容器内拖拽时生效。
    scroll_drag: Option<ScrollDragState>,
    /// 该窗口的 widget 构建函数。
    builder: WidgetBuilder,
    /// 关闭守卫：窗口关闭请求时调用，决定是允许关闭还是由集成方自行处理。
    close_guard: Option<CloseGuard>,
    /// 该窗口独立的 UI 状态表，不与其他窗口共享（多窗口状态隔离）。
    state: Rc<RefCell<StateMap>>,
    /// Inspector 句柄（feature `inspector` 时 Some）
    #[cfg(feature = "inspector")]
    inspector: Option<crate::inspector::Inspector>,
}

// ============================================================================
// 工具函数
// ============================================================================

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
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn pack_softbuffer_pixel(p: vello_cpu::color::PremulRgba8) -> u32 {
    (p.b as u32) | ((p.g as u32) << 8) | ((p.r as u32) << 16)
}

// ============================================================================
// Application
// ============================================================================

pub struct Application {
    specs: Vec<WindowSpec>,
    windows: HashMap<WindowId, WindowContext>,
    /// 外部消息源：跨线程把事件投递进 UI。事件循环被唤醒后逐个 poll。
    external_sources: Vec<Box<dyn crate::external::ExternalSource>>,
    /// `ExternalEvent::Data` 的处理回调（UI 线程执行，可自由使用 Rc 状态）。
    external_data_handler: Option<Box<dyn FnMut(Box<dyn std::any::Any + Send>)>>,
    /// 窗口尺寸变化回调（UI 线程执行）。参数：(窗口逻辑尺寸宽, 高)。
    resize_handler: Option<Box<dyn FnMut(u32, u32)>>,
}

impl Application {
    /// 创建一个单窗口 / 多窗口应用。
    ///
    /// 第一个窗口通过 `new` 指定，后续窗口通过 `.window()` 追加。
    ///
    /// ```ignore
    /// Application::new(
    ///     WindowConfig::new().title("主窗口").size(900, 720),
    ///     |ctx| Box::new(my_widget()),
    /// )
    /// .window(
    ///     WindowConfig::new().title("工具窗口").size(300, 400),
    ///     |ctx| Box::new(tools_widget()),
    /// )
    /// .run();
    /// ```
    pub fn new(
        config: WindowConfig,
        builder: impl Fn(&mut BuildContext) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self {
            specs: vec![WindowSpec {
                config,
                builder: Box::new(builder),
                close_guard: None,
                #[cfg(feature = "inspector")]
                inspector: false,
            }],
            windows: HashMap::new(),
            external_sources: Vec::new(),
            external_data_handler: None,
            resize_handler: None,
        }
    }

    /// 注册一个外部事件源。
    ///
    /// 事件循环每次被 [`crate::external::wake`] 唤醒后，会对所有已注册的外部源
    /// 调用 `poll`，把外部事件（`rebuild`/`redraw`/`data`）分发到 UI 线程。
    /// 常用于：terminal 的 PTY/SSH 读线程把数据投递进 UI。
    pub fn external_source(mut self, source: Box<dyn crate::external::ExternalSource>) -> Self {
        self.external_sources.push(source);
        self
    }

    /// 注册 `ExternalEvent::Data` 的处理回调。
    ///
    /// 回调在 **UI 线程** 执行，可以自由访问 Rc/RefCell 状态。
    /// 例如 terminal 收到后端数据后，在此回调里 advance 内核并渲染进 SharedSurface。
    pub fn external_data_handler<F: FnMut(Box<dyn std::any::Any + Send>) + 'static>(
        mut self,
        handler: F,
    ) -> Self {
        self.external_data_handler = Some(Box::new(handler));
        self
    }

    /// 注册窗口尺寸变化回调（UI 线程执行）。参数为窗口逻辑尺寸 `(宽, 高)`。
    ///
    /// 例如 terminal 在这里感知窗口尺寸变化，重新计算 cols×rows、resize 内核/后端，
    /// 并重建 SharedSurface。
    pub fn on_resize<F: FnMut(u32, u32) + 'static>(mut self, handler: F) -> Self {
        self.resize_handler = Some(Box::new(handler));
        self
    }

    /// 追加一个窗口（多窗口支持）。返回 `self` 以便链式调用。
    pub fn window(
        mut self,
        config: WindowConfig,
        builder: impl Fn(&mut BuildContext) -> Box<dyn Widget> + 'static,
    ) -> Self {
        self.specs.push(WindowSpec {
            config,
            builder: Box::new(builder),
            close_guard: None,
            #[cfg(feature = "inspector")]
            inspector: false,
        });
        self
    }

    /// 为最近添加的窗口启用 Inspector（通过 Chrome DevTools 查看 Widget 树）。
    ///
    /// 需要 feature `inspector` 编译；非该 feature 下为 no-op。
    #[cfg(feature = "inspector")]
    pub fn inspector(mut self, enabled: bool) -> Self {
        if let Some(spec) = self.specs.last_mut() {
            spec.inspector = enabled;
            if enabled {
                eprintln!(
                    "[lieui-inspector] enabled for window; open the printed DevTools URL in Chrome"
                );
            }
        }
        self
    }

    /// 为最近添加的窗口设置关闭守卫。
    ///
    /// 关闭守卫是一个「关闭前的操作钩子」，在窗口收到关闭请求时被调用。它只应做
    /// *判定*，绝不应该决定 UI——弹窗、保存对话框等交互完全由集成方自己实现。
    ///
    /// 守卫回调接收两个参数：
    /// - `&Runtime`：当前窗口运行时，集成方可用它读取状态、自行 `push` 一个确认
    ///   对话框（Modal 层）等。
    /// - `&dyn Fn()`：一个「请求真正关闭」的回调。当集成方自行弹出的对话框被用户
    ///   确认退出时，调用它即可真正关闭窗口；该调用**绕过守卫**，不会再触发
    ///   `CloseAction::Cancel`，因此不会造成重复弹窗。
    ///
    /// 守卫返回一个 [`CloseAction`]：
    /// - [`CloseAction::Allow`]：立即关闭窗口（集成方无需做任何事）。
    /// - [`CloseAction::Cancel`]：本次关闭被拦截，窗口保持打开。集成方通常会在此时
    ///   自行弹出一个确认对话框，并在用户确认后调用上述「请求真正关闭」回调。
    ///
    /// ```ignore
    /// Application::new(config, builder)
    ///     .close_guard(|rt, request_close| {
    ///         if has_unsaved_changes(rt) {
    ///             // 集成方自己实现弹窗，确认按钮的 on_click 中调用 request_close()
    ///             show_my_confirm_dialog(rt, request_close);
    ///             CloseAction::Cancel
    ///         } else {
    ///             CloseAction::Allow
    ///         }
    ///     })
    ///     .run();
    /// ```
    pub fn close_guard(
        mut self,
        guard: impl Fn(&Runtime, &dyn Fn()) -> CloseAction + 'static,
    ) -> Self {
        if let Some(spec) = self.specs.last_mut() {
            spec.close_guard = Some(Box::new(guard));
        }
        self
    }

    pub fn run(mut self) {
        let el: EventLoop<()> = EventLoop::new().unwrap();
        el.set_control_flow(ControlFlow::Wait);
        // 注册跨线程唤醒代理，供外部线程（ExternalSource 的投递侧）唤醒事件循环。
        crate::external::set_proxy(el.create_proxy());
        let _ = el.run_app(&mut self);
    }

    /// 真正关闭一个窗口（绕过关闭守卫），并清理事件循环状态。
    fn close_window(&mut self, wid: &WindowId, el: &ActiveEventLoop) {
        self.windows.remove(wid);
        // 注销该窗口的标志，避免泄漏
        state::unregister_window(*wid);
        // 清理该窗口在 EventManager 中的焦点/捕获状态
        if self.windows.is_empty() {
            el.exit();
        }
    }

    /// 调用关闭守卫，决定是否允许关闭当前窗口。
    ///
    /// 返回 `true` 表示允许立即关闭（守卫返回 [`CloseAction::Allow`] 或没有守卫）；
    /// 返回 `false` 表示本次关闭被守卫拦截，窗口保持打开，由集成方自行处理后续
    /// 交互（如弹出确认对话框），并在合适时机调用 [`Runtime::request_close`] 真正关闭。
    ///
    /// 守卫回调的第二个参数为「请求真正关闭」回调：集成方在自行弹出的确认对话框被
    /// 用户确认后调用它即可关闭窗口，该调用绕过守卫，不会再次触发拦截。
    fn handle_close_request(&mut self, wid: &WindowId) -> bool {
        let ctx = self.windows.get_mut(wid).expect("window exists");
        // 构造「请求真正关闭」回调： bypass 关闭守卫直接关闭窗口。
        let request_close = || state::request_window_close();
        let action = ctx
            .close_guard
            .as_ref()
            .map_or(CloseAction::Allow, |guard| {
                guard(&ctx.runtime, &request_close)
            });
        let allow = matches!(action, CloseAction::Allow);
        let _ = request_close;
        // `ctx` 在此函数结束时自动释放借用，调用方即可安全地关闭窗口。
        allow
    }
}

// ============================================================================
// ApplicationHandler 实现（winit 事件循环驱动）
// ============================================================================

impl ApplicationHandler<()> for Application {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.windows.is_empty() {
            let specs: Vec<WindowSpec> = std::mem::take(&mut self.specs);
            for spec in specs {
                self.create_window(el, spec);
            }
        }
    }

    /// 外部线程通过 [`crate::external::wake`] 唤醒事件循环后进入这里。
    ///
    /// 对所有注册的外部源执行 `poll`，把外部事件分发到 UI 线程：
    /// - `Rebuild` → `request_rebuild`
    /// - `Redraw`  → `request_redraw`
    /// - `Data`    → 只登记为脏（由集成方在 widget 回调中消费），不直接触发重建
    fn user_event(&mut self, _el: &ActiveEventLoop, (): ()) {
        if self.external_sources.is_empty() {
            return;
        }
        // 收集本批事件，再统一分发，避免在 poll 中直接操作窗口状态。
        let mut rebuild = false;
        let mut redraw = false;
        // Data 事件在 sink 里直接交给外部数据处理器（UI 线程执行，可碰 Rc）。
        let mut handler = self.external_data_handler.take();
        let mut sources = std::mem::take(&mut self.external_sources);
        for src in sources.iter_mut() {
            src.poll(&mut |ev| match ev {
                crate::external::ExternalEvent::Rebuild => rebuild = true,
                crate::external::ExternalEvent::Redraw => redraw = true,
                crate::external::ExternalEvent::Data(payload) => {
                    if let Some(h) = handler.as_mut() {
                        h(payload);
                    }
                    // 数据事件本身不触发重建；是否重绘由数据处理器决定。
                }
            });
        }
        self.external_sources = sources;
        self.external_data_handler = handler;
        if rebuild {
            crate::state::request_rebuild();
        } else if redraw {
            crate::state::request_redraw();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, wid: WindowId, event: WindowEvent) {
        // 标记「当前窗口」，使所有 `state::request_*` 都路由到该窗口，
        // 实现 per-window 的重建/重绘/关闭标志，避免多窗口信号互相干扰。
        state::set_current_window(wid);

        // 关闭请求单独处理：调用守卫并（在允许时）关闭窗口，均不持有 `ctx` 借用，
        // 因此不会与下方 `self.windows.get_mut` 的借用冲突。
        if matches!(event, WindowEvent::CloseRequested) {
            let allow = self.handle_close_request(&wid);
            state::clear_current_window();
            if allow {
                self.close_window(&wid, el);
            }
            return;
        }
        // 先取出 resize 回调，避免与下面 `self.windows.get_mut` 的借用冲突。
        let mut resize_handler = self.resize_handler.take();
        let Some(ctx) = self.windows.get_mut(&wid) else {
            state::clear_current_window();
            self.resize_handler = resize_handler;
            return;
        };
        match event {
            WindowEvent::Resized(s) => {
                if s.width > 0 && s.height > 0 && (s.width, s.height) != ctx.window_size {
                    ctx.window_size = (s.width, s.height);
                    ctx.renderer.resize(s.width as u16, s.height as u16);
                    ctx.compositor = crate::render::Compositor::new(
                        s.width.max(1),
                        s.height.max(1),
                        [240, 240, 240, 255],
                    );
                    // 通知外部（如 terminal）窗口尺寸变化，重新计算网格/重建 surface。
                    if let Some(h) = resize_handler.as_mut() {
                        h(s.width, s.height);
                    }
                    // 强制重新布局 + 重新渲染：让 SharedSurface 的 bounds 更新到新窗口尺寸，
                    // 与 resize 后的 surface 尺寸保持一致（否则旧 bounds 会让 compositor 越界）。
                    ctx.runtime.request_layout();
                    ctx.runtime.request_render();
                    if let Some(w) = &ctx.window {
                        w.request_redraw();
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                ctx.mouse_pos = Point::new(position.x as f32, position.y as f32);

                // ScrollView 拖拽滚动优先：捕获期间不再走普通 hover 路径。
                if ctx.scroll_drag.is_some() {
                    ctx.handle_scroll_drag_move(ctx.mouse_pos);
                    return;
                }

                let hit = ctx.build_hit_result();
                let effects = {
                    let tree = &ctx.runtime.layers.tree;
                    let mut em = ctx.runtime.layers.event_manager.borrow_mut();
                    em.handle_mouse_move(ctx.mouse_pos, hit.as_ref(), tree, |id, event, ctx| {
                        WindowContext::handle_lie_event(tree, id, event, ctx)
                    })
                };
                let needs_redraw = effects.needs_render();
                WindowContext::apply_event_effects(&mut ctx.runtime, &effects);
                if needs_redraw && let Some(w) = &ctx.window {
                    w.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => {
                if let Some(hit) = ctx.build_hit_result() {
                    let btn = if button == winit::event::MouseButton::Left {
                        MouseButton::Left
                    } else if button == winit::event::MouseButton::Right {
                        MouseButton::Right
                    } else {
                        MouseButton::Middle
                    };
                    let effects = {
                        let tree = &ctx.runtime.layers.tree;
                        let modifiers = map_winit_modifiers(ctx.modifiers);
                        let mut em = ctx.runtime.layers.event_manager.borrow_mut();
                        em.handle_mouse_down(
                            ctx.mouse_pos,
                            btn,
                            modifiers,
                            &hit,
                            tree,
                            |id, event, ctx| WindowContext::handle_lie_event(tree, id, event, ctx),
                        )
                    };
                    WindowContext::apply_event_effects(&mut ctx.runtime, &effects);

                    // 在普通 widget 处理之后尝试启动 ScrollView 拖拽滚动/滚动条交互。
                    ctx.try_start_scroll_drag(&hit, btn, &effects);
                }

                if let Some(w) = &ctx.window {
                    w.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button,
                ..
            } => {
                let hit = ctx.build_hit_result().or_else(|| {
                    let cap = ctx.runtime.layers.event_manager.borrow().mouse_capture();
                    cap.map(|id| HitTestResult {
                        target: id,
                        path: vec![id],
                    })
                });
                if let Some(hit) = hit {
                    let btn = if button == winit::event::MouseButton::Left {
                        MouseButton::Left
                    } else {
                        MouseButton::Right
                    };
                    let effects = {
                        let tree = &ctx.runtime.layers.tree;
                        let mut em = ctx.runtime.layers.event_manager.borrow_mut();
                        em.handle_mouse_up(ctx.mouse_pos, btn, &hit, tree, |id, event, ctx| {
                            WindowContext::handle_lie_event(tree, id, event, ctx)
                        })
                    };
                    WindowContext::apply_event_effects(&mut ctx.runtime, &effects);
                }
                if button == winit::event::MouseButton::Left {
                    ctx.end_scroll_drag();
                }
                if let Some(w) = &ctx.window {
                    w.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => (x * 16.0, y * 16.0),
                    winit::event::MouseScrollDelta::PixelDelta(p) => (p.x as f32, p.y as f32),
                };
                let dy = if cfg!(target_os = "macos") { dy } else { -dy };
                let hit = ctx.build_hit_result();
                if let Some(hit) = &hit {
                    let point = ctx.mouse_pos;
                    let effects = {
                        let tree = &ctx.runtime.layers.tree;
                        let mut em = ctx.runtime.layers.event_manager.borrow_mut();
                        em.handle_wheel(point, dx, dy, hit, |id, event, ctx| {
                            WindowContext::handle_lie_event(tree, id, event, ctx)
                        })
                    };
                    WindowContext::apply_event_effects(&mut ctx.runtime, &effects);
                    ctx.runtime.handle_wheel_scroll(hit, dx, dy);
                }
                if let Some(w) = &ctx.window {
                    w.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                ctx.modifiers = m.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let key = map_winit_key(&event.logical_key);
                let modifiers = map_winit_modifiers(ctx.modifiers);
                let effects = {
                    let tree = &ctx.runtime.layers.tree;
                    let mut em = ctx.runtime.layers.event_manager.borrow_mut();
                    match event.state {
                        ElementState::Pressed => {
                            em.handle_key_down(key, modifiers, |id, ev, ctx| {
                                WindowContext::handle_lie_event(tree, id, ev, ctx)
                            })
                        }
                        ElementState::Released => {
                            em.handle_key_up(key, modifiers, |id, ev, ctx| {
                                WindowContext::handle_lie_event(tree, id, ev, ctx)
                            })
                        }
                    }
                };
                WindowContext::apply_event_effects(&mut ctx.runtime, &effects);
                if (effects.needs_render() || effects.needs_rebuild())
                    && let Some(w) = &ctx.window
                {
                    w.request_redraw();
                }
            }
            WindowEvent::Ime(ime) => {
                let effects = {
                    let tree = &ctx.runtime.layers.tree;
                    let mut em = ctx.runtime.layers.event_manager.borrow_mut();
                    match ime {
                        Ime::Preedit(text, cursor) => {
                            let (start, end) =
                                cursor.map_or((None, None), |(s, e)| (Some(s), Some(e)));
                            em.handle_ime_preedit(text, start, end, |id, ev, ctx| {
                                WindowContext::handle_lie_event(tree, id, ev, ctx)
                            })
                        }
                        Ime::Commit(text) => em.handle_ime_commit(text, |id, ev, ctx| {
                            WindowContext::handle_lie_event(tree, id, ev, ctx)
                        }),
                        Ime::Disabled => em.handle_ime_disabled(|id, ev, ctx| {
                            WindowContext::handle_lie_event(tree, id, ev, ctx)
                        }),
                        Ime::Enabled => EventEffects::default(),
                    }
                };
                WindowContext::apply_event_effects(&mut ctx.runtime, &effects);
                if (effects.needs_render() || effects.needs_rebuild())
                    && let Some(w) = &ctx.window
                {
                    w.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                let viewport = (
                    ctx.runtime.viewport.width as u32,
                    ctx.runtime.viewport.height as u32,
                );
                let size_mismatch = ctx.window_size != viewport;
                let rebuild_requested = state::take_rebuild_requested(wid);
                if !ctx.rendered_once || ctx.surface.is_none() || rebuild_requested || size_mismatch
                {
                    ctx.build_and_render(size_mismatch, rebuild_requested);
                } else {
                    // 仅重绘路径：`request_redraw()`（如 SharedSurface 脏区更新）只置位了
                    // 全局 redraw 标记，并不会置位 runtime.needs_render。这里补一次
                    // `request_render()`，确保 `frame_visual_update` 生成渲染树，
                    // 从而 `present_frame` 能把 SharedSurface 的脏区合成上屏。
                    ctx.runtime.request_render();
                    ctx.render_visuals();
                }
            }
            _ => {}
        }

        // 集成方可能通过 `state::request_window_close` 请求真正关闭窗口（通常在自行实现的
        // 确认弹窗被用户确认后）。该标志绕过关闭守卫，此处统一消费并关闭窗口。
        let _ = ctx;
        if state::take_window_close_requested(wid) {
            self.close_window(&wid, el);
        }
        self.resize_handler = resize_handler;
        state::clear_current_window();
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let (fired, next) = crate::animation::tick(Instant::now());
        if fired {
            // 当前无窗口上下文，request_redraw 会自动广播到所有已注册窗口
            state::request_redraw();
        }
        match next {
            Some(t) => el.set_control_flow(ControlFlow::WaitUntil(t)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
        // 逐窗口取走重绘标记，仅对真正请求重绘的窗口发起 RedrawRequested
        for (wid, ctx) in &self.windows {
            if state::take_redraw_requested(*wid)
                && let Some(w) = &ctx.window
            {
                w.request_redraw();
            }
        }
    }
}

// ============================================================================
// Application 私有方法
// ============================================================================

impl Application {
    /// 创建一个窗口并初始化上下文。
    fn create_window(&mut self, el: &ActiveEventLoop, spec: WindowSpec) {
        let (ww, wh) = spec.config.size;
        let mut wa = Window::default_attributes()
            .with_title(&spec.config.title)
            .with_inner_size(LogicalSize::new(ww, wh))
            .with_resizable(spec.config.resizable)
            .with_decorations(spec.config.decorations);
        if let Some((mw, mh)) = spec.config.min_size {
            wa = wa.with_min_inner_size(LogicalSize::new(mw, mh));
        }
        if let Some((mw, mh)) = spec.config.max_size {
            wa = wa.with_max_inner_size(LogicalSize::new(mw, mh));
        }
        if let Some(icon_path) = &spec.config.icon
            && let Ok(img) = load_icon(icon_path)
        {
            wa = wa.with_window_icon(Some(img));
        }
        if spec.config.always_on_top {
            wa = wa.with_window_level(WindowLevel::AlwaysOnTop);
        }
        if let Some((x, y)) = spec.config.position {
            wa = wa.with_position(LogicalPosition::new(x as f64, y as f64));
        }

        let window = Rc::new(el.create_window(wa).unwrap());
        let s = window.inner_size();
        let wid = window.id();
        let wsize = (s.width, s.height);

        // 注册窗口，使其拥有独立的重建/重绘/关闭标志
        state::register_window(wid);

        let viewport = Size::new(s.width as f32, s.height as f32);
        let rt = Runtime::new(viewport);

        let renderer = VelloRenderer::new(s.width as u16, s.height as u16);
        let compositor = crate::render::Compositor::new(
            s.width.max(1),
            s.height.max(1),
            [240, 240, 240, 255], // 与 VelloRenderer 背景一致
        );

        // 每个窗口拥有独立的 UI 状态表，保证多窗口之间状态不互相串味
        let state = Rc::new(RefCell::new(StateMap::new()));

        let ctx = WindowContext {
            wid,
            runtime: rt,
            renderer,
            compositor,
            window: Some(window),
            surface: None,
            window_size: wsize,
            mouse_pos: Point::zero(),
            modifiers: winit::keyboard::ModifiersState::empty(),
            rendered_once: false,
            last_ime_allowed: false,
            last_ime_cursor: None,
            scroll_drag: None,
            builder: spec.builder,
            close_guard: spec.close_guard,
            state,
            #[cfg(feature = "inspector")]
            inspector: if spec.inspector {
                Some(crate::inspector::Inspector::start())
            } else {
                None
            },
        };

        self.windows.insert(wid, ctx);
    }
}

// ============================================================================
// WindowContext 方法
// ============================================================================

impl WindowContext {
    fn build_and_render(&mut self, viewport_changed: bool, rebuild_requested: bool) {
        let pw = self.renderer.width();
        let ph = self.renderer.height();
        if pw == 0 || ph == 0 {
            return;
        }

        if viewport_changed || !self.rendered_once {
            let (ww, wh) = self.window_size;
            self.runtime.set_viewport(Size::new(ww as f32, wh as f32));
        }

        // 构建 UI（使用本窗口独立的状态表）
        let mut ctx = crate::widget::BuildContext::new(Rc::clone(&self.state));

        // Inspector：在开始构建前，准备根描述节点收集
        #[cfg(feature = "inspector")]
        let inspect_root: Option<Rc<RefCell<crate::inspector::WidgetDescNode>>> =
            self.inspector.as_ref().map(|_| {
                Rc::new(RefCell::new(crate::inspector::WidgetDescNode {
                    name: "Root".to_string(),
                    path: "".to_string(),
                    text: None,
                    children: Vec::new(),
                }))
            });
        #[cfg(feature = "inspector")]
        if let Some(root_node) = &inspect_root {
            ctx.begin_inspect(Rc::clone(root_node));
        }

        let root_widget = (self.builder)(&mut ctx);
        let view_tree = root_widget.build(&mut ctx);

        // Inspector：构建结束，冻结描述树并写入共享
        #[cfg(feature = "inspector")]
        if let Some(desc) = ctx.finish_inspect() {
            if let Some(insp) = &self.inspector {
                insp.set_snapshot(desc);
            }
        }

        // 提交到 Runtime
        if !self.runtime.submit_view_tree(view_tree, rebuild_requested) {
            // 树无变化，跳过渲染
            return;
        }

        // 执行 Reconciliation → 布局 → 渲染并取回渲染元素
        let elements = self.runtime.frame(self.wid);
        if elements.is_empty() {
            return;
        }
        let pix = self.renderer.render(&elements);
        self.present_frame(&elements, pix.data());
        self.update_ime_state();
        self.rendered_once = true;
    }

    /// 应用事件回调产生的副作用：重建 / 重排 / 重绘。
    fn apply_event_effects(runtime: &mut Runtime, effects: &EventEffects) {
        if effects.needs_rebuild() {
            state::request_rebuild();
        }
        if effects.needs_layout() {
            runtime.request_layout();
        }
        if effects.needs_render() {
            runtime.request_render();
        }
    }

    fn render_visuals(&mut self) {
        let elements = self.runtime.frame_visual_update();
        if elements.is_empty() {
            return;
        }
        let pix = self.renderer.render(&elements);
        self.present_frame(&elements, pix.data());
        self.update_ime_state();
    }

    /// 把 UI pixmap 与各 SharedSurface 合屏后上屏。
    ///
    /// 流程：
    /// 1. 先把 UI pixmap 整体复制进 compositor backing（SharedSurface 在 UI 通道内是不透明占位，
    ///    会被背景色填充，故此处直接覆盖即可）。
    /// 2. 从渲染元素里取出 `VisualElement::SharedSurface`，按 id 解析真实表面，
    ///    由 compositor 把其脏区合成到 backing 之上（覆盖 UI 通道里的占位背景色）。
    /// 3. 上屏时只用 backing 的最终像素。
    ///
    /// 这是"进程内 compositor"的落点：SharedSurface 的高频增量更新不再触发
    /// UI 全量 rebuild/光栅化，只需在合屏阶段增量覆盖脏区。
    fn present_frame(&mut self, elements: &[crate::render::visual::LayeredElement], ui_pixmap: &[vello_cpu::color::PremulRgba8]) {
        let (w, h) = self.window_size;
        if w == 0 || h == 0 {
            return;
        }
        // 1. 同步 compositor backing 尺寸。
        if self.compositor.width != w || self.compositor.height != h {
            self.compositor = crate::render::Compositor::new(
                w.max(1),
                h.max(1),
                [240, 240, 240, 255],
            );
        }
        // 2. 收集并定位 SharedSurface 覆盖的区域（矩形列表）。
        let mut surfaces: Vec<std::rc::Rc<crate::render::surface::SharedSurface>> = Vec::new();
        let mut surface_rects: Vec<(
            crate::render::surface::SurfaceId,
            crate::geometry::Rect,
        )> = Vec::new();
        for el in elements {
            if let crate::render::visual::VisualElement::SharedSurface { id, bounds, .. } = &el.element
            {
                if let Some(surf) = crate::render::surface::resolve_surface(*id) {
                    surfaces.push(surf);
                    surface_rects.push((
                        *id,
                        crate::geometry::Rect::new(
                            bounds.x0 as f32,
                            bounds.y0 as f32,
                            (bounds.x1 - bounds.x0) as f32,
                            (bounds.y1 - bounds.y0) as f32,
                        ),
                    ));
                }
            }
        }

        // 3. 把 UI pixmap 拷入 backing，但**跳过 SharedSurface 覆盖的区域**。
        //
        // 关键：surface 像素必须在 backing 中持久保留。若每帧无条件用 UI pixmap
        // 覆盖整个 backing，那么当 surface 没有新脏区时（`take_dirty` 为空），
        // surface 区域就会被 UI 背景色覆盖，导致白屏。因此这里只把 UI 像素写入
        // surface 之外的部分，surface 自身像素由 `composite_surface` 持久维护。
        {
            let ww = self.compositor.width as usize;
            let wh = self.compositor.height as usize;
            let backing = &mut self.compositor.backing;
            for py in 0..wh {
                let row_start = py * ww;
                // 找出本行被 surface 覆盖的 x 区间集合（按像素块跳过）。
                // surface 数量少、尺寸大，这里用"逐像素排除"成本可接受（终端 surface 铺满时几乎全跳过）。
                let covered = |px: usize| {
                    surface_rects.iter().any(|(_, r)| {
                        (px as f32) >= r.x
                            && (px as f32) < r.x + r.width
                            && (py as f32) >= r.y
                            && (py as f32) < r.y + r.height
                    })
                };
                // 若整行都不被覆盖，直接整行块拷（常见于 surface 之外区域）。
                let any_covered = surface_rects
                    .iter()
                    .any(|(_, r)| (py as f32) >= r.y && (py as f32) < r.y + r.height);
                if !any_covered {
                    let di = row_start * 4;
                    for (dst_chunk, p) in backing[di..di + ww * 4]
                        .chunks_exact_mut(4)
                        .zip(ui_pixmap[row_start..row_start + ww].iter())
                    {
                        dst_chunk.copy_from_slice(&[p.r, p.g, p.b, p.a]);
                    }
                } else {
                    for px in 0..ww {
                        if !covered(px) {
                            let src = row_start + px;
                            let p = &ui_pixmap[src];
                            let di = src * 4;
                            backing[di..di + 4].copy_from_slice(&[p.r, p.g, p.b, p.a]);
                        }
                    }
                }
            }
        }

        // 4. 合成 SharedSurface：只更新有脏区的表面（无新脏区则保留 backing 里
        //    已有的 surface 像素，不会被 UI 覆盖）。
        for s in &surfaces {
            if let Some(&(_, rect)) = surface_rects.iter().find(|(id, _)| *id == s.id) {
                self.compositor
                    .composite_surface(s, (rect.x, rect.y));
            }
        }

        // 5. 上屏 backing 的最终像素（拷贝到局部，避免与 &mut self 冲突）。
        let backing = self.compositor.backing.clone();
        self.blit_to_window(&backing);
    }

    /// 根据当前焦点与布局结果更新 IME 的启用状态及候选窗位置。
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

    /// 把 raw RGBA8（每像素 4 字节）逐像素打包进 softbuffer 帧缓冲并上屏。
    ///
    /// 相比旧的 `&[PremulRgba8]` 入口，本函数直接消费 compositor backing 的
    /// raw RGBA8 字节，避免中间转换，且为后续"只上屏脏区"留出改造点。
    fn blit_to_window(&mut self, data: &[u8]) {
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
        let byte_len = buf_len * 4;
        let copy_len = byte_len.min(data.len());
        let dst = &mut buf[..buf_len];
        // 逐像素打包：softbuffer 期望 BGRA（低字节蓝）。raw RGBA8 → BGRA8。
        for (d, px) in dst[..copy_len / 4].iter_mut().zip(data[..copy_len].chunks_exact(4)) {
            let r = px[0];
            let g = px[1];
            let b = px[2];
            *d = (b as u32) | ((g as u32) << 8) | ((r as u32) << 16);
        }
        const CLEAR: u32 = 0x00F0F0F0;
        dst[copy_len / 4..].fill(CLEAR);
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

    // ========== 事件处理 ==========

    fn build_hit_result(&mut self) -> Option<HitTestResult> {
        let (_kind, target, _handle) = self.runtime.layers.hit_test_top(self.mouse_pos)?;
        let path = self.runtime.layers.path_to(target);
        Some(HitTestResult { target, path })
    }

    fn handle_lie_event(
        tree: &crate::runtime::element::ElementTree,
        id: crate::core::ElementId,
        event: &crate::event::Event,
        ctx: &mut crate::event::EventContext,
    ) {
        ctx.set_event(event.clone());
        ctx.set_current(id, tree.layout(id).rect());
        crate::event::dispatch_node_listeners(tree, id, event, ctx);
    }

    // ========== ScrollView 拖拽/滚动条辅助 ==========

    fn find_scroll_container(&self, hit: &HitTestResult) -> Option<crate::core::ElementId> {
        for &id in hit.path.iter().rev() {
            if let Some(node) = self.runtime.layers.tree.get_node_ref(id)
                && node.layout().overflow_scroll
            {
                return Some(id);
            }
        }
        None
    }

    fn has_mouse_down_listener(
        tree: &crate::runtime::element::ElementTree,
        id: crate::core::ElementId,
    ) -> bool {
        tree.listeners(id)
            .iter()
            .any(|l| l.event == crate::event::EventType::MouseDown)
    }

    fn has_click_listener(
        tree: &crate::runtime::element::ElementTree,
        id: crate::core::ElementId,
    ) -> bool {
        tree.listeners(id)
            .iter()
            .any(|l| l.event == crate::event::EventType::Click)
    }

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

        if let Some(track) = Self::scrollbar_track_rect(tree, container_id)
            && track.contains(point)
            && let Some((thumb_y, thumb_size)) = Self::scrollbar_thumb_info(tree, container_id)
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
                let ratio = ((point.y - track.y) / track.height).clamp(0.0, 1.0);
                let viewport_h = tree.layout(container_id).height;
                let content_h = tree.content_size(container_id).1;
                let max_scroll = (content_h - viewport_h).max(0.0);
                let new_y = ratio * max_scroll;
                let (ox, _) = tree.scroll_offset(container_id);
                self.runtime.scroll_to(container_id, ox, new_y);
            }
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            return;
        }

        let container_idx = hit
            .path
            .iter()
            .position(|&id| id == container_id)
            .unwrap_or(hit.path.len());
        let descendant_interactive = hit.path.iter().skip(container_idx + 1).any(|&id| {
            Self::has_mouse_down_listener(tree, id)
                || Self::has_click_listener(tree, id)
                || tree.is_interactive(id)
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
}

// ============================================================================
// 图标加载辅助
// ============================================================================

fn load_icon(path: &std::path::Path) -> Result<winit::window::Icon, Box<dyn std::error::Error>> {
    let img = image::open(path)?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(winit::window::Icon::from_rgba(rgba.into_raw(), w, h)?)
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::pack_softbuffer_pixel;
    use vello_cpu::color::PremulRgba8;

    #[test]
    fn pack_softbuffer_obeys_0rgb_format() {
        let red = PremulRgba8::from_u8_array([255, 0, 0, 255]);
        assert_eq!(pack_softbuffer_pixel(red), 0x00FF0000);

        let green = PremulRgba8::from_u8_array([0, 255, 0, 255]);
        assert_eq!(pack_softbuffer_pixel(green), 0x0000FF00);

        let blue = PremulRgba8::from_u8_array([0, 0, 255, 255]);
        assert_eq!(pack_softbuffer_pixel(blue), 0x000000FF);

        let white = PremulRgba8::from_u8_array([255, 255, 255, 255]);
        assert_eq!(pack_softbuffer_pixel(white), 0x00FFFFFF);

        let semi = PremulRgba8::from_u8_array([128, 64, 32, 128]);
        assert_eq!(pack_softbuffer_pixel(semi), 0x00804020);
    }
}
