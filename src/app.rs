//! **应用层**：多窗口编排 + 视图模型契约 + 帧驱动骨架（M1：**不含 winit、不含光栅化**）。
//!
//! ## 本模块的职责边界
//!
//! 只留**跨窗口**的东西：
//! - [`ViewModel`]：用户视图模型契约（`view` / `on_tick` / `on_external` / `on_close_request`）
//! - `VmAdapter` / [`erased`]：把 `Rc<V>` 擦除成 `Rc<dyn WindowView>`
//! - [`App`]：窗口表、id 分配、`frame_all`、跨窗口请求（开窗 / 关窗）
//!
//! **单窗口**的一切（配置、事件分发、命中、布局、绘制、会话状态、`FrameStats`）
//! 都已迁到 [`crate::window`]。原`lieui::app::WindowConfig` 等路径
//! 通过 `pub use` 重导出**继续可用** —— 模块拆分不应成为对外 API 的破坏性变更。
//!
//! ## 帧驱动
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

use std::rc::Rc;
use std::time::Instant;

/// 窗口相关类型**原定义在 `app.rs`，现已迁到 `window.rs`**。
///
/// 这里`pub use` 重导出，是为了让**既有路径继续可用**（`lieui::app::WindowConfig`、
/// `lieui::app::CloseAction`、`lieui::app::open_window` …）——
/// 模块拆分不应该成为对外 API 的破坏性变更。
pub use crate::window::{CloseAction, CloseWindow, ExternalData, WindowConfig};
pub use crate::window::{FrameStats, PointerOutcome, WindowCtx};
pub use crate::window::{close_self, open_window};

use crate::event::Ctx;
use crate::reactive::Runtime;
use crate::view::ViewBuf;
use crate::window::WindowId;

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
        self.windows.iter().find(|w| w.id() == id)
    }

    pub fn window_ctx_mut(&mut self, id: WindowId) -> Option<&mut WindowCtx> {
        self.windows.iter_mut().find(|w| w.id() == id)
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
            if let Some(ctx) = self.windows.iter_mut().find(|c| c.id() == w) {
                ctx.external(&rt, data);
            }
        }
        let now = Instant::now();
        self.windows
            .iter_mut()
            .map(|w| {
                w.tick(&rt, now);
                (w.id(), w.frame(&rt))
            })
            .collect()
    }

    /// 帧之间的**非渲染**工作：本地投递队列 → 定时器 / 动画 / `on_tick` →
    /// 判断**是否需要出一帧**。返回 `true` = 平台层应当 `request_redraw`。
    ///
    /// ## 为什么这段在 `App` 而不是平台层
    ///
    /// 平台层（`platform::Runner`）需要真实 winit 窗口，**单测里构造不出来** ——
    /// 而"该不该重绘"恰恰是排障时最需要钉住的一环（见 [`WindowCtx::needs_frame`]
    /// 文档里那个"遮罩永远收不掉"的缺陷）。放在 `App` 上，它就和 `frame_all`
    /// 一样可以被无头测试直接驱动、直接断言。
    ///
    /// 平台层在 `RedrawRequested` **之外**的唤醒点（`about_to_wait` / `user_event`）
    /// 调它，拿到 `true` 就请求重绘 —— 少了这一环，定时器回调改了状态却没人画，
    /// 就是**停帧**。
    pub fn pump(&mut self, now: Instant) -> bool {
        let rt = self.rt.clone();
        let mut did_work = false;

        // 本地投递队列（A6：`run()` 之前 spawn 的任务会往这里投）
        for (w, data) in rt.take_pending_external() {
            if let Some(ctx) = self.windows.iter_mut().find(|c| c.id() == w) {
                ctx.external(&rt, data);
                did_work = true;
            }
        }

        for w in self.windows.iter_mut() {
            w.animate(now); // 光标闪烁翻相位 / 遮罩卡片标脏（无聚焦输入框时 no-op）
            did_work |= w.tick(&rt, now);
            // ★ 遮罩在 ⇒ 必须真的出一帧（判据细节见 `WindowCtx::needs_frame`）。
            //   少了这一句，"打开文件后 loading 遮罩不消失"就会复现：
            //   定时唤醒到了、`tick` 没事可做 ⇒ 判定"没事做" ⇒ 不重绘 ⇒
            //   `frame()` 不跑 ⇒ `Runtime::reap_busy` 不跑 ⇒ 遮罩永远收不掉。
            did_work |= w.needs_frame(&rt);
        }

        did_work
    }

    /// 关闭窗口（注销其脏标志，丢弃其保留树）。
    ///
    /// 顺带**取消该窗口的后台任务**并清掉它的忙碌项（否则线程会继续跑到结束、
    /// 结果却无人接收；见 [`crate::task`]）。
    pub fn close_window(&mut self, id: WindowId) -> bool {
        let before = self.windows.len();
        self.windows.retain(|w| w.id() != id);
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
#[path = "app_tests.rs"]
mod tests;

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
// ── 测试用的类型再导入 ──
// `app_tests.rs` 通过 `use super::*` 继承这里的导入。拆分模块后这些类型
// 已迁到 `window.rs`，但**测试断言的是窗口级行为**（帧统计、命中、布局），
// 让它们继续从 `app` 可见可以避免给3600 行测试逐个加前缀。
// ── 测试用的类型再导入（`#[cfg(test)]`）──
//
// `app_tests.rs` 通过 `use super::*` 继承这里的导入。模块拆分后这些类型迁到了
// `window.rs` / 其他模块，但**这些测试断言的正是窗口级行为**（帧统计、命中、布局、
// 事件分发）。让它们继续从 `app` 可见，避免给 3600 行测试逐个加前缀 ——
// 测试的可读性优先于"导入必须最小化"。
//
// ★ 另一种做法（把测试改成 `use crate::window::*; use crate::app::*;`）更"干净"，
//   但会让每条断言都带上模块前缀，且将来 `app.rs` 再次拆分时又得重来一遍。
#[cfg(test)]
use crate::event::{DispatchOutcome, Event, EventKind};
#[cfg(test)]
use crate::input::InputEvent;
#[cfg(test)]
use crate::reactive::Dirty;
#[cfg(test)]
use crate::track::{FocusState, NodeId, Track};
#[cfg(test)]
use crate::window::{BLINK_PERIOD, TOOLTIP_DELAY};
#[cfg(test)]
use lieui_geom::{Point, Rect, Size};
