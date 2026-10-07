//! 窗口与帧驱动（M1 骨架：**不含 winit、不含光栅化**）。
//!
//! 这一层把前面几块拼起来：
//!
//! ```text
//! Signal::set ──▶ Runtime.dirty[window] |= VIEW        （只置位，不立即干活）
//!                        │
//!           App::frame_all() / WindowCtx::frame()
//!                        ▼
//!   ① dirty.VIEW   → begin_view → vm.view(&mut view_buf) → end_view → align()
//!   ② dirty.LAYOUT → （M2：脏边界重排）——  M1 只报告"待重排"
//!   ③ dirty.PAINT  → （M3：脏区光栅化）——  M1 只取走脏矩形
//!   ④ dirty.PRESENT→ （M3：damage 上屏）
//! ```
//!
//! 多窗口（设计 §3.14）：`Runtime` / `Signal` 是 **App 级共享**；`Track` / 描述缓冲 / 脏标志
//! 是**每窗口一套**。`App` 通过 `Rc<dyn WindowView>` 擦除，所以**不同窗口可以是不同的 `ViewModel` 类型**，
//! 连 `App` 本身都不带泛型。

use std::any::Any;
use std::rc::Rc;
use std::time::Instant;

use lieui_geom::{Rect, Size};

use lieui_geom::Point;

use crate::align::{AlignStats, align};
use crate::cmd::{CmdBuf, apply_cmds};
use crate::event::{Ctx, DispatchOutcome, Event, EventKind, EventView, PointerButton};
use crate::event::{KeyCode, NamedKey};
use crate::focus;
use crate::hit;
use crate::input::{self, InputEvent};
use crate::layout::{self, LayoutStats};
use crate::reactive::{Dirty, Runtime};
use crate::render::{RenderStats, Renderer};
use crate::track::{FocusState, Layer, NodeId, Track};
use crate::view::ViewBuf;
use crate::window::WindowId;

// ───────────────────────── 窗口配置 ─────────────────────────

/// 窗口配置（M4 会补 icon / position / 主题 / 字体等）
#[derive(Clone, Debug, PartialEq)]
pub struct WindowConfig {
    pub title: String,
    pub size: Size,
    pub min_size: Option<Size>,
    pub max_size: Option<Size>,
    /// 窗口底色（必须**不透明**：softbuffer 无 alpha 通道）。
    /// `None` = 跟随主题的 `window_background`（`rt.set_theme` 时随之变化）；
    /// 显式设置的底色**优先于主题**（主题切换不再改它）。
    pub background: Option<lieui_geom::Color>,
    pub resizable: bool,
    pub decorations: bool,
    pub always_on_top: bool,
    /// 是否使用框架的 loading 遮罩（默认 `true`）。
    ///
    /// 关掉后 `Runtime::begin_busy` / `spawn_task_busy` 只维护状态，
    /// 遮罩由用户自己渲染（`Runtime::busy_items` 能拿到忙碌项）。
    pub auto_busy_overlay: bool,
    /// **整窗重绘模式**（默认 `false` = 用脏区局部重绘）。
    ///
    /// 打开后每帧都整窗重绘：脏区带来的收益（实测 1280×720 下局部重绘比整窗快
    /// 20~90×）全部放弃，换来"绝不会因为漏标脏区而留残影"的确定性。
    /// 用途：① 排查残影类 bug 时一键对照；② 对正确性要求高于性能的场景。
    pub full_repaint: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "lieui".to_string(),
            size: Size::new(800.0, 600.0),
            min_size: None,
            max_size: None,
            background: None,
            resizable: true,
            decorations: true,
            always_on_top: false,
            auto_busy_overlay: true,
            full_repaint: false,
        }
    }
}

impl WindowConfig {
    pub fn background(mut self, c: lieui_geom::Color) -> Self {
        self.background = Some(c);
        self
    }

    /// 关闭框架的 loading 遮罩（自己渲染，见 `Runtime::busy_items`）
    pub fn auto_busy_overlay(mut self, on: bool) -> Self {
        self.auto_busy_overlay = on;
        self
    }

    /// 整窗重绘模式：放弃脏区带来的局部重绘收益，换取"绝不因漏标而残影"的确定性。
    ///
    /// 排查残影 bug 时的对照开关：若打开后残影消失，说明某处**漏标脏区**
    /// （自绘节点改状态没 `cx.damage(..)`、改了非 `Signal` 状态没 `invalidate`……）。
    pub fn full_repaint(mut self, on: bool) -> Self {
        self.full_repaint = on;
        self
    }
}

impl WindowConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }

    pub fn size(mut self, w: f32, h: f32) -> Self {
        self.size = Size::new(w, h);
        self
    }

    pub fn min_size(mut self, w: f32, h: f32) -> Self {
        self.min_size = Some(Size::new(w, h));
        self
    }

    pub fn max_size(mut self, w: f32, h: f32) -> Self {
        self.max_size = Some(Size::new(w, h));
        self
    }

    pub fn resizable(mut self, v: bool) -> Self {
        self.resizable = v;
        self
    }

    pub fn decorations(mut self, v: bool) -> Self {
        self.decorations = v;
        self
    }

    pub fn always_on_top(mut self, v: bool) -> Self {
        self.always_on_top = v;
        self
    }
}

/// 关闭请求的裁决（`GettingFocus` 那种"返回值可取消"风格，不用闭包）
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum CloseAction {
    #[default]
    Close,
    Cancel,
}

// ───────────────────────── 请求队列的便捷入口 ─────────────────────────

/// `Ctx` 请求关窗的载荷
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CloseWindow(pub WindowId);

/// 在 `update()` / `on_tick()` 里请求开一个新窗口（真窗口由平台层在下一帧创建）。
///
/// ```ignore
/// fn on_tick(self: &Rc<Self>, cx: &mut Ctx, _now: Instant) {
///     if self.need_about.get() {
///         self.need_about.set(false);
///         lieui::app::open_window(cx, WindowConfig::new().title("关于").size(320.0, 200.0), About::default());
///     }
/// }
/// ```
pub fn open_window(cx: &Ctx, cfg: WindowConfig, vm: impl ViewModel) {
    cx.request::<(WindowConfig, Rc<dyn WindowView>)>((cfg, erased(Rc::new(vm))));
}

/// 请求关闭**当前**窗口（`CloseAction::Close` 之外的显式入口）
pub fn close_self(cx: &Ctx) {
    cx.request(CloseWindow(cx.window()));
}

// ───────────────────────── 外部数据 ─────────────────────────

/// 外部来源推给 UI 线程的数据（PTY 输出、后台任务结果……）。
///
/// 必须 `Send`：它要能穿过 `RepaintHandle`（`EventLoopProxy`）从别的线程回到 UI 线程。
pub struct ExternalData(Box<dyn Any + Send>);

impl ExternalData {
    pub fn new<T: Send + 'static>(v: T) -> Self {
        Self(Box::new(v))
    }

    pub fn downcast<T: 'static>(self) -> Option<T> {
        self.0.downcast::<T>().ok().map(|b| *b)
    }

    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }
}

impl std::fmt::Debug for ExternalData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ExternalData(..)")
    }
}

// ───────────────────────── ViewModel ─────────────────────────

/// 窗口视图模型：**一个窗口一个实例**（不同窗口可以是不同类型，见 §3.14）。
///
/// 约束（§3.3）：
/// 1. `view()` **不在每帧跑**——挂载一次 + 每次 `Signal` 变更后再跑一次；
/// 2. `view()` 必须**无副作用**，内部禁止 `Signal::set`（debug 下会 panic）；
/// 3. 视图态（hover/滚动/编辑缓冲）不归 Model，由保留树持有。
pub trait ViewModel: 'static {
    /// 声明式：状态 → 描述。receiver 是 `&Rc<Self>`，让闭包能捕获 `Rc<Self>` 调自己的命令方法。
    fn view(self: &Rc<Self>, v: &mut ViewBuf);

    /// 外部数据（后台线程经 `RepaintHandle` 回到 UI 线程后调用）
    fn on_external(self: &Rc<Self>, _cx: &mut Ctx, _data: ExternalData) {}

    /// 关闭请求；返回 [`CloseAction::Cancel`] 可拦截
    fn on_close_request(self: &Rc<Self>, _cx: &mut Ctx) -> CloseAction {
        CloseAction::Close
    }

    /// 系统 DPI 缩放变了（窗口被拖到别的显示器 / 系统改了缩放比例）。
    ///
    /// **布局不用重排**——逻辑坐标系没变，变的只是"一个逻辑像素占几个物理像素"。
    /// 需要你做的只有一件事：**按像素缓存的资源作废**（PDF 页栅格、缩略图、位图图标）。
    /// 光栅器已经按新 scale 重建，几何 / 文本 / 图片都会以新分辨率重画；
    /// 但你自己缓存的那份 `Vec<u8>` 还是旧分辨率的，放大后会发虚。
    ///
    /// ```ignore
    /// fn on_scale_changed(self: &Rc<Self>, _cx: &mut Ctx, scale: f32) {
    ///     self.scale.set(scale);      // 渲染时用它算目标像素尺寸
    ///     self.cache.borrow_mut().clear();
    /// }
    /// ```
    ///
    /// 时机：窗口创建时（若系统缩放 ≠ 1.0）与之后每次变化各一次，都在**本帧 `view()` 之前**。
    /// 初始值：没收到回调就按 `1.0` 处理（100% 缩放不会触发回调）。
    fn on_scale_changed(self: &Rc<Self>, _cx: &mut Ctx, _scale: f32) {}

    /// 逐帧钩子（每帧都跑）；注意 `view()` 不会因此重跑，只重绘由 `cx.damage(..)` 指定的区域
    fn on_tick(self: &Rc<Self>, _cx: &mut Ctx, _now: Instant) {}

    /// **动画帧**：只在上一帧调过 `cx.request_animation()` 时被调用（见 [`crate::timer`]）。
    ///
    /// 想持续动画就在回调里再 `cx.request_animation()` 一次；停手即停帧（空闲零功耗）。
    /// `dt` = 距上一帧的时长（做按时间推进的动画用它，别假设固定步长）。
    /// 需要重绘时自己标脏（`cx.damage(node)`）——框架不会替你决定重绘范围。
    fn on_animation(self: &Rc<Self>, _cx: &mut Ctx, _now: Instant, _dt: std::time::Duration) {}
}

/// 对象安全的窗口视图。
///
/// `ViewModel::view` 的 receiver 是 `&Rc<Self>`，不能直接 `dyn`，所以加这一层薄擦除——
/// 换来"**不同窗口可以是不同的 `ViewModel` 类型**"，以及 `App` 自身不带泛型。
pub trait WindowView {
    fn view_erased(&self, v: &mut ViewBuf);
    fn on_tick(&self, cx: &mut Ctx, now: Instant);
    fn on_animation(&self, cx: &mut Ctx, now: Instant, dt: std::time::Duration);
    fn on_external(&self, cx: &mut Ctx, data: ExternalData);
    fn on_close_request(&self, cx: &mut Ctx) -> CloseAction;
    fn on_scale_changed(&self, cx: &mut Ctx, scale: f32);
}

/// 把 `Rc<V>` 包装成 `dyn WindowView` 的适配器。
///
/// 为什么需要这层包装：Rust 的 unsizing 强转 `Rc<V> → Rc<dyn Trait>` 要求 `V: Trait + Sized`，
/// 而 `ViewModel::view` 的 receiver 是 `&Rc<Self>` —— 拿不到 `Rc` 就调不了它。
/// 所以真正的 `WindowView` 实现挂在**持有 `Rc<V>` 的适配器**上（多一次指针跳转，可忽略）。
struct VmAdapter<V: ViewModel>(Rc<V>);

impl<V: ViewModel> WindowView for VmAdapter<V> {
    fn view_erased(&self, v: &mut ViewBuf) {
        V::view(&self.0, v)
    }

    fn on_tick(&self, cx: &mut Ctx, now: Instant) {
        V::on_tick(&self.0, cx, now)
    }

    fn on_animation(&self, cx: &mut Ctx, now: Instant, dt: std::time::Duration) {
        V::on_animation(&self.0, cx, now, dt)
    }

    fn on_external(&self, cx: &mut Ctx, data: ExternalData) {
        V::on_external(&self.0, cx, data)
    }

    fn on_close_request(&self, cx: &mut Ctx) -> CloseAction {
        V::on_close_request(&self.0, cx)
    }

    fn on_scale_changed(&self, cx: &mut Ctx, scale: f32) {
        V::on_scale_changed(&self.0, cx, scale)
    }
}

/// 把 `Rc<VM>` 擦除成窗口视图句柄。
///
/// 同一个 `Rc<VM>` 可以擦除多次给多个窗口（**共享一个 VM 实例**），
/// 也可以各建各的（每个窗口一个实例）。
pub fn erased<V: ViewModel>(vm: Rc<V>) -> Rc<dyn WindowView> {
    Rc::new(VmAdapter(vm))
}

// ───────────────────────── 帧统计 ─────────────────────────

/// 一帧做了/该做什么（测试与调试的观测点）
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameStats {
    /// 本帧消费掉的脏标志
    pub dirty: Dirty,
    /// 是否重跑了 `view()` + `align`
    pub view_ran: bool,
    pub align: AlignStats,
    /// 本次重排统计（`ran == false` 表示本帧没有重排）
    pub layout: LayoutStats,
    /// 本帧被重新落位的锚定层数（popup / tooltip 跟随锚点）
    pub anchored_layers: usize,
    /// 跑完后**仍有**待重排（正常恒为 `false`）
    pub layout_pending: bool,
    /// 本帧是否需要重绘（消费脏区前的事实）
    pub paint_pending: bool,
    /// 待上屏（**M4** 由 softbuffer 消费；M3 只到"像素已就绪"）
    pub present_pending: bool,
    /// 本帧累积的脏矩形（M3 用它决定行带；M4 会连同一起交给 `present_with_damage`）
    pub damage: Vec<Rect>,
    /// 整窗脏
    pub damage_all: bool,
    /// 本次展开 + 光栅化统计
    pub render: RenderStats,
}

impl FrameStats {
    /// 这一帧有没有实际干活
    pub fn is_idle(&self) -> bool {
        !self.view_ran && !self.layout.ran && !self.paint_pending && !self.present_pending
    }

    /// 本帧光栅化的像素数（基准/调试用）
    pub fn rasterized_pixels(&self) -> u64 {
        self.render.raster.pixels
    }
}

/// 一次指针输入的处理结果
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointerOutcome {
    /// 实际分发的事件条数（含 hover 进入/离开与合成的 `Tapped`）
    pub events: usize,
    /// 是否有任一处理器声明已处理
    pub handled: bool,
    /// 合成的点击目标
    pub tapped: Option<NodeId>,
    /// 是否触发了滚轮的框架默认滚动
    pub scrolled: bool,
}

// ───────────────────────── 窗口运行时 ─────────────────────────

/// 一个窗口的全部可变状态（每窗口一套）
pub struct WindowCtx {
    id: WindowId,
    cfg: WindowConfig,
    size: Size,
    track: Track,
    view_buf: ViewBuf,
    view: Rc<dyn WindowView>,
    last_align: AlignStats,
    renderer: Renderer,
    /// 底色是否由用户显式指定（true ⇒ 主题切换不再改窗口底色）
    explicit_background: bool,
    /// 框架自管的交互会话（tooltip / 右键菜单 / 闪烁时钟 / loading spinner）
    ///
    /// 见 [`Sessions`] 的模块级说明：为什么聚在一起、为什么不装进 `Layer`。
    sess: Sessions,
    /// 上一帧的时刻（算动画 `dt`）
    last_tick: Instant,
    /// 本窗口的**时钟**下次到期时刻（定时器 / 动画帧；`frame()` 里刷新）
    next_clock: Option<Instant>,
    /// DPI 缩放**变过几次**（初值 0）。
    ///
    /// 应用按像素缓存的资源（PDF 页栅格 / 缩略图 / 图标位图）用它做失效判据：
    /// 纪元一变，旧栅格就是按错的物理分辨率渲染的（拖到 2× 屏上会发虚）。
    /// 与 [`WindowCtx::set_scale_factor`] / `ViewModel::on_scale_changed` 配套。
    scale_epoch: u64,
}

/// 悬停到 tooltip 浮出的延迟
pub const TOOLTIP_DELAY: std::time::Duration = std::time::Duration::from_millis(600);

/// 一个进行中的 tooltip 会话
struct TooltipSession {
    /// 带 tooltip 描述的节点（tooltip 层锚在它上面）
    target: NodeId,
    /// 悬停开始时刻（arm 计时起点）
    since: Instant,
    /// 已浮出的 tooltip 层根（`None` = 还在计时）
    layer: Option<crate::track::RootId>,
}

/// 一个打开着的**上下文菜单**会话（`DescRef::context_menu`）。
///
/// 状态全在框架里：app 只声明"这个元素有菜单"，**不用**自己存"谁被右键了 / 光标在哪"，
/// 也不用在 `view()` 里写 `if let Some(menu) = …`。
struct CtxMenuSession {
    /// 被右键的节点（菜单挂在它的描述里）
    target: NodeId,
    /// 右键时的光标位置（逻辑坐标）—— 弹层锚点
    at: Point,
    /// 该节点的菜单构造器（对齐时从节点搬来）
    builder: crate::menu::ContextMenu,
}

/// 框架自管的**交互会话**：每窗口一份、`WindowCtx` 里的一组临时运行态。
///
/// ## 它们为什么必须是同一种东西
///
/// 都是"框架替应用记住的、临时的交互状态"（目标节点 + 计时 + 实例），
/// 都不属于 `view()` 的描述、也不属于应用 —— 所以放在一起，才能一眼看出
/// "哪几样是框架在替你记的"，而不是散在一个 17 字段的 struct 里靠注释分辨。
///
/// ## 为什么**不**装进 `Layer` / `LayerOpts`
///
/// 层是**描述**（`view()` 声明、`align` 增删）；会话是**运行态**（活的计时与目标）。
/// 混在一起会让 `align` 变成"两层状态的管理者"，直接破掉
/// "结构只由 `view()` 描述"这条不变量。会话与层的关系是：**会话*生产*层，而不是层**。
///
/// ## 三个层的生命周期不同，是刻意而非巧合
///
/// 取决于"内容要不要每帧重算"：
///
/// - **tooltip**：文案静态 ⇒ 框架自己建一次子树、走 `add_framework_root`
///   （`align` 不管它，也不重建内容）；
/// - **右键菜单 / loading 遮罩**：内容每帧都可能变（菜单要捕当下值、忙碌项会变）
///   ⇒ 每帧**注入**进描述树，由 `align` 增删。
#[derive(Default)]
struct Sessions {
    /// tooltip 会话（悬停 → 计时 → 浮出；见 [`WindowCtx::sync_tooltip`]）
    tooltip: Option<TooltipSession>,
    /// 打开着的右键菜单（见 [`DescRef::context_menu`]）
    ctx_menu: Option<CtxMenuSession>,
    /// 光标闪烁的**时钟**（相位本身在 [`crate::track::Track::blink_on`]）
    ///
    /// 分工：这里只存"下一次翻转时刻"（一种唤醒源），可见与否的状态由 `Track`
    /// 跟着焦点节点保留 —— 输入框跨帧存活、时钟随"有没有焦点"而有无。
    /// 没焦点的窗口这里恒为 `None` ⇒ 不产生唤醒 ⇒ 空闲零功耗。
    next_blink: Option<Instant>,
    /// loading 遮罩的 spinner 实例（跨帧保留；`Color` = 主题 accent，变了才重建）
    ///
    /// 为什么要缓存：spinner 的真身是树里的 `Kind::Custom` 节点，对齐按 **Rc 指针**
    /// 判等（同一 cell = 实例跨帧保留）。而遮罩每帧重新注入 ⇒ 每次新建 cell 会被
    /// 当成"换数据"而重建动画（一直在原地重新开始转）。所以按 accent 缓存同一个实例。
    spinner: Option<(crate::geom::Color, crate::custom::CustomCell)>,
}

/// 光标闪烁周期
pub const BLINK_PERIOD: std::time::Duration = std::time::Duration::from_millis(530);

impl WindowCtx {
    pub fn new(id: WindowId, cfg: WindowConfig, view: Rc<dyn WindowView>, rt: &Runtime) -> Self {
        rt.register_window(id);
        // 首帧必须跑一次 view()，否则树是空的
        rt.mark(id, Dirty::VIEW);
        let size = cfg.size;
        // 底色：显式设置优先；否则跟随主题
        let background = cfg.background.unwrap_or_else(|| rt.theme().window_background);
        let explicit_background = cfg.background.is_some();
        let renderer = Renderer::new(size, background);
        Self {
            id,
            cfg,
            size,
            track: Track::new(),
            view_buf: ViewBuf::new(),
            view,
            last_align: AlignStats::default(),
            renderer,
            explicit_background,
            sess: Sessions::default(),
            last_tick: Instant::now(),
            next_clock: None,
            scale_epoch: 0,
        }
    }

    pub fn id(&self) -> WindowId {
        self.id
    }

    pub fn config(&self) -> &WindowConfig {
        &self.cfg
    }

    /// 客户区尺寸（布局的可用空间）
    pub fn size(&self) -> Size {
        self.size
    }

    /// 窗口尺寸变化（M4 由 winit 调用）：整窗重排 + 整窗脏 + 重建 pixmap
    pub fn set_size(&mut self, rt: &Runtime, size: Size) {
        if self.size == size {
            return;
        }
        self.size = size;
        self.renderer.resize(size);
        self.track.mark_all_layout_dirty();
        rt.mark(self.id, Dirty::LAYOUT | Dirty::PAINT | Dirty::PRESENT);
    }

    /// 当前帧的图像（未渲染时是全零/旧帧）
    pub fn pixmap(&self) -> &vello_cpu::Pixmap {
        self.renderer.pixmap()
    }

    /// 换窗口底色（主题切换；调用方应同时 `mark_all_layout_dirty` + 整窗脏）
    pub fn set_background(&mut self, rt: &Runtime, color: lieui_geom::Color) {
        self.renderer.set_background(color);
        self.track.damage_whole_window();
        rt.mark(self.id, Dirty::PAINT | Dirty::PRESENT);
    }

    /// DPI 缩放变化（由平台层调用，Windows 上来自 winit `ScaleFactorChanged`）。
    ///
    /// **只改光栅分辨率，不动布局**：逻辑尺寸是唯一真源，DPI 变化不改变可用逻辑面积
    /// （窗口的物理尺寸由平台层按 `逻辑 × 新 scale` 请求回来）。所以这里做三件事：
    ///
    /// 1. 重建 pixmap（`physical = logical × scale`）；
    /// 2. 整窗脏 + `scale_epoch += 1`；
    /// 3. 回调 [`ViewModel::on_scale_changed`] —— 应用按像素缓存的资源（页栅格 / 缩略图）
    ///    该按新分辨率重建了，否则拖到 2× 屏上会被拉伸发虚。
    ///
    /// 返回是否真的变了（`scale` 非有限/≤0 或与当前一致 ⇒ `false`，不派发）。
    ///
    /// 注意：命中测试吃的是**逻辑**坐标，所以平台层要把 winit 的**物理**光标位置除以
    /// scale（winit 0.30 的 `WindowEvent::CursorMoved::position` 就是物理坐标）。
    pub fn set_scale_factor(&mut self, rt: &Runtime, scale: f32) -> bool {
        if !scale.is_finite() || scale <= 0.0 || (self.renderer.scale() - scale).abs() < 1e-6 {
            return false;
        }
        self.renderer.set_scale(scale);
        self.track.damage_whole_window();
        self.scale_epoch += 1;
        rt.mark(self.id, Dirty::PAINT | Dirty::PRESENT);

        let mut cx = Ctx::new(rt, self.id, EventView::external());
        self.view.on_scale_changed(&mut cx, scale);
        if !cx.cmds().is_empty() {
            let cmds = cx.take_cmds();
            let d = apply_cmds(&mut self.track, &cmds);
            rt.mark(self.id, d);
        }
        true
    }

    /// DPI 缩放变过几次（见 `ViewModel::on_scale_changed`）
    pub fn scale_epoch(&self) -> u64 {
        self.scale_epoch
    }

    pub fn scale_factor(&self) -> f32 {
        self.renderer.scale()
    }

    /// 物理尺寸（pixmap / softbuffer surface 的尺寸）
    pub fn physical_size(&self) -> Size {
        self.renderer.physical_size()
    }

    pub fn track(&self) -> &Track {
        &self.track
    }

    pub fn track_mut(&mut self) -> &mut Track {
        &mut self.track
    }

    pub fn view_buf(&self) -> &ViewBuf {
        &self.view_buf
    }

    /// 最近一次 `align` 的统计
    pub fn last_align(&self) -> AlignStats {
        self.last_align
    }

    /// 内容根（挂载后才有）
    pub fn content_root(&self) -> Option<NodeId> {
        self.track.content_root().map(|r| r.node)
    }

    // ── 帧 ──

    /// 跑一帧：消费脏标志 → `view()`/`align` → **重排** → 取走脏区（M3 在这里光栅化 + 上屏）。
    pub fn frame(&mut self, rt: &Runtime) -> FrameStats {
        let mut dirty = rt.take_dirty(self.id);
        // ⓪-bis 右键菜单会话的存活检查（**每帧**，理由见 `drop_dead_context_menu`）。
        //   它可能要关掉菜单 ⇒ 那就得**本帧**重跑 view()，否则描述树里还留着注入的弹层。
        if self.drop_dead_context_menu() {
            dirty |= Dirty::VIEW;
        }
        let mut st = FrameStats {
            dirty,
            ..Default::default()
        };

        // ⓪ 忙碌收尾：收掉"最短可见时间"已过的遮罩项（见 `task::set_busy_min_visible`）。
        // 放在帧首：快任务挂上遮罩后立刻结束的情况，也能保证遮罩被画出来过。
        rt.reap_busy(Instant::now());
        // 窗口尺寸登记（给 `view()` 里的"适应窗口"一类计算用；晚一帧无妨）
        rt.set_window_size(self.id, self.size);

        // ⓪ 主题同步：全局主题变了 ⇒ 渲染器换 token 快照 + **整窗重绘**。
        // 窗口底色仅在"未显式指定"时跟随主题（`WindowConfig.background` 优先）。
        //
        // 为什么要整窗脏：换主题影响的不只是"自己有 paint 脏的节点"——窗口底色、以及画面里
        // 未被 token 覆盖的兜底色（光标/选区/滚动条/焦点框等）都会变；只靠既有脏矩形会留下
        // 旧底色的残块（此前 `set_theme` 只置 `Dirty::PAINT`，底色没被重画）。
        let theme = rt.theme();
        if self.renderer.options_mut().theme != theme {
            let bg_follows = !self.explicit_background;
            let opts = self.renderer.options_mut();
            opts.theme = theme;
            if bg_follows {
                opts.background = theme.window_background;
            }
            self.track.damage_whole_window();
        }

        // ① 响应式回路：任何 Signal 变更都只置 VIEW，这里统一消费一次（批处理免费）
        if dirty.contains(Dirty::VIEW) {
            // ★ 值守用 RAII 守卫（D8）：`view()` 是用户代码，panic 时手写的 `end_view()`
            //   永远不会执行 ⇒ `in_view` 永久污染 ⇒ 此后所有 `Signal::set` 都被拦。
            let _view_guard = rt.begin_view(self.id);
            // 主题快照先注入：DSL 在构造 widget 时把 token 烘焙进描述（设计 §3.10）
            self.view_buf.set_theme(rt.theme());
            self.view_buf.begin();
            self.view.view_erased(&mut self.view_buf);
            // 后台任务忙碌 ⇒ 追加框架 loading 遮罩层（声明式：忙碌项清空就不声明，
            // 下一帧 align 的 stale 清理会把旧层删掉）
            self.push_busy_overlay(rt);
            // 打开着的右键菜单：同样**追加**一个弹层（声明式：会话结束就不追加，
            // 下一帧 align 的 stale 清理会把旧层删掉）
            self.push_context_menu_layer();
            drop(_view_guard);

            st.view_ran = true;
            self.last_align = align(&mut self.track, &self.view_buf);
            st.align = self.last_align;
        }

        // ② 重排：只跑"边界集合"的子树（见 `layout::layout`）
        if dirty.contains(Dirty::LAYOUT) || self.track.has_layout_dirty() {
            st.layout = layout::layout(&mut self.track, self.size);
        }

        // ②.5 锚定层落位：popup / tooltip 要等布局给出锚点 rect 与自身尺寸才能定位。
        // 挪了层的旧 ∪ 新矩形已登记进脏区 ⇒ 本帧的渲染自然会覆盖。
        st.anchored_layers = layout::place_anchored_layers(&mut self.track, self.size);

        // ②.6 滚动变化：偏移真的变了（滚轮 / 拖滚动条 / ScrollTo）⇒ 给容器派发
        // `ScrollChanged`（Direct，带新偏移）。信号处理器据此重跑 view——虚拟列表靠它换窗。
        for sc_id in self.track.take_scroll_changes() {
            if !self.track.contains(sc_id) {
                continue;
            }
            let offset = self.track.scroll_offset(sc_id);
            let path = vec![sc_id];
            self.dispatch(rt, &path, &Event::Scroll { offset });
        }

        // ③ 光栅化：消费脏区（只重画受影响的行带，其余像素保留上一帧）。
        // `full_repaint` 模式：不看脏区，每帧整窗重绘（确定性优先）。
        st.layout_pending = self.track.has_layout_dirty();
        let (mut damage, mut damage_all) = self.track.take_damage();
        if self.cfg.full_repaint {
            damage.clear();
            damage_all = true;
        }
        st.damage = damage;
        st.damage_all = damage_all;
        st.paint_pending = dirty.contains(Dirty::PAINT) || st.damage_all || !st.damage.is_empty();
        if st.paint_pending {
            st.render = self.renderer.render(&self.track, &st.damage, st.damage_all);
        }

        // ④ 上屏：M4 由 softbuffer 消费（`present_with_damage`）；M3 到此"像素已就绪"
        st.present_pending = dirty.contains(Dirty::PRESENT) || st.paint_pending;

        // ⑤ 刷新时钟：下一帧何时该醒（定时器 / 动画帧），供 `next_wakeup` 汇总
        self.next_clock = rt.next_deadline(self.id);

        st
    }

    // ── 动画（光标闪烁）──

    /// 翻转闪烁相位。**只在有键盘聚焦的输入框时动作**——没有聚焦输入框的窗口
    /// 永远不会被唤醒（空闲帧零功耗）。返回 true ⇒ 本帧有重绘义务。
    ///
    /// 平台层负责定时唤醒（`ControlFlow::WaitUntil`）；打字会重置相位（光标常亮）。
    pub fn animate(&mut self, now: Instant) -> bool {
        // loading 遮罩：spinner 相位由挂钟决定 ⇒ 每个动画帧只需把卡片标脏重绘
        // （不重跑 `view()`，也不重排）
        if let Some(card) = self.busy_card() {
            self.track.mark_paint_dirty(card);
        }

        let focused_input = self.track.focused.filter(|f| self.track.input_is_active(*f)).is_some();

        if !focused_input {
            if self.track.blink_on {
                self.track.blink_on = false;
                self.sess.next_blink = None;
                // 擦掉残留的光标
                if let Some(f) = self.track.focused {
                    self.track.mark_paint_dirty(f);
                }
                return true;
            }
            self.sess.next_blink = None;
            return false;
        }

        match self.sess.next_blink {
            None => {
                // 刚拿到焦点：光标先亮
                self.track.blink_on = true;
                self.sess.next_blink = Some(now + BLINK_PERIOD);
                if let Some(f) = self.track.focused {
                    self.track.mark_paint_dirty(f);
                }
                true
            }
            Some(t) if now >= t => {
                self.track.blink_on = !self.track.blink_on;
                self.sess.next_blink = Some(now + BLINK_PERIOD);
                if let Some(f) = self.track.focused {
                    self.track.mark_paint_dirty(f);
                }
                true
            }
            Some(_) => false,
        }
    }

    /// 下一次需要唤醒的时刻（平台层据此设置 `ControlFlow::WaitUntil`）。
    ///
    /// 汇总**所有**唤醒源，平台层不必知道有哪些动画：
    /// - 框架内部：光标闪烁、tooltip 计时、loading spinner；
    /// - 时钟（[`crate::timer`]）：定时器到期、动画帧（`next_clock`，`frame()` 里刷新）。
    ///
    /// 没有任何来源 ⇒ `None`（平台用 `ControlFlow::Wait`：空闲零功耗）。
    ///
    /// `rt` 用来问"本窗口还有没有忙碌项"（忙碌项的真相在 [`Runtime`] 里，这里不再
    /// 缓存一份每帧刷新的快照 —— 缓存意味着多一处可能过期的真相，而调用方手里就有 `rt`）。
    pub fn next_wakeup(&self, rt: &Runtime) -> Option<Instant> {
        // 有遮罩（含"挂着等最短可见时间到点"的）⇒ 定时唤醒：驱动 spinner + 到点收尾
        let spin = rt
            .is_busy(self.id)
            .then(|| Instant::now() + crate::overlay::SPIN_PERIOD);
        let blink = match (self.sess.next_blink, self.sess.tooltip.as_ref()) {
            (Some(b), Some(t)) if t.layer.is_none() => Some(b.min(t.since + TOOLTIP_DELAY)),
            (_, Some(t)) if t.layer.is_none() => Some(t.since + TOOLTIP_DELAY),
            (b, _) => b,
        };
        [spin, blink, self.next_clock].into_iter().flatten().min()
    }

    // ── loading 遮罩（后台任务忙碌时由框架声明）──

    /// 声明本帧的忙碌遮罩（无忙碌项或窗口关闭了自动遮罩 ⇒ 什么都不做）
    fn push_busy_overlay(&mut self, rt: &Runtime) {
        if !self.cfg.auto_busy_overlay {
            return;
        }
        let items = rt.busy_items(self.id);
        if items.is_empty() {
            return;
        }
        // spinner 实例跨帧保留（主题 accent 变了才重建）
        let accent = rt.theme().accent;
        if self.sess.spinner.as_ref().map(|(c, _)| *c) != Some(accent) {
            self.sess.spinner = Some((accent, crate::custom::cell(crate::overlay::Spinner::new(accent))));
        }
        let spinner = self.sess.spinner.as_ref().unwrap().1.clone();
        crate::overlay::push_busy_overlay(&mut self.view_buf, &items, spinner);
    }

    /// 遮罩卡片节点（loading 动画的标脏目标）。
    ///
    /// 结构固定：tag 层根 → 第一个子节点（卡片）。找不到 ⇒ `None`（没有遮罩）。
    fn busy_card(&self) -> Option<NodeId> {
        let root = self.track.root_by_tag(crate::overlay::BUSY_OVERLAY_TAG)?;
        self.track.children(root.node).first().copied()
    }

    // ── 事件 ──
    ///
    /// `path` 是命中链（`path[0]` 最外层、`path.last()` 目标），M1 由调用方给出（测试/无头场景），
    /// M2 起由命中测试产出。
    pub fn dispatch(&mut self, rt: &Runtime, path: &[NodeId], ev: &Event) -> DispatchOutcome {
        let mut cmds = CmdBuf::new();

        // ① 用户处理器（两段式：只读收集 → 调用；处理器只拿 `&mut Ctx`）
        //
        // ★ **用户先于内置**（D54）。此前是"内置先跑、用户后跑"，于是用户的
        //   `cx.mark_handled()` 只能当马后炮：它既无法阻止**已跑完**的内置行为，
        //   也无法阻止**后续**的内置行为 —— 语义上等于没有。
        //   现在 `handled` 真正成为"事件已被消费"的开关：标记后内置行为不再执行，
        //   与 WinUI / WPF 的 `Handled` 语义一致。
        let out = crate::event::dispatch(rt, self.id, &self.track, path, ev, &mut cmds);

        // ② 框架内置行为（未 `handled` 时兜底；直接拿 `&mut Track`，不占处理器槽位）
        //
        //   传 `&Event` 而非摘要：IME 预编辑带字符串 payload。
        //   放在用户之后是**语义要求**，不是性能考量 —— 控件的默认行为
        //   （勾选切换、输入插入、拖拽跟踪）应当是"用户没接手时的兜底"。
        if !out.handled {
            crate::widgets::handle_route(&mut self.track, path, ev, &mut cmds);
        }

        // ③ 落命令（借用释放后统一写树）
        if !cmds.is_empty() {
            let d = apply_cmds(&mut self.track, cmds.as_slice());
            rt.mark(self.id, d);
        }

        // ④ 打字重置闪烁相位：输入期间光标保持常亮（500ms 内没有新输入才熄灭）
        if matches!(
            ev.kind(),
            EventKind::CharacterReceived | EventKind::KeyDown | EventKind::PointerPressed
        ) && self.track.focused.filter(|f| self.track.input_is_active(*f)).is_some()
        {
            self.track.blink_on = true;
            self.sess.next_blink = Some(Instant::now() + BLINK_PERIOD);
        }
        out
    }

    // ── 命中 ──

    /// 命中链（纯函数，无副作用）
    pub fn hit(&self, p: Point) -> Vec<NodeId> {
        hit::hit_path(&self.track, p)
    }

    /// 命中目标（最深节点）
    pub fn hit_target(&self, p: Point) -> Option<NodeId> {
        hit::hit_test(&self.track, p)
    }

    // ── 指针输入 ──

    /// 一步指针输入：状态机 → 逐个分发 → 未被处理的滚轮走框架默认滚动。
    ///
    /// M4 的 winit 层只需把 `CursorMoved` / `MouseInput` / `MouseWheel` 翻译成 [`InputEvent`]。
    pub fn pointer(&mut self, rt: &Runtime, ev: InputEvent) -> PointerOutcome {
        let step = input::step(&mut self.track, ev);
        let mut outcome = PointerOutcome {
            events: step.events.len(),
            tapped: step.tapped,
            ..Default::default()
        };

        let mut wheel_path: Option<Vec<NodeId>> = None;
        for (path, e) in step.events {
            if e.kind() == EventKind::PointerWheelChanged {
                wheel_path = Some(path.clone());
            }
            let d = self.dispatch(rt, &path, &e);
            outcome.handled |= d.handled;
        }

        if let InputEvent::Wheel { delta, .. } = ev
            && !outcome.handled
            && let Some(path) = wheel_path
        {
            outcome.scrolled = input::default_wheel_scroll(&mut self.track, &path, delta);
        }

        // tooltip 会话跟随 hover：命中链变了 ⇒ 重新定位目标（移开/按下 ⇒ 关闭）
        if step.hover_changed || matches!(ev, InputEvent::Down { .. }) {
            self.sync_tooltip();
        }
        // 右键菜单会话：右键打开 / 点外面或点菜单项后收起
        self.sync_context_menu(rt, &ev, outcome.tapped);

        outcome
    }

    // ── 上下文菜单（框架自管会话；注入成普通 Popup 层）──

    /// 命中链里**最深**那个声明了右键菜单的节点（`DescRef::context_menu`）
    fn context_menu_target(&self, pos: Point) -> Option<(NodeId, crate::menu::ContextMenu)> {
        self.hit(pos).into_iter().rev().find_map(|id| {
            let n = self.track.get(id)?;
            n.context_menu.clone().map(|menu| (id, menu))
        })
    }

    /// 打开着的菜单弹层的层根节点（按 [`crate::menu::CTX_MENU_TAG`] 认领）
    fn context_menu_root(&self) -> Option<NodeId> {
        self.track
            .roots_of(crate::track::Layer::Popup)
            .find(|r| r.tag == Some(crate::menu::CTX_MENU_TAG))
            .map(|r| r.node)
    }

    /// 视图中是否有**落在菜单弹层内**的节点（判断"这次点击算不算点菜单")
    fn hit_inside_context_menu(&self, pos: Point) -> bool {
        let Some(root) = self.context_menu_root() else {
            return false;
        };
        let path = self.hit(pos);
        path.contains(&root) || path.iter().any(|id| hit::path_to(&self.track, *id).contains(&root))
    }

    /// 指针输入后同步右键菜单会话（打开 / 收起）。
    ///
    /// 规则（对齐 WinUI `ContextFlyout`）：
    ///
    /// | 输入 | 结果 |
    /// |---|---|
    /// | 右键按在带菜单的元素上 | 打开（或换目标重新定位到新光标处） |
    /// | 右键按在没有菜单的地方 | 收起 |
    /// | 左键按在菜单**外** | 收起（轻关闭） |
    /// | 菜单项被点击 | 收起（先派发 `Tapped` 再收，与 WinUI 一致） |
    ///
    /// 会话一变就置 `Dirty::VIEW`：弹层是"注入进描述树"的，不重跑 `view()` 它不会出现。
    fn sync_context_menu(&mut self, rt: &Runtime, ev: &InputEvent, tapped: Option<NodeId>) {
        let pos = ev.pos();
        let mut open: Option<CtxMenuSession> = None;
        let mut close = false;

        match ev {
            InputEvent::Down {
                button: PointerButton::Right,
                ..
            } => match pos.and_then(|p| self.context_menu_target(p)) {
                Some((target, builder)) => {
                    // 换个目标就换会话（同一个目标重复右键：仍按新位置重开，跟手）
                    let at = pos.expect("有命中就有位置");
                    let same = self
                        .sess
                        .ctx_menu
                        .as_ref()
                        .is_some_and(|s| s.target == target && s.at == at);
                    if !same {
                        open = Some(CtxMenuSession { target, at, builder });
                    }
                }
                None => close = true,
            },
            InputEvent::Down { .. } => close = pos.is_some_and(|p| !self.hit_inside_context_menu(p)),
            _ => {}
        }

        // 菜单项被点击 ⇒ 收起（`Tapped` 派发已经在 `pointer()` 里完成了）。
        // 禁用项**除外**：它什么都没做，菜单也不该消失（WinUI 语义）。
        //
        // 判据是"命中路径上有没有禁用节点"，而不是"最深节点自己是不是禁用"：
        // 一个禁用行里的**最深命中通常是 spacer / 标签**（`flex_grow` 的空 Box 也会
        // 吃掉命中），只看最深节点会把"点禁用项"误判成"点到了可用项"。
        if let Some(tapped) = tapped
            && self.sess.ctx_menu.is_some()
            && let Some(root) = self.context_menu_root()
        {
            let path = hit::path_to(&self.track, tapped);
            if path.contains(&root)
                && !path
                    .iter()
                    .any(|id| self.track.get(*id).is_some_and(|n| !n.interaction.enabled))
            {
                close = true;
            }
        }

        let had = self.sess.ctx_menu.is_some();
        if close {
            self.sess.ctx_menu = None;
        }
        let opened = open.is_some();
        if let Some(sess) = open {
            self.sess.ctx_menu = Some(sess);
        }
        if close || had || opened {
            // 收起 / 打开 / 换目标都要重跑 view()（弹层是注入的）
            rt.mark(self.id, Dirty::VIEW);
        }
    }

    /// 把打开着的右键菜单**注入**进本帧的描述树（`frame()` 在 `align` 之前调）。
    ///
    /// 先做"目标还在吗"的检查（照 tooltip 会话的 `alive`）：元素被回收
    /// （虚拟列表滚走、切页面、那一项不再声明菜单）⇒ 会话结束、菜单不再注入。
    fn drop_dead_context_menu(&mut self) -> bool {
        if self.sess.ctx_menu.is_none() {
            return false;
        }
        // 存活判据：目标节点还在、且**仍声明着**菜单（元素被回收 / 那一项不再有菜单）。
        //
        // 为什么必须**每帧**查、不能只查 VIEW 帧：这件事是上一帧 `align` 才写进保留树的，
        // 而注入发生在 align **之前**。若只在 VIEW 分支里查，"变化那一帧"查到的还是旧树
        // ⇒ 菜单不会被清。
        let alive = self
            .sess
            .ctx_menu
            .as_ref()
            .is_some_and(|s| self.track.get(s.target).is_some_and(|n| n.context_menu.is_some()));
        if alive {
            return false;
        }
        self.sess.ctx_menu = None;
        // 调用方**必须**把 `Dirty::VIEW` 并进本帧（`rt.mark` 会落到下一帧的
        // `take_dirty` 之后，白等一帧）：弹层是"注入进描述树"的，不重跑 `view()`
        // 描述里就没有它，align 的 stale 清理也删不掉那一层 ⇒ 菜单留在屏幕上。
        true
    }

    /// 把打开着的右键菜单**注入**进本帧的描述树（`frame()` 在 `align` 之前调）
    fn push_context_menu_layer(&mut self) {
        let Some((at, builder)) = self.sess.ctx_menu.as_ref().map(|s| (s.at, s.builder.clone())) else {
            return;
        };
        self.view_buf.push_context_menu(at, &builder);
    }

    // ── tooltip（框架自管层）──

    /// 根据当前 hover 链重算 tooltip 目标：从最深节点向上找第一个带 tooltip 的。
    /// 目标变了 ⇒ 关掉旧层、重新计时；没有目标 ⇒ 整个会话结束。
    fn sync_tooltip(&mut self) {
        let target = self
            .track
            .hover_path
            .iter()
            .rev()
            .copied()
            .find(|id| self.track.get(*id).is_some_and(|n| n.tooltip.is_some()));

        match (target, &mut self.sess.tooltip) {
            (Some(t), Some(s)) if s.target == t => {} // 悬停目标没变：继续计时/保持
            (t, s) => {
                // 目标变了（或离开）：关掉旧层
                if let Some(sess) = s
                    && let Some(rid) = sess.layer.take()
                {
                    self.track.remove_root(rid);
                }
                *s = t.map(|target| TooltipSession {
                    target,
                    since: Instant::now(),
                    layer: None,
                });
            }
        }
    }

    /// 已浮出的 tooltip 同步主题配色。
    ///
    /// tooltip 层是**框架自管层**（`align` 不会删除它，也不会重建内容），所以主题切换后
    /// 必须在这里补一次上色，否则它一直挂着旧主题的底色/文字色。
    fn sync_tooltip_theme(&mut self) {
        let theme = self.renderer.options().theme;
        let Some(rid) = self.sess.tooltip.as_ref().and_then(|s| s.layer) else {
            return;
        };
        let Some(node) = self.track.root(rid).map(|r| r.node) else {
            return;
        };
        let changed = self.track.get(node).is_some_and(|n| {
            n.paint.background_color != Some(theme.tooltip_background) || n.text.color != theme.tooltip_text
        });
        if !changed {
            return;
        }
        if let Some(n) = self.track.get_mut(node) {
            n.paint.background_color = Some(theme.tooltip_background);
            n.text.color = theme.tooltip_text;
        }
        self.track.mark_paint_dirty(node);
    }

    /// 每帧推进 tooltip 会话（到时浮出；目标失效/文本消失 ⇒ 收回）。
    pub fn update_tooltip(&mut self, now: Instant) -> bool {
        self.sync_tooltip_theme();
        let theme = self.renderer.options().theme;
        let mut open: Option<(NodeId, String)> = None;
        let mut close: Option<crate::track::RootId> = None;

        if let Some(sess) = &mut self.sess.tooltip {
            let alive = self.track.get(sess.target).is_some_and(|n| n.tooltip.is_some());
            if !alive {
                if let Some(rid) = sess.layer.take() {
                    close = Some(rid);
                }
                self.sess.tooltip = None;
            } else if sess.layer.is_none() && now >= sess.since + TOOLTIP_DELAY {
                open = self
                    .track
                    .get(sess.target)
                    .and_then(|n| n.tooltip.clone())
                    .map(|text| (sess.target, text));
            }
        }

        let changed = open.is_some() || close.is_some();
        if let Some(rid) = close {
            self.track.remove_root(rid);
        }
        if let Some((target, text)) = open {
            let rid = self.open_tooltip_layer(&theme, target, text);
            if let Some(sess) = &mut self.sess.tooltip {
                sess.layer = Some(rid);
            }
        }
        changed
    }

    /// 建 tooltip 层（锚到目标节点右侧，放不下自动翻到左侧/钳到视口）。
    fn open_tooltip_layer(
        &mut self,
        theme: &crate::theme::Theme,
        target: NodeId,
        text: String,
    ) -> crate::track::RootId {
        let node = self.track.create(crate::track::Kind::Text(text), None);
        if let Some(n) = self.track.get_mut(node) {
            n.paint.background_color = Some(theme.tooltip_background);
            n.paint.border_radius = 4.0;
            n.text.color = theme.tooltip_text;
            n.text.spec.font_size = 12.0;
            n.text.spec.wrap = false;
            // 按**墨迹盒**参与尺寸与居中：节点高 = 墨迹高 ⇒ 上下各 6px 的 padding 对称，
            // 字形视觉中心与卡片中心重合（行盒的 ascent/descent 不对称会让文本看着偏上）。
            n.text.spec.optical_align = true;
            n.layout = n
                .layout
                .clone()
                .padding_top(6.0)
                .padding_bottom(6.0)
                .padding_left(10.0)
                .padding_right(10.0);
        }
        let rid = self.track.add_framework_root(crate::track::Layer::Tooltip, None, node);
        if let Some(r) = self.track.root_mut(rid) {
            r.opts.anchor = Some(crate::track::Anchor {
                target: crate::track::AnchorTarget::Node(target),
                placement: crate::track::Placement::RightOf,
            });
        }
        rid
    }

    // ── 键盘 / 焦点 ──

    /// 键盘事件：发给焦点节点的祖先链（无焦点时发给内容根，冒泡到根）
    pub fn key(&mut self, rt: &Runtime, ev: Event) -> DispatchOutcome {
        // ★ Escape 关闭最上层的可关闭浮层（D15）。
        //
        // 为什么放在 `dispatch` **之前**：浮层关闭是**框架级**响应，
        // 不该依赖"恰好有节点监听 KeyDown"。此前 `NamedKey::Escape` 只有枚举、
        // 无任何消费点 —— 菜单 / 弹层一旦打开就**只能用鼠标点外面关掉**，
        // 这是所有 GUI 框架的基线能力。
        //
        // 语义与 WinUI / WPF 一致：**只关最上面一个**。菜单开着子菜单时，
        // 期望一次 Escape 只关子菜单，而不是把整串浮层一起关掉。
        // 因此复用 `dismiss_on_outside_click` 这个既有的"可关闭"配置，
        // 不另立一套开关。
        //
        // ★★ 必须同时校验 `code`：Escape 是 **`KeyCode::Named(NamedKey::Escape)`**
        //   而不是 `EventKind` 的变体。只判`kind == KeyDown` 会让**任意按键**
        //   都关闭弹层 —— 那是个很容易写出来、且很难在测试里发现的严重错误。
        if ev.kind() == EventKind::KeyDown
            && ev.summary().key == Some(KeyCode::Named(NamedKey::Escape))
            && let Some(dismissed) = self.escape_dismiss_topmost()
        {
            let path = hit::path_to(&self.track, dismissed);
            let out = self.dispatch(rt, &path, &Event::simple(EventKind::Dismissed));
            return DispatchOutcome { handled: true, ..out };
        }

        let path = match self.track.focused {
            Some(id) => hit::path_to(&self.track, id),
            None => match self.content_root() {
                Some(root) => hit::path_to(&self.track, root),
                None => Vec::new(),
            },
        };
        if path.is_empty() {
            return DispatchOutcome::default();
        }
        self.dispatch(rt, &path, &ev)
    }

    /// z序**最上面**那个可关闭浮层的层根节点（`Dismissed` 事件的收件人）。
    ///
    /// 与 `input::dismiss_outside_popups` 的区别：后者是"点击在**所有**弹层外"
    /// ⇒ 一次性全关；本函数是"Escape" ⇒ **只关最上面一个**。
    /// 用 `z_ordered_roots_top_down()` 保证与绘制顺序一致（D6 建立的同源遍历）。
    fn escape_dismiss_topmost(&self) -> Option<NodeId> {
        self.track.z_ordered_roots_top_down().into_iter().find_map(|r| {
            let closable = matches!(r.layer, Layer::Popup | Layer::Tooltip) && r.opts.dismiss_on_outside_click;
            closable.then_some(r.node)
        })
    }

    /// Tab / Shift+Tab 焦点迁移（框架默认行为），并派发 `LostFocus` / `GotFocus`
    pub fn tab(&mut self, rt: &Runtime, forward: bool) -> Option<NodeId> {
        let target = focus::next_tab(&self.track, self.track.focused, forward);
        self.focus(rt, target, FocusState::Keyboard);
        target
    }

    /// 程序化聚焦（并派发 `LostFocus` / `GotFocus`）
    pub fn focus(&mut self, rt: &Runtime, target: Option<NodeId>, state: FocusState) {
        let change = focus::set_focus(&mut self.track, target, state);
        for (id, kind) in [(change.lost, EventKind::LostFocus), (change.got, EventKind::GotFocus)] {
            if let Some(id) = id {
                let path = hit::path_to(&self.track, id);
                self.dispatch(rt, &path, &Event::simple(kind));
            }
        }
    }

    /// 逐帧钩子。`cx.damage(..)` 只标脏，不会重跑 `view()`。
    ///
    /// 每帧顺序（统一时钟，见 [`crate::timer`]）：
    /// ① 到期定时器（回调在表外执行 ⇒ 回调里能安全地再建定时器）
    /// ② 若上一帧请求过动画帧 ⇒ `ViewModel::on_animation`（带 `dt`）
    /// ③ `ViewModel::on_tick`（框架既有钩子：光标闪烁相位等）
    ///
    /// 返回**本轮是否真的做了事**（消费了定时器 / 动画帧 / 产生了命令）。
    ///
    /// ★ 这个返回值是帧调度收敛的关键（`refactor-plan` 主线 B）：
    /// `RedrawRequested` 之外的唤醒点（`about_to_wait` / `user_event`）只**消费**定时器、
    /// **不渲染**；它们靠这个返回值决定"要不要 `request_redraw`"。
    /// 少了这一步 ⇒ 定时器回调改了状态却没有任何重绘请求 ⇒ **画面停在旧帧（停帧）**。
    pub fn tick(&mut self, rt: &Runtime, now: Instant) -> bool {
        // tooltip 会话推进（到时浮出 / 目标失效收回）
        let tooltip_ran = self.update_tooltip(now);
        let dt = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        let mut cx = Ctx::new(rt, self.id, EventView::tick());
        let mut did_work = tooltip_ran;

        // ① 定时器：取走到期的（表外执行，避免回调里借用到同一张表）
        let due = rt.take_due_timers(self.id, now);
        did_work |= !due.is_empty();
        for mut timer in due {
            if let Some(mut cb) = timer.take_cb() {
                cb(&mut cx);
                timer.cb = Some(cb); // 周期定时器要还回去
            }
            rt.reschedule_timer(timer, now);
        }

        // ② 动画帧（经典 RAF 语义：回调里再 `cx.request_animation()` 才继续）
        if rt.take_animation_request(self.id) {
            self.view.on_animation(&mut cx, now, dt);
            did_work = true;
        }

        // ③ 既有逐帧钩子
        self.view.on_tick(&mut cx, now);

        if !cx.cmds().is_empty() {
            let cmds = cx.take_cmds();
            let d = apply_cmds(&mut self.track, &cmds);
            rt.mark(self.id, d);
            did_work = true;
        }
        did_work
    }

    /// 外部数据（后台线程 → UI 线程）
    ///
    /// 框架先消费自己的消息（任务进度 / 任务完成收尾，见 [`crate::task`]）：
    /// - 进度消息 ⇒ 到此为止（驱动遮罩）；
    /// - 任务完成 ⇒ 框架收尾（清任务表 + 收遮罩），**再**交给 `ViewModel::on_external`
    ///   取载荷。
    pub fn external(&mut self, rt: &Runtime, data: ExternalData) {
        if crate::task::on_task_message(rt, self.id, &data) {
            return;
        }
        let mut cx = Ctx::new(rt, self.id, EventView::external());
        self.view.on_external(&mut cx, data);
        if !cx.cmds().is_empty() {
            let cmds = cx.take_cmds();
            let d = apply_cmds(&mut self.track, &cmds);
            rt.mark(self.id, d);
        }
    }

    /// 关闭请求：`Cancel` 表示拦截
    pub fn close_requested(&mut self, rt: &Runtime) -> CloseAction {
        let mut cx = Ctx::new(rt, self.id, EventView::simple(EventKind::Unloaded));
        let action = self.view.on_close_request(&mut cx);
        // ★ 命令缓冲必须落树（D4）。此前建了 `cx`、调了回调，却从不 `take_cmds()` ——
        //   于是关闭回调里的 `cx.damage` / `cx.focus` / `cx.scroll_to` 全部静默失效，
        //   且没有任何报错。和 `external` / `tick` 走同一套收尾。
        // 无论用户是拦截(Cancel)还是确认关闭，落树都有意义：拦截时用户可能想改 UI。
        if !cx.cmds().is_empty() {
            let cmds = cx.take_cmds();
            let d = apply_cmds(&mut self.track, &cmds);
            rt.mark(self.id, d);
        }
        action
    }
}

impl std::fmt::Debug for WindowCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowCtx")
            .field("id", &self.id)
            .field("title", &self.cfg.title)
            .field("nodes", &self.track.len())
            .finish()
    }
}

// ───────────────────────── App ─────────────────────────

/// 应用：窗口表 + 共享运行时。
///
/// **没有泛型参数**——窗口的 `ViewModel` 已被擦除成 `Rc<dyn WindowView>`。
#[derive(Default)]
pub struct App {
    rt: Runtime,
    next_window: u32,
    windows: Vec<WindowCtx>,
}

impl App {
    pub fn new(rt: Runtime) -> Self {
        Self {
            rt,
            next_window: 1,
            windows: Vec::new(),
        }
    }

    pub fn runtime(&self) -> Runtime {
        self.rt.clone()
    }

    /// 注册一个窗口。`vm` 可以是任意 `impl ViewModel`（**不同窗口类型可以不同**）。
    pub fn window<V: ViewModel>(&mut self, cfg: WindowConfig, vm: V) -> WindowId {
        self.window_erased(cfg, erased(Rc::new(vm)))
    }

    /// 以**已擦除的句柄**注册窗口。
    ///
    /// 用于两个场景：① 多窗口**共享同一个 VM 实例**（`erased(rc.clone())` 两次）；
    /// ② 窗口视图由用户自己实现 `WindowView`（不经 `ViewModel`）。
    pub fn window_erased(&mut self, cfg: WindowConfig, view: Rc<dyn WindowView>) -> WindowId {
        let id = WindowId::new(self.next_window);
        self.next_window += 1;
        self.windows.push(WindowCtx::new(id, cfg, view, &self.rt));
        id
    }

    pub fn windows(&self) -> &[WindowCtx] {
        &self.windows
    }

    pub fn window_ctx(&self, id: WindowId) -> Option<&WindowCtx> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn window_ctx_mut(&mut self, id: WindowId) -> Option<&mut WindowCtx> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// 逐窗口跑一帧（**与平台层一帧同构**：消费外部事件 → tick（定时器/动画/on_tick）
    /// → frame）；返回每个窗口的 `FrameStats`。
    ///
    /// 顺带消费 [`Runtime::take_pending_external`]（本地投递队列）：**没有平台唤醒器**
    /// 时（无头测试、自己驱动帧）这就是"事件循环"，后台任务/自定义事件的投递会自动落地。
    ///
    /// ⚠️ 这里**不能**再假设"有平台时队列恒空"（A6 修复前的注释正是这么写的，而它是错的）：
    /// `TaskCtx` 持spawn 时的槽位快照，`run()` 之前 spawn 的任务会往`Local` 队列投递。
    /// 现在 `platform::Runner::tick` 也 drain 这个队列，且 `set_waker` 会给旧队列
    /// 装上转发器（见 [`crate::task`]），两边都 drain 之后该假设才成立。
    pub fn frame_all(&mut self) -> Vec<(WindowId, FrameStats)> {
        let rt = self.rt.clone();
        for (w, data) in rt.take_pending_external() {
            if let Some(ctx) = self.windows.iter_mut().find(|c| c.id == w) {
                ctx.external(&rt, data);
            }
        }
        let now = Instant::now();
        self.windows
            .iter_mut()
            .map(|w| {
                w.tick(&rt, now);
                (w.id, w.frame(&rt))
            })
            .collect()
    }

    /// 关闭窗口（注销其脏标志，丢弃其保留树）。
    ///
    /// 顺带**取消该窗口的后台任务**并清掉它的忙碌项（否则线程会继续跑到结束、
    /// 结果却无人接收；见 [`crate::task`]）。
    pub fn close_window(&mut self, id: WindowId) -> bool {
        let before = self.windows.len();
        self.windows.retain(|w| w.id != id);
        let removed = self.windows.len() != before;
        if removed {
            self.rt.cancel_tasks_of(id);
            self.rt.cancel_timers_of(id);
            self.rt.unregister_window(id);
        }
        removed
    }

    /// 立即以**已擦除句柄**开窗（`Ctx` 请求走 [`App::drain_requests`]）
    pub fn open_window_erased(&mut self, cfg: WindowConfig, view: Rc<dyn WindowView>) -> WindowId {
        let id = WindowId::new(self.next_window);
        self.next_window += 1;
        self.windows.push(WindowCtx::new(id, cfg, view, &self.rt));
        id
    }

    // ── 请求队列（`Ctx::request` / `app::open_window` 写入）──

    /// 处理待处理请求：开窗 / 关窗。返回 `(新增的窗口, 被关闭的窗口)` 供平台层同步真实窗口。
    ///
    /// 无窗口环境（测试）也能用 —— 这正是把请求做成队列而不是直接建窗的原因。
    pub fn drain_requests(&mut self) -> (Vec<WindowId>, Vec<WindowId>) {
        let opens: Vec<(WindowConfig, Rc<dyn WindowView>)> =
            self.rt.requests().take::<(WindowConfig, Rc<dyn WindowView>)>();
        let closes: Vec<CloseWindow> = self.rt.requests().take::<CloseWindow>();

        let mut opened = Vec::with_capacity(opens.len());
        for (cfg, view) in opens {
            opened.push(self.open_window_erased(cfg, view));
        }
        let mut closed = Vec::new();
        for CloseWindow(id) in closes {
            if self.close_window(id) {
                closed.push(id);
            }
        }
        (opened, closed)
    }

    /// 还有待处理请求吗
    pub fn has_pending_requests(&self) -> bool {
        !self.rt.requests().is_empty()
    }

    /// 进入 winit 事件循环（阻塞直到退出）。**M4**。
    ///
    /// 需要窗口层（`platform`）：它负责创建真实窗口、把 winit 事件翻译成
    /// [`InputEvent`] / [`Event`]，并把 pixmap 用 `present_with_damage` 上屏。
    #[cfg(feature = "winit")]
    pub fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        crate::platform::run(self, None)
    }

    /// 同上，但在进入循环前把 [`crate::platform::RepaintHandle`] 交给调用方
    /// （后台线程靠它唤醒 UI 线程）。
    #[cfg(feature = "winit")]
    pub fn run_with_handle(
        self,
        on_ready: impl FnOnce(crate::platform::RepaintHandle) + 'static,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::platform::run(self, Some(Box::new(on_ready)))
    }

    /// 本帧有窗口需要干活吗（M4 用它决定是否 `request_redraw`）
    pub fn any_dirty(&self) -> bool {
        self.rt.windows().iter().any(|w| !self.rt.peek_dirty(*w).is_empty())
    }

    // 说明：`run()`（winit 事件循环）与 `Ctx::open_window`（动态开窗）在 **M4** 落地——
    // 它们需要真实的 winit 窗口与事件循环；M1 只做"纯逻辑的帧驱动"，便于无头单测。
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, EventKind, KeyCode, Modifiers, NamedKey, PointerButton, PointerId};
    use crate::reactive::{Signal, act1};
    use crate::theme::Theme;
    use crate::track::{Kind, Layer, Placement};
    use std::time::Duration;

    // ── 一个 counter ViewModel（就是 §九 示例的形态，去掉 winit 部分）──

    struct Counter {
        count: Signal<i32>,
    }

    impl Counter {
        fn inc(&self) {
            self.count.update(|v| *v += 1);
        }
    }

    impl ViewModel for Counter {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            v.column(|c| {
                c.gap(12.0);
                c.text("Counter").font_size(48.0);
                c.text(self.count.get().to_string()).font_size(72.0);
                c.row(|r| {
                    r.button("+1").on_tap(act(self, Self::inc));
                });
            });
        }
    }

    fn act<T: 'static>(vm: &Rc<T>, f: fn(&T)) -> impl Fn() + 'static {
        let me = Rc::clone(vm);
        move || f(&me)
    }

    fn find_text(t: &Track, needle: &str) -> Option<NodeId> {
        t.descendants(t.content_root()?.node)
            .into_iter()
            .find(|n| matches!(t.get(*n).map(|x| &x.kind), Some(Kind::Text(s)) if s == needle))
    }

    fn setup() -> (Runtime, App, Rc<Counter>, WindowId) {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Counter {
            count: Signal::new(&rt, 0),
        });
        let id = app.window_erased(WindowConfig::new().title("Counter"), erased(vm.clone()));
        (rt, app, vm, id)
    }

    #[test]
    fn first_frame_mounts_the_tree() {
        let (_, mut app, _, id) = setup();
        let stats = app.frame_all();
        let (_, st) = &stats[0];

        assert!(st.view_ran);
        assert_eq!(st.align.created, 5, "column + 2 text + row + 1 button = 5 个节点");
        assert!(st.damage_all, "首次挂载整窗脏");

        let w = app.window_ctx(id).unwrap();
        assert_eq!(w.track().len(), 5);
        assert!(find_text(w.track(), "0").is_some());
    }

    #[test]
    fn second_frame_is_idle_without_state_change() {
        let (_, mut app, _, _) = setup();
        app.frame_all();
        let stats = app.frame_all();
        let (_, st) = &stats[0];

        assert!(st.is_idle(), "无状态变化应完全空闲：{st:?}");
        assert_eq!(st.align.patched, 0);
        assert!(st.damage.is_empty() && !st.damage_all);
    }

    #[test]
    fn signal_change_reruns_view_and_patches_only_that_node() {
        let (_, mut app, vm, id) = setup();
        app.frame_all();

        vm.count.set(1); // 只置脏，不立即干活
        assert!(app.any_dirty());

        let stats = app.frame_all();
        let (_, st) = &stats[0];
        assert!(st.view_ran);
        assert_eq!(st.align.patched, 1, "只有一个文本节点变化");
        assert_eq!(st.align.created, 0);
        assert_eq!(st.align.destroyed, 0);

        let w = app.window_ctx(id).unwrap();
        assert!(find_text(w.track(), "1").is_some());
        assert!(find_text(w.track(), "0").is_none());
        // 节点身份保持（视图态不丢）
        assert_eq!(w.track().len(), 5);
    }

    #[test]
    fn button_closure_changes_state_and_next_frame_renders_it() {
        let (rt, mut app, vm, id) = setup();
        app.frame_all();

        // 模拟点击：命中链 = 内容根 → row → button（M2 会由命中测试给出）
        let w = app.window_ctx(id).unwrap();
        let root = w.content_root().unwrap();
        let row = w.track().children(root)[2];
        let button = w.track().children(row)[0];
        let path = vec![root, row, button];

        let out = app.window_ctx_mut(id).unwrap().dispatch(
            &rt,
            &path,
            &Event::Simple {
                kind: EventKind::Tapped,
            },
        );
        assert_eq!(out.invoked, 1, "只有按钮注册了处理器");
        assert_eq!(vm.count.get(), 1, "闭包直接改了 Signal");

        // 下一帧：重跑 view() → align → 只 patch 那一行文本
        let stats = app.frame_all();
        assert!(stats[0].1.view_ran);
        assert_eq!(stats[0].1.align.patched, 1);
        let w = app.window_ctx(id).unwrap();
        assert!(find_text(w.track(), "1").is_some());
    }

    #[test]
    fn handler_can_request_repaint_without_rerunning_view() {
        struct Once {
            count: Signal<i32>,
        }
        impl ViewModel for Once {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text(self.count.get().to_string())
                        .on_tap_with(|cx| cx.request_repaint());
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(
            WindowConfig::new(),
            Once {
                count: Signal::new(&rt, 0),
            },
        );
        app.frame_all();

        // 命中链：内容根 → 那个文本节点（处理器挂在文本节点上）
        let w = app.window_ctx(id).unwrap();
        let root = w.content_root().unwrap();
        let text = w.track().children(root)[0];
        let path = vec![root, text];

        let d0 = rt.peek_dirty(id);
        assert!(d0.is_empty(), "空闲窗口不该带脏标志");

        app.window_ctx_mut(id).unwrap().dispatch(
            &rt,
            &path,
            &Event::Simple {
                kind: EventKind::Tapped,
            },
        );

        let stats = app.frame_all();
        let st = &stats[0].1;
        assert!(!st.view_ran, "request_repaint 不应触发 view()");
        assert!(st.paint_pending);
    }

    #[test]
    fn two_windows_share_signals_but_keep_separate_trees() {
        let rt = Runtime::new();
        let shared = Signal::new(&rt, 7);

        struct Panel {
            v: Signal<i32>,
            label: &'static str,
        }
        impl ViewModel for Panel {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text(format!("{} = {}", self.label, self.v.get()));
                });
            }
        }

        let mut app = App::new(rt.clone());
        let a = app.window(
            WindowConfig::new().title("A"),
            Panel {
                v: shared.clone(),
                label: "A",
            },
        );
        let b = app.window(
            WindowConfig::new().title("B"),
            Panel {
                v: shared.clone(),
                label: "B",
            },
        );
        app.frame_all();

        assert!(find_text(app.window_ctx(a).unwrap().track(), "A = 7").is_some());
        assert!(find_text(app.window_ctx(b).unwrap().track(), "B = 7").is_some());

        // 改共享 signal：两个窗口都会重跑 view()（R1 保守传播）
        shared.set(8);
        let stats = app.frame_all();
        assert!(stats[0].1.view_ran && stats[1].1.view_ran);
        assert!(find_text(app.window_ctx(a).unwrap().track(), "A = 8").is_some());
        assert!(find_text(app.window_ctx(b).unwrap().track(), "B = 8").is_some());

        // 两棵树的节点数一致但互不影响
        assert_eq!(
            app.window_ctx(a).unwrap().track().len(),
            app.window_ctx(b).unwrap().track().len()
        );
    }

    #[test]
    fn closing_a_window_drops_its_flags() {
        let (rt, mut app, _, id) = setup();
        app.frame_all();
        assert!(app.close_window(id));
        assert!(app.window_ctx(id).is_none());
        assert!(rt.windows().is_empty());
        assert!(!app.close_window(id), "重复关闭返回 false");
    }

    #[test]
    fn tick_hook_runs_without_rerunning_view() {
        use std::cell::Cell;

        struct Anim {
            ticks: Rc<Cell<u32>>,
        }
        impl ViewModel for Anim {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("t").key("t");
                });
            }
            fn on_tick(self: &Rc<Self>, cx: &mut Ctx, _now: Instant) {
                self.ticks.set(self.ticks.get() + 1);
                cx.damage_all();
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let ticks = Rc::new(Cell::new(0));
        let _ = app.window(WindowConfig::new(), Anim { ticks: ticks.clone() });
        app.frame_all();
        assert_eq!(ticks.get(), 1, "一帧 = 一次 on_tick");

        let stats = app.frame_all();
        assert_eq!(ticks.get(), 2, "每帧都跑 on_tick");
        let st = &stats[0].1;
        assert!(!st.view_ran, "on_tick 只重绘，不重跑 view()");
        assert!(st.paint_pending);
    }

    #[test]
    fn external_data_reaches_the_view_model() {
        struct Ext {
            got: Rc<std::cell::Cell<u32>>,
        }
        impl ViewModel for Ext {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("x");
                });
            }
            fn on_external(self: &Rc<Self>, cx: &mut Ctx, data: ExternalData) {
                if let Some(n) = data.downcast::<u32>() {
                    self.got.set(n);
                }
                cx.damage_all();
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let got = Rc::new(std::cell::Cell::new(0));
        let id = app.window(WindowConfig::new(), Ext { got: got.clone() });
        app.frame_all();

        app.window_ctx_mut(id).unwrap().external(&rt, ExternalData::new(42u32));
        assert_eq!(got.get(), 42);
        assert!(app.frame_all()[0].1.paint_pending);
    }

    #[test]
    fn close_request_can_be_cancelled() {
        struct Guard;
        impl ViewModel for Guard {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("g");
                });
            }
            fn on_close_request(self: &Rc<Self>, _cx: &mut Ctx) -> CloseAction {
                CloseAction::Cancel
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new(), Guard);
        app.frame_all();
        assert_eq!(
            app.window_ctx_mut(id).unwrap().close_requested(&rt),
            CloseAction::Cancel
        );
    }

    /// 回归（D4）：`close_requested` 必须把 `Ctx` 的命令缓冲落树。
    ///
    /// bug 表现：此前建了 `cx`、调了回调，却从不 `take_cmds()` ——
    /// 关闭回调里排队的 `damage` / `focus` / `scroll_to` **全部静默失效**，
    /// 且没有任何报错（用户只在"关窗前想改一下 UI"这种场景才会撞上）。
    #[test]
    fn close_request_applies_queued_commands() {
        struct Closer;
        impl ViewModel for Closer {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("g");
                });
            }
            fn on_close_request(self: &Rc<Self>, cx: &mut Ctx) -> CloseAction {
                cx.damage_all(); // 排队一条"整窗脏"，不需要 NodeId
                CloseAction::Cancel
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new(), Closer);
        app.frame_all();

        let w = app.window_ctx_mut(id).unwrap();
        let _ = w.track_mut().take_damage(); // 确保脏区是空的
        assert_eq!(w.close_requested(&rt), CloseAction::Cancel);

        let (damage, all) = w.track_mut().take_damage();
        assert!(
            all || !damage.is_empty(),
            "关闭回调里的 cx.damage_all() 必须落树（修复前这里恒为空）"
        );
    }

    // ─────────────── M2：布局 / 命中 / 输入 端到端 ───────────────

    fn center(t: &Track, id: NodeId) -> Point {
        let r = crate::layout::rect_of(t, id);
        Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
    }

    #[test]
    fn frame_runs_layout_then_goes_idle() {
        let (_, mut app, _, id) = setup();
        let first = app.frame_all()[0].1.clone();

        assert!(first.view_ran);
        assert!(first.layout.ran, "首帧必须重排");
        assert!(first.layout.nodes >= 5);
        assert!(!first.layout_pending, "跑完后不应再有重排义务");

        let w = app.window_ctx(id).unwrap();
        let root = w.content_root().unwrap();
        let rect = crate::layout::rect_of(w.track(), root);
        assert_eq!(rect.width, w.size().width, "内容根撑满窗口宽度");
        assert_eq!(rect.height, w.size().height);

        let second = app.frame_all()[0].1.clone();
        assert!(second.is_idle(), "无变化时第二帧应空闲：{second:?}");
    }

    #[test]
    fn resize_relayouts_the_whole_window() {
        let (rt, mut app, _, id) = setup();
        app.frame_all();

        app.window_ctx_mut(id).unwrap().set_size(&rt, Size::new(200.0, 150.0));
        let st = app.frame_all()[0].1.clone();
        assert!(st.layout.ran);
        assert!(st.layout.moved > 0, "尺寸变化应移动节点");
        assert!(st.damage_all);

        let w = app.window_ctx(id).unwrap();
        let root = w.content_root().unwrap();
        assert_eq!(crate::layout::rect_of(w.track(), root).width, 200.0);
    }

    #[test]
    fn click_reaches_the_button_through_hit_testing() {
        let (rt, mut app, vm, id) = setup();
        app.frame_all();

        let w = app.window_ctx(id).unwrap();
        let root = w.content_root().unwrap();
        let row = w.track().children(root)[2];
        let button = w.track().children(row)[0];
        let pos = center(w.track(), button);
        assert!(crate::layout::rect_of(w.track(), button).width > 0.0);

        // 用命中测试算出来的位置（不是手写 path）驱动输入
        let down = app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Down {
                pointer: PointerId(0),
                pos,
                button: PointerButton::Left,
            },
        );
        assert!(down.events > 0);

        let up = app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Up {
                pointer: PointerId(0),
                pos,
                button: PointerButton::Left,
            },
        );
        assert_eq!(up.tapped, Some(button), "命中测试 → 点击合成");
        assert_eq!(vm.count.get(), 1, "闭包已改 Signal");

        let st = app.frame_all()[0].1.clone();
        assert!(st.view_ran);
        assert_eq!(st.align.patched, 1, "只有那一行文本变了");
        let w = app.window_ctx(id).unwrap();
        assert!(find_text(w.track(), "1").is_some());
    }

    #[test]
    fn press_outside_the_button_then_move_away_does_not_click() {
        let (rt, mut app, vm, id) = setup();
        app.frame_all();

        let w = app.window_ctx(id).unwrap();
        let root = w.content_root().unwrap();
        let row = w.track().children(root)[2];
        let button = w.track().children(row)[0];
        let start = center(w.track(), button);
        let away = Point::new(5.0, 290.0);

        let w = app.window_ctx_mut(id).unwrap();
        w.pointer(
            &rt,
            InputEvent::Down {
                pointer: PointerId(0),
                pos: start,
                button: PointerButton::Left,
            },
        );
        let up = w.pointer(
            &rt,
            InputEvent::Up {
                pointer: PointerId(0),
                pos: away,
                button: PointerButton::Left,
            },
        );

        assert_eq!(up.tapped, None);
        assert_eq!(vm.count.get(), 0);
    }

    #[test]
    fn hover_follows_the_pointer_and_marks_damage() {
        let (rt, mut app, _, id) = setup();
        app.frame_all();
        let _ = app.frame_all(); // 清空挂载脏区

        let w = app.window_ctx(id).unwrap();
        let root = w.content_root().unwrap();
        let row = w.track().children(root)[2];
        let button = w.track().children(row)[0];
        let pos = center(w.track(), button);

        let w = app.window_ctx_mut(id).unwrap();
        let out = w.pointer(
            &rt,
            InputEvent::Move {
                pointer: PointerId(0),
                pos,
            },
        );
        assert!(out.events > 0);
        assert!(w.track().state(button).pointer_over);
        assert!(w.track().state(root).pointer_over, "整条链都置 pointer_over");

        // 只重绘，不重跑 view()
        let st = app.frame_all()[0].1.clone();
        assert!(!st.view_ran, "hover 不经过 view()");
        assert!(st.paint_pending);
        assert!(st.damage_all || !st.damage.is_empty());

        let w = app.window_ctx_mut(id).unwrap();
        w.pointer(&rt, InputEvent::Leave);
        assert!(!w.track().state(button).pointer_over);
    }

    #[test]
    fn tab_moves_focus_in_declaration_order() {
        struct Form;
        impl ViewModel for Form {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("x").tab_stop(true);
                    c.text("y").tab_stop(true);
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new(), Form);
        app.frame_all();

        let w = app.window_ctx(id).unwrap();
        let root = w.content_root().unwrap();
        let kids = w.track().children(root).to_vec();
        assert!(kids.len() >= 2);

        let w = app.window_ctx_mut(id).unwrap();
        // 走真焦点入口（内部 set_focus + LostFocus/GotFocus 派发）
        w.focus(&rt, Some(kids[0]), FocusState::Keyboard);
        assert_eq!(w.track().focused, Some(kids[0]));
        assert_eq!(w.track().get(kids[0]).unwrap().focus_state, FocusState::Keyboard);

        let next = w.tab(&rt, true);
        assert_eq!(next, Some(kids[1]));
        assert_eq!(w.track().focused, Some(kids[1]));
        assert_eq!(w.track().get(kids[0]).unwrap().focus_state, FocusState::Unfocused);

        // 循环回第一个
        assert_eq!(w.tab(&rt, true), Some(kids[0]));
        assert_eq!(w.tab(&rt, false), Some(kids[1]), "Shift+Tab 反向");
    }

    #[test]
    fn wheel_scrolls_the_container_when_no_handler_claims_it() {
        struct List {
            n: Signal<i32>,
        }
        impl ViewModel for List {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    for i in 0..20 {
                        c.text(format!("row {i}")).font_size(20.0);
                    }
                });
                let _ = self.n.get();
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(200.0, 100.0), List { n: Signal::new(&rt, 0) });
        app.frame_all();

        // 手工把内容根变成滚动容器（M5 的 Scroll 组件会做这件事）
        {
            let w = app.window_ctx_mut(id).unwrap();
            let root = w.content_root().unwrap();
            w.track_mut().get_mut(root).unwrap().layout.overflow_scroll = true;
            w.track_mut().mark_all_layout_dirty();
        }
        let st = app.frame_all()[0].1.clone();
        assert!(st.layout.ran);

        let (root, content_h) = {
            let w = app.window_ctx(id).unwrap();
            let root = w.content_root().unwrap();
            (root, w.track().get(root).unwrap().content_size.height)
        };
        assert!(content_h > 100.0, "内容比视口高：{content_h}");

        let out = app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Wheel {
                pointer: PointerId(0),
                pos: Point::new(10.0, 50.0),
                delta: (0.0, -60.0), // 负 = 向下滚 ⇒ offset 增大
            },
        );
        assert!(out.scrolled, "无处理器认领 ⇒ 框架默认滚动");
        let w = app.window_ctx(id).unwrap();
        assert_eq!(w.track().scroll_offset(root), (0.0, 60.0));

        // 记一个子节点（row 5）的滚动前位置
        let y_before = {
            let w = app.window_ctx(id).unwrap();
            let t = w.track();
            let rows = t.children(root);
            crate::layout::rect_of(t, rows[5]).y
        };

        // 滚动 ⇒ 边界重排（子树按新偏移平移）+ 重绘：内容真的动
        let st = app.frame_all()[0].1.clone();
        assert!(st.layout.ran, "滚动触发边界重排（子树平移到新偏移）");
        assert!(st.paint_pending);
        let y_after = {
            let w = app.window_ctx(id).unwrap();
            let t = w.track();
            let rows = t.children(root);
            crate::layout::rect_of(t, rows[5]).y
        };
        assert!(
            (y_after - (y_before - 60.0)).abs() < 0.5,
            "内容随偏移上移 60：{y_before} → {y_after}"
        );
    }

    // ─────────────── M3：光栅化 / 脏区 ───────────────

    fn pixel(ctx: &WindowCtx, x: u16, y: u16) -> vello_cpu::color::PremulRgba8 {
        let pix = ctx.pixmap();
        pix.data()[usize::from(y) * usize::from(pix.width()) + usize::from(x)]
    }

    fn opaque(r: u8, g: u8, b: u8) -> vello_cpu::color::PremulRgba8 {
        vello_cpu::color::PremulRgba8::from_u8_array([r, g, b, 255])
    }

    #[test]
    fn frame_paints_background_and_content() {
        struct Paint;
        impl ViewModel for Paint {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.container(|k| {
                        k.width(50.0);
                        k.height(50.0);
                        k.background(lieui_geom::Color::RED);
                    });
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(200.0, 120.0), Paint);
        let st = app.frame_all()[0].1.clone();
        assert!(st.render.raster.rasterized);
        assert!(st.render.ops >= 2, "底色 + 内容：{:?}", st.render);

        let ctx = app.window_ctx(id).unwrap();
        assert_eq!(pixel(ctx, 25, 25), opaque(255, 0, 0), "红块已画出");
        assert_eq!(pixel(ctx, 150, 100), opaque(245, 245, 245), "其余是窗口底色");
    }

    #[test]
    fn idle_frame_does_not_render() {
        let (_, mut app, _, id) = setup();
        app.frame_all();
        let st = app.frame_all()[0].1.clone();

        assert!(!st.paint_pending);
        assert!(!st.present_pending);
        assert!(!st.render.raster.rasterized, "空闲帧不碰像素");
        assert_eq!(st.rasterized_pixels(), 0);
        let _ = id;
    }

    #[test]
    fn damage_really_limits_repainting() {
        struct Two;
        impl ViewModel for Two {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.container(|k| {
                        k.width(50.0);
                        k.height(50.0);
                        k.background(lieui_geom::Color::RED);
                    });
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(200.0, 120.0), Two);
        app.frame_all();
        assert_eq!(pixel(app.window_ctx(id).unwrap(), 25, 25), opaque(255, 0, 0));

        // 直接改颜色（不经过 align）并**只报告远处一小块脏区**
        {
            let w = app.window_ctx_mut(id).unwrap();
            let root = w.content_root().unwrap();
            let block = w.track().children(root)[0];
            w.track_mut().get_mut(block).unwrap().paint.background_color = Some(lieui_geom::Color::GREEN);
            w.track_mut().damage_rect(Rect::new(150.0, 90.0, 10.0, 10.0));
            rt.mark(id, Dirty::PAINT | Dirty::PRESENT);
        }

        let st = app.frame_all()[0].1.clone();
        assert!(st.paint_pending);
        assert!(st.render.raster.pixels < 200 * 120 / 5, "只画了脏区行带");
        // 脏区之外**保持上一帧**的红色 —— 这正是"局部光栅化"的证据
        assert_eq!(
            pixel(app.window_ctx(id).unwrap(), 25, 25),
            opaque(255, 0, 0),
            "未报告脏区的地方不应被重画"
        );

        // 整窗脏 ⇒ 这次才更新成绿色
        {
            let w = app.window_ctx_mut(id).unwrap();
            w.track_mut().damage_whole_window();
            rt.mark(id, Dirty::PAINT | Dirty::PRESENT);
        }
        app.frame_all();
        assert_eq!(
            pixel(app.window_ctx(id).unwrap(), 25, 25),
            opaque(0, 128, 0),
            "整窗脏后内容更新"
        );
    }

    #[test]
    fn click_only_repaints_a_band() {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Counter {
            count: Signal::new(&rt, 0),
        });
        let id = app.window_erased(WindowConfig::new().size(400.0, 300.0), erased(Rc::clone(&vm)));
        app.frame_all();

        let w = app.window_ctx(id).unwrap();
        let root = w.content_root().unwrap();
        let row = w.track().children(root)[2];
        let button = w.track().children(row)[0];
        let pos = center(w.track(), button);

        {
            let w = app.window_ctx_mut(id).unwrap();
            w.pointer(
                &rt,
                InputEvent::Down {
                    pointer: PointerId(0),
                    pos,
                    button: PointerButton::Left,
                },
            );
            w.pointer(
                &rt,
                InputEvent::Up {
                    pointer: PointerId(0),
                    pos,
                    button: PointerButton::Left,
                },
            );
        }
        assert_eq!(vm.count.get(), 1);

        let st = app.frame_all()[0].1.clone();
        assert!(st.view_ran && st.layout.ran);
        assert!(st.paint_pending);
        let total = 400u64 * 300;
        assert!(
            st.rasterized_pixels() < total / 5,
            "只重画了脏区：{} / {total}（batches={}）",
            st.rasterized_pixels(),
            st.render.raster.batches
        );
        // 渲染确实产生了内容（不是空 pixmap）
        let ctx = app.window_ctx(id).unwrap();
        assert_eq!(pixel(ctx, 1, 299), opaque(245, 245, 245));
    }

    #[test]
    fn resize_rebuilds_the_pixmap_and_repaints() {
        let (rt, mut app, _, id) = setup();
        app.frame_all();
        {
            let ctx = app.window_ctx(id).unwrap();
            assert_eq!(ctx.pixmap().width(), 800);
            assert_eq!(ctx.pixmap().height(), 600);
        }

        app.window_ctx_mut(id).unwrap().set_size(&rt, Size::new(320.0, 240.0));
        let st = app.frame_all()[0].1.clone();
        assert!(st.damage_all);
        assert!(st.render.raster.rasterized);
        assert_eq!(st.render.raster.batches, 1, "整窗 ⇒ 一个批次");
        assert_eq!(st.render.raster.pixels, 320 * 240);

        let ctx = app.window_ctx(id).unwrap();
        assert_eq!(ctx.pixmap().width(), 320);
        assert_eq!(ctx.pixmap().height(), 240);
        assert_eq!(pixel(ctx, 300, 200), opaque(245, 245, 245));
    }

    // ─────────────── M4：窗口请求队列 / DPI ───────────────

    #[test]
    fn ctx_can_request_a_new_window() {
        struct Panel;
        impl ViewModel for Panel {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("panel");
                });
            }
        }

        struct Root {
            open: Signal<bool>,
        }
        impl ViewModel for Root {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("root");
                });
            }
            fn on_tick(self: &Rc<Self>, cx: &mut Ctx, _now: Instant) {
                if self.open.get() {
                    self.open.set(false);
                    crate::app::open_window(cx, WindowConfig::new().title("panel").size(120.0, 60.0), Panel);
                }
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Root {
            open: Signal::new(&rt, false),
        });
        let root_id = app.window_erased(WindowConfig::new(), erased(Rc::clone(&vm)));
        app.frame_all();
        assert_eq!(app.windows().len(), 1);
        assert!(!app.has_pending_requests());

        // 请求开窗（队列化，不立即建窗）
        vm.open.set(true);
        app.window_ctx_mut(root_id).unwrap().tick(&rt, Instant::now());
        assert!(app.has_pending_requests());

        let (opened, closed) = app.drain_requests();
        assert_eq!(opened.len(), 1);
        assert!(closed.is_empty());
        assert_eq!(app.windows().len(), 2);

        // 新窗口照样能跑完整管线
        let stats = app.frame_all();
        let (_, st) = stats.iter().find(|(w, _)| *w == opened[0]).unwrap();
        assert!(st.view_ran && st.render.raster.rasterized);
        assert!(find_text(app.window_ctx(opened[0]).unwrap().track(), "panel").is_some());
    }

    #[test]
    fn ctx_can_close_its_own_window() {
        struct SelfClosing {
            done: Signal<bool>,
        }
        impl ViewModel for SelfClosing {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("bye");
                });
            }
            fn on_tick(self: &Rc<Self>, cx: &mut Ctx, _now: Instant) {
                if self.done.get() {
                    crate::app::close_self(cx);
                }
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(
            WindowConfig::new(),
            SelfClosing {
                done: Signal::new(&rt, true),
            },
        );
        app.frame_all();
        app.window_ctx_mut(id).unwrap().tick(&rt, Instant::now());

        let (opened, closed) = app.drain_requests();
        assert!(opened.is_empty());
        assert_eq!(closed, vec![id]);
        assert!(app.window_ctx(id).is_none());
        assert_eq!(app.runtime().windows().len(), 0, "脏标志表也清掉了");
    }

    #[test]
    fn scale_factor_resizes_the_pixmap_without_relayout() {
        let (rt, mut app, _, id) = setup();
        app.frame_all();
        {
            let ctx = app.window_ctx(id).unwrap();
            assert_eq!(ctx.pixmap().width(), 800);
            assert_eq!(ctx.scale_factor(), 1.0);
        }

        app.window_ctx_mut(id).unwrap().set_scale_factor(&rt, 2.0);
        let st = app.frame_all()[0].1.clone();
        assert!(!st.layout.ran, "DPI 变化不吃布局");
        assert!(st.paint_pending);

        let ctx = app.window_ctx(id).unwrap();
        assert_eq!(ctx.pixmap().width(), 1600);
        assert_eq!(ctx.pixmap().height(), 1200);
        assert_eq!(ctx.size(), Size::new(800.0, 600.0), "逻辑尺寸不变");
        // 命中测试用逻辑坐标：内容根仍是 800×600 的逻辑矩形
        assert!(app.window_ctx(id).unwrap().hit_target(Point::new(10.0, 10.0)).is_some());
    }

    /// DPI 变化要**告知应用**：`on_scale_changed` 收到新比例，`scale_epoch` 递增
    /// —— 应用按像素缓存的资源（页栅格 / 缩略图）靠这两个信号失效重建。
    ///
    /// 同时钉住"没变就不打扰"：同一个比例重复设置、非法比例（NaN / 0 / 负）都不触发。
    #[test]
    fn scale_change_notifies_the_view_model_and_bumps_the_epoch() {
        struct Vm {
            seen: Rc<std::cell::RefCell<Vec<f32>>>,
        }
        impl ViewModel for Vm {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("hi");
                });
            }
            fn on_scale_changed(self: &Rc<Self>, _cx: &mut Ctx, scale: f32) {
                self.seen.borrow_mut().push(scale);
            }
        }

        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(800.0, 600.0), Vm { seen: Rc::clone(&seen) });
        app.frame_all();
        assert!(seen.borrow().is_empty(), "没变过就不该通知");
        assert_eq!(app.window_ctx(id).unwrap().scale_epoch(), 0);

        let ctx = app.window_ctx_mut(id).unwrap();
        assert!(ctx.set_scale_factor(&rt, 2.0), "变了 ⇒ true");
        assert_eq!(ctx.scale_epoch(), 1);
        assert_eq!(ctx.physical_size(), Size::new(1600.0, 1200.0), "物理尺寸 = 逻辑 × 2");

        // 同一个值 / 非法值：都不算"变化"
        assert!(!ctx.set_scale_factor(&rt, 2.0), "重复设同一个值 ⇒ false");
        assert!(!ctx.set_scale_factor(&rt, f32::NAN), "NaN ⇒ false");
        assert!(!ctx.set_scale_factor(&rt, 0.0), "0 ⇒ false");
        assert_eq!(ctx.scale_epoch(), 1, "纪元只该动一次");
        assert_eq!(&*seen.borrow(), &[2.0], "只通知了那一次");

        // 再变一次 ⇒ 纪元 +1、再通知一次（应用据此重建缓存）
        assert!(app.window_ctx_mut(id).unwrap().set_scale_factor(&rt, 1.5));
        assert_eq!(app.window_ctx(id).unwrap().scale_epoch(), 2);
        assert_eq!(&*seen.borrow(), &[2.0, 1.5]);
    }

    /// **0×0 的窗口（= 最小化）跑完整帧不许 panic**：pixmap 退到 1×1、布局照跑，
    /// 还原后能正常重绘。
    ///
    /// 这条**不是**上面那个崩溃的回归护栏（无头环境复现不出让容器变负高度的那套几何 ——
    /// 真机那次是 Windows 最小化客户区 + 应用自身的固定高度 chrome 共同算出来的）。
    /// 回归护栏在 `widgets::tests::scroll_parts_survives_collapsed_and_short_viewports`：
    /// 它直接给滚动条喂"负高度 / 零高度 / 比最小 thumb 还矮"的视口，已验证去掉修复就 panic。
    #[test]
    fn a_minimized_window_frames_without_panicking() {
        struct Tall;
        impl ViewModel for Tall {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                // 照抄 pdfkit 侧栏的排布（行 + Stretch + 带 padding 的滚动容器）
                v.column(|root| {
                    root.expand(true);
                    root.row(|body| {
                        body.expand(true);
                        body.align_items(crate::flex::FlexAlign::Stretch);
                        body.container(|side| {
                            side.width(220.0);
                            side.padding(8.0);
                            side.scroll(|sc| {
                                sc.expand(true);
                                sc.column(|list| {
                                    for i in 0..200 {
                                        list.text(format!("第 {i} 行")).height(20.0);
                                    }
                                });
                            });
                        });
                    });
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(300.0, 200.0), Tall);
        app.frame_all();

        // 最小化：winit 就是这么发尺寸的（0×0）
        app.window_ctx_mut(id).unwrap().set_size(&rt, Size::new(0.0, 0.0));
        app.frame_all(); // ← 以前在这里 panic

        // 还原：布局与重绘都该恢复正常
        app.window_ctx_mut(id).unwrap().set_size(&rt, Size::new(300.0, 200.0));
        let st = app.frame_all()[0].1.clone();
        assert!(st.layout.ran, "还原后重排");
        assert!(st.paint_pending, "还原后重绘");
        assert_eq!(app.window_ctx(id).unwrap().pixmap().width(), 300, "物理尺寸回到窗口宽");
    }

    // ─────────────── M5：内置行为 + 双向绑定 ───────────────

    struct Controls {
        volume: Signal<f32>,
        agree: Signal<bool>,
    }

    impl ViewModel for Controls {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            v.column(|c| {
                c.gap(8.0);
                c.slider_bound(&self.volume, 0.0, 10.0);
                c.checkbox_bound(&self.agree);
            });
        }
    }

    fn controls() -> (Runtime, App, Rc<Controls>, WindowId, NodeId, NodeId) {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Controls {
            volume: Signal::new(&rt, 0.0),
            agree: Signal::new(&rt, false),
        });
        let id = app.window_erased(WindowConfig::new().size(200.0, 120.0), erased(Rc::clone(&vm)));
        app.frame_all();
        let (slider, checkbox) = {
            let w = app.window_ctx(id).unwrap();
            let root = w.content_root().unwrap();
            let kids = w.track().children(root);
            (kids[0], kids[1])
        };
        (rt, app, vm, id, slider, checkbox)
    }

    #[test]
    fn slider_drag_writes_back_the_bound_signal() {
        let (rt, mut app, vm, id, slider, _) = controls();
        let rect = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), slider);
        assert_eq!(rect.width, 140.0, "slider 默认宽");

        let p = Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
        let out = app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Down {
                pointer: PointerId(0),
                pos: p,
                button: PointerButton::Left,
            },
        );
        assert!(out.events > 0);
        assert!((vm.volume.get() - 5.0).abs() < 0.01, "拖到中点 ⇒ 5");
        {
            let w = app.window_ctx(id).unwrap();
            assert_eq!(w.track().captured_by(PointerId(0)), Some(slider), "拖拽期间捕获指针");
            assert!(matches!(
                w.track().get(slider).map(|n| &n.kind),
                Some(Kind::Slider { dragging: true, .. })
            ));
        }

        // 拖到右端（并越界一点）⇒ 钳到 max
        app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Move {
                pointer: PointerId(0),
                pos: Point::new(rect.right() + 50.0, p.y),
            },
        );
        assert_eq!(vm.volume.get(), 10.0);

        // 松开 ⇒ 结束拖拽 + 释放捕获
        app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Up {
                pointer: PointerId(0),
                pos: Point::new(rect.right(), p.y),
                button: PointerButton::Left,
            },
        );
        let w = app.window_ctx(id).unwrap();
        assert_eq!(w.track().captured_by(PointerId(0)), None);
        assert!(matches!(
            w.track().get(slider).map(|n| &n.kind),
            Some(Kind::Slider { dragging: false, .. })
        ));
    }

    #[test]
    fn slider_drag_and_the_next_view_do_not_fight() {
        let (rt, mut app, vm, id, slider, _) = controls();
        let rect = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), slider);
        let p = Point::new(rect.x + rect.width * 0.25, rect.y + 5.0);

        let w = app.window_ctx_mut(id).unwrap();
        w.pointer(
            &rt,
            InputEvent::Down {
                pointer: PointerId(0),
                pos: p,
                button: PointerButton::Left,
            },
        );
        w.pointer(
            &rt,
            InputEvent::Up {
                pointer: PointerId(0),
                pos: p,
                button: PointerButton::Left,
            },
        );
        assert!((vm.volume.get() - 2.5).abs() < 0.01);

        // 立即写节点 + 写 signal ⇒ 下一帧 view() 产出的描述值与节点一致 ⇒ 零 patch
        let st = app.frame_all()[0].1.clone();
        assert_eq!(st.align.patched, 0, "不应产生「值回弹」式补丁：{st:?}");
        assert!(!st.layout.ran || st.layout.moved == 0);
    }

    #[test]
    fn unbound_slider_ignores_the_pointer() {
        struct Plain;
        impl ViewModel for Plain {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.slider(0.5);
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(200.0, 120.0), Plain);
        app.frame_all();
        let slider = {
            let w = app.window_ctx(id).unwrap();
            w.track().children(w.content_root().unwrap())[0]
        };
        let rect = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), slider);

        app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Down {
                pointer: PointerId(0),
                pos: Point::new(rect.right(), rect.y + 5.0),
                button: PointerButton::Left,
            },
        );
        let w = app.window_ctx(id).unwrap();
        assert!(
            matches!(w.track().get(slider).map(|n| &n.kind), Some(Kind::Slider { value, dragging, .. }) if (*value - 0.5).abs() < 1e-3 && !*dragging),
            "未绑定 ⇒ 框架不接管拖拽（desc 是唯一真相）：{:?}",
            w.track().get(slider).map(|n| &n.kind)
        );
        assert_eq!(w.track().captured_by(PointerId(0)), None);
    }

    #[test]
    fn bound_checkbox_toggles_on_tap() {
        let (rt, mut app, vm, id, _, checkbox) = controls();
        let rect = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), checkbox);
        let p = Point::new(rect.x + 5.0, rect.y + 5.0);

        let w = app.window_ctx_mut(id).unwrap();
        let down = w.pointer(
            &rt,
            InputEvent::Down {
                pointer: PointerId(0),
                pos: p,
                button: PointerButton::Left,
            },
        );
        let up = w.pointer(
            &rt,
            InputEvent::Up {
                pointer: PointerId(0),
                pos: p,
                button: PointerButton::Left,
            },
        );
        assert_eq!(down.tapped, None);
        assert_eq!(up.tapped, Some(checkbox));
        assert!(vm.agree.get(), "Tapped ⇒ 框架翻转绑定的 signal");

        // 再点一次翻回来
        let w = app.window_ctx_mut(id).unwrap();
        w.pointer(
            &rt,
            InputEvent::Down {
                pointer: PointerId(0),
                pos: p,
                button: PointerButton::Left,
            },
        );
        w.pointer(
            &rt,
            InputEvent::Up {
                pointer: PointerId(0),
                pos: p,
                button: PointerButton::Left,
            },
        );
        assert!(!vm.agree.get());
    }

    #[test]
    fn unbound_checkbox_is_left_to_user_handlers() {
        struct Plain {
            clicks: Rc<std::cell::Cell<u32>>,
        }
        impl ViewModel for Plain {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                let clicks = Rc::clone(&self.clicks);
                v.column(|c| {
                    c.checkbox(false).on_tap(move || clicks.set(clicks.get() + 1));
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let clicks = Rc::new(std::cell::Cell::new(0));
        let id = app.window(
            WindowConfig::new().size(200.0, 120.0),
            Plain {
                clicks: Rc::clone(&clicks),
            },
        );
        app.frame_all();
        let cb = {
            let w = app.window_ctx(id).unwrap();
            w.track().children(w.content_root().unwrap())[0]
        };
        let rect = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), cb);
        let p = Point::new(rect.x + 5.0, rect.y + 5.0);

        let w = app.window_ctx_mut(id).unwrap();
        w.pointer(
            &rt,
            InputEvent::Down {
                pointer: PointerId(0),
                pos: p,
                button: PointerButton::Left,
            },
        );
        w.pointer(
            &rt,
            InputEvent::Up {
                pointer: PointerId(0),
                pos: p,
                button: PointerButton::Left,
            },
        );

        assert_eq!(clicks.get(), 1, "用户处理器被调用");
        let w = app.window_ctx(id).unwrap();
        assert!(
            matches!(
                w.track().get(cb).map(|n| &n.kind),
                Some(Kind::Checkbox { checked: false })
            ),
            "未绑定 ⇒ 框架不改状态"
        );
    }

    // ─────────────── M5：输入框（Input）───────────────

    struct Form {
        name: Signal<String>,
    }

    impl ViewModel for Form {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            v.column(|c| {
                c.input_bound(&self.name).placeholder("姓名");
            });
        }
    }

    fn form(initial: &str) -> (Runtime, App, Rc<Form>, WindowId, NodeId) {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Form {
            name: Signal::new(&rt, initial.to_string()),
        });
        let id = app.window_erased(WindowConfig::new().size(240.0, 80.0), erased(Rc::clone(&vm)));
        app.frame_all();
        let input = {
            let w = app.window_ctx(id).unwrap();
            w.track().children(w.content_root().unwrap())[0]
        };
        (rt, app, vm, id, input)
    }

    fn input_state(app: &App, id: WindowId, input: NodeId) -> (String, usize, usize, String) {
        let w = app.window_ctx(id).unwrap();
        match w.track().get(input).map(|n| &n.kind) {
            Some(Kind::Input {
                text,
                caret,
                anchor,
                preedit,
                ..
            }) => (text.clone(), *caret, *anchor, preedit.clone()),
            other => panic!("不是输入框：{other:?}"),
        }
    }

    /// 在输入框右端点一下：拿到焦点 + 光标落到末尾（多数用例的默认动作）
    fn click_input(app: &mut App, rt: &Runtime, id: WindowId, input: NodeId) {
        let rect = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), input);
        click_input_at(app, rt, id, input, rect.right());
    }

    /// 在输入框里点一下（并让它拿到焦点）
    fn click_input_at(app: &mut App, rt: &Runtime, id: WindowId, input: NodeId, x: f32) {
        let rect = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), input);
        let p = Point::new(x, rect.y + rect.height * 0.5);
        let w = app.window_ctx_mut(id).unwrap();
        w.pointer(
            rt,
            InputEvent::Down {
                pointer: PointerId(0),
                pos: p,
                button: PointerButton::Left,
            },
        );
        w.pointer(
            rt,
            InputEvent::Up {
                pointer: PointerId(0),
                pos: p,
                button: PointerButton::Left,
            },
        );
    }

    fn type_chars(app: &mut App, rt: &Runtime, id: WindowId, s: &str) {
        for ch in s.chars() {
            app.window_ctx_mut(id).unwrap().key(rt, Event::char_received(ch));
        }
    }

    fn press_key(app: &mut App, rt: &Runtime, id: WindowId, code: KeyCode, mods: Modifiers) {
        app.window_ctx_mut(id)
            .unwrap()
            .key(rt, Event::key_with(EventKind::KeyDown, code, mods));
    }

    #[test]
    fn typing_writes_back_to_the_bound_signal() {
        let (rt, mut app, vm, id, input) = form("");
        click_input(&mut app, &rt, id, input);
        assert_eq!(app.window_ctx(id).unwrap().track().focused, Some(input));

        type_chars(&mut app, &rt, id, "abc");
        assert_eq!(vm.name.get(), "abc");
        let (text, caret, anchor, _) = input_state(&app, id, input);
        assert_eq!((text.as_str(), caret, anchor), ("abc", 3, 3));

        // 编辑回写的值与描述一致 ⇒ 下一帧不产生补丁（光标不会被"值回弹"打断）
        let st = app.frame_all()[0].1.clone();
        assert_eq!(st.align.patched, 0, "{st:?}");
    }

    #[test]
    fn click_places_the_caret_at_the_clicked_glyph() {
        let (rt, mut app, _, id, input) = form("abcd");
        let rect = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), input);
        let spec = app
            .window_ctx(id)
            .unwrap()
            .track()
            .get(input)
            .unwrap()
            .text
            .spec
            .clone();
        let pad = app.window_ctx(id).unwrap().track().get(input).unwrap().layout.padding[0];
        let w_ab = lieui_text::TextEngine::measure_text("ab", &spec).0 as f32;

        click_input_at(&mut app, &rt, id, input, rect.x + pad + w_ab);
        assert_eq!(input_state(&app, id, input).1, 2, "点在 a 与 b 之间");

        type_chars(&mut app, &rt, id, "X");
        assert_eq!(input_state(&app, id, input).0, "abXcd");
    }

    #[test]
    fn backspace_delete_and_arrows_edit_around_the_caret() {
        let (rt, mut app, vm, id, input) = form("abcd");
        click_input(&mut app, &rt, id, input); // 光标落在末尾
        assert_eq!(input_state(&app, id, input).1, 4);

        press_key(&mut app, &rt, id, KeyCode::Named(NamedKey::Left), Modifiers::EMPTY);
        press_key(&mut app, &rt, id, KeyCode::Named(NamedKey::Left), Modifiers::EMPTY);
        assert_eq!(input_state(&app, id, input).1, 2);

        press_key(&mut app, &rt, id, KeyCode::Named(NamedKey::Backspace), Modifiers::EMPTY);
        assert_eq!(input_state(&app, id, input).0, "acd");

        press_key(&mut app, &rt, id, KeyCode::Named(NamedKey::Delete), Modifiers::EMPTY);
        assert_eq!(input_state(&app, id, input).0, "ad");
        assert_eq!(vm.name.get(), "ad");

        press_key(&mut app, &rt, id, KeyCode::Named(NamedKey::Home), Modifiers::EMPTY);
        assert_eq!(input_state(&app, id, input).1, 0);
        press_key(&mut app, &rt, id, KeyCode::Named(NamedKey::End), Modifiers::EMPTY);
        assert_eq!(input_state(&app, id, input).1, 2);
    }

    #[test]
    fn shift_arrows_select_and_typing_replaces_the_selection() {
        let (rt, mut app, vm, id, input) = form("abcd");
        click_input(&mut app, &rt, id, input);

        press_key(&mut app, &rt, id, KeyCode::Named(NamedKey::Left), Modifiers::SHIFT);
        press_key(&mut app, &rt, id, KeyCode::Named(NamedKey::Left), Modifiers::SHIFT);
        let (_, caret, anchor, _) = input_state(&app, id, input);
        assert_eq!((caret, anchor), (2, 4), "从末尾向左扩选两个字符");

        type_chars(&mut app, &rt, id, "Z");
        assert_eq!(input_state(&app, id, input).0, "abZ", "输入替换选区");
        assert_eq!(vm.name.get(), "abZ");
    }

    #[test]
    fn ctrl_a_selects_all_and_backspace_clears_it() {
        let (rt, mut app, vm, id, input) = form("hello");
        click_input(&mut app, &rt, id, input);

        press_key(&mut app, &rt, id, KeyCode::Char('a'), Modifiers::CTRL);
        let (_, caret, anchor, _) = input_state(&app, id, input);
        assert_eq!((caret, anchor), (5, 0), "全选");

        press_key(&mut app, &rt, id, KeyCode::Named(NamedKey::Backspace), Modifiers::EMPTY);
        assert_eq!(input_state(&app, id, input).0, "");
        assert_eq!(vm.name.get(), "");
    }

    #[test]
    fn dragging_extends_the_selection() {
        let (rt, mut app, _, id, input) = form("abcdef");
        let rect = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), input);
        let mid_y = rect.y + rect.height * 0.5;

        // 按住（不松手）落在左侧
        {
            let w = app.window_ctx_mut(id).unwrap();
            w.pointer(
                &rt,
                InputEvent::Down {
                    pointer: PointerId(0),
                    pos: Point::new(rect.x + 6.0, mid_y),
                    button: PointerButton::Left,
                },
            );
        }
        assert_eq!(input_state(&app, id, input).1, 0, "点在左侧 ⇒ 光标 0");
        assert_eq!(
            app.window_ctx(id).unwrap().track().captured_by(PointerId(0)),
            Some(input),
            "编辑期间捕获指针"
        );

        // 拖到右侧 ⇒ 扩选（锚点不动）
        app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Move {
                pointer: PointerId(0),
                pos: Point::new(rect.right(), mid_y),
            },
        );
        let (_, caret, anchor, _) = input_state(&app, id, input);
        assert!(
            caret > anchor,
            "向右拖 ⇒ 光标在锚点右侧（caret={caret} anchor={anchor}）"
        );

        // 松手后捕获释放
        app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Up {
                pointer: PointerId(0),
                pos: Point::new(rect.right(), mid_y),
                button: PointerButton::Left,
            },
        );
        assert_eq!(app.window_ctx(id).unwrap().track().captured_by(PointerId(0)), None);
    }

    #[test]
    fn ime_preedit_then_commit_inserts_text() {
        let (rt, mut app, vm, id, input) = form("");
        click_input(&mut app, &rt, id, input);

        // 预编辑：只进组合串，不动文本
        app.window_ctx_mut(id).unwrap().dispatch(
            &rt,
            &[input],
            &Event::ImePreedit {
                text: "zhong".to_string(),
                cursor: Some((0, 5)),
            },
        );
        assert_eq!(input_state(&app, id, input).3, "zhong");
        assert_eq!(input_state(&app, id, input).0, "");
        assert_eq!(vm.name.get(), "");

        // 提交：一个字符一条 CharacterReceived（平台的实现方式）
        type_chars(&mut app, &rt, id, "中");
        let (text, _, _, preedit) = input_state(&app, id, input);
        assert_eq!(text, "中");
        assert_eq!(preedit, "", "提交后组合串清空");
        assert_eq!(vm.name.get(), "中");
    }

    #[test]
    fn losing_focus_clears_the_preedit() {
        let (rt, mut app, _, id, input) = form("");
        click_input(&mut app, &rt, id, input);
        app.window_ctx_mut(id).unwrap().dispatch(
            &rt,
            &[input],
            &Event::ImePreedit {
                text: "ab".to_string(),
                cursor: None,
            },
        );
        assert_eq!(input_state(&app, id, input).3, "ab");

        app.window_ctx_mut(id)
            .unwrap()
            .dispatch(&rt, &[input], &Event::simple(EventKind::LostFocus));
        assert_eq!(input_state(&app, id, input).3, "");
    }

    #[test]
    fn unfocused_input_ignores_characters() {
        let (rt, mut app, vm, id, input) = form("");
        // 没有点进去（没有焦点）时敲字不应落进输入框
        type_chars(&mut app, &rt, id, "x");
        assert_eq!(input_state(&app, id, input).0, "");
        assert_eq!(vm.name.get(), "");
    }

    #[test]
    fn unbound_input_ignores_editing() {
        struct Plain;
        impl ViewModel for Plain {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.input("固定文本");
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(240.0, 80.0), Plain);
        app.frame_all();
        let input = {
            let w = app.window_ctx(id).unwrap();
            w.track().children(w.content_root().unwrap())[0]
        };
        click_input(&mut app, &rt, id, input);
        type_chars(&mut app, &rt, id, "zzz");

        let (text, caret, _, _) = input_state(&app, id, input);
        assert_eq!(text, "固定文本", "未绑定 ⇒ 编辑不生效");
        assert_eq!(caret, "固定文本".len());
    }

    #[test]
    fn model_change_replaces_the_buffer_and_puts_the_caret_at_the_end() {
        let (rt, mut app, vm, id, input) = form("ab");
        click_input(&mut app, &rt, id, input);
        press_key(&mut app, &rt, id, KeyCode::Named(NamedKey::Home), Modifiers::EMPTY);
        assert_eq!(input_state(&app, id, input).1, 0);

        // 模型侧改值（模拟"外部同步"）
        vm.name.set("hello".to_string());
        app.frame_all();

        let (text, caret, anchor, _) = input_state(&app, id, input);
        assert_eq!((text.as_str(), caret, anchor), ("hello", 5, 5));
    }

    #[test]
    fn tab_moves_focus_away_from_an_input() {
        struct TwoFields {
            a: Signal<String>,
            b: Signal<String>,
        }
        impl ViewModel for TwoFields {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.input_bound(&self.a);
                    c.input_bound(&self.b);
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(TwoFields {
            a: Signal::new(&rt, String::new()),
            b: Signal::new(&rt, String::new()),
        });
        let id = app.window_erased(WindowConfig::new().size(240.0, 120.0), erased(Rc::clone(&vm)));
        app.frame_all();
        let (first, second) = {
            let w = app.window_ctx(id).unwrap();
            let kids = w.track().children(w.content_root().unwrap());
            (kids[0], kids[1])
        };

        click_input(&mut app, &rt, id, first);
        type_chars(&mut app, &rt, id, "a");
        assert_eq!(vm.a.get(), "a");

        app.window_ctx_mut(id).unwrap().tab(&rt, true);
        assert_eq!(app.window_ctx(id).unwrap().track().focused, Some(second));

        type_chars(&mut app, &rt, id, "b");
        assert_eq!((vm.a.get(), vm.b.get()), ("a".to_string(), "b".to_string()));
    }

    // ─────────────── M5：锚定层（popup）───────────────

    struct Menu {
        open: Signal<bool>,
    }

    impl ViewModel for Menu {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            v.column(|c| {
                c.gap(8.0);
                c.button("菜单")
                    .key("btn")
                    .on_tap(act(self, |s| s.open.set(!s.open.get())));
            });
            if self.open.get() {
                v.popup_at("btn", Placement::Below, |p| {
                    p.text("菜单项 A");
                    p.text("菜单项 B");
                });
            }
        }
    }

    #[test]
    fn popup_opens_on_tap_and_follows_the_anchor() {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(
            WindowConfig::new().size(300.0, 200.0),
            Menu {
                open: Signal::new(&rt, false),
            },
        );
        app.frame_all();
        let btn = {
            let w = app.window_ctx(id).unwrap();
            w.track().children(w.content_root().unwrap())[0]
        };
        assert!(
            app.window_ctx(id)
                .unwrap()
                .track()
                .roots_of(Layer::Popup)
                .next()
                .is_none(),
            "没开菜单前没有 popup 层"
        );

        // 点按钮 ⇒ open = true ⇒ view() 声明 popup ⇒ 布局后落位
        let rect = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), btn);
        let p = rect.center();
        {
            let w = app.window_ctx_mut(id).unwrap();
            w.pointer(
                &rt,
                InputEvent::Down {
                    pointer: PointerId(0),
                    pos: p,
                    button: PointerButton::Left,
                },
            );
            w.pointer(
                &rt,
                InputEvent::Up {
                    pointer: PointerId(0),
                    pos: p,
                    button: PointerButton::Left,
                },
            );
        }
        app.frame_all();

        let (br, pr) = {
            let w = app.window_ctx(id).unwrap();
            let t = w.track();
            let popup = t.roots_of(Layer::Popup).next().expect("popup 层出现了").node;
            (crate::layout::rect_of(t, btn), crate::layout::rect_of(t, popup))
        };
        assert_eq!(pr.x, br.x, "Below 与锚点左对齐");
        assert!(
            (pr.y - (br.bottom() + 4.0)).abs() < 0.5,
            "层在锚点下方留间距：{br:?} -> {pr:?}"
        );

        // 再点一次（按钮被 popup 挡不住：popup 在按钮下方）⇒ 关闭 ⇒ popup 层消失
        {
            let w = app.window_ctx_mut(id).unwrap();
            w.pointer(
                &rt,
                InputEvent::Down {
                    pointer: PointerId(0),
                    pos: p,
                    button: PointerButton::Left,
                },
            );
            w.pointer(
                &rt,
                InputEvent::Up {
                    pointer: PointerId(0),
                    pos: p,
                    button: PointerButton::Left,
                },
            );
        }
        app.frame_all();
        assert!(
            app.window_ctx(id)
                .unwrap()
                .track()
                .roots_of(Layer::Popup)
                .next()
                .is_none(),
            "再点一次后 popup 层消失"
        );
    }

    // ─────────────── M6：ComboBox（A 档组合：按钮 + 锚定弹层）───────────────

    struct Picker {
        open: Signal<bool>,
        choice: Signal<String>,
    }

    /// **A 档组合函数**（≈ 组件就是普通函数）：锚定按钮 + 轻关闭弹层。
    /// 不需要任何新机制——这正是"组合覆盖 ~90%"的验收。
    fn combo(vm: &Rc<Picker>, v: &mut ViewBuf, anchor_key: &str, options: &[&str]) {
        v.column(|c| {
            c.button("选择水果")
                .key(anchor_key)
                .on_tap(act(vm, |s| s.open.set(!s.open.get())));
        });
        if vm.open.get() {
            v.popup_at(anchor_key, Placement::Below, |p| {
                // 轻关闭：点击弹层之外 ⇒ 层根收到 Dismissed ⇒ 翻自己的状态
                let me = Rc::clone(vm);
                p.on(EventKind::Dismissed, move |_| me.open.set(false));
                for opt in options {
                    let me = Rc::clone(vm);
                    let label = opt.to_string();
                    p.text(opt.to_string()).padding(6.0).width(80.0).on_tap(move || {
                        me.choice.set(label.clone());
                        me.open.set(false);
                    });
                }
            });
        }
    }

    impl ViewModel for Picker {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            combo(self, v, "combo-btn", &["苹果", "香蕉", "樱桃"]);
        }
    }

    fn picker() -> (Runtime, App, Rc<Picker>, WindowId) {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Picker {
            open: Signal::new(&rt, false),
            choice: Signal::new(&rt, String::new()),
        });
        let id = app.window_erased(WindowConfig::new().size(200.0, 200.0), erased(Rc::clone(&vm)));
        app.frame_all();
        (rt, app, vm, id)
    }

    fn popup_root_of(app: &App, id: WindowId) -> Option<NodeId> {
        app.window_ctx(id)
            .unwrap()
            .track()
            .roots_of(Layer::Popup)
            .next()
            .map(|r| r.node)
    }

    #[test]
    fn combo_dropdown_opens_selects_and_light_dismisses() {
        let (rt, mut app, vm, id) = picker();
        let btn = app
            .window_ctx(id)
            .unwrap()
            .track()
            .children(app.window_ctx(id).unwrap().content_root().unwrap())[0];

        assert!(popup_root_of(&app, id).is_none(), "展开前没有弹层");

        // ① 点锚点 ⇒ 弹层出现，锚定按钮下方、左对齐
        tap_node(&mut app, &rt, id, btn);
        app.frame_all();
        let (br, pr, items) = {
            let w = app.window_ctx(id).unwrap();
            let t = w.track();
            let popup = popup_root_of(&app, id).expect("弹层出现");
            let items: Vec<NodeId> = t
                .descendants(popup)
                .into_iter()
                .filter(|n| matches!(t.get(*n).unwrap().kind, Kind::Text(_)))
                .collect();
            (crate::layout::rect_of(t, btn), crate::layout::rect_of(t, popup), items)
        };
        assert_eq!(pr.x, br.x, "Below 左对齐");
        assert!((pr.y - (br.bottom() + 4.0)).abs() < 0.5);
        assert_eq!(items.len(), 3, "三个选项");

        // ② 点"香蕉" ⇒ 选中 + 弹层关闭（选项自己翻 open）
        tap_node(&mut app, &rt, id, items[1]);
        assert_eq!(vm.choice.get(), "香蕉");
        assert!(!vm.open.get());
        app.frame_all();
        assert!(popup_root_of(&app, id).is_none(), "选择后弹层消失");

        // ③ 再展开，点击弹层之外 ⇒ 轻关闭
        tap_node(&mut app, &rt, id, btn);
        app.frame_all();
        assert!(popup_root_of(&app, id).is_some());
        {
            let w = app.window_ctx_mut(id).unwrap();
            w.pointer(
                &rt,
                InputEvent::Down {
                    pointer: PointerId(0),
                    pos: Point::new(190.0, 190.0),
                    button: PointerButton::Left,
                },
            );
            w.pointer(
                &rt,
                InputEvent::Up {
                    pointer: PointerId(0),
                    pos: Point::new(190.0, 190.0),
                    button: PointerButton::Left,
                },
            );
        }
        assert!(!vm.open.get(), "点外部 ⇒ Dismissed ⇒ 关闭");
        app.frame_all();
        assert!(popup_root_of(&app, id).is_none());

        // ④ 锚点在关闭状态再点 ⇒ 重新展开（toggle 与 dismiss 不互相打架）
        tap_node(&mut app, &rt, id, btn);
        assert!(vm.open.get());
    }

    #[test]
    fn anchor_click_while_open_closes_the_dropdown() {
        let (rt, mut app, vm, id) = picker();
        let btn = app
            .window_ctx(id)
            .unwrap()
            .track()
            .children(app.window_ctx(id).unwrap().content_root().unwrap())[0];

        tap_node(&mut app, &rt, id, btn);
        app.frame_all();
        assert!(popup_root_of(&app, id).is_some());

        // 弹层开着时再点锚点：Tapped(toggle→false) 先于 Dismissed(→false) ⇒ 关闭且保持关闭
        tap_node(&mut app, &rt, id, btn);
        assert!(!vm.open.get());
        app.frame_all();
        assert!(popup_root_of(&app, id).is_none());
    }

    // ─────────────── M6：MenuBar（组合 + 嵌套弹层）───────────────

    struct MenuVm {
        /// 当前打开的菜单名（"" = 全关）——声明式互斥，同 Radio 原理
        open_menu: Signal<String>,
        /// "查找"子菜单是否展开
        sub_open: Signal<bool>,
        last: Signal<String>,
    }

    impl ViewModel for MenuVm {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            // ── 菜单条：一排锚点按钮（行固定高，按钮不被拉伸成整窗）──
            v.row(|r| {
                r.height(28.0);
                r.button("文件").key("menu-文件").on_tap(act1(
                    self,
                    |s, m: String| {
                        // toggle 自己（打开时再点 = 关闭）
                        s.open_menu.set(if s.open_menu.get() == m { String::new() } else { m });
                    },
                    "文件".to_string(),
                ));
                r.button("编辑").key("menu-编辑").on_tap(act1(
                    self,
                    |s, m: String| {
                        s.open_menu.set(if s.open_menu.get() == m { String::new() } else { m });
                    },
                    "编辑".to_string(),
                ));
            });

            // ── 弹层：声明式存在（open_menu 是唯一真相），锚定各自的按钮 ──
            if self.open_menu.get() == "文件" {
                v.popup_at("menu-文件", Placement::Below, |p| {
                    let me = Rc::clone(self);
                    // 轻关闭守卫：只关自己（避免清掉刚打开的兄弟菜单）
                    p.on(EventKind::Dismissed, move |_| {
                        if me.open_menu.get() == "文件" {
                            me.open_menu.set(String::new());
                        }
                    });
                    for item in ["新建", "打开"] {
                        let me = Rc::clone(self);
                        let label = item.to_string();
                        p.text(item.to_string()).padding(6.0).width(90.0).on_tap(move || {
                            me.last.set(label.clone());
                            me.open_menu.set(String::new());
                        });
                    }
                });
            }
            if self.open_menu.get() == "编辑" {
                v.popup_at("menu-编辑", Placement::Below, |p| {
                    let me = Rc::clone(self);
                    p.on(EventKind::Dismissed, move |_| {
                        if me.open_menu.get() == "编辑" {
                            me.open_menu.set(String::new());
                        }
                    });
                    p.text("撤销").padding(6.0).width(90.0).on_tap({
                        let me = Rc::clone(self);
                        move || {
                            me.last.set("撤销".to_string());
                            me.open_menu.set(String::new());
                        }
                    });
                    // 子菜单锚点：这一项本身是下级弹层的锚
                    p.text("查找 ▸")
                        .padding(6.0)
                        .width(90.0)
                        .key("item-查找")
                        .on_tap(act(self, |s| s.sub_open.set(!s.sub_open.get())));
                });
            }
            // ── 子层级：锚点在另一个弹层里（RightOf），验证嵌套弹层 ──
            if self.open_menu.get() == "编辑" && self.sub_open.get() {
                v.popup_at("item-查找", Placement::RightOf, |p| {
                    let me = Rc::clone(self);
                    p.text("查找内容").padding(6.0).width(90.0).on_tap(move || {
                        me.last.set("查找内容".to_string());
                        me.open_menu.set(String::new());
                        me.sub_open.set(false);
                    });
                });
            }
        }
    }

    fn menu_vm() -> (Runtime, App, Rc<MenuVm>, WindowId) {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(MenuVm {
            open_menu: Signal::new(&rt, String::new()),
            sub_open: Signal::new(&rt, false),
            last: Signal::new(&rt, String::new()),
        });
        let id = app.window_erased(WindowConfig::new().size(300.0, 220.0), erased(Rc::clone(&vm)));
        app.frame_all();
        (rt, app, vm, id)
    }

    fn menu_buttons(app: &App, id: WindowId) -> (NodeId, NodeId) {
        let w = app.window_ctx(id).unwrap();
        let kids = w.track().children(w.content_root().unwrap());
        (kids[0], kids[1])
    }

    fn popup_texts(app: &App, id: WindowId) -> Vec<String> {
        let w = app.window_ctx(id).unwrap();
        let t = w.track();
        let Some(popup) = t.roots_of(Layer::Popup).next().map(|r| r.node) else {
            return Vec::new();
        };
        t.descendants(popup)
            .into_iter()
            .filter_map(|n| match &t.get(n).unwrap().kind {
                Kind::Text(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn menu_bar_opens_one_menu_at_a_time_and_reanchors() {
        let (rt, mut app, _, id) = menu_vm();
        let (btn_file, btn_edit) = menu_buttons(&app, id);

        assert!(popup_texts(&app, id).is_empty(), "初始没有弹层");

        // 打开"文件"
        tap_node(&mut app, &rt, id, btn_file);
        app.frame_all();
        assert_eq!(popup_texts(&app, id), vec!["新建".to_string(), "打开".to_string()]);

        // 直接点"编辑"：文件关闭、编辑打开（tapped 先行、dismiss 只关自己）
        tap_node(&mut app, &rt, id, btn_edit);
        app.frame_all();
        assert_eq!(popup_texts(&app, id), vec!["撤销".to_string(), "查找 ▸".to_string()]);

        // 弹层重新锚定到"编辑"按钮下方（跨帧跟随锚点）
        let (br, pr) = {
            let w = app.window_ctx(id).unwrap();
            let t = w.track();
            let popup = t.roots_of(Layer::Popup).next().unwrap().node;
            (crate::layout::rect_of(t, btn_edit), crate::layout::rect_of(t, popup))
        };
        assert_eq!(pr.x, br.x, "x 对齐：br={br:?} pr={pr:?}");
        assert!((pr.y - (br.bottom() + 4.0)).abs() < 0.5, "y 锚定：br={br:?} pr={pr:?}");

        // 再点"编辑"：关闭
        tap_node(&mut app, &rt, id, btn_edit);
        app.frame_all();
        assert!(popup_texts(&app, id).is_empty());
    }

    #[test]
    fn menu_item_fires_and_submenu_nests_to_the_right() {
        let (rt, mut app, vm, id) = menu_vm();
        let (btn_file, _) = menu_buttons(&app, id);

        // 文件 → 新建：动作触发 + 菜单关闭
        tap_node(&mut app, &rt, id, btn_file);
        app.frame_all();
        let item = {
            let w = app.window_ctx(id).unwrap();
            let t = w.track();
            let popup = t.roots_of(Layer::Popup).next().unwrap().node;
            t.descendants(popup)
                .into_iter()
                .find(|n| matches!(&t.get(*n).unwrap().kind, Kind::Text(s) if s == "新建"))
                .expect("有 新建 项")
        };
        tap_node(&mut app, &rt, id, item);
        assert_eq!(vm.last.get(), "新建");
        assert_eq!(vm.open_menu.get(), "");
        app.frame_all();
        assert!(popup_texts(&app, id).is_empty());

        // 编辑 → 查找 ▸：子菜单出现在右侧（RightOf，锚点是弹层内的项）
        let (_, btn_edit) = menu_buttons(&app, id);
        tap_node(&mut app, &rt, id, btn_edit);
        app.frame_all();
        let find_item = {
            let w = app.window_ctx(id).unwrap();
            let t = w.track();
            let popup = t.roots_of(Layer::Popup).next().unwrap().node;
            t.descendants(popup)
                .into_iter()
                .find(|n| matches!(&t.get(*n).unwrap().kind, Kind::Text(s) if s == "查找 ▸"))
                .expect("有 查找 项")
        };
        tap_node(&mut app, &rt, id, find_item);
        app.frame_all();

        // 两个弹层：菜单 + 子菜单（同层后声明者在上面）
        let (item_r, sub_r) = {
            let w = app.window_ctx(id).unwrap();
            let t = w.track();
            let mut roots = t.roots_of(Layer::Popup).map(|r| r.node);
            let _menu = roots.next().unwrap();
            let sub = roots.next().expect("子菜单弹层存在");
            (crate::layout::rect_of(t, find_item), crate::layout::rect_of(t, sub))
        };
        assert!(
            (sub_r.x - (item_r.right() + 4.0)).abs() < 0.5,
            "子菜单在锚点右侧：item={item_r:?} sub={sub_r:?}"
        );
        assert!((sub_r.y - item_r.y).abs() < 0.5, "子菜单与锚点顶对齐");

        // 点子菜单项：动作 + 全部关闭（含子菜单状态）
        let sub_item = {
            let w = app.window_ctx(id).unwrap();
            let t = w.track();
            let mut roots = t.roots_of(Layer::Popup).map(|r| r.node);
            let _menu = roots.next().unwrap();
            let sub = roots.next().unwrap();
            t.descendants(sub)
                .into_iter()
                .find(|n| matches!(&t.get(*n).unwrap().kind, Kind::Text(s) if s == "查找内容"))
                .expect("有 查找内容 项")
        };
        tap_node(&mut app, &rt, id, sub_item);
        assert_eq!(vm.last.get(), "查找内容");
        assert_eq!(vm.open_menu.get(), "");
        assert!(!vm.sub_open.get());
        app.frame_all();
        assert!(popup_texts(&app, id).is_empty(), "全部弹层关闭");
    }

    #[test]
    fn clicking_outside_closes_the_open_menu() {
        let (rt, mut app, vm, id) = menu_vm();
        let (btn_file, _) = menu_buttons(&app, id);

        tap_node(&mut app, &rt, id, btn_file);
        app.frame_all();
        assert!(!popup_texts(&app, id).is_empty());

        // 点窗口右下角（远离菜单条与弹层）⇒ 轻关闭
        {
            let w = app.window_ctx_mut(id).unwrap();
            w.pointer(
                &rt,
                InputEvent::Down {
                    pointer: PointerId(0),
                    pos: Point::new(280.0, 200.0),
                    button: PointerButton::Left,
                },
            );
            w.pointer(
                &rt,
                InputEvent::Up {
                    pointer: PointerId(0),
                    pos: Point::new(280.0, 200.0),
                    button: PointerButton::Left,
                },
            );
        }
        assert_eq!(vm.open_menu.get(), "", "点外部 ⇒ Dismissed ⇒ 关闭");
        app.frame_all();
        assert!(popup_texts(&app, id).is_empty());
    }

    // ─────────────── M6：VirtualList（ScrollChanged + 组合）───────────────

    /// 用**框架 API**（`ViewBuf::virtual_list`）声明 1000 行长列表。
    struct LongList {
        vl: VirtualListState,
        items: Vec<u32>,
    }

    impl ViewModel for LongList {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            v.scroll(|s| {
                s.height(240.0);
                s.width(140.0);
                // 只物化可见窗口：窗口起点由 `vl` 记住，`ScrollChanged` 时自动回写
                s.virtual_list(
                    &self.vl,
                    &self.items,
                    |i| *i as u64,
                    24.0,
                    240.0,
                    |v, i| {
                        v.row(|r| {
                            r.height(24.0);
                            r.text(format!("Item {i}")).font_size(16.0);
                        });
                    },
                );
            });
        }
    }

    fn long_list_with(count: u32) -> (Runtime, App, Rc<LongList>, WindowId) {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(LongList {
            vl: VirtualListState::new(&rt),
            items: (0..count).collect(),
        });
        let id = app.window_erased(WindowConfig::new().size(200.0, 300.0), erased(Rc::clone(&vm)));
        app.frame_all();
        (rt, app, vm, id)
    }

    fn long_list() -> (Runtime, App, Rc<LongList>, WindowId) {
        long_list_with(1000)
    }

    /// 滚动容器 = 内容根本身（`v.scroll` 在 view 顶层声明）
    fn scroll_node(app: &App, id: WindowId) -> NodeId {
        app.window_ctx(id).unwrap().content_root().unwrap()
    }

    fn visible_labels(app: &App, id: WindowId) -> Vec<String> {
        let w = app.window_ctx(id).unwrap();
        let t = w.track();
        let scroll = scroll_node(app, id);
        t.descendants(scroll)
            .into_iter()
            .filter_map(|n| match &t.get(n).unwrap().kind {
                Kind::Text(s) if s.starts_with("Item ") => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    /// 可见行 = `(item 下标, 行节点)`（行节点 = keyed item 的根，也是 key 所在处）
    fn visible_rows(app: &App, id: WindowId) -> Vec<(u32, NodeId)> {
        let w = app.window_ctx(id).unwrap();
        let t = w.track();
        let scroll = scroll_node(app, id);
        let mut out = Vec::new();
        for n in t.descendants(scroll) {
            let Some(node) = t.get(n) else { continue };
            if let Kind::Text(s) = &node.kind
                && let Some(rest) = s.strip_prefix("Item ")
                && let Ok(i) = rest.parse::<u32>()
                && let Some(p) = t.parent_of(n)
            {
                out.push((i, p));
            }
        }
        out
    }

    #[test]
    fn virtual_list_materializes_only_the_visible_window() {
        let (rt, mut app, _vm, id) = long_list();
        let scroll = scroll_node(&app, id);

        // 1000 行只渲染 11 行（240/24 + 1），标签是前 11 个
        let labels = visible_labels(&app, id);
        assert_eq!(labels.len(), 11, "只物化可见窗口：{}", labels.len());
        assert_eq!(labels[0], "Item 0");
        assert_eq!(labels[10], "Item 10");

        // 内容尺寸是"虚拟"的完整高度 ⇒ 滚动条诚实
        assert!(
            (app.window_ctx(id)
                .unwrap()
                .track()
                .get(scroll)
                .unwrap()
                .content_size
                .height
                - 24000.0)
                .abs()
                < 0.5,
            "内容高 = 1000 × 24"
        );

        // 滚一屏：ScrollChanged ⇒ first=10 ⇒ 下一帧换一批行
        {
            let w = app.window_ctx_mut(id).unwrap();
            w.pointer(
                &rt,
                InputEvent::Wheel {
                    pointer: PointerId(0),
                    pos: Point::new(60.0, 120.0),
                    delta: (0.0, -240.0), // 负 = 向下滚一屏
                },
            );
        }
        app.frame_all(); // 派发 ScrollChanged ⇒ 信号写入（本帧还是旧行）
        app.frame_all(); // view 重跑 ⇒ 换窗
        let labels = visible_labels(&app, id);
        assert_eq!(labels.len(), 11, "窗口大小不变");
        assert_eq!(labels[0], "Item 10", "窗口前移 10 行：{labels:?}");
        assert_eq!(labels[10], "Item 20");

        // 滚动位置在保留树里（视图态）⇒ 与信号一致，不回弹
        assert!((app.window_ctx(id).unwrap().track().scroll_offset(scroll).1 - 240.0).abs() < 0.5);
    }

    /// **key 复用**：滚动后仍然可见的行必须复用原节点（`NodeId` 不变）
    /// —— 这是"行内状态不丢"（选中 / 输入框 / 展开态）的前提。
    #[test]
    fn virtual_list_reuses_rows_by_key_when_scrolling() {
        let (rt, mut app, _vm, id) = long_list();
        let scroll = scroll_node(&app, id);

        let before = visible_rows(&app, id);
        assert_eq!(before.len(), 11, "窗口 240/24 + 1");

        // 滚 5 行（120px）
        app.window_ctx_mut(id)
            .unwrap()
            .track_mut()
            .set_scroll_offset(scroll, (0.0, 120.0));
        app.frame_all(); // 派发 ScrollChanged ⇒ 回写窗口起点
        app.frame_all(); // view 重跑 ⇒ 换窗

        let after = visible_rows(&app, id);
        assert_eq!(after[0].0, 5, "窗口前移 5 行：{after:?}");
        assert_eq!(after[10].0, 15);

        // 重叠区（Item 5..=10）两帧都在 ⇒ 节点必须复用
        for (idx, node) in before.iter().filter(|(i, _)| (5..=10).contains(i)) {
            let (_, after_node) = after
                .iter()
                .find(|(i, _)| i == idx)
                .unwrap_or_else(|| panic!("Item {idx} 应仍在窗口内"));
            assert_eq!(node, after_node, "Item {idx} 的行节点应被 key 复用（而不是按下标重建）");
        }
        let _ = &rt;
    }

    /// 边界：空列表 / 起点越界都不能 panic、不能越界切片
    #[test]
    fn virtual_list_handles_empty_and_out_of_range() {
        let (_rt, mut app, vm, id) = long_list_with(0);
        assert!(visible_rows(&app, id).is_empty(), "空列表物化 0 行");
        let scroll = scroll_node(&app, id);
        assert!(
            app.window_ctx(id)
                .unwrap()
                .track()
                .get(scroll)
                .unwrap()
                .content_size
                .height
                < 0.5,
            "空列表内容高为 0"
        );

        // 起点远超 count：clamp 后不物化任何行
        vm.vl.set_first(99_999);
        app.frame_all();
        assert!(visible_rows(&app, id).is_empty());

        // 只有 1 项时也正常
        let (_rt2, app2, _vm2, id2) = long_list_with(1);
        let rows = visible_rows(&app2, id2);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 0);
    }

    /// `virtual_list` 的虚拟占位：内容总高 = count × item_h（滚动条因此"诚实"），
    /// 且**没有**额外的占位节点（用内容列的 padding 撑，而非空 row）。
    #[test]
    fn virtual_list_pads_instead_of_inserting_spacer_nodes() {
        let (_rt, app, _vm, id) = long_list();
        let w = app.window_ctx(id).unwrap();
        let t = w.track();
        let scroll = scroll_node(&app, id);
        // 滚动容器 → 内容列 → 11 行（没有第二个占位节点）
        let col = t.children(scroll)[0];
        assert_eq!(t.children(col).len(), 11, "内容列只放可见行");
        assert_eq!(t.get(scroll).unwrap().content_size.height, 24.0 * 1000.0);
    }

    /// 虚拟列表**包在普通 column 里**（"标题 + 列表"这种最常见排布）也必须能滚：
    /// `ScrollChanged` 处理器要挂在最近的**滚动容器祖先**上。
    ///
    /// 症状（pdfkit 侧栏真实遇到）：处理器挂在当前节点 ⇒ 事件永远不来 ⇒ 窗口起点不推进，
    /// 看起来"只有前 20 项、滚下去没有新行"。
    #[test]
    fn virtual_list_nested_in_a_plain_column_still_advances_its_window() {
        struct Nested {
            vl: VirtualListState,
            items: Vec<u32>,
        }
        impl ViewModel for Nested {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.scroll(|sc| {
                    sc.height(240.0);
                    sc.width(160.0);
                    sc.column(|list| {
                        // 标题：普通行，虚拟列表在它下面
                        list.text("页面").font_size(13.0);
                        list.virtual_list(
                            &self.vl,
                            &self.items,
                            |i| *i as u64,
                            24.0,
                            240.0,
                            |v, i| {
                                v.row(|r| {
                                    r.height(24.0);
                                    r.text(format!("Item {i}")).font_size(16.0);
                                });
                            },
                        );
                    });
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Nested {
            vl: VirtualListState::new(&rt),
            items: (0..1000).collect(),
        });
        let id = app.window_erased(WindowConfig::new().size(200.0, 300.0), erased(Rc::clone(&vm)));
        app.frame_all();

        let scroll = scroll_node(&app, id);
        let before = visible_labels(&app, id);
        assert_eq!(before.first().map(String::as_str), Some("Item 0"), "{before:?}");

        // 内容高度按"虚拟"总高算 ⇒ 滚动条诚实（能一路滚到最后）
        {
            let ctx = app.window_ctx(id).unwrap();
            let content = ctx.track().get(scroll).unwrap().content_size.height;
            assert!(
                content > 1000.0 * 24.0 * 0.9,
                "内容高度应接近 1000 行（虚拟 padding 计入）：{content}"
            );
        }

        // 滚 10 行 ⇒ 窗口起点推进（修复前不动，永远只有前 11 行）
        app.window_ctx_mut(id)
            .unwrap()
            .track_mut()
            .set_scroll_offset(scroll, (0.0, 240.0));
        app.frame_all(); // 派发 ScrollChanged ⇒ 回写窗口起点
        app.frame_all(); // view 重跑 ⇒ 换窗

        let after = visible_labels(&app, id);
        assert_eq!(
            after.first().map(String::as_str),
            Some("Item 10"),
            "滚动后应物化新的窗口：{after:?}"
        );
    }

    #[test]
    fn scrolling_back_reveals_earlier_items() {
        let (_rt, mut app, _vm, id) = long_list();
        let scroll = scroll_node(&app, id);

        // 直接把偏移滚到很深（模拟 ScrollTo / 大量滚轮）
        app.window_ctx_mut(id)
            .unwrap()
            .track_mut()
            .set_scroll_offset(scroll, (0.0, 24000.0 - 240.0));
        app.frame_all(); // 派发 ScrollChanged ⇒ 信号写入
        app.frame_all(); // view 重跑 ⇒ 换窗
        let labels = visible_labels(&app, id);
        assert_eq!(labels.last().map(String::as_str), Some("Item 999"), "{labels:?}");
    }

    #[test]
    fn popup_layer_draws_an_opaque_surface() {
        let (rt, mut app, _, id) = picker();
        let btn = app
            .window_ctx(id)
            .unwrap()
            .track()
            .children(app.window_ctx(id).unwrap().content_root().unwrap())[0];

        tap_node(&mut app, &rt, id, btn);
        app.frame_all();
        let popup = popup_root_of(&app, id).expect("弹层出现");
        let pr = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), popup);

        // 采样弹层的内边距区域（无内容覆盖）⇒ 应是不透明的弹层底色，而非窗口底色
        let ctx = app.window_ctx(id).unwrap();
        let p = pixel(ctx, (pr.x + 3.0) as u16, (pr.y + 3.0) as u16);
        assert_eq!(
            p,
            opaque(
                Theme::light().input_background.r,
                Theme::light().input_background.g,
                Theme::light().input_background.b
            ),
            "弹层底色 = 主题 input_background（不透明）：{p:?}"
        );
    }

    #[test]
    fn hover_repaints_the_button_with_its_hover_color() {
        struct Btn;
        impl ViewModel for Btn {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.button("B");
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(200.0, 100.0), Btn);
        app.frame_all();
        let btn = app
            .window_ctx(id)
            .unwrap()
            .track()
            .children(app.window_ctx(id).unwrap().content_root().unwrap())[0];
        let r = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), btn);
        // 采样点：左缘内侧（避开圆角与文字）
        let (sx, sy) = ((r.x + 5.0) as u16, (r.center().y) as u16);

        let before = { pixel(app.window_ctx(id).unwrap(), sx, sy) };

        // 指针移到按钮上 ⇒ hover 态 ⇒ 重绘为 hover 色
        app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Move {
                pointer: PointerId(0),
                pos: r.center(),
            },
        );
        app.frame_all();

        let after = { pixel(app.window_ctx(id).unwrap(), sx, sy) };
        let t = Theme::light();
        assert_eq!(before, opaque(t.control.r, t.control.g, t.control.b), "常态 = control");
        assert_eq!(
            after,
            opaque(t.control_hover.r, t.control_hover.g, t.control_hover.b),
            "hover = control_hover"
        );
    }

    // ─────────────── M6：开关 / 单选 ───────────────

    struct Toggles {
        on: Signal<bool>,
        color: Signal<String>,
    }

    impl ViewModel for Toggles {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            v.column(|c| {
                c.gap(8.0);
                c.switch_bound(&self.on);
                c.radio_bound(&self.color, "red");
                c.radio_bound(&self.color, "green");
                // 只显示形态（未绑定 ⇒ 内置行为不激活）
                c.switch(true);
                c.radio(true);
            });
        }
    }

    fn toggles() -> (Runtime, App, Rc<Toggles>, WindowId, Vec<NodeId>) {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Toggles {
            on: Signal::new(&rt, false),
            color: Signal::new(&rt, "red".to_string()),
        });
        let id = app.window_erased(WindowConfig::new().size(200.0, 200.0), erased(Rc::clone(&vm)));
        app.frame_all();
        let kids = app
            .window_ctx(id)
            .unwrap()
            .track()
            .children(app.window_ctx(id).unwrap().content_root().unwrap())
            .to_vec();
        (rt, app, vm, id, kids)
    }

    fn tap_node(app: &mut App, rt: &Runtime, id: WindowId, node: NodeId) {
        let p = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), node).center();
        let w = app.window_ctx_mut(id).unwrap();
        w.pointer(
            rt,
            InputEvent::Down {
                pointer: PointerId(0),
                pos: p,
                button: PointerButton::Left,
            },
        );
        w.pointer(
            rt,
            InputEvent::Up {
                pointer: PointerId(0),
                pos: p,
                button: PointerButton::Left,
            },
        );
    }

    #[test]
    fn switch_bound_toggles_the_signal_and_the_desc_follows() {
        let (rt, mut app, vm, id, kids) = toggles();
        tap_node(&mut app, &rt, id, kids[0]);
        assert!(vm.on.get(), "点击 ⇒ 写回 signal");

        // 下一帧 desc 跟上（apply_to 判等 ⇒ 零补丁，不回弹）
        app.frame_all();

        tap_node(&mut app, &rt, id, kids[0]);
        assert!(!vm.on.get());

        // 未绑定的只显示形态：点不变
        tap_node(&mut app, &rt, id, kids[3]);
        assert!(!vm.on.get());
    }

    #[test]
    fn radio_group_is_mutually_exclusive_via_the_view_rerun() {
        let (rt, mut app, vm, id, kids) = toggles();
        assert_eq!(vm.color.get(), "red", "初始选中 red");

        // 点 green ⇒ signal 变 ⇒ 下一帧两个 radio 的 selected 由对齐器自然更新
        tap_node(&mut app, &rt, id, kids[2]);
        assert_eq!(vm.color.get(), "green");

        app.frame_all();
        let (red_sel, green_sel) = {
            let t = app.window_ctx(id).unwrap().track();
            (
                matches!(t.get(kids[1]).unwrap().kind, Kind::Radio { selected: true, .. }),
                matches!(t.get(kids[2]).unwrap().kind, Kind::Radio { selected: true, .. }),
            )
        };
        assert!(!red_sel && green_sel, "声明式互斥：red=false green=true");

        // 点回 red：同样只需一次信号写入
        tap_node(&mut app, &rt, id, kids[1]);
        assert_eq!(vm.color.get(), "red");
    }

    #[test]
    fn unbound_radio_ignores_taps() {
        let (rt, mut app, vm, id, kids) = toggles();
        tap_node(&mut app, &rt, id, kids[4]);
        assert_eq!(vm.color.get(), "red", "未绑定 ⇒ 编辑不生效");
    }

    // ─────────────── M5：主题 ───────────────

    // ─────────────── 后台任务 + loading 遮罩（框架级） ───────────────

    #[test]
    fn background_task_shows_overlay_animates_and_delivers_its_result() {
        use crate::task::TaskEvent;

        /// 任务结果的落地目标
        struct Worker {
            report: Signal<Option<u32>>,
        }
        impl ViewModel for Worker {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.center();
                    c.text(format!("report={:?}", self.report.get()));
                });
            }
            fn on_external(self: &Rc<Self>, _cx: &mut Ctx, data: ExternalData) {
                if let Some(ev) = data.downcast::<TaskEvent>()
                    && let Some(r) = ev.payload.downcast::<u32>()
                {
                    self.report.set(Some(r));
                }
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Worker {
            report: Signal::new(&rt, None),
        });
        let id = app.window_erased(WindowConfig::new().size(400.0, 300.0), erased(Rc::clone(&vm)));
        app.frame_all();

        // 无头环境没有平台唤醒器 ⇒ 投递走本地队列（测试正好用它当"事件循环"）
        assert!(!rt.is_online());

        // 起任务 + 遮罩
        let handle = rt.spawn_task_busy(id, "正在处理…", |ctx| {
            ctx.progress(1, 2);
            7u32
        });

        // 下一帧：view 重跑（遮罩是声明式的）⇒ 遮罩层出现在保留树里
        let stats = app.frame_all();
        assert!(stats[0].1.view_ran, "忙碌状态变化 ⇒ 重跑 view");
        {
            let w = app.window_ctx(id).unwrap();
            assert!(
                w.track().root_by_tag(crate::overlay::BUSY_OVERLAY_TAG).is_some(),
                "遮罩层已声明"
            );
        }

        // 动画：动画帧把**卡片**标脏（只重绘，不重跑 view），并给出下一次唤醒时刻
        {
            let ctx = app.window_ctx_mut(id).unwrap();
            let _ = ctx.track_mut().take_damage(); // 清掉前面的脏区，只看动画帧贡献
            ctx.animate(Instant::now());
            let (damage, all) = ctx.track_mut().take_damage();
            assert!(all || !damage.is_empty(), "有遮罩 ⇒ 动画帧有重绘义务");
            assert!(ctx.next_wakeup(&rt).is_some(), "遮罩 ⇒ 定时唤醒（spinner）");
        }
        // 遮罩确实画到了屏幕上：卡片内的 padding 区应是卡片底色（不是窗口底色）
        {
            let ctx = app.window_ctx(id).unwrap();
            let track = ctx.track();
            let root = track
                .root_by_tag(crate::overlay::BUSY_OVERLAY_TAG)
                .expect("遮罩层在")
                .node;
            let card = track.children(root)[0];
            let r = crate::layout::rect_of(track, card);
            let light = Theme::light();
            assert_eq!(
                pixel(ctx, (r.x + 10.0) as u16, (r.y + 10.0) as u16),
                opaque(
                    light.input_background.r,
                    light.input_background.g,
                    light.input_background.b
                ),
                "窗口里出现了遮罩卡片（{r:?}）"
            );
        }

        // 等任务跑完（`frame_all` 会消费本地投递队列 ⇒ 等价于平台层的事件循环）
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while (!handle.is_done() || vm.report.get().is_none()) && Instant::now() < deadline {
            app.frame_all();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        app.frame_all();

        // 结果落地 + 遮罩自动收起
        assert_eq!(vm.report.get(), Some(7), "任务返回值经 TaskEvent 落到 Signal");
        assert!(!rt.is_busy(id) && !rt.has_tasks(id), "完成 ⇒ 任务表与忙碌项都清空");
        {
            let ctx = app.window_ctx(id).unwrap();
            assert!(
                ctx.track().root_by_tag(crate::overlay::BUSY_OVERLAY_TAG).is_none(),
                "完成 ⇒ 遮罩层消失"
            );
            assert!(
                ctx.next_wakeup(&rt).is_none(),
                "遮罩消失且没有聚焦输入框/tooltip ⇒ 回到零唤醒（空闲零功耗）"
            );
        }
    }

    // ─────────────── 统一时钟：定时器 / 动画帧 ───────────────

    use crate::event::Emitter;
    use crate::view::VirtualListState;
    use std::cell::Cell;

    /// 计时器 + 动画的观察 VM
    struct Clocked {
        timeouts: Rc<Cell<u32>>,
        intervals: Rc<Cell<u32>>,
        frames: Rc<Cell<u32>>,
        last_dt_ms: Rc<Cell<u128>>,
        /// 还要不要再要一帧（模拟"动画结束就停"）
        keep_animating: Rc<Cell<bool>>,
    }

    impl ViewModel for Clocked {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            v.column(|c| {
                c.text("clock");
            });
        }

        fn on_animation(self: &Rc<Self>, cx: &mut Ctx, _now: Instant, dt: std::time::Duration) {
            self.frames.set(self.frames.get() + 1);
            self.last_dt_ms.set(dt.as_millis());
            if self.keep_animating.get() {
                cx.request_animation(); // 经典 RAF：想继续就再要一帧
            }
        }
    }

    fn clocked_vm() -> Clocked {
        Clocked {
            timeouts: Rc::new(Cell::new(0)),
            intervals: Rc::new(Cell::new(0)),
            frames: Rc::new(Cell::new(0)),
            last_dt_ms: Rc::new(Cell::new(0)),
            keep_animating: Rc::new(Cell::new(true)),
        }
    }

    #[test]
    fn timeouts_fire_once_and_intervals_keep_firing_until_cancelled() {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = clocked_vm();
        let id = app.window(
            WindowConfig::new().size(200.0, 120.0),
            Clocked {
                timeouts: Rc::clone(&vm.timeouts),
                intervals: Rc::clone(&vm.intervals),
                frames: Rc::clone(&vm.frames),
                last_dt_ms: Rc::clone(&vm.last_dt_ms),
                keep_animating: Rc::clone(&vm.keep_animating),
            },
        );
        app.frame_all();

        // 一次性：立即到期 ⇒ 下一帧触发一次，之后不再有
        rt.set_timeout(id, std::time::Duration::ZERO, {
            let t = Rc::clone(&vm.timeouts);
            move |_| t.set(t.get() + 1)
        });
        assert!(rt.next_deadline(id).is_some(), "有定时器 ⇒ 平台会定时唤醒");
        app.frame_all();
        assert_eq!(vm.timeouts.get(), 1);
        assert!(rt.next_deadline(id).is_none(), "一次性定时器跑完就没了");
        app.frame_all();
        assert_eq!(vm.timeouts.get(), 1, "不会再触发");

        // 周期：首次立即到期，之后每 1ms 一次；取消后停
        let handle = rt.set_interval(id, std::time::Duration::ZERO, {
            let i = Rc::clone(&vm.intervals);
            move |_| i.set(i.get() + 1)
        });
        app.frame_all();
        let after_first = vm.intervals.get();
        assert!(after_first >= 1, "周期定时器触发");
        std::thread::sleep(std::time::Duration::from_millis(3)); // 跨过下一个间隔
        app.frame_all();
        assert!(vm.intervals.get() > after_first, "还会继续触发");

        handle.cancel();
        let stopped = vm.intervals.get();
        app.frame_all();
        assert_eq!(vm.intervals.get(), stopped, "取消后不再触发");
        assert!(rt.next_deadline(id).is_none());
    }

    #[test]
    fn animation_frames_run_only_while_requested_and_expose_dt() {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = clocked_vm();
        let id = app.window(
            WindowConfig::new().size(200.0, 120.0),
            Clocked {
                timeouts: Rc::clone(&vm.timeouts),
                intervals: Rc::clone(&vm.intervals),
                frames: Rc::clone(&vm.frames),
                last_dt_ms: Rc::clone(&vm.last_dt_ms),
                keep_animating: Rc::clone(&vm.keep_animating),
            },
        );
        app.frame_all();
        assert_eq!(vm.frames.get(), 0, "没请求就不跑动画");

        rt.request_animation(id);
        app.frame_all();
        assert_eq!(vm.frames.get(), 1, "请求一次 = 一帧");
        assert!(rt.next_deadline(id).is_some(), "持续动画 ⇒ 平台持续唤醒");

        std::thread::sleep(std::time::Duration::from_millis(2));
        app.frame_all();
        assert_eq!(vm.frames.get(), 2, "回调里再请求 ⇒ 连续跑");
        assert!(vm.last_dt_ms.get() >= 1, "dt 是真实帧间隔（毫秒）");

        // 停手：不再请求 ⇒ 动画停（空闲零功耗）
        vm.keep_animating.set(false);
        app.frame_all();
        let last = vm.frames.get();
        app.frame_all();
        app.frame_all();
        assert_eq!(vm.frames.get(), last, "不请求就不再跑");
        assert!(rt.next_deadline(id).is_none());
        assert!(app.window_ctx(id).unwrap().next_wakeup(&rt).is_none(), "回到零唤醒");
    }

    #[test]
    fn custom_events_reach_on_external_through_the_frame_driver() {
        struct Bus {
            got: Signal<Vec<String>>,
        }
        impl ViewModel for Bus {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("bus");
                });
            }
            fn on_external(self: &Rc<Self>, _cx: &mut Ctx, data: ExternalData) {
                if let Some(s) = data.downcast::<String>() {
                    self.got.update(|v| v.push(s));
                }
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Bus {
            got: Signal::new(&rt, Vec::new()),
        });
        let id = app.window_erased(WindowConfig::new(), erased(Rc::clone(&vm)));
        app.frame_all();

        // UI 线程发事件
        assert!(rt.emit(id, "hello".to_string()));
        // 跨线程（或业务层）用发射器发事件
        let tx = Emitter::<String>::new(&rt, id);
        let t = std::thread::spawn(move || tx.emit("world".to_string()));
        assert!(t.join().unwrap());

        app.frame_all(); // 帧驱动消费队列
        let mut got = vm.got.get();
        got.sort();
        assert_eq!(got, vec!["hello".to_string(), "world".to_string()]);
    }

    /// `full_repaint`：不看脏区，每帧整窗重绘（逃生舱 / 残影对照工具）
    #[test]
    fn full_repaint_mode_always_redraws_the_whole_window() {
        struct Page {
            n: Signal<u32>,
        }
        impl ViewModel for Page {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text(format!("n={}", self.n.get()));
                });
            }
        }

        // 普通模式：只改一个文本 ⇒ 只重画那一小块
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Page { n: Signal::new(&rt, 0) });
        let _id = app.window_erased(WindowConfig::new().size(400.0, 300.0), erased(Rc::clone(&vm)));
        app.frame_all();
        vm.n.set(1);
        let st = app.frame_all();
        assert!(!st[0].1.damage_all, "默认走脏区");
        assert!(st[0].1.render.raster.pixels < 400 * 300, "不是整窗");

        // 整窗重绘模式：同样只改一个文本，但整窗重画
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Page { n: Signal::new(&rt, 0) });
        let _id = app.window_erased(
            WindowConfig::new().size(400.0, 300.0).full_repaint(true),
            erased(Rc::clone(&vm)),
        );
        app.frame_all();
        vm.n.set(1);
        let st = app.frame_all();
        assert!(st[0].1.damage_all, "full_repaint ⇒ 整窗脏");
        assert_eq!(st[0].1.render.raster.pixels, 400 * 300, "整窗像素都重画");
    }

    /// 快活儿也要看得见遮罩：忙碌段在**两次出帧之间**开始又结束（小文件读盘的真实情形，
    /// 完成消息往往先于下一帧被处理），配了最短可见时间后下一帧仍然画得出遮罩。
    #[test]
    fn a_fast_busy_section_still_shows_the_overlay() {
        struct Empty;
        impl ViewModel for Empty {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("主界面");
                });
            }
        }

        let rt = Runtime::new();
        rt.set_busy_min_visible(Duration::from_millis(300));
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(400.0, 300.0), Empty);
        app.frame_all();

        // 忙碌段瞬间开始又结束（中间没有出过帧）
        let busy = rt.begin_busy(id, "正在打开…");
        busy.finish();

        // 下一帧：遮罩必须在 —— 没有它用户就什么都看不到
        app.frame_all();
        {
            let ctx = app.window_ctx(id).unwrap();
            assert!(
                ctx.track().root_by_tag(crate::overlay::BUSY_OVERLAY_TAG).is_some(),
                "最短可见期内遮罩要在"
            );
            assert!(ctx.next_wakeup(&rt).is_some(), "还要定时唤醒去收它");
        }

        // 到点 ⇒ 收掉；下一帧没有遮罩，回到零唤醒
        assert!(rt.reap_busy(Instant::now() + Duration::from_millis(400)));
        app.frame_all();
        {
            let ctx = app.window_ctx(id).unwrap();
            assert!(
                ctx.track().root_by_tag(crate::overlay::BUSY_OVERLAY_TAG).is_none(),
                "到点 ⇒ 遮罩收起"
            );
            assert!(ctx.next_wakeup(&rt).is_none(), "遮罩收掉 ⇒ 回到零唤醒");
        }
    }

    /// 整窗图片（PDF 预览那类）不得盖住浮层：遮罩是**后**声明的 Modal 层，必须压在图上。
    ///
    /// 症状来源：光栅层原先把图片 op 统一放到批次末尾 blit（"图片总在最上层"），
    /// 于是预览图盖住了 loading 遮罩 —— 用户"看不到进度 modal"。
    #[test]
    fn busy_overlay_covers_a_full_window_image() {
        struct Pic;
        impl ViewModel for Pic {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                // 整窗一张纯红图（2×2 拉伸铺满）
                v.image(std::sync::Arc::new(crate::ImageData {
                    width: 2,
                    height: 2,
                    rgba: vec![
                        255, 0, 0, 255, 255, 0, 0, 255, //
                        255, 0, 0, 255, 255, 0, 0, 255,
                    ],
                }))
                .expand(true);
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(400.0, 300.0), Pic);
        app.frame_all();
        {
            let ctx = app.window_ctx(id).unwrap();
            assert_eq!(pixel(ctx, 200, 150), opaque(255, 0, 0), "先确认整窗都是图");
        }

        // 忙碌遮罩压上来：卡片底色必须出现在图上（旧实现这里还是红的）
        let busy = rt.begin_busy(id, "正在打开…");
        app.frame_all();
        {
            let ctx = app.window_ctx(id).unwrap();
            let root = ctx
                .track()
                .root_by_tag(crate::overlay::BUSY_OVERLAY_TAG)
                .expect("遮罩层在")
                .node;
            let card = ctx.track().children(root)[0];
            let r = crate::layout::rect_of(ctx.track(), card);
            let light = Theme::light();
            assert_eq!(
                pixel(ctx, (r.x + 10.0) as u16, (r.y + 10.0) as u16),
                opaque(
                    light.input_background.r,
                    light.input_background.g,
                    light.input_background.b
                ),
                "遮罩卡片压在图片之上"
            );
        }
        busy.finish();
    }

    /// 忙碌遮罩是 Modal 层 ⇒ 下层的按钮点不到（交互被挡住）
    #[test]
    fn busy_overlay_blocks_input_to_the_content_below() {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Counter {
            count: Signal::new(&rt, 0),
        });
        let id = app.window_erased(WindowConfig::new().size(400.0, 300.0), erased(Rc::clone(&vm)));
        app.frame_all();

        // `Counter` 的 view 结构固定：内容根第 3 个子节点是个 row，里面第 1 个是「+1」按钮
        let button = {
            let w = app.window_ctx(id).unwrap();
            let root = w.content_root().unwrap();
            let row = w.track().children(root)[2];
            w.track().children(row)[0]
        };

        tap_node(&mut app, &rt, id, button);
        assert_eq!(vm.count.get(), 1, "平时点得到");

        // 遮罩起来：同样点一下 ⇒ 下层按钮收不到
        let busy = rt.begin_busy(id, "后台任务进行中…");
        app.frame_all();
        tap_node(&mut app, &rt, id, button);
        assert_eq!(vm.count.get(), 1, "遮罩挡住下层交互");

        // 收起遮罩 ⇒ 恢复交互
        busy.finish();
        app.frame_all();
        tap_node(&mut app, &rt, id, button);
        assert_eq!(vm.count.get(), 2, "遮罩消失后恢复交互");
    }

    /// 跟随系统：OS 深色上报 ⇒ 重跑 view + 窗口底色跟随（未显式指定底色时）
    #[test]
    fn system_theme_mode_follows_the_os_and_repaints_the_window() {
        use crate::theme::ThemeMode;

        struct Empty;
        impl ViewModel for Empty {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("hi");
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(60.0, 40.0), Empty);
        app.frame_all();
        let light = Theme::light();
        {
            let ctx = app.window_ctx(id).unwrap();
            let bg = pixel(ctx, 5, 35); // 文字下方
            assert_eq!(
                bg,
                opaque(
                    light.window_background.r,
                    light.window_background.g,
                    light.window_background.b
                ),
                "默认浅色底"
            );
        }

        // 切「跟随系统」：默认浅色不变
        rt.set_theme_mode(ThemeMode::System);
        app.frame_all();
        assert_eq!(rt.theme(), Theme::light());

        // OS 转深色 ⇒ 底色与控件一起跟随（view 重跑）
        rt.set_system_dark(true);
        let st = app.frame_all()[0].1.clone();
        assert!(st.view_ran, "系统主题变化 ⇒ view 重跑");
        let dark = Theme::dark();
        {
            let ctx = app.window_ctx(id).unwrap();
            let bg = pixel(ctx, 5, 35);
            assert_eq!(
                bg,
                opaque(
                    dark.window_background.r,
                    dark.window_background.g,
                    dark.window_background.b
                ),
                "跟随系统 ⇒ 窗口底色变深"
            );
        }
    }

    #[test]
    fn theme_switch_recolors_the_controls_on_the_next_frame() {
        struct Btn;
        impl ViewModel for Btn {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.center();
                    c.button("按钮");
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(200.0, 200.0), Btn);
        app.frame_all();

        // 取按钮左下角内侧一点（避开圆角与文字）
        let (sx, sy) = {
            let w = app.window_ctx(id).unwrap();
            let t = w.track();
            let r = crate::layout::rect_of(t, t.children(w.content_root().unwrap())[0]);
            ((r.x + 6.0) as u16, (r.bottom() - 3.0) as u16)
        };
        let light = Theme::light();
        let dark = Theme::dark();

        // 浅色：按钮底 = control token
        {
            let ctx = app.window_ctx(id).unwrap();
            assert_eq!(
                pixel(ctx, sx, sy),
                opaque(light.control.r, light.control.g, light.control.b),
                "按钮底色 = 主题 control"
            );
        }

        // 切深色 ⇒ 重跑 view（token 重新烘焙）+ 整窗重绘 ⇒ 按钮变深
        rt.set_theme(dark);
        let st = app.frame_all()[0].1.clone();
        assert!(st.view_ran, "主题切换 ⇒ view 重跑");
        {
            let ctx = app.window_ctx(id).unwrap();
            assert_eq!(
                pixel(ctx, sx, sy),
                opaque(dark.control.r, dark.control.g, dark.control.b),
                "切主题后按钮底色跟随"
            );
        }

        // 同值切换是 no-op：不再重跑
        rt.set_theme(dark);
        let st = app.frame_all()[0].1.clone();
        assert!(!st.view_ran, "同主题切换是 no-op");
    }

    #[test]
    fn explicit_window_background_survives_theme_switch() {
        struct Empty;
        impl ViewModel for Empty {
            fn view(self: &Rc<Self>, _v: &mut ViewBuf) {}
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(
            WindowConfig::new()
                .size(100.0, 100.0)
                .background(lieui_geom::Color::RED),
            Empty,
        );
        app.frame_all();

        rt.set_theme(Theme::dark());
        app.frame_all();
        let ctx = app.window_ctx(id).unwrap();
        assert_eq!(pixel(ctx, 50, 50), opaque(255, 0, 0), "显式底色优先于主题");
    }

    // ─────────────── M6 收尾：光标闪烁 + 输入框水平滚动 ───────────────

    #[test]
    fn tooltip_opens_after_hover_delay_and_closes_on_leave() {
        struct T;
        impl ViewModel for T {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("hover me").font_size(20.0).tooltip("i am a tooltip");
                    c.text("plain");
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(300.0, 200.0), T);
        app.frame_all();

        // 找到带 tooltip 的文本节点
        let target = {
            let w = app.window_ctx(id).unwrap();
            let root = w.content_root().unwrap();
            w.track()
                .children(root)
                .iter()
                .copied()
                .find(|k| w.track().get(*k).is_some_and(|n| n.tooltip.is_some()))
                .unwrap()
        };

        // 悬停到目标上（会话 armed，但未到延迟 ⇒ 无 tooltip 层）
        let center = {
            let w = app.window_ctx(id).unwrap();
            let r = crate::layout::rect_of(w.track(), target);
            Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
        };
        app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Move {
                pointer: PointerId(0),
                pos: center,
            },
        );
        app.frame_all();
        assert!(
            app.window_ctx(id)
                .unwrap()
                .track()
                .roots_of(crate::track::Layer::Tooltip)
                .next()
                .is_none(),
            "未到延迟 ⇒ 不应出现 tooltip 层"
        );

        // 时间到（tick 推进会话）⇒ 浮出，且锚到目标节点
        let now = Instant::now() + TOOLTIP_DELAY + Duration::from_millis(50);
        app.window_ctx_mut(id).unwrap().tick(&rt, now);
        app.frame_all();
        {
            let w = app.window_ctx(id).unwrap();
            let tip_root = w
                .track()
                .roots_of(crate::track::Layer::Tooltip)
                .next()
                .expect("延迟到 ⇒ tooltip 层应出现");
            assert!(tip_root.framework, "tooltip 层是框架自管的");
            match &tip_root.opts.anchor.as_ref().unwrap().target {
                crate::track::AnchorTarget::Node(t) => assert_eq!(*t, target, "锚到 hover 节点"),
                other => panic!("tooltip 锚点应为 Node：{other:?}"),
            }
        }

        // 切换主题 ⇒ 已浮出的 tooltip 也要换色（框架自管层不会被 view 重建）
        let dark = Theme::dark();
        rt.set_theme(dark);
        app.frame_all();
        app.window_ctx_mut(id).unwrap().tick(&rt, now);
        app.frame_all();
        {
            let w = app.window_ctx(id).unwrap();
            let tip_root = w
                .track()
                .roots_of(crate::track::Layer::Tooltip)
                .next()
                .expect("主题切换不影响 tooltip 的存在");
            let n = w.track().get(tip_root.node).unwrap();
            assert_eq!(
                n.paint.background_color,
                Some(dark.tooltip_background),
                "tooltip 底色跟随主题"
            );
            assert_eq!(n.text.color, dark.tooltip_text, "tooltip 文字色跟随主题");
        }

        // 移开 ⇒ 消失（align 不得回收框架自管层——消失必须来自会话本身）
        app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Move {
                pointer: PointerId(0),
                pos: Point::new(150.0, 150.0),
            },
        );
        app.frame_all();
        assert!(
            app.window_ctx(id)
                .unwrap()
                .track()
                .roots_of(crate::track::Layer::Tooltip)
                .next()
                .is_none(),
            "移开 ⇒ tooltip 层应消失"
        );
    }

    /// tooltip 的文本按**墨迹盒**居中：盒高 = 墨迹高 + 上下各 6px，
    /// 于是字形上下留白相等（行盒 ascent/descent 不对称会让文本看着往上顶）。
    /// 像素级验证：上下 padding 带是纯底色，字形只出现在中间那条墨迹带里。
    #[test]
    fn tooltip_text_is_centered_by_its_ink_box() {
        struct T;
        impl ViewModel for T {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text("hover me").font_size(20.0).tooltip("i am a tooltip");
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let id = app.window(WindowConfig::new().size(300.0, 200.0), T);
        app.frame_all();

        let target = {
            let w = app.window_ctx(id).unwrap();
            let root = w.content_root().unwrap();
            w.track()
                .children(root)
                .iter()
                .copied()
                .find(|k| w.track().get(*k).is_some_and(|n| n.tooltip.is_some()))
                .unwrap()
        };
        let center = {
            let w = app.window_ctx(id).unwrap();
            let r = crate::layout::rect_of(w.track(), target);
            Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
        };
        app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Move {
                pointer: PointerId(0),
                pos: center,
            },
        );
        let now = Instant::now() + TOOLTIP_DELAY + Duration::from_millis(50);
        app.window_ctx_mut(id).unwrap().tick(&rt, now);
        app.frame_all();

        let w = app.window_ctx(id).unwrap();
        let tip_node = w
            .track()
            .roots_of(crate::track::Layer::Tooltip)
            .next()
            .expect("tooltip 层在")
            .node;
        let n = w.track().get(tip_node).unwrap();
        assert!(n.text.spec.optical_align, "tooltip 走墨迹盒对齐");

        let rect = crate::layout::rect_of(w.track(), tip_node);
        let ink = lieui_text::TextEngine::ink_bounds("i am a tooltip", &n.text.spec).expect("tooltip 文本有墨迹");
        assert!(
            (rect.height - (ink.height() + 12.0)).abs() < 0.01,
            "tooltip 盒高 = 墨迹高 + 12：{} vs {}",
            rect.height,
            ink.height()
        );

        // 远离圆角（左右各让 8px）取横带：上/下 padding 带必须是纯底色，
        // 字形只出现在中间那条墨迹带里 ⇒ 上下留白相等。
        let xs: Vec<u16> = ((rect.x + 8.0) as u16..=(rect.right() - 8.0) as u16).collect();
        let bg = pixel(w, xs[0], (rect.y + 2.0) as u16);
        let band_uniform = |y: u16| xs.iter().all(|x| pixel(w, *x, y) == bg);
        assert!(band_uniform((rect.y + 2.0) as u16), "顶部 padding 带是纯底色");
        assert!(band_uniform((rect.bottom() - 2.0) as u16), "底部 padding 带是纯底色");

        let mid: Vec<u16> = ((rect.y + 6.0) as u16..(rect.y + 6.0 + ink.height()) as u16).collect();
        let ink_px = mid
            .iter()
            .filter(|y| xs.iter().any(|x| pixel(w, *x, **y) != bg))
            .count();
        assert!(ink_px > 0, "中间墨迹带里应有字形像素");
    }

    #[test]
    fn caret_blinks_only_while_an_input_is_focused() {
        let (rt, mut app, _, id, input) = form("abc");

        // 未聚焦：animate 是 no-op（不会唤醒，也不会画光标）
        assert!(!app.window_ctx_mut(id).unwrap().animate(Instant::now()));
        assert!(!app.window_ctx(id).unwrap().track().blink_on);

        // 点进输入框：dispatch 重置相位 ⇒ 光标常亮
        click_input(&mut app, &rt, id, input);
        assert!(app.window_ctx(id).unwrap().track().blink_on);

        // 静止一个周期 ⇒ 翻转为灭；再一个周期 ⇒ 又亮
        let later = Instant::now() + BLINK_PERIOD + Duration::from_millis(50);
        assert!(app.window_ctx_mut(id).unwrap().animate(later));
        assert!(!app.window_ctx(id).unwrap().track().blink_on, "周期到 ⇒ 灭");
        assert!(app.window_ctx_mut(id).unwrap().animate(later + BLINK_PERIOD));
        assert!(app.window_ctx(id).unwrap().track().blink_on, "再翻转为亮");

        // 未到周期：不动（这就是"聚焦才动，其余零功耗"的保证）
        assert!(
            !app.window_ctx_mut(id)
                .unwrap()
                .animate(later + BLINK_PERIOD + Duration::from_millis(100))
        );
    }

    #[test]
    fn input_horizontally_scrolls_to_keep_the_caret_visible() {
        let (rt, mut app, vm, id, input) = form("");
        click_input(&mut app, &rt, id, input);

        // 打 40 个字符：远超默认 200 宽的输入框
        type_chars(&mut app, &rt, id, "0123456789abcdef0123456789abcdef01234567");
        app.frame_all();

        let (scroll, caret) = {
            let w = app.window_ctx(id).unwrap();
            match w.track().get(input).map(|n| &n.kind) {
                Some(Kind::Input { caret, scroll, .. }) => (*scroll, *caret),
                other => panic!("{other:?}"),
            }
        };
        assert!(caret == 40);
        assert!(scroll > 0.0, "文本超宽 ⇒ 自动水平滚动，光标保持可见：{scroll}");

        // Home：光标回 0 ⇒ 滚动也回 0（可见窗口跟随光标）
        press_key(&mut app, &rt, id, KeyCode::Named(NamedKey::Home), Modifiers::EMPTY);
        app.frame_all();
        let (caret2, scroll2) = {
            let w = app.window_ctx(id).unwrap();
            match w.track().get(input).map(|n| &n.kind) {
                Some(Kind::Input { caret, scroll, .. }) => (*caret, *scroll),
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(caret2, 0);
        assert!((scroll2 - 0.0).abs() < 0.01, "Home ⇒ 滚动回 0：{scroll2}");
        assert_eq!(vm.name.get().len(), 40);
    }

    // ─────────────── 上下文菜单（`DescRef::context_menu`）───────────────

    /// 一个带右键菜单的列表（每行都声明菜单）
    struct CtxMenuVm {
        /// 事件记录（测试观测用）
        hits: Signal<Vec<String>>,
        /// "当前页"：闭包在**渲染时**才跑，所以菜单项文字用的是它的**当下**值
        page: Signal<u32>,
        /// 行是否还声明菜单（false ⇒ 模拟"那一项不再有菜单"）
        with_menu: Signal<bool>,
    }

    impl ViewModel for CtxMenuVm {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            let (page, with_menu) = (self.page.get(), self.with_menu.get());
            v.column(|list| {
                for i in 1..=3u32 {
                    let label = format!("行 {i}");
                    let me = Rc::clone(self);
                    list.container(|c| {
                        // 同一份标签出现两次：一次可左键点，一次带右键菜单
                        c.text(&label)
                            .padding(6.0)
                            .on_tap(move || me.hits.update(|v| v.push(format!("tap 行 {i}"))));
                        if with_menu {
                            let me_menu = Rc::clone(self);
                            c.text(&label).padding(6.0).context_menu(move |m| {
                                let (a, b) = (Rc::clone(&me_menu), Rc::clone(&me_menu));
                                m.item(format!("复制行 {i}")).on_tap(move || {
                                    a.hits.update(|v| v.push(format!("menu 行 {i} @{page}")));
                                });
                                m.separator();
                                m.item("删除").enabled(i > 1).on_tap(move || {
                                    b.hits.update(|v| v.push(format!("del 行 {i}")));
                                });
                            });
                        }
                    });
                }
            });
        }
    }

    fn menu_setup() -> (Runtime, App, Rc<CtxMenuVm>, WindowId) {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(CtxMenuVm {
            hits: Signal::new(&rt, Vec::new()),
            page: Signal::new(&rt, 7),
            with_menu: Signal::new(&rt, true),
        });
        let id = app.window_erased(WindowConfig::new().size(400.0, 300.0), erased(Rc::clone(&vm)));
        app.frame_all();
        (rt, app, vm, id)
    }

    /// 找到那个**带菜单**的 `label` 文本节点的中心
    fn menu_row_center(app: &App, id: WindowId, label: &str) -> Point {
        let w = app.window_ctx(id).unwrap();
        let t = w.track();
        let node = t
            .descendants(w.content_root().unwrap())
            .into_iter()
            .find(|n| {
                t.get(*n)
                    .is_some_and(|x| x.context_menu.is_some() && matches!(&x.kind, Kind::Text(s) if s == label))
            })
            .unwrap_or_else(|| panic!("找不到带菜单的 {label:?}"));
        center(t, node)
    }

    /// 框架注入的菜单弹层层根（`None` = 当前没有菜单打开）
    fn ctx_menu_root(app: &App, id: WindowId) -> Option<NodeId> {
        app.window_ctx(id).unwrap().context_menu_root()
    }

    /// 菜单里第 `i` 项（0 基，跳过分隔线）的中心 —— 从**布局树**取，不手算几何
    ///
    /// 菜单项的位置由字号/内边距/分隔线高度决定，测试里手算的行高只会 fragile。
    fn ctx_menu_item_center(app: &App, id: WindowId, i: usize) -> Point {
        let root = ctx_menu_root(app, id).expect("菜单应打开");
        let w = app.window_ctx(id).unwrap();
        let t = w.track();
        let menu = t.children(root)[0];
        let rows: Vec<NodeId> = t
            .children(menu)
            .iter()
            .copied()
            .filter(|r| !t.children(*r).is_empty())
            .collect();
        center(t, rows[i])
    }

    /// 右键（按下即可，框架在 Down 就开菜单）
    fn right_click(app: &mut App, rt: &Runtime, id: WindowId, pos: Point) {
        app.window_ctx_mut(id).unwrap().pointer(
            rt,
            InputEvent::Down {
                pointer: PointerId(0),
                pos,
                button: PointerButton::Right,
            },
        );
        app.frame_all();
    }

    #[test]
    fn right_click_opens_the_menu_at_the_cursor() {
        let (rt, mut app, _, id) = menu_setup();
        assert!(ctx_menu_root(&app, id).is_none(), "还没右键，不该有菜单层");

        let pos = menu_row_center(&app, id, "行 2");
        right_click(&mut app, &rt, id, pos);

        let root = ctx_menu_root(&app, id).expect("右键 ⇒ 菜单层出现");
        let r = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), root);
        assert!(
            (r.x - pos.x).abs() < 1.0 && (r.y - (pos.y + 4.0)).abs() < 1.0,
            "菜单贴在光标下方 4px：{r:?} vs 光标 {pos:?}"
        );
    }

    #[test]
    fn right_click_on_an_element_without_a_menu_opens_nothing() {
        let (rt, mut app, _, id) = menu_setup();
        let pos = menu_row_center(&app, id, "行 2"); // 先确认菜单能开
        right_click(&mut app, &rt, id, pos);
        assert!(ctx_menu_root(&app, id).is_some());

        // 右键按在没有菜单的地方 ⇒ 收起（WinUI：flyout 也这样）
        let away = Point::new(390.0, 295.0);
        right_click(&mut app, &rt, id, away);
        assert!(ctx_menu_root(&app, id).is_none(), "空白处右键 ⇒ 收起");
    }

    #[test]
    fn the_menu_content_comes_from_the_declared_builder() {
        let (rt, mut app, _, id) = menu_setup();
        let at = menu_row_center(&app, id, "行 2");
        right_click(&mut app, &rt, id, at);

        let root = ctx_menu_root(&app, id).unwrap();
        let w = app.window_ctx(id).unwrap();
        let t = w.track();
        let menu = t.children(root)[0];
        // 分隔线没有子节点 ⇒ 过滤掉，只留"有标签的行"
        let labels: Vec<String> = t
            .children(menu)
            .iter()
            .filter(|row| !t.children(**row).is_empty())
            .map(|row| n_text(t, t.children(*row)[0]).to_string())
            .collect();
        assert_eq!(
            labels,
            vec!["复制行 2".to_string(), "删除".to_string()],
            "闭包在渲染时才跑 ⇒ 捕到的是当下的 page"
        );
    }

    /// 节点的文本内容（不是"按内容找节点"，而是"取这个节点的文本"）
    fn n_text(t: &Track, n: NodeId) -> &str {
        match &t.get(n).unwrap().kind {
            Kind::Text(s) => s.as_str(),
            other => panic!("应是文本节点：{other:?}"),
        }
    }

    #[test]
    fn clicking_an_item_runs_it_and_closes_the_menu() {
        let (rt, mut app, vm, id) = menu_setup();
        let pos = menu_row_center(&app, id, "行 2");
        right_click(&mut app, &rt, id, pos);

        // 点第一项
        let item_pos = ctx_menu_item_center(&app, id, 0);
        {
            let w = app.window_ctx_mut(id).unwrap();
            w.pointer(
                &rt,
                InputEvent::Down {
                    pointer: PointerId(0),
                    pos: item_pos,
                    button: PointerButton::Left,
                },
            );
            let up = w.pointer(
                &rt,
                InputEvent::Up {
                    pointer: PointerId(0),
                    pos: item_pos,
                    button: PointerButton::Left,
                },
            );
            assert!(up.tapped.is_some(), "点到了菜单项");
        }
        app.frame_all();

        assert_eq!(vm.hits.get(), vec!["menu 行 2 @7".to_string()]);
        assert!(ctx_menu_root(&app, id).is_none(), "点菜单项后收起");
    }

    #[test]
    fn a_disabled_item_neither_runs_nor_closes() {
        let (rt, mut app, vm, id) = menu_setup();
        // "行 1" 的「删除」是禁用的（i > 1）
        let pos = menu_row_center(&app, id, "行 1");
        right_click(&mut app, &rt, id, pos);

        let item_pos = ctx_menu_item_center(&app, id, 1);
        {
            let w = app.window_ctx_mut(id).unwrap();
            w.pointer(
                &rt,
                InputEvent::Down {
                    pointer: PointerId(0),
                    pos: item_pos,
                    button: PointerButton::Left,
                },
            );
            w.pointer(
                &rt,
                InputEvent::Up {
                    pointer: PointerId(0),
                    pos: item_pos,
                    button: PointerButton::Left,
                },
            );
        }
        app.frame_all();

        assert!(vm.hits.get().is_empty(), "禁用项不执行");
        assert!(ctx_menu_root(&app, id).is_some(), "禁用项被点 ⇒ 菜单不收");
    }

    #[test]
    fn left_click_outside_closes_the_menu() {
        let (rt, mut app, _, id) = menu_setup();
        let at = menu_row_center(&app, id, "行 2");
        right_click(&mut app, &rt, id, at);
        assert!(ctx_menu_root(&app, id).is_some());

        let away = Point::new(390.0, 5.0);
        {
            let w = app.window_ctx_mut(id).unwrap();
            w.pointer(
                &rt,
                InputEvent::Down {
                    pointer: PointerId(0),
                    pos: away,
                    button: PointerButton::Left,
                },
            );
        }
        app.frame_all();
        assert!(ctx_menu_root(&app, id).is_none(), "点外面 ⇒ 轻关闭");
    }

    #[test]
    fn right_click_elsewhere_moves_the_menu_to_the_new_target() {
        let (rt, mut app, _, id) = menu_setup();
        let at1 = menu_row_center(&app, id, "行 1");
        right_click(&mut app, &rt, id, at1);
        assert!(ctx_menu_root(&app, id).is_some());

        let pos2 = menu_row_center(&app, id, "行 3");
        right_click(&mut app, &rt, id, pos2);
        let root = ctx_menu_root(&app, id).expect("仍在");
        let r = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), root);
        assert!(
            (r.x - pos2.x).abs() < 1.0 && (r.y - (pos2.y + 4.0)).abs() < 1.0,
            "菜单跟到新的光标位置：{r:?} vs {pos2:?}"
        );
    }

    /// 目标节点不再声明菜单（列表滚走 / 那一项变了）⇒ 自动收起，
    /// **不需要**应用写任何"我关了"的代码（照 tooltip 会话的 `alive` 检查）
    #[test]
    fn menu_closes_itself_when_the_target_stops_declaring_it() {
        let (rt, mut app, vm, id) = menu_setup();
        let at = menu_row_center(&app, id, "行 2");
        right_click(&mut app, &rt, id, at);
        assert!(ctx_menu_root(&app, id).is_some());

        vm.with_menu.set(false);
        // 两帧：第一帧 view() 重跑后 `align` 才把"该节点不再声明菜单"写进保留树，
        // 而注入发生在 align **之前**（同一帧里读的是上一帧的树）⇒ 第二帧才收。
        // 这个 1 帧延迟肉眼不可见，代价是逻辑上更简单（不维护 desc↔track 的下标映射）。
        app.frame_all();
        app.frame_all();
        assert!(ctx_menu_root(&app, id).is_none(), "目标没了 ⇒ 菜单自动收起");
    }

    // ─────────────── M6 修复回归：滚动平移 + 滚动容器内拖滑块 ───────────────

    #[test]
    fn scrolling_translates_the_content_on_the_next_layout() {
        use crate::track::Layer;

        let mut t = crate::track::Track::new();
        let sc = t.create(Kind::Box, None);
        {
            let n = t.get_mut(sc).unwrap();
            n.layout.dim = [200.0, 200.0];
            n.layout.overflow_scroll = true;
        }
        t.add_root(Layer::Content, None, sc);
        let child = t.create(Kind::Box, None);
        t.get_mut(child).unwrap().layout.dim = [100.0, 400.0];
        t.append_child(sc, child);
        crate::layout::layout(&mut t, Size::new(400.0, 400.0));
        let y0 = crate::layout::rect_of(&t, child).y;

        // 滚动 100 ⇒ 下一次布局把子原点平移 -100
        assert!(t.set_scroll_offset(sc, (0.0, 100.0)));
        crate::layout::layout(&mut t, Size::new(400.0, 400.0));
        let y1 = crate::layout::rect_of(&t, child).y;
        assert!((y1 - (y0 - 100.0)).abs() < 0.5, "内容随偏移平移：{y0} → {y1}");
    }

    #[test]
    fn slider_inside_scroll_container_drags() {
        struct Holder {
            vol: Signal<f32>,
        }
        impl ViewModel for Holder {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.scroll(|s| {
                    s.width(300.0);
                    s.height(200.0);
                    s.slider_bound(&self.vol, 0.0, 10.0);
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Holder {
            vol: Signal::new(&rt, 0.0),
        });
        let id = app.window_erased(WindowConfig::new().size(400.0, 300.0), erased(Rc::clone(&vm)));
        app.frame_all();

        let scroll = app.window_ctx(id).unwrap().content_root().unwrap();
        let slider = app.window_ctx(id).unwrap().track().children(scroll)[0];
        let r = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), slider);

        // 按下并拖到滑块右端
        {
            let w = app.window_ctx_mut(id).unwrap();
            w.pointer(
                &rt,
                InputEvent::Down {
                    pointer: PointerId(0),
                    pos: r.center(),
                    button: PointerButton::Left,
                },
            );
            w.pointer(
                &rt,
                InputEvent::Move {
                    pointer: PointerId(0),
                    pos: Point::new(r.right() - 2.0, r.center().y),
                },
            );
        }
        assert!(vm.vol.get() > 9.0, "拖到右端 ⇒ 音量接近 10：{}", vm.vol.get());
    }

    #[test]
    fn disabled_slider_ignores_the_pointer() {
        struct Off {
            volume: Signal<f32>,
        }
        impl ViewModel for Off {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.slider_bound(&self.volume, 0.0, 10.0).enabled(false);
                });
            }
        }

        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Off {
            volume: Signal::new(&rt, 1.0),
        });
        let id = app.window_erased(WindowConfig::new().size(200.0, 120.0), erased(Rc::clone(&vm)));
        app.frame_all();
        let slider = {
            let w = app.window_ctx(id).unwrap();
            w.track().children(w.content_root().unwrap())[0]
        };
        let rect = crate::layout::rect_of(app.window_ctx(id).unwrap().track(), slider);

        app.window_ctx_mut(id).unwrap().pointer(
            &rt,
            InputEvent::Down {
                pointer: PointerId(0),
                pos: Point::new(rect.right(), rect.y + 5.0),
                button: PointerButton::Left,
            },
        );
        assert_eq!(vm.volume.get(), 1.0, "禁用 ⇒ 内置行为不执行");
        assert_eq!(app.window_ctx(id).unwrap().track().captured_by(PointerId(0)), None);
    }

    #[test]
    fn focus_ring_is_repainted_without_rerunning_view() {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Counter {
            count: Signal::new(&rt, 0),
        });
        let id = app.window_erased(WindowConfig::new(), erased(vm));
        app.frame_all();

        // 找到按钮并给它 tab_stop + 键盘焦点（模拟 Tab 导航）
        let button = {
            let w = app.window_ctx(id).unwrap();
            let root = w.content_root().unwrap();
            let row = w.track().children(root)[2];
            w.track().children(row)[0]
        };
        app.window_ctx_mut(id)
            .unwrap()
            .track_mut()
            .get_mut(button)
            .unwrap()
            .tab_stop = true;
        app.window_ctx_mut(id).unwrap().tab(&rt, true).expect("有可聚焦节点");

        let st = app.frame_all()[0].1.clone();
        assert!(!st.view_ran, "焦点变化不重跑 view()");
        assert!(st.paint_pending);
        assert!(st.render.scene.ops > 0);
    }

    // ─────────────────── 主线 B：帧调度闭环 ───────────────────

    /// 回归（主线 B）：`WindowCtx::tick` 必须在**真的做了事**时返回 `true`。
    ///
    /// 这是帧调度收敛到单一入口的**闭环依据**：`about_to_wait` / `user_event`
    /// 改成只 [`Self::pump`]（消费定时器、**不渲染**）之后，
    /// 它们靠这个返回值决定"要不要 `request_redraw`"。
    ///
    /// 若它恒返回 `false`，后果是**停帧**：定时器回调改了状态，
    /// 但没有任何人请求重绘 ⇒ 画面一直停在旧帧，而回调确实在跑。
    /// 这是"把 `frame` 收敛到单一入口"最容易踩的坑（`refactor-plan` 主线 B 的
    /// 关键补充：改法正确但漏掉闭环 ⇒ 把"帧跑 3 次"换成"定时器和动画停帧"）。
    #[test]
    fn tick_reports_true_when_a_timer_fires() {
        let (rt, mut app, _, id) = setup();
        app.frame_all(); // 首帧：把初始脏消费掉

        // ① 没有定时器 ⇒ 没事做
        {
            let w = app.window_ctx_mut(id).unwrap();
            assert!(!w.tick(&rt, Instant::now()), "无定时器/动画时 tick 应返回 false");
        }

        // ② 定时器到期 ⇒ 必须返回 true（闭环的依据）
        rt.set_timeout(id, std::time::Duration::ZERO, |_| {});
        {
            let w = app.window_ctx_mut(id).unwrap();
            assert!(
                w.tick(&rt, Instant::now()),
                "定时器到期时 tick 必须返回 true，否则 pump 后没人 request_redraw ⇒ 停帧"
            );
        }

        // ③ 一次性定时器消费后 ⇒ 回到 false
        {
            let w = app.window_ctx_mut(id).unwrap();
            assert!(!w.tick(&rt, Instant::now()), "一次性定时器已消费，tick 应回到 false");
        }
    }

    /// 配套：定时器回调**改了状态**时，`tick` 返回 true 且该窗口被标脏
    /// —— 后半句才是"画面会更新"的直接保证。
    #[test]
    fn timer_callback_that_writes_state_marks_the_window_dirty() {
        let (rt, mut app, _, id) = setup();
        app.frame_all();
        let _ = app.frame_all()[0].1.clone(); // 基线：空闲

        // 定时器里写一个 signal ⇒ 应当置脏
        let sig = Signal::new(&rt, 0i32);
        struct T(Signal<i32>);
        impl ViewModel for T {
            fn view(self: &Rc<Self>, v: &mut ViewBuf) {
                v.column(|c| {
                    c.text(format!("{}", self.0.get()));
                });
            }
        }
        let id2 = app.window(WindowConfig::new(), T(sig.clone()));
        app.frame_all();
        let _ = app.frame_all(); // 吃干净

        rt.set_timeout(id2, std::time::Duration::ZERO, move |cx| {
            cx.damage_all();
        });
        let w = app.window_ctx_mut(id2).unwrap();
        assert!(w.tick(&rt, Instant::now()), "定时器到期 ⇒ tick 返回 true");

        // 该窗口应当有待呈现的脏（PRESENT），否则 pump 之后画面不会更新
        let d = rt.peek_dirty(id2);
        assert!(
            !d.is_empty(),
            "定时器回调 damage 之后窗口应被标脏（实际 dirty = {d:?}）"
        );
        let _ = id;
    }
}

/// **`handled` 语义**（D54 / S4 · E1）—— 用户处理器优先于框架内置行为。
///
/// ## 修复前的问题
///
/// `WindowCtx::dispatch` 原来是「内置行为先跑、用户 handler 后跑」：
///
///   ① `widgets::handle_route(...)` ← 内置行为全部跑完（CheckBox 已切换 checked）
///   ② `event::dispatch(...)`      ← 用户的 `cx.mark_handled()` 到这里才有机会执行
///
/// 于是 `mark_handled()` 只能当**马后炮**：既不能阻止**已经跑完**的内置行为，
/// 也不能阻止**后续**的内置行为 —— 语义上等于不存在。用户无法表达
/// "这个 CheckBox 的勾选由我自己处理"。
#[cfg(test)]
mod handled_semantics {
    use super::*;
    use crate::reactive::Signal;
    use crate::track::Kind;
    use std::rc::Rc;

    struct Vm {
        /// true 时在 tap 里立刻 `mark_handled()`
        intercept: bool,
        /// ★ 必须用 `checkbox_bound`：**无绑定的 CheckBox 内置行为不切换**
        ///   （`checkbox_handle` 里`if !bound { return; }` —— "未绑定就不改模型"）。
        ///   所以要观察内置行为，就必须给它一个绑定。
        bound: Signal<bool>,
        taps: Signal<usize>,
    }

    impl ViewModel for Vm {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            v.column(|c| {
                c.text("选项");
                c.checkbox_bound(&self.bound).on_tap_with({
                    let me = self.clone();
                    move |cx: &mut Ctx| {
                        if me.intercept {
                            // ★ 用户选择"我自己处理" ⇒ 内置行为应当让位
                            cx.mark_handled();
                            me.bound.set(!me.bound.get());
                        }
                        me.taps.set(me.taps.get() + 1);
                    }
                });
            });
        }
    }

    fn setup(intercept: bool) -> (Runtime, App, Rc<Vm>, WindowId) {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Vm {
            intercept,
            bound: Signal::new(&rt, false),
            taps: Signal::new(&rt, 0),
        });
        let id = app.window_erased(WindowConfig::new(), erased(vm.clone()));
        app.frame_all();
        (rt, app, vm, id)
    }

    /// checkbox 在树里的位置。
    ///
    /// 注意 `column` **不产生节点** —— 它的子节点直接挂在内容根下
    /// （与 `button_closure_changes_state_and_next_frame_renders_it` 里
    /// `children(root)[2]` 的取法一致），所以这里不需要多取一层。
    fn checkbox_path(app: &App, id: WindowId) -> Vec<NodeId> {
        let w = app.window_ctx(id).unwrap();
        let root = w.content_root().unwrap();
        let cb = w.track().children(root)[1];
        vec![root, cb]
    }

    fn tap(app: &mut App, rt: &Runtime, id: WindowId, path: &[NodeId]) -> DispatchOutcome {
        app.window_ctx_mut(id).unwrap().dispatch(
            rt,
            path,
            &Event::Simple {
                kind: EventKind::Tapped,
            },
        )
    }

    /// **节点上**的 `checked`（即 `toggle_checked` 直接改的那个字段）。
    ///
    /// ★ 刻意看 `kind.checked` 而**不是**绑定的 Signal：
    /// 内置行为 `toggle_checked` **立即**改节点；Signal 绑定则要等**下一帧**
    /// `view()` 重建时才反映出来。两者时序不同，正好用来区分"谁改的"。
    fn node_checked(app: &App, id: WindowId) -> bool {
        let w = app.window_ctx(id).unwrap();
        let cb = *checkbox_path(app, id).last().unwrap();
        matches!(
            w.track().get(cb).map(|n| &n.kind),
            Some(Kind::Checkbox { checked: true })
        )
    }

    /// ★ 核心回归：用户 `mark_handled()` 后，**内置的勾选切换不得发生**。
    ///
    /// 修复前内置行为先跑，`toggle_checked` 已经把节点翻转 ⇒ 这条断言会失败。
    #[test]
    fn marking_handled_suppresses_the_builtin_toggle() {
        let (rt, mut app, vm, id) = setup(true);
        let path = checkbox_path(&app, id);

        let out = tap(&mut app, &rt, id, &path);
        assert!(out.handled, "处理器标记了 handled");
        assert_eq!(out.invoked, 1, "处理器被调用了一次");
        assert_eq!(vm.taps.get(), 1, "处理器确实执行了");
        assert!(
            !node_checked(&app, id),
            "★ `mark_handled()` 之后内置的勾选切换必须被抑制（修复前这里会变成 true）"
        );
        // 顺带确认：下一帧后 Signal 绑定会把用户自己的修改反映到节点上
        app.frame_all();
        assert!(node_checked(&app, id), "用户自己改的 Signal 应在下一帧生效");
    }

    /// 反方向：**未标记 `handled` 时内置行为照常执行**。
    /// 只测上一条不够 —— 万一 `handled` 让内置行为永远不跑，这条会挂。
    #[test]
    fn without_handled_the_builtin_toggle_still_runs() {
        let (rt, mut app, vm, id) = setup(false);
        let path = checkbox_path(&app, id);

        let out = tap(&mut app, &rt, id, &path);
        assert!(!out.handled, "处理器没有标记 handled");
        assert_eq!(vm.taps.get(), 1);
        assert!(
            node_checked(&app, id),
            "未标记 handled ⇒ 内置行为应当**立即**翻转节点（不等下一帧）"
        );

        // 再点一次 ⇒ 应当切回 false（证明状态可变，不是一次性效果）
        tap(&mut app, &rt, id, &path);
        assert!(!node_checked(&app, id), "第二次点击应切回未勾选");
    }
}

/// **Escape 关闭浮层**（D15 / S4 · E2-5）—— 弹层与菜单的基线能力。
///
/// ## 修复前
///
/// `NamedKey::Escape` 只有枚举定义、**无任何消费点**。菜单 / 弹层一旦打开，
/// 就只能用鼠标点外面关掉 —— 键盘用户无法退出，任何 GUI 框架的基线能力缺失。
///
/// ## 语义（与 WinUI / WPF 一致）
///
/// **只关最上面一个**。菜单开着子菜单时，一次 Escape 期望只关子菜单，
/// 而不是把整串浮层一起关掉。
/// "可关闭"沿用既有的 `LayerOpts::dismiss_on_outside_click` 配置，不另立开关。
///
/// ## 为什么自带一个 ViewModel
///
/// 本模块**刻意不复用** `app.rs` 内 `mod tests` 里的 `picker()` / `tap_node()`：
/// 那两个是 `mod tests` 的**私有** helper，而本模块位于顶层，
/// `use super::*` 只能看到 `app.rs` 顶层可见的项。
/// 依赖跨模块的私有测试helper 是本项目反复踩坑的来源（见操作日志），
/// 这里选择"多写 20 行、换零耦合"。
#[cfg(test)]
mod escape_dismiss {
    use super::*;
    use crate::event::{KeyCode, Modifiers, NamedKey};
    use crate::reactive::Signal;
    use crate::track::{Layer, Placement};
    use std::rc::Rc;

    struct Vm {
        open: Signal<bool>,
    }

    impl ViewModel for Vm {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            v.column(|c| {
                let me = self.clone();
                c.button("打开").on_tap(move || me.open.set(true));
            });
            if self.open.get() {
                v.popup_at("打开", Placement::Below, |p| {
                    let me = self.clone();
                    // 轻关闭：点击弹层之外 ⇒ 层根收到 Dismissed
                    p.on(EventKind::Dismissed, move |_| me.open.set(false));
                    p.text("项��");
                });
            }
        }
    }

    fn setup() -> (Runtime, App, WindowId) {
        let rt = Runtime::new();
        let mut app = App::new(rt.clone());
        let vm = Rc::new(Vm {
            open: Signal::new(&rt, false),
        });
        let id = app.window_erased(WindowConfig::new(), erased(vm));
        app.frame_all();
        (rt, app, id)
    }

    /// z 序最上层的 Popup 层根
    fn popup_of(app: &App, id: WindowId) -> Option<NodeId> {
        let w = app.window_ctx(id).unwrap();
        w.track()
            .roots()
            .iter()
            .filter(|r| r.layer == Layer::Popup)
            .map(|r| r.node)
            .next_back()
    }

    /// 点开弹层（直接改状态 + 走一帧，让 `view()` 声明弹层）
    fn open_popup(app: &mut App, rt: &Runtime, id: WindowId) {
        assert!(popup_of(app, id).is_none(), "展开前没有弹层");
        let btn = app
            .window_ctx(id)
            .unwrap()
            .track()
            .children(app.window_ctx(id).unwrap().content_root().unwrap())[0];
        // 直接派发 Tapped 给按钮
        let path = {
            let w = app.window_ctx(id).unwrap();
            let root = w.content_root().unwrap();
            let mut p = crate::hit::path_to(w.track(), root);
            p.push(btn);
            p
        };
        app.window_ctx_mut(id).unwrap().dispatch(
            rt,
            &path,
            &Event::Simple {
                kind: EventKind::Tapped,
            },
        );
        app.frame_all();
        assert!(popup_of(app, id).is_some(), "弹层已打开");
    }

    fn press(app: &mut App, rt: &Runtime, id: WindowId, code: KeyCode) -> DispatchOutcome {
        app.window_ctx_mut(id)
            .unwrap()
            .key(rt, Event::key_with(EventKind::KeyDown, code, Modifiers::EMPTY))
    }

    /// ★ 核心：弹层打开时，Escape 关闭它。
    #[test]
    fn escape_closes_an_open_popup() {
        let (rt, mut app, id) = setup();
        open_popup(&mut app, &rt, id);

        let out = press(&mut app, &rt, id, KeyCode::Named(NamedKey::Escape));
        assert!(out.handled, "Escape 被当作已处理");
        app.frame_all();
        assert!(
            popup_of(&app, id).is_none(),
            "★ Escape 应当关闭弹层（修复前这里仍然是 Some）"
        );
    }

    /// ★★ **反向陷阱**：**其它按键不得关闭弹层**。
    ///
    /// 这条测试是为一个**真实踩过的坑**写的：Escape 是
    /// `KeyCode::Named(NamedKey::Escape)`，**不是** `EventKind` 的变体。
    /// 第一版实现只判`ev.kind() == KeyDown`，于是**按任意键都关弹层** ——
    /// 而"Escape 能关弹层"那条测试**照样通过**，缺陷完全隐形。
    ///
    /// 这就是"只测正向不测反向"的典型后果。
    #[test]
    fn other_keys_do_not_close_the_popup() {
        let (rt, mut app, id) = setup();
        open_popup(&mut app, &rt, id);

        for code in [
            KeyCode::Named(NamedKey::Enter),
            KeyCode::Named(NamedKey::Tab),
            KeyCode::Char('a'),
            KeyCode::Named(NamedKey::Delete),
        ] {
            press(&mut app, &rt, id, code);
            app.frame_all();
            assert!(
                popup_of(&app, id).is_some(),
                "按键 {code:?} 不得关闭弹层（Escape 才是关闭键）"
            );
        }
    }

    /// 没有弹层时 Escape 不产生任何关闭动作。
    #[test]
    fn escape_without_popup_is_inert() {
        let (rt, mut app, id) = setup();
        assert!(popup_of(&app, id).is_none());
        press(&mut app, &rt, id, KeyCode::Named(NamedKey::Escape));
        app.frame_all();
        assert!(popup_of(&app, id).is_none());
    }
}
