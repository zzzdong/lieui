//! 窗口相关：**身份、配置、单窗口上下文**。
//!
//! M1 只定义框架自己的 `WindowId`（不引入 winit）；M4 接 winit 时在其
//! `WindowId` 与我们的 id 之间做一次映射，其余代码不感知。
//!
//! ## 为什么要从 `app.rs` 拆出来
//!
//! `App` 是**多窗口编排者**（开窗 / 关窗 / 帧循环 / 跨窗口请求），
//! `WindowCtx` 是**单个窗口的驱动器**（事件分发、命中、布局、绘制、会话状态）。
//! 两者变更节奏完全不同：窗口级行为（拖拽、置顶、DPI）只动 `window.rs`，
//! 不该每次都和 `App` 的多窗口逻辑一起过眼。
//!
//! 依赖方向是**单向**的：`App` 持有 `Vec<WindowCtx>`，而 `WindowCtx` 不引用 `App`。
//! （`window.rs` 里用到 `ViewModel` / `erased` 属于**类型**依赖，
//! 与 `App` 的**值**依赖不构成循环。）

use std::any::Any;
use std::rc::Rc;
use std::time::Instant;

use lieui_geom::{Point, Rect, Size};

use crate::align::{AlignStats, align};
use crate::app::{ViewModel, WindowView, erased};
use crate::cmd::{CmdBuf, apply_cmds};
use crate::event::{Ctx, DispatchOutcome, Event, EventKind, EventView, KeyCode, NamedKey, PointerButton};
use crate::focus;
use crate::hit;
use crate::input::{self, InputEvent};
use crate::layout::{self, LayoutStats};
use crate::reactive::{Dirty, Runtime};
use crate::render::{RenderStats, Renderer};
use crate::track::{FocusState, Layer, NodeId, Track};
use crate::view::ViewBuf;

/// 框架内的窗口身份（`u32`，由 App 分配，从 1 开始）
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct WindowId(pub u32);

impl WindowId {
    pub const fn new(v: u32) -> Self {
        Self(v)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for WindowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WindowId({})", self.0)
    }
}

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
            // ★★ 滚完必须把脏标写回 **Runtime**，否则屏幕上什么都不会动。
            //
            // `default_wheel_scroll` 只调 `Track::set_scroll_offset`，那只会置
            // **Node 的 Flags**（`mark_layout_dirty`）；而 `frame()` 开头是
            // `rt.take_dirty(id)` —— 它读的是 **Runtime 的脏标**，两者不是一回事。
            //
            // 于是形成这样一个「静默失效」链：
            //   滚轮 ⇒ Track 里的偏移变了 ⇒ 但 Runtime 仍是干净的
            //        ⇒ 平台层 `window_event` 末尾的 `rt.peek_dirty(id)` 为空
            //        ⇒ **不会 `request_redraw()`** ⇒ 永远不跑 `frame()`
            //        ⇒ 偏移改了、像素没动 ⇒ 用户看到"滚轮完全没反应"。
            //
            // 对照：走 `Cmd` 的交互（含**拖动滚动条**）没问题 ——
            // `dispatch` 的第 ③ 步里有 `rt.mark(self.id, apply_cmds(..))`。
            // 这也解释了为什么"拖滚动条有效果"：它的重绘是被 Cmd 触发的，
            // 而不是被滚动本身触发 ⇒ 重绘时机跟着 hover 走 ⇒ 手感"不跟随"。
            if outcome.scrolled {
                rt.mark(self.id, Dirty::LAYOUT | Dirty::PAINT | Dirty::PRESENT);
            }
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
    pub(crate) fn context_menu_root(&self) -> Option<NodeId> {
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
