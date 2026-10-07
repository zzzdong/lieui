//! 窗口层（M4）：winit 事件循环 + softbuffer 上屏。
//!
//! 这一层**只做搬运**，不含 UI 逻辑（设计 §3.8）：
//!
//! ```text
//! winit 事件 ──翻译──▶ InputEvent / Event ──▶ WindowCtx（命中 → 分发 → 状态）
//!                                             │
//! 帧循环（about_to_wait）：drain_requests → per-window tick → frame → present
//!                                             ▼
//!                       pixmap(premultiplied) ──unpremul──▶ softbuffer 0x00RRGGBB（仅脏区）
//! ```
//!
//! 三条约定：
//! 1. **逻辑 / 物理坐标**：布局与命中都吃**逻辑**像素，pixmap 与 softbuffer surface 是**物理**像素；
//!    winit 给的光标位置是物理的 ⇒ 进 `InputEvent` 前除以 `scale`。
//!
//! ## DPI（系统缩放）契约
//!
//! - **一个真源**：逻辑尺寸（[`crate::app::WindowCtx::size`]）。物理尺寸恒等于
//!   `round(逻辑 × scale)`，由光栅器（pixmap）与平台层（surface）各自从它推出来；
//!   任何一侧都不单独记"自己那份像素数"。
//! - **DPI 变化不改布局**：拖到别的显示器后**逻辑可用面积不变**，只是每逻辑像素占更多
//!   物理像素（与 Windows / macOS 的系统缩放一致）。`WM_DPICHANGED` 时窗口的物理尺寸
//!   必须跟着放大，否则逻辑面积会缩水 —— winit 在 Windows 上默认就是这么给的
//!   （`platform_impl/windows/event_loop.rs` 里按 `旧尺寸 → 旧逻辑 → 新物理` 换算），
//!   我们仍显式 `request_inner_size` 一次，好让这条契约不依赖平台默认值。
//! - **光标**：winit 0.30 的 `WindowEvent::CursorMoved::position` 是
//!   `PhysicalPosition<f64>`（见 `winit::event` 的定义），所以 [`physical_to_logical`]
//!   除以 scale 是**必须**的，别当成重复换算删掉。
//! - **DPI 感知**：Windows 上由 winit 的 `EventLoopBuilder` 默认完成
//!   （`dpi_aware: true` ⇒ `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`，
//!   逐级回退到 V1 / `SetProcessDPIAware`），所以**不需要**应用清单里的 `dpiAware`，
//!   lieui 也不必自己调 Win32（本 crate `#![forbid(unsafe_code)]`，做不了 FFI）。
//!   **自带宿主循环的嵌入方**：建 `EventLoopBuilder` 时别关掉 DPI 感知即可 ——
//!   winit 的默认值就是 `true`；若你显式 `with_dpi_aware(false)`，进程会退回
//!   系统位图拉伸（整个界面发虚），且 `scale_factor()` 恒为 1.0。
//! 2. **局部上屏**：只把脏区那几行从 pixmap 打包进 softbuffer 缓冲，再用 `present_with_damage`
//!    提交；`age() == 0`（缓冲内容未定义）或整窗脏时退化为全量 `present()`。
//! 3. **关闭守卫**：`CloseRequested` 先问 `ViewModel::on_close_request`，`Cancel` 可拦截。
//!
//! 归 M5：IME 提交的文本 / 剪贴板（`Ctrl+C/V`）由组件（Input）消费 ——
//! 本层只负责把 `Ime::Commit` / 按键翻译成事件派发到焦点节点。

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use lieui_geom::{Point, Rect, Size};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ControlFlow;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId as OsWindowId};

use crate::app::{App, CloseAction, ExternalData};
use crate::event::{Event, EventKind, KeyCode, Modifiers, PointerButton, PointerId};
use crate::input::InputEvent;
use crate::reactive::Dirty;
use crate::track::Kind;
use crate::window::WindowId;

// ───────────────────────── 系统剪贴板 ─────────────────────────

/// 写系统剪贴板。
///
/// 每次调用新建 `arboard::Clipboard`：Windows 上常驻句柄会在剪贴板被别的进程占用时失效，
/// 即用即开反而更稳（代价是每次一次 OpenClipboard，用户操作频率下可忽略）。
fn clipboard_set(text: &str) {
    match arboard::Clipboard::new() {
        Ok(mut cb) => {
            if let Err(e) = cb.set_text(text.to_string()) {
                eprintln!("[lieui] 写剪贴板失败：{e}");
            }
        }
        Err(e) => eprintln!("[lieui] 打开剪贴板失败：{e}"),
    }
}

fn clipboard_get() -> Option<String> {
    arboard::Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok())
}

/// 跨线程事件（`RepaintHandle` → 事件循环）
#[derive(Debug)]
pub enum AppEvent {
    /// 唤醒事件循环跑一帧（后台线程改完状态后调用）
    Wake,
    /// 给某个窗口投递外部数据
    External { window: WindowId, data: ExternalData },
}

/// 显式重绘句柄（**不是**全局单例）：后台线程用它唤醒 UI 线程。
///
/// 关键点：`Signal` 是 `!Send`，所以后台线程不能直接写 `Signal`，
/// 只能"投递数据 + 唤醒"，由 UI 线程在 `on_external` 里写。
#[derive(Clone)]
pub struct RepaintHandle {
    proxy: EventLoopProxy<AppEvent>,
}

impl RepaintHandle {
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self { proxy }
    }

    /// 唤醒 UI 线程（返回是否投递成功；事件循环已退出时为 `false`）
    pub fn wake(&self) -> bool {
        self.proxy.send_event(AppEvent::Wake).is_ok()
    }

    /// 投递外部数据并唤醒
    pub fn post_external(&self, window: WindowId, data: ExternalData) -> bool {
        self.proxy.send_event(AppEvent::External { window, data }).is_ok()
    }
}

/// 平台唤醒器：`RepaintHandle` 是 [`Waker`](crate::task::Waker) 的唯一 winit 实现。
///
/// 实现 trait 之后，`Runtime::set_waker` 就能把它交给框架 —— 任务（`Runtime::spawn_task`）
/// 与 `TaskCtx::post` 从此不需要调用方自己传句柄。
impl crate::task::Waker for RepaintHandle {
    fn wake(&self) -> bool {
        // 显式写固有方法，避免与 trait 方法混淆
        RepaintHandle::wake(self)
    }

    fn post(&self, window: WindowId, data: ExternalData) -> bool {
        RepaintHandle::post_external(self, window, data)
    }
}

impl std::fmt::Debug for RepaintHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RepaintHandle")
    }
}

// ───────────────────────── 上屏：像素打包（纯函数，可无头测试）─────────────────────────

/// premultiplied RGBA8 → softbuffer 的 `0x00RRGGBB`（softbuffer 忽略 alpha）。
///
/// 窗口底色不透明 ⇒ 绝大多数像素走 `a == 255` 的直通路径；非不透明像素做**反预乘**，
/// 保证"半透明内容合成到不透明底"的结果正确（不会比预期更暗）。
pub fn pack_xrgb(src: &[vello_cpu::color::PremulRgba8], out: &mut [u32]) -> usize {
    let n = src.len().min(out.len());
    for i in 0..n {
        let p = src[i];
        let (r, g, b) = match p.a {
            255 => (p.r, p.g, p.b),
            0 => (0, 0, 0),
            a => {
                let un = |v: u8| {
                    let v = u32::from(v) * 255 + u32::from(a) / 2;
                    (v / u32::from(a)).min(255) as u8
                };
                (un(p.r), un(p.g), un(p.b))
            }
        };
        out[i] = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
    }
    n
}

/// 只把 `rects` **覆盖到的像素**从 `src`（premul RGBA8）拷进 `dst`（u32 XRGB），
/// 返回实际拷贝的像素数。
///
/// ## 为什么抽成独立函数（D22 / C1）
///
/// 此前局部上屏是**内联**在 `present` 里的，而它按 `row * stride .. +stride` **整行拷**、
/// **完全忽略 `r.x` / `r.width`** ⇒ 一个 40×20 的脏区被展开成 20 行 × 全窗宽，
/// 呈现带宽白白翻倍（光栅化已经是局部的，瓶颈就卡在上屏这一侧）。
///
/// 而它内联在 `#[cfg(feature = "winit")]` 的平台层里、需要真实 `surface` ⇒ **无法测试**。
/// 抽成纯函数后，`copy_damage_rects_only_touches_the_rect` 才能钉住"拷贝量 ∝ 脏区面积"。
///
/// ## 语义
///
/// - `stride` = 每行像素数（= pixmap 宽）；`src.len() == stride * 高度`，
///   所以"行内越界"与"整体越界"都用同一处`saturating_sub` 夹住，**不会 panic**。
/// - 超出 `src` / `dst` 的部分**静默裁剪**（与 `pack_xrgb` 的取 min 语义一致）。
/// - 宽度为 0 / 高度为 0 的矩形直接跳过。
pub fn copy_damage_rects(
    src: &[vello_cpu::color::PremulRgba8],
    dst: &mut [u32],
    stride: usize,
    rects: &[softbuffer::Rect],
) -> usize {
    if stride == 0 {
        return 0;
    }
    let mut copied = 0usize;
    for r in rects {
        let x0 = r.x as usize;
        let y0 = r.y as usize;
        // 横向裁到本行内（x0 越界 ⇒ w 变 0）
        let w = (r.width.get() as usize).min(stride.saturating_sub(x0));
        let h = r.height.get() as usize;
        if w == 0 || h == 0 {
            continue;
        }
        for row in y0..y0.saturating_add(h) {
            let start = row * stride + x0;
            // 纵向/整体越界在此一并夹住（src.len() == stride * 高度）
            let w_fit = w
                .min(src.len().saturating_sub(start))
                .min(dst.len().saturating_sub(start));
            if w_fit == 0 {
                break; // 整行都在界外 ⇒ 更后面的行也不在
            }
            copied += pack_xrgb(&src[start..start + w_fit], &mut dst[start..start + w_fit]);
        }
    }
    copied
}

/// 逻辑脏区 → 物理像素的 softbuffer 矩形（取整、裁剪、去重）
pub fn softbuffer_damage(physical: Size, damage: &[Rect], damage_all: bool, scale: f32) -> Vec<softbuffer::Rect> {
    let scaled: Vec<Rect> = damage
        .iter()
        .map(|d| Rect::new(d.x * scale, d.y * scale, d.width * scale, d.height * scale))
        .collect();
    crate::render::damage_batches(physical, &scaled, damage_all)
        .into_iter()
        .map(|r| {
            let nz = |v: f32| NonZeroU32::new((v.max(1.0).round() as u32).max(1)).unwrap_or(NonZeroU32::MIN);
            softbuffer::Rect {
                x: r.x.max(0.0).round() as u32,
                y: r.y.max(0.0).round() as u32,
                width: nz(r.width),
                height: nz(r.height),
            }
        })
        .collect()
}

// ───────────────────────── DPI：逻辑 / 物理换算（纯函数，可无头测试）─────────────────────────

/// 把 DPI 缩放夹到"能除"的范围（0 / NaN / 负数 ⇒ 1.0）。
///
/// 平台的 `scale_factor` 理论上恒 > 0，但**除零会污染整条布局链**（NaN 尺寸 ⇒ 全树失效），
/// 所以入口统一夹一次。
pub fn sane_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }
}

/// 物理光标位置 → 逻辑坐标（命中测试吃逻辑坐标）。
///
/// winit 0.30 的 `WindowEvent::CursorMoved::position` 是 `PhysicalPosition<f64>`，
/// 所以这个除法**不是重复换算**；模块头"DPI 契约"有论证。
pub fn physical_to_logical(pos: PhysicalPosition<f64>, scale: f32) -> Point {
    let s = sane_scale(scale);
    Point::new(pos.x as f32 / s, pos.y as f32 / s)
}

/// 物理窗口尺寸 → 逻辑尺寸（`Resized` 事件用）。
pub fn physical_size_to_logical(size: PhysicalSize<u32>, scale: f32) -> Size {
    let s = sane_scale(scale);
    Size::new(size.width as f32 / s, size.height as f32 / s)
}

/// 逻辑尺寸 → 物理窗口尺寸（DPI 变化时按"保住逻辑尺寸"请求新尺寸用）。
pub fn logical_size_to_physical(size: Size, scale: f32) -> PhysicalSize<u32> {
    let s = sane_scale(scale);
    PhysicalSize::new(
        (size.width.max(0.0) * s).round().max(1.0) as u32,
        (size.height.max(0.0) * s).round().max(1.0) as u32,
    )
}

// ───────────────────────── 运行器 ─────────────────────────

struct WinSurface {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    /// 上一次 `surface.resize` 的物理尺寸。
    /// **初值是 `None`**：softbuffer 的 surface 建出来是 0×0，必须先 resize 才能 `buffer_mut()`
    /// （否则 win32 后端直接 panic：`Must set size of surface before calling buffer_mut()`）。
    size: Option<(u32, u32)>,
}

/// 一个 App = 一个运行器 = 事件循环内的全部窗口
struct Runner {
    app: App,
    context: Option<softbuffer::Context<Rc<Window>>>,
    windows: HashMap<WindowId, WinSurface>,
    cursor: Point,
    modifiers: winit::keyboard::ModifiersState,
    /// 打印每帧统计（`LIEUI_TRACE=1`；观测脏区效果用）
    trace: bool,
    /// 退出时置位（`App::run` 用不到返回值，这里留作观测点）
    exit_requested: bool,
}

impl Runner {
    fn new(app: App) -> Self {
        Self {
            app,
            context: None,
            windows: HashMap::new(),
            cursor: Point::zero(),
            modifiers: winit::keyboard::ModifiersState::empty(),
            trace: std::env::var_os("LIEUI_TRACE").is_some(),
            exit_requested: false,
        }
    }

    fn app_window_id(&self, os: OsWindowId) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|(_, w)| w.window.id() == os)
            .map(|(id, _)| *id)
    }

    /// 为已注册的 `WindowCtx` 创建真实窗口
    fn create_window(&mut self, el: &ActiveEventLoop, id: WindowId) {
        if self.windows.contains_key(&id) {
            return;
        }
        let Some(ctx) = self.app.window_ctx(id) else {
            return;
        };
        let cfg = ctx.config().clone();

        let mut attrs = WindowAttributes::default()
            .with_title(cfg.title.clone())
            .with_inner_size(LogicalSize::new(cfg.size.width as f64, cfg.size.height as f64))
            .with_resizable(cfg.resizable)
            .with_decorations(cfg.decorations);
        if let Some(min) = cfg.min_size {
            attrs = attrs.with_min_inner_size(LogicalSize::new(min.width as f64, min.height as f64));
        }
        if let Some(max) = cfg.max_size {
            attrs = attrs.with_max_inner_size(LogicalSize::new(max.width as f64, max.height as f64));
        }

        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("[lieui] 创建窗口失败：{e}");
                return;
            }
        };
        // IME：允许系统输入法（M5 的 Input 组件消费 Commit/Preedit）
        window.set_ime_allowed(true);
        // 上报初始的系统深浅色（`ThemeMode::System` 据此立即选对预设）
        if let Some(t) = window.theme() {
            self.app
                .runtime()
                .set_system_dark(matches!(t, winit::window::Theme::Dark));
        }

        let context = self
            .context
            .get_or_insert_with(|| softbuffer::Context::new(Rc::clone(&window)).expect("softbuffer context"));
        let surface = match softbuffer::Surface::new(context, Rc::clone(&window)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[lieui] 创建表面失败：{e}");
                return;
            }
        };

        let phys = window.inner_size();
        let scale = window.scale_factor() as f32;
        // 注意：`size` 保持 `None`（surface 还没 resize，首次 present 时再设）
        self.windows.insert(
            id,
            WinSurface {
                window,
                surface,
                size: None,
            },
        );

        // 用真实窗口尺寸/DPI 校正视图（窗口管理器可能给了别的尺寸）
        let rt = self.app.runtime();
        if let Some(ctx) = self.app.window_ctx_mut(id) {
            ctx.set_scale_factor(&rt, scale);
            let logical = Size::new(phys.width as f32 / scale, phys.height as f32 / scale);
            ctx.set_size(&rt, logical);
        }
    }

    fn destroy_window(&mut self, id: WindowId) {
        self.windows.remove(&id);
        if self.windows.is_empty() {
            // 没有窗口了：Context 依赖窗口的 display handle，一并丢弃
            self.context = None;
        }
    }

    /// 一帧：**请求 → 本地投递队列 → 定时器 → frame → 上屏**（只在 `RedrawRequested` 调用）。
    ///
    /// ★ 主线 B：`frame()` 的**唯一入口**。此前 `about_to_wait` / `user_event` 也会跑
    /// 完整一帧，于是"一次鼠标移动"会跑 2 次 `frame`（第二次无脏、空跑）。
    /// 现在那两个唤醒点只调 [`Self::pump`]（消费定时器，不渲染）。
    fn tick(&mut self, el: &ActiveEventLoop, now: Instant) {
        let (opened, closed) = self.app.drain_requests();
        for id in opened {
            self.create_window(el, id);
        }
        for id in closed {
            self.destroy_window(id);
        }

        // ★ 消费本地投递队列（A6）。**此前平台层完全没做这件事**，于是：
        //
        //   `TaskCtx` 持有的是 **spawn 时**的 `WakerSlot` 快照，而平台 waker 是
        //   在 `run()` 里才 `set_waker` 注入的 ⇒ **在 `run()` 之前 spawn 的任务**
        //   （典型：ViewModel 构造期起预加载 / 预热任务）永远持有 `Local` 槽位，
        //   `post()` 全部落进 `LocalQueue`。而 `LocalQueue` 只被
        //   `App::frame_all` 消费，平台层走的是逐窗口 `ctx.tick/frame`，**从不调它**
        //   ⇒ 消息积压、无任何人消费 ⇒ **任务永不回调、界面毫无反应，且零报错**。
        //
        // `App::frame_all` 的文档注释曾写"有平台时队列恒空 ⇒ 零开销"——**那个假设是错的**，
        // 正是这个 bug 的来源。现在两边都 drain，假设才成立。
        {
            let rt = self.app.runtime();
            for (w, data) in rt.take_pending_external() {
                if let Some(ctx) = self.app.window_ctx_mut(w) {
                    ctx.external(&rt, data);
                }
            }
        }

        let ids: Vec<WindowId> = self.app.windows().iter().map(|w| w.id()).collect();
        for id in ids {
            let rt = self.app.runtime();
            // 让 `ctx` 的借用在这一小段里结束，随后 `self.present` 才能借 `&mut self`
            let stats = {
                let Some(ctx) = self.app.window_ctx_mut(id) else {
                    continue;
                };
                // 光标闪烁：翻相位（没聚焦输入框时是 no-op）
                ctx.animate(now);
                ctx.tick(&rt, now);
                ctx.frame(&rt)
            };
            if self.trace && !stats.is_idle() {
                eprintln!(
                    "[lieui] {id:?} view={} align.patched={} layout[边界={} 移动={}] paint={}px batches={} present={}",
                    stats.view_ran,
                    stats.align.patched,
                    stats.layout.boundaries,
                    stats.layout.moved,
                    stats.rasterized_pixels(),
                    stats.render.raster.batches,
                    stats.present_pending,
                );
            }
            if stats.present_pending {
                self.present(id, &stats.damage, stats.damage_all);
            }
        }

        // 还有窗口就继续等事件（不主动 request_redraw：脏了才画）
        if self.windows.is_empty() && self.app.windows().is_empty() {
            self.exit_requested = true;
            el.exit();
            return;
        }

        // 光标闪烁调度：有聚焦输入框 ⇒ 定时唤醒；否则纯 Wait（空闲帧零功耗）
        self.schedule_wakeup(el);
    }

    /// 只消费**定时器 / 动画帧 / 本地投递**，**不渲染**。返回"是否有窗口真的做了事"。
    ///
    /// ★ 主线 B 的闭环（`refactor-plan` §四·主线 B）：这是 `RedrawRequested` **之外**
    /// 的唤醒点（`about_to_wait` / `user_event`）走的路径。
    /// 定时器回调改了状态之后**必须有人请求重绘**，否则画面停在旧帧 ——
    /// "把 `frame` 收敛到单一入口"这个改法如果少了这一环，就会把
    /// "帧跑 3 次"换成**"定时器和动画停帧"**。
    fn pump(&mut self, now: Instant) -> bool {
        let rt = self.app.runtime();
        let mut did_work = false;

        // 本地投递队列（A6：`run()` 之前 spawn 的任务会往这里投）
        for (w, data) in rt.take_pending_external() {
            if let Some(ctx) = self.app.window_ctx_mut(w) {
                ctx.external(&rt, data);
                did_work = true;
            }
        }

        let ids: Vec<WindowId> = self.app.windows().iter().map(|w| w.id()).collect();
        for id in ids {
            let Some(ctx) = self.app.window_ctx_mut(id) else {
                continue;
            };
            ctx.animate(now); // 光标闪烁翻相位（无聚焦输入框时 no-op）
            did_work |= ctx.tick(&rt, now);
        }
        did_work
    }

    /// 全部窗口请求重绘（`pump` 发现状态变化后调用，否则停帧）。
    fn request_redraw_all(&self) {
        for ws in self.windows.values() {
            ws.window.request_redraw();
        }
    }

    /// 按"下一个时钟事件"设定唤醒策略（无事件则纯 `Wait`，空闲零功耗）。
    fn schedule_wakeup(&self, el: &ActiveEventLoop) {
        let mut wakeup: Option<Instant> = None;
        // `next_wakeup` 要问"还有没有忙碌项"（spinner），真相在 Runtime 里 ——
        // 好处是窗口不必每帧缓存一份 busy 快照，也就没有"过期真相"这回事。
        let rt = self.app.runtime();
        for id in self.app.windows().iter().map(|w| w.id()) {
            if let Some(ctx) = self.app.window_ctx(id)
                && let Some(t) = ctx.next_wakeup(&rt)
            {
                wakeup = Some(match wakeup {
                    Some(prev) if prev <= t => prev,
                    _ => t,
                });
            }
        }
        el.set_control_flow(match wakeup {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }

    /// 把 pixmap 的脏区打包进 softbuffer 并提交
    fn present(&mut self, id: WindowId, damage: &[Rect], damage_all: bool) {
        let Some(ctx) = self.app.window_ctx(id) else {
            return;
        };
        let physical = ctx.physical_size();
        let scale = ctx.scale_factor();
        let pixmap = ctx.pixmap();
        let pw = u32::from(pixmap.width());
        let ph = u32::from(pixmap.height());

        let Some(ws) = self.windows.get_mut(&id) else {
            return;
        };
        let (Some(nw), Some(nh)) = (NonZeroU32::new(pw), NonZeroU32::new(ph)) else {
            return;
        };

        // surface 尺寸跟随 pixmap（物理像素）；首次必须 resize（surface 初始 0×0）
        let mut resized = false;
        if ws.size != Some((pw, ph)) {
            if ws.surface.resize(nw, nh).is_ok() {
                ws.size = Some((pw, ph));
                resized = true;
            } else {
                return;
            }
        }

        ws.window.pre_present_notify();

        let mut buffer: softbuffer::Buffer<'_, Rc<Window>, Rc<Window>> = match ws.surface.buffer_mut() {
            Ok(b) => b,
            Err(_) => return,
        };
        let full = resized || damage_all || buffer.age() == 0;
        let batches = softbuffer_damage(physical, damage, full, scale);

        let src = pixmap.data();
        let stride = pw as usize;
        // `Buffer` 通过 `DerefMut<Target = [u32]>` 暴露像素（softbuffer 的 u32 = 0x00RRGGBB）
        {
            let dst: &mut [u32] = &mut buffer;
            if full {
                pack_xrgb(src, dst);
            } else {
                // ★ 只拷脏区**矩形内的像素**（C1 / D22）。此前是"按整行拷"
                //   （`row*stride .. +stride`，忽略 `r.x`/`r.width`）
                // ⇒ 40×20 的脏区被展开成 20 行 × 全窗宽，呈现带宽白白翻倍。
                let _ = copy_damage_rects(src, dst, stride, &batches);
            }
        }

        if full {
            let _ = buffer.present();
        } else {
            let _ = buffer.present_with_damage(&batches);
        }
    }

    // ── 事件翻译 ──

    /// 该窗口当前的 DPI 缩放（窗口不在 ⇒ 1.0）
    fn window_scale(&self, id: WindowId) -> f32 {
        self.app.window_ctx(id).map(|c| c.scale_factor()).unwrap_or(1.0)
    }

    fn to_logical(&self, id: WindowId, p: PhysicalPosition<f64>) -> Point {
        physical_to_logical(p, self.window_scale(id))
    }

    fn push_input(&mut self, id: WindowId, ev: InputEvent) {
        let rt = self.app.runtime();
        if let Some(ctx) = self.app.window_ctx_mut(id) {
            ctx.pointer(&rt, ev);
        }
    }

    fn button_of(b: MouseButton) -> PointerButton {
        match b {
            MouseButton::Left => PointerButton::Left,
            MouseButton::Right => PointerButton::Right,
            MouseButton::Middle => PointerButton::Middle,
            MouseButton::Back => PointerButton::Other(3),
            MouseButton::Forward => PointerButton::Other(4),
            MouseButton::Other(n) => PointerButton::Other(n),
        }
    }

    fn modifiers(m: winit::keyboard::ModifiersState) -> Modifiers {
        let mut out = Modifiers::EMPTY;
        if m.shift_key() {
            out |= Modifiers::SHIFT;
        }
        if m.control_key() {
            out |= Modifiers::CTRL;
        }
        if m.alt_key() {
            out |= Modifiers::ALT;
        }
        if m.super_key() {
            out |= Modifiers::META;
        }
        out
    }

    /// Ctrl+C / X / V：直接操作聚焦的输入框（剪贴板归平台层，核心不依赖 `arboard`）
    fn handle_clipboard(&mut self, id: WindowId, ev: &Event) -> bool {
        let Event::Key {
            code, modifiers: mods, ..
        } = ev
        else {
            return false;
        };
        if !mods.ctrl() {
            return false;
        }
        let KeyCode::Char(c) = code else {
            return false;
        };
        let lower = c.to_ascii_lowercase();
        if !matches!(lower, 'c' | 'x' | 'v') {
            return false;
        }

        let focus = match self.app.window_ctx(id).and_then(|w| w.track().focused) {
            Some(f) => f,
            None => return false,
        };
        // 只有文本输入框吃剪贴板（其它组件返回 false，让事件继续冒泡）
        let is_input = self
            .app
            .window_ctx(id)
            .map(|w| matches!(w.track().get(focus).map(|n| &n.kind), Some(Kind::Input { .. })))
            .unwrap_or(false);
        if !is_input {
            return false;
        }

        let copied = self
            .app
            .window_ctx(id)
            .and_then(|w| w.track().input_selected_text(focus));
        match lower {
            'c' => {
                if let Some(text) = copied {
                    clipboard_set(&text);
                }
            }
            'x' => {
                if let Some(text) = copied {
                    clipboard_set(&text);
                    if let Some(w) = self.app.window_ctx_mut(id) {
                        w.track_mut().input_backspace(focus);
                    }
                }
            }
            'v' => {
                if let Some(text) = clipboard_get()
                    && let Some(w) = self.app.window_ctx_mut(id)
                {
                    w.track_mut().input_insert(focus, &text);
                }
            }
            _ => return false,
        }
        let rt = self.app.runtime();
        rt.mark(id, Dirty::PAINT | Dirty::PRESENT);
        if let Some(ws) = self.windows.get(&id) {
            ws.window.request_redraw();
        }
        true
    }

    fn key_code(key: &Key) -> KeyCode {
        match key {
            Key::Named(n) => KeyCode::Named(match n {
                NamedKey::Enter => crate::event::NamedKey::Enter,
                NamedKey::Escape => crate::event::NamedKey::Escape,
                NamedKey::Tab => crate::event::NamedKey::Tab,
                NamedKey::Backspace => crate::event::NamedKey::Backspace,
                NamedKey::Delete => crate::event::NamedKey::Delete,
                NamedKey::Insert => crate::event::NamedKey::Insert,
                NamedKey::Space => crate::event::NamedKey::Space,
                NamedKey::ArrowLeft => crate::event::NamedKey::Left,
                NamedKey::ArrowRight => crate::event::NamedKey::Right,
                NamedKey::ArrowUp => crate::event::NamedKey::Up,
                NamedKey::ArrowDown => crate::event::NamedKey::Down,
                NamedKey::Home => crate::event::NamedKey::Home,
                NamedKey::End => crate::event::NamedKey::End,
                NamedKey::PageUp => crate::event::NamedKey::PageUp,
                NamedKey::PageDown => crate::event::NamedKey::PageDown,
                NamedKey::Shift => crate::event::NamedKey::Shift,
                NamedKey::Control => crate::event::NamedKey::Control,
                NamedKey::Alt => crate::event::NamedKey::Alt,
                NamedKey::Meta => crate::event::NamedKey::Meta,
                other => crate::event::NamedKey::Other(*other as u32),
            }),
            Key::Character(s) => s
                .chars()
                .next()
                .map(KeyCode::Char)
                .unwrap_or(KeyCode::Named(crate::event::NamedKey::Other(0))),
            _ => KeyCode::Named(crate::event::NamedKey::Other(0)),
        }
    }

    fn on_window_event(&mut self, _el: &ActiveEventLoop, os_id: OsWindowId, event: WindowEvent) {
        let Some(id) = self.app_window_id(os_id) else {
            return;
        };
        let rt = self.app.runtime();

        match event {
            WindowEvent::CloseRequested => {
                let action = self
                    .app
                    .window_ctx_mut(id)
                    .map(|c| c.close_requested(&rt))
                    .unwrap_or(CloseAction::Close);
                if action == CloseAction::Close {
                    self.app.close_window(id);
                    self.destroy_window(id);
                }
            }

            // 系统深浅色切换：`ThemeMode::System` 时立即换主题（其余模式只记录）
            WindowEvent::ThemeChanged(t) => {
                rt.set_system_dark(matches!(t, winit::window::Theme::Dark));
            }

            WindowEvent::Resized(size) => {
                // 逻辑尺寸是**唯一真源**：物理尺寸只是它的 `× scale` 表现，
                // 事件到达时按**当前** scale 换算回去（顺序在 DPI 变化后 ⇒ 已是新 scale）。
                let scale = self.window_scale(id);
                if let Some(ctx) = self.app.window_ctx_mut(id) {
                    ctx.set_size(&rt, physical_size_to_logical(size, scale));
                }
            }

            // 系统 DPI 变了（拖到别的显示器 / 系统改缩放比例）。
            //
            // 契约：**逻辑尺寸不变**，物理尺寸按新 scale 放大 ⇒ 显式请求一次
            // `逻辑 × 新 scale`。winit 在 Windows 上默认已经算好同样的尺寸
            // （`platform_impl/windows/event_loop.rs` 按"旧物理 → 旧逻辑 → 新物理"换算），
            // 这里再写一次是为了不依赖平台默认值：X11/Wayland 上若不主动请求，
            // 窗口会保持原像素数 ⇒ 逻辑面积被除以 scale（内容突然"变小"）。
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut inner_size_writer,
            } => {
                let scale = scale_factor as f32;
                if scale.is_finite() && scale > 0.0 {
                    if let Some(logical) = self.app.window_ctx(id).map(|c| c.size()) {
                        // 失败（后端不支持同步改尺寸）不算错：平台的 `Resized` 会兜底。
                        let _ = inner_size_writer.request_inner_size(logical_size_to_physical(logical, scale));
                    }
                    if let Some(ctx) = self.app.window_ctx_mut(id) {
                        ctx.set_scale_factor(&rt, scale);
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let stats = {
                    let Some(ctx) = self.app.window_ctx_mut(id) else {
                        return;
                    };
                    ctx.tick(&rt, now);
                    ctx.frame(&rt)
                };
                if stats.present_pending {
                    self.present(id, &stats.damage, stats.damage_all);
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let pos = self.to_logical(id, position);
                self.cursor = pos;
                self.push_input(
                    id,
                    InputEvent::Move {
                        pointer: PointerId(0),
                        pos,
                    },
                );
            }

            WindowEvent::CursorLeft { .. } => {
                self.push_input(id, InputEvent::Leave);
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let b = Self::button_of(button);
                let pos = self.cursor;
                let ev = match state {
                    ElementState::Pressed => InputEvent::Down {
                        pointer: PointerId(0),
                        pos,
                        button: b,
                    },
                    ElementState::Released => InputEvent::Up {
                        pointer: PointerId(0),
                        pos,
                        button: b,
                    },
                };
                self.push_input(id, ev);
            }

            WindowEvent::MouseWheel { delta, .. } => {
                // 符号原样透传：winit 约定正 y = 滚轮上推（各平台一致，macOS 的
                // "自然滚动"由系统换算）。消费端见 `input::default_wheel_scroll`。
                let d = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x * 48.0, y * 48.0),
                    MouseScrollDelta::PixelDelta(p) => {
                        let scale = self
                            .app
                            .window_ctx(id)
                            .map(|c| c.scale_factor())
                            .unwrap_or(1.0)
                            .max(0.001);
                        (p.x as f32 / scale, p.y as f32 / scale)
                    }
                };
                let pos = self.cursor;
                self.push_input(
                    id,
                    InputEvent::Wheel {
                        pointer: PointerId(0),
                        pos,
                        delta: d,
                    },
                );
            }

            WindowEvent::Focused(false) => {
                // 丢焦点 ⇒ 取消指针交互（必发 PointerCaptureLost，拖拽态可清理）
                self.push_input(id, InputEvent::Cancel { pointer: PointerId(0) });
            }

            WindowEvent::ModifiersChanged(m) => {
                self.modifiers = m.state();
            }

            WindowEvent::KeyboardInput { event: key, .. } => {
                let code = Self::key_code(&key.logical_key);
                let kind = match key.state {
                    ElementState::Pressed => EventKind::KeyDown,
                    ElementState::Released => EventKind::KeyUp,
                };
                let mods = Self::modifiers(self.modifiers);

                // Tab / Shift+Tab：框架默认的焦点迁移
                if key.state == ElementState::Pressed && code == KeyCode::Named(crate::event::NamedKey::Tab) {
                    let forward = !self.modifiers.shift_key();
                    if let Some(ctx) = self.app.window_ctx_mut(id) {
                        ctx.tab(&rt, forward);
                    }
                }

                let mut ev = Event::key_with(kind, code, mods);
                if let Event::Key { text, repeat: r, .. } = &mut ev {
                    *r = key.repeat;
                    if kind == EventKind::KeyDown
                        && let Key::Character(s) = &key.logical_key
                    {
                        *text = Some(s.to_string());
                    }
                }
                // 剪贴板是 OS 服务 ⇒ 平台层先处理（光标在输入框里才生效）
                if kind == EventKind::KeyDown && self.handle_clipboard(id, &ev) {
                    return;
                }
                let path = self.focus_path(id);
                self.dispatch(id, &path, &ev);
            }

            WindowEvent::Ime(ime) => match ime {
                Ime::Commit(text) => {
                    // 每个字符一条 CharacterReceived（文本输入的正确入口）
                    for ch in text.chars() {
                        let path = self.focus_path(id);
                        self.dispatch(id, &path, &Event::char_received(ch));
                    }
                }
                Ime::Preedit(text, cursor) => {
                    let path = self.focus_path(id);
                    self.dispatch(
                        id,
                        &path,
                        &Event::ImePreedit {
                            text,
                            cursor: cursor.map(|(a, b)| (a as u32, b as u32)),
                        },
                    );
                }
                Ime::Enabled | Ime::Disabled => {}
            },

            _ => {}
        }
    }

    fn focus_path(&self, id: WindowId) -> Vec<crate::track::NodeId> {
        self.app
            .window_ctx(id)
            .map(|c| match c.track().focused {
                Some(f) => crate::hit::path_to(c.track(), f),
                None => c.content_root().map(|r| vec![r]).unwrap_or_default(),
            })
            .unwrap_or_default()
    }

    fn dispatch(&mut self, id: WindowId, path: &[crate::track::NodeId], ev: &Event) {
        if path.is_empty() {
            return;
        }
        let rt = self.app.runtime();
        if let Some(ctx) = self.app.window_ctx_mut(id) {
            ctx.dispatch(&rt, path, ev);
        }
    }
}

impl ApplicationHandler<AppEvent> for Runner {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        // 为所有已注册窗口建真实窗口（`App::window()` 在 run 之前注册的）
        let ids: Vec<WindowId> = self.app.windows().iter().map(|w| w.id()).collect();
        for id in ids {
            self.create_window(el, id);
        }
        self.tick(el, Instant::now());
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, event: AppEvent) {
        match event {
            // 后台线程只要 UI 跑一帧（如任务进度上报）—— 只消费定时器，渲染交给
            // 下面的"脏了就 request_redraw"（`frame` 收敛到 `RedrawRequested` 之后，
            // 这里再跑完整帧就又把入口扩回两个了）。
            AppEvent::Wake => {
                self.pump(Instant::now());
            }
            AppEvent::External { window, data } => {
                let rt = self.app.runtime();
                if let Some(ctx) = self.app.window_ctx_mut(window) {
                    ctx.external(&rt, data);
                }
                self.pump(Instant::now());
            }
        }
        // 唤醒后立刻重绘（避免等下一个周期）
        let dirty: Vec<WindowId> = self.app.windows().iter().map(|w| w.id()).collect();
        for id in dirty {
            let rt = self.app.runtime();
            if let Some(ctx) = self.app.window_ctx_mut(id) {
                let d = rt.peek_dirty(id);
                if d.contains(Dirty::PRESENT) || d.contains(Dirty::PAINT) || d.contains(Dirty::VIEW) {
                    if let Some(ws) = self.windows.get(&id) {
                        ws.window.request_redraw();
                    }
                    let _ = ctx;
                }
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, os_id: OsWindowId, event: WindowEvent) {
        self.on_window_event(el, os_id, event);
        // 交互产生的脏 ⇒ 请求重绘（winit 只在需要时才会给 RedrawRequested）
        if let Some(id) = self.app_window_id(os_id) {
            let rt = self.app.runtime();
            let d = rt.peek_dirty(id);
            if !d.is_empty()
                && let Some(ws) = self.windows.get(&id)
            {
                ws.window.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // ★ 主线 B：这里**不再**跑完整帧（`frame()` 收敛到 `RedrawRequested` 单一入口）。
        // 但**必须消费定时器**，否则定时器永远不被触发；而消费之后**必须请求重绘**，
        // 否则回调改了状态却没人画 ⇒ 停帧。这就是 `pump` 返回值的用途。
        if self.pump(Instant::now()) {
            self.request_redraw_all();
        }
        // 请求队列可能在事件里被写入（如关闭按钮 ⇒ `ctx.close_window`）
        let (opened, closed) = self.app.drain_requests();
        for id in opened {
            self.create_window(el, id);
        }
        for id in closed {
            self.destroy_window(id);
        }
        self.schedule_wakeup(el);
    }
}

/// 进入事件循环（阻塞）。`App` 被移入运行器。
///
/// `on_ready` 在事件循环建好后、进入循环前调用 —— 这是拿到 [`RepaintHandle`]
/// 交给后台线程的唯一时机（`EventLoopProxy` 必须由 `EventLoop` 创建）。
pub fn run(app: App, on_ready: Option<Box<dyn FnOnce(RepaintHandle)>>) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop: EventLoop<AppEvent> = EventLoop::with_user_event().build()?;
    let handle = RepaintHandle::new(event_loop.create_proxy());
    // 唤醒器注入运行时：`Runtime::spawn_task` / `TaskCtx::post` / `Runtime::wake`
    // 从此自带通道 —— 调用方**不再必须**用 `run_with_handle` 手动传递句柄。
    app.runtime().set_waker(Arc::new(handle.clone()));
    if let Some(f) = on_ready {
        f(handle);
    }
    let mut runner = Runner::new(app);
    event_loop.run_app(&mut runner)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use vello_cpu::color::PremulRgba8;

    fn px(r: u8, g: u8, b: u8, a: u8) -> PremulRgba8 {
        PremulRgba8::from_u8_array([r, g, b, a])
    }

    #[test]
    fn pack_xrgb_passes_through_opaque_pixels() {
        let src = [px(255, 128, 64, 255), px(0, 0, 0, 255)];
        let mut out = [0u32; 2];
        assert_eq!(pack_xrgb(&src, &mut out), 2);
        assert_eq!(out[0], 0x00FF_8040);
        assert_eq!(out[1], 0x0000_0000);
    }

    #[test]
    fn pack_xrgb_unpremultiplies_and_ignores_alpha() {
        // 半透明白：premultiplied 之后是 (128,128,128,128) ⇒ 反预乘回 (255,255,255)
        let src = [px(128, 128, 128, 128)];
        let mut out = [0u32; 1];
        pack_xrgb(&src, &mut out);
        assert_eq!(out[0], 0x00FF_FFFF);

        // alpha = 0 ⇒ 视为背景（黑），不产生除零
        let src = [px(0, 0, 0, 0)];
        pack_xrgb(&src, &mut out);
        assert_eq!(out[0], 0);
    }

    #[test]
    fn pack_xrgb_stops_at_the_shorter_side() {
        let src = [px(1, 2, 3, 255); 5];
        let mut out = [0u32; 2];
        assert_eq!(pack_xrgb(&src, &mut out), 2);
    }

    #[test]
    fn softbuffer_damage_scales_and_clamps() {
        let physical = Size::new(200.0, 100.0);
        // 逻辑 (10,10,20,20) 在 2x 下 = 物理 (20,20,40,40)
        let d = softbuffer_damage(physical, &[Rect::new(10.0, 10.0, 20.0, 20.0)], false, 2.0);
        assert_eq!(d.len(), 1);
        assert_eq!(rect_of(&d[0]), (20, 20, 40, 40));

        // 越界被裁掉
        let d = softbuffer_damage(physical, &[Rect::new(500.0, 500.0, 10.0, 10.0)], false, 1.0);
        assert!(d.is_empty());

        // 整窗脏 ⇒ 一条覆盖全窗的矩形
        let d = softbuffer_damage(physical, &[], true, 1.0);
        assert_eq!(d.len(), 1);
        assert_eq!(rect_of(&d[0]), (0, 0, 200, 100));
    }

    #[test]
    fn softbuffer_damage_uses_physical_size_at_fractional_scale() {
        let physical = Size::new(150.0, 75.0); // 逻辑 100×50 @1.5x
        let d = softbuffer_damage(physical, &[Rect::new(0.0, 0.0, 100.0, 50.0)], false, 1.5);
        assert_eq!(d.len(), 1);
        assert_eq!(rect_of(&d[0]), (0, 0, 150, 75));
    }

    fn rect_of(r: &softbuffer::Rect) -> (u32, u32, u32, u32) {
        (r.x, r.y, r.width.get(), r.height.get())
    }

    // ── DPI：逻辑 / 物理换算 ──

    /// 光标：winit 给的是**物理**坐标 ⇒ 命中前必须除以 scale。
    ///
    /// 这条测试是**防回归的钉子**：曾经有人以为 winit 给的是逻辑坐标，
    /// 差点把这行除法当成"重复换算"删掉（那会让 2× 屏上的点击全部错位一半）。
    #[test]
    fn cursor_position_is_converted_from_physical_to_logical() {
        let p = PhysicalPosition::new(300.0, 150.0);
        assert_eq!(physical_to_logical(p, 1.0), Point::new(300.0, 150.0));
        assert_eq!(physical_to_logical(p, 2.0), Point::new(150.0, 75.0));
        assert_eq!(physical_to_logical(p, 1.5), Point::new(200.0, 100.0));
    }

    /// 坏的 scale（0 / NaN / 负）不能把布局污染成 NaN：统一按 1.0 处理
    #[test]
    fn insane_scale_falls_back_to_one() {
        let p = PhysicalPosition::new(10.0, 20.0);
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(sane_scale(bad), 1.0, "scale = {bad}");
            assert_eq!(physical_to_logical(p, bad), Point::new(10.0, 20.0));
        }
    }

    /// 窗口尺寸的双向换算自洽：`物理 → 逻辑 → 物理` 回到原值（非小数缩放时精确）
    #[test]
    fn physical_and_logical_sizes_round_trip() {
        let logical = Size::new(800.0, 600.0);
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let physical = logical_size_to_physical(logical, scale);
            assert_eq!(physical.width, (800.0 * scale).round() as u32, "scale {scale}");
            assert_eq!(physical.height, (600.0 * scale).round() as u32, "scale {scale}");
            // 1.25 / 2 / 3 能精确回推；1.5 的 600 → 900 也精确
            let back = physical_size_to_logical(physical, scale);
            assert!(
                (back.width - logical.width).abs() < 0.001 && (back.height - logical.height).abs() < 0.001,
                "scale {scale}: {back:?} != {logical:?}"
            );
        }
    }

    /// 尺寸至少 1×1：缩到 0 会让 softbuffer 的 `resize` 拿不到 `NonZeroU32`
    #[test]
    fn physical_size_never_collapses_to_zero() {
        let p = logical_size_to_physical(Size::new(0.0, 0.0), 2.0);
        assert_eq!((p.width, p.height), (1, 1));
    }

    // ─────────────────── C1 / D22：局部上屏只拷矩形内像素 ───────────────────

    /// 构造 `w × h` 的 premul pixmap，像素值按 `(x, y)` 唯一编码，便于定位拷贝范围。
    fn marker_pixmap(w: u32, h: u32) -> Vec<vello_cpu::color::PremulRgba8> {
        use vello_cpu::color::PremulRgba8;
        (0..(w * h) as usize)
            .map(|i| {
                let v = (i % 251 + 1) as u8;
                PremulRgba8 {
                    r: v,
                    g: v,
                    b: v,
                    a: 255,
                }
            })
            .collect()
    }

    /// ⚠️ `softbuffer::Length` 是 `NonZeroU32` ⇒ 长度 0 **无法表达**。
    /// `softbuffer_damage` 里同样用 `.max(1)` 兜底（见其 `nz`），这里保持一致：
    /// 传 0 会被夹成 1。
    fn sb_rect(x: u32, y: u32, w: u32, h: u32) -> softbuffer::Rect {
        let nz = |v: u32| NonZeroU32::new(v.max(1)).unwrap_or(NonZeroU32::MIN);
        softbuffer::Rect {
            x,
            y,
            width: nz(w),
            height: nz(h),
        }
    }

    /// 回归（D22 / C1）：局部上屏的拷贝量必须 **∝ 脏区面积**，而不是"脏区高度 × 全窗宽"。
    ///
    /// bug 表现：旧实现按 `row * stride .. +stride` 整行拷、忽略 `r.x` / `r.width`
    /// ⇒ 40×20 的脏区实际拷 20 × 全窗宽像素。
    #[test]
    fn copy_damage_rects_only_touches_the_rect() {
        const W: u32 = 200;
        const H: u32 = 100;
        let src = marker_pixmap(W, H);
        let mut dst = vec![0u32; src.len()];
        let stride = W as usize;

        // 一个 40×20 的小脏区
        let rects = [sb_rect(10, 5, 40, 20)];
        let copied = copy_damage_rects(&src, &mut dst, stride, &rects);

        assert_eq!(copied, 40 * 20, "拷贝量应恰好等于脏区面积");
        // 脏区外的像素一个都不该被动
        let touched = dst.iter().filter(|d| **d != 0).count();
        assert_eq!(touched, 40 * 20, "脏区外不得被写入");
        // 脏区内每个像素都应被写入，且值与源一致（打包后 r 通道在高位）
        for row in 5..25usize {
            for col in 10..50usize {
                let i = row * stride + col;
                assert_ne!(dst[i], 0, "({row},{col}) 在脏区内却没被拷贝");
            }
        }
        // 同行但脏区外（x < 10 / x >= 50）不应被动
        assert_eq!(dst[5 * stride + 9], 0, "同行脏区左侧不应被动");
        assert_eq!(dst[5 * stride + 50], 0, "同行脏区右侧不应被动");
        // 相邻行也不应被动
        assert_eq!(dst[4 * stride + 10], 0, "脏区上方一行不应被动");
        assert_eq!(dst[25 * stride + 10], 0, "脏区下方一行不应被动");
    }

    /// 多个不相邻的脏区：拷贝量应是各面积之和（而不是并集/整窗）。
    #[test]
    fn copy_damage_rects_sums_multiple_rects() {
        const W: u32 = 64;
        const H: u32 = 64;
        let src = marker_pixmap(W, H);
        let mut dst = vec![0u32; src.len()];
        let stride = W as usize;

        let rects = [sb_rect(0, 0, 10, 10), sb_rect(50, 50, 8, 8)];
        let copied = copy_damage_rects(&src, &mut dst, stride, &rects);
        assert_eq!(copied, 100 + 64);
        assert_eq!(dst.iter().filter(|d| **d != 0).count(), 100 + 64);
    }

    /// 越界必须**静默裁剪**而不是 panic（窗口被最小化 / 脏区超出表面尺寸）。
    #[test]
    fn copy_damage_rects_clips_out_of_bounds() {
        const W: u32 = 32;
        const H: u32 = 32;
        let src = marker_pixmap(W, H);
        let stride = W as usize;
        let mut dst = vec![0u32; src.len()];

        // 右边界溢出
        let copied = copy_damage_rects(&src, &mut dst, stride, &[sb_rect(30, 0, 10, 2)]);
        assert_eq!(copied, 2 * 2, "只应拷贝界内那2 列");
        // 下边界溢出
        let mut dst2 = vec![0u32; src.len()];
        let copied2 = copy_damage_rects(&src, &mut dst2, stride, &[sb_rect(0, 31, 4, 10)]);
        assert_eq!(copied2, 4, "下边界溢出：只拷界内那1 行 × 4 列");
        // 完全在界外
        let mut dst3 = vec![0u32; src.len()];
        assert_eq!(
            copy_damage_rects(&src, &mut dst3, stride, &[sb_rect(100, 100, 5, 5)]),
            0
        );
        assert!(dst3.iter().all(|d| *d == 0));
        // 零尺寸：`Length` 是 NonZero ⇒ 宽度 0 被夹成 1 ⇒ 拷 1 像素（与 `softbuffer_damage` 的
        // `.max(1)` 兜底一致）。断言的是这个**已记录在案**的行为，不是"零尺寸不拷"。
        let mut dst4 = vec![0u32; src.len()];
        assert_eq!(copy_damage_rects(&src, &mut dst4, stride, &[sb_rect(0, 0, 0, 1)]), 1);
        // stride 为 0 不应除零 panic
        assert_eq!(copy_damage_rects(&src, &mut dst3, 0, &[sb_rect(0, 0, 5, 5)]), 0);
    }

    /// 回归对照：**旧行为**（整行拷）的拷贝量是多少 —— 量化 D22 到底浪费了多少。
    /// 这条不是断言旧行为，而是把"整窗 × 高度"与"脏区面积"的倍数关系固定下来，
    /// 让"修了之后省了多少"有据可查。
    #[test]
    fn old_behaviour_would_copy_the_whole_row() {
        const W: u32 = 200;
        const H: u32 = 100;
        let src = marker_pixmap(W, H);
        let stride = W as usize;
        let rect = sb_rect(10, 5, 40, 20);

        let new_copied = {
            let mut dst = vec![0u32; src.len()];
            copy_damage_rects(&src, &mut dst, stride, &[rect])
        };
        let old_copied = rect.height.get() as usize * stride; // 旧实现：整行 × 行数

        assert_eq!(new_copied, 800);
        assert_eq!(old_copied, 20 * 200);
        assert_eq!(
            old_copied / new_copied,
            5,
            "40×20 的脏区，旧实现多拷了 5 倍（= 整窗宽 / 脏区宽）"
        );
    }
}
