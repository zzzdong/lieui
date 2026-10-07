//! R1 响应式：`Runtime`（脏标志表）+ `Signal<T>`（`Rc` 句柄）+ `act()`。
//!
//! 设计要点（`docs/architecture-v3.md` §3.1 / §3.3 / §3.8 / §3.14）：
//!
//! - **不追踪依赖**：`Signal::set/update` 只置位脏标志，**不立即**重跑 `view()`。
//!   帧循环统一消费一次 ⇒ 一次事件里改 N 个 signal 只重跑一次（批处理免费），且无重入问题。
//! - **0 thread_local / 0 全局单例**：`Signal` 自带 `Rc<RuntimeInner>`，所以 `get()/set()`
//!   在任何位置都能自己找到 runtime，无需环境上下文。
//! - **保守传播**：R1 不知道哪个窗口读了哪个 signal，因此 `set` 会给**所有已注册窗口**置 `VIEW`；
//!   其他窗口随后会重跑自己的 `view()` + `align`（逐字段比 ⇒ 几乎全是 no-op）。
//! - **约束**：`view()` 执行期间禁止 `set`（会自我触发循环），debug 下 panic 拦下。
//! - **代价**：`Signal` 是 `Clone`（`Rc`）不是 `Copy`；`!Send`（跨线程走 `RepaintHandle`，M4）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::window::WindowId;

/// 脏标志（手写位集，避免为此引入 `bitflags` 依赖）
#[derive(Copy, Clone, PartialEq, Eq, Default, Debug)]
pub struct Dirty(u8);

impl Dirty {
    pub const EMPTY: Self = Self(0);
    /// 视图需重跑（`view()` → `align`）
    pub const VIEW: Self = Self(1);
    /// 需要重排（脏边界）
    pub const LAYOUT: Self = Self(1 << 1);
    /// 需要重绘（脏区光栅化）
    pub const PAINT: Self = Self(1 << 2);
    /// 需要上屏（damage 提交）
    pub const PRESENT: Self = Self(1 << 3);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

impl std::ops::BitOr for Dirty {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Dirty {
    fn bitor_assign(&mut self, rhs: Self) {
        self.insert(rhs);
    }
}

impl std::ops::BitAnd for Dirty {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

/// 响应式运行时：App 的字段，所有窗口共享（`Signal` 的宿主）。
///
/// `Clone` 只是 `Rc` 引用计数 +1 —— 可以自由 clone 到各窗口的 ViewModel 构造函数里。
#[derive(Clone, Default)]
pub struct Runtime {
    pub(crate) inner: Rc<RuntimeInner>,
}

#[derive(Default)]
pub(crate) struct RuntimeInner {
    /// 每窗口一份脏标志（`Vec` 而非 `HashMap`：窗口数量是个位数，顺序即注册顺序）
    pub(crate) windows: RefCell<Vec<(WindowId, Dirty)>>,
    /// 当前正在执行 `view()` 的窗口；`view()` 期间禁止 `Signal::set`
    pub(crate) in_view: Cell<Option<WindowId>>,
    /// 待处理请求（开窗 / 关窗 / 外部数据），由 `app` 层解释载荷类型
    pub(crate) requests: RequestQueue,
    /// 全局主题（设计 §3.10：`Theme` 是 App 的普通字段，无 thread_local）
    pub(crate) theme: RefCell<crate::theme::Theme>,
    /// 主题模式（主题从哪来：预设 / 跟随系统 / 自定义）
    pub(crate) theme_mode: Cell<crate::theme::ThemeMode>,
    /// 操作系统的深色状态（平台层上报；`System` 模式下据此选预设）
    pub(crate) system_dark: Cell<bool>,
    /// **跨线程投递中心**（见 [`crate::task::Poster`]）。
    ///
    /// 平台层进入事件循环时把 `waker` 装进去（见 `Runtime::set_waker`）；
    /// 未装 = 本地队列模式。★ 它是**共享的 `Arc`**，所以先 clone 出去的
    /// 投递句柄也会立刻看见后装入的平台 waker —— 这是 A6 的结构性修复。
    pub(crate) poster: crate::task::Poster,
    /// 定时器的自增 id（原先与忙碌项共用一个计数器 `next_task_id`；
    /// 忙碌项随"遮罩是组件"一起移除后，它对定时器来说该叫自己的名字）
    pub(crate) next_timer_id: Cell<u64>,
    /// 定时器表（`set_timeout` / `set_interval`；UI 线程闭包）
    pub(crate) timers: RefCell<Vec<crate::timer::Timer>>,
    /// 请求了下一动画帧的窗口（`request_animation`；每帧回调后清空，要持续就再请求）
    pub(crate) animating: RefCell<Vec<WindowId>>,
    /// **回调执行期间**被取消的定时器 id（`refactor-plan` D9）。
    ///
    /// `take_due_timers` 会把到期的定时器**移出表**再执行回调（这样回调里能安全地
    /// 再设定时器）。副作用是：回调内调 [`crate::timer::TimerHandle::cancel`] 时，
    /// 该定时器已经不在表里 ⇒ `retain` 命中不到 ⇒ **取消无效**。
    /// 关窗同理：`cancel_timers_of` 把表里该窗口的定时器删干净，
    /// 而执行完的周期定时器又被 `reschedule_timer` 推回 ⇒ **永不触发的孤儿**，
    /// 且一直持有闭包捕获。
    ///
    /// 这里记一份"已取消"名单，`reschedule_timer` 放回**之前**查它。
    ///
    /// 只有"回调执行期间取消"这种罕见情况才会进来（普通 `cancel()` 走 `retain` 即可），
    /// 所以这个集合很小；`reschedule_timer` 消费后立即移除。
    pub(crate) cancelled_timers: RefCell<std::collections::HashSet<u64>>,
    /// 每窗口的**逻辑像素尺寸**（帧驱动每帧写入；给 `view()` 里的"适应窗口"一类计算用）
    pub(crate) window_sizes: RefCell<Vec<(WindowId, lieui_geom::Size)>>,
}

/// 待处理请求槽（类型擦除；**谁放谁取**）。
///
/// 为什么需要它：`Ctx` 只拿得到 `Runtime`，但"开窗/关窗"的载荷类型
/// （`(WindowConfig, Rc<dyn WindowView>)`）定义在 `app` 层 —— 让 `event`/`reactive`
/// 反向依赖 `app` 会破坏分层。这里只存 `Box<dyn Any>`，`Runtime` 不关心里面是什么。
#[derive(Default, Clone)]
pub struct RequestQueue {
    items: Rc<RefCell<Vec<Box<dyn std::any::Any>>>>,
}

impl RequestQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push<T: 'static>(&self, v: T) {
        self.items.borrow_mut().push(Box::new(v));
    }

    /// 取出**指定类型**的全部请求（其它类型原样留下）
    pub fn take<T: 'static>(&self) -> Vec<T> {
        let mut items = self.items.borrow_mut();
        let mut out = Vec::new();
        let mut rest = Vec::with_capacity(items.len());
        for item in items.drain(..) {
            match item.downcast::<T>() {
                Ok(v) => out.push(*v),
                Err(other) => rest.push(other),
            }
        }
        *items = rest;
        out
    }

    pub fn len(&self) -> usize {
        self.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.borrow().is_empty()
    }
}

impl std::fmt::Debug for RequestQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestQueue").field("pending", &self.len()).finish()
    }
}

impl Runtime {
    pub fn new() -> Self {
        Self::default()
    }

    // ── 窗口注册 ──

    pub fn register_window(&self, id: WindowId) {
        let mut w = self.inner.windows.borrow_mut();
        if !w.iter().any(|(x, _)| *x == id) {
            w.push((id, Dirty::empty()));
        }
    }

    /// 注销窗口（关闭时调用）。其脏标志随之丢弃。
    pub fn unregister_window(&self, id: WindowId) {
        self.inner.windows.borrow_mut().retain(|(x, _)| *x != id);
        // ★ `window_sizes` 也必须清（D59）：它只有 `push` / `find`、**从无删除路径**
        //   （`set_window_size` 在帧驱动里每帧调用），此前 `unregister_window` 只删脏标志表
        //   ⇒ **每次开关窗泄漏一条**（`WindowId` + `Size`），长时间开合窗会稳步增长。
        self.inner.window_sizes.borrow_mut().retain(|(x, _)| *x != id);
    }

    /// 已注册窗口（按注册顺序）
    pub fn windows(&self) -> Vec<WindowId> {
        self.inner.windows.borrow().iter().map(|(x, _)| *x).collect()
    }

    /// 待处理请求槽（开窗 / 关窗；`Ctx::request` 写入，`App` 取出并解释）
    pub fn requests(&self) -> &RequestQueue {
        &self.inner.requests
    }

    // ── 脏标志 ──

    /// 给**所有**窗口置脏。
    ///
    /// R1 不追踪依赖，所以 `Signal::set` 无法知道"谁读了它"，只能保守地全部置位；
    /// 多窗口下另一个窗口最坏只是白跑一次 `view()` + 逐字段比的 `align`。
    pub(crate) fn mark_all(&self, d: Dirty) {
        for (_, flags) in self.inner.windows.borrow_mut().iter_mut() {
            flags.insert(d);
        }
    }

    /// 当前主题（快照）。设计 §3.10：`Theme` 是 App 的普通字段。
    pub fn theme(&self) -> crate::theme::Theme {
        *self.inner.theme.borrow()
    }

    /// 当前主题模式（主题从哪来）
    pub fn theme_mode(&self) -> crate::theme::ThemeMode {
        self.inner.theme_mode.get()
    }

    /// 设定具体主题（自定义 token 集）：模式转为 `Custom`，**不再跟随系统**。
    ///
    /// 所有窗口重跑 `view()`（描述里的颜色随新主题重新烘焙）+ 整窗重绘。
    /// 同值调用是 no-op（避免无意义的全窗重绘）。
    pub fn set_theme(&self, t: crate::theme::Theme) {
        self.inner.theme_mode.set(crate::theme::ThemeMode::Custom);
        self.apply_theme(t);
    }

    /// 切换主题模式：
    /// - `Light` / `Dark`：应用内置预设；
    /// - `System`：跟随系统（按已上报的 OS 深色状态立即应用；未上报时按浅色）；
    /// - `Custom`：只改模式，不动生效主题。
    pub fn set_theme_mode(&self, mode: crate::theme::ThemeMode) {
        use crate::theme::{Theme, ThemeMode};
        self.inner.theme_mode.set(mode);
        let t = match mode {
            ThemeMode::Light => Theme::light(),
            ThemeMode::Dark => Theme::dark(),
            ThemeMode::System => {
                if self.inner.system_dark.get() {
                    Theme::dark()
                } else {
                    Theme::light()
                }
            }
            ThemeMode::Custom => return,
        };
        self.apply_theme(t);
    }

    /// 平台层上报操作系统的深色状态（winit `ThemeChanged` / 初始 `window.theme()`）。
    ///
    /// 只有 `ThemeMode::System` 会因此立即换主题；其它模式下仅记录，供之后切到
    /// `System` 时使用。
    pub fn set_system_dark(&self, dark: bool) {
        use crate::theme::{Theme, ThemeMode};
        if self.inner.system_dark.replace(dark) == dark {
            return;
        }
        if self.inner.theme_mode.get() == ThemeMode::System {
            self.apply_theme(if dark { Theme::dark() } else { Theme::light() });
        }
    }

    /// 生效主题变更的统一入口（同值 no-op；变了 ⇒ 所有窗口重跑 view + 整窗重绘）
    fn apply_theme(&self, t: crate::theme::Theme) {
        if *self.inner.theme.borrow() == t {
            return;
        }
        *self.inner.theme.borrow_mut() = t;
        self.mark_all(Dirty::VIEW | Dirty::PAINT | Dirty::PRESENT);
    }

    /// 给单个窗口置脏（内部 patch / `Ctx` 使用）。未注册窗口静默忽略。
    pub fn mark(&self, id: WindowId, d: Dirty) {
        if let Some((_, flags)) = self.inner.windows.borrow_mut().iter_mut().find(|(x, _)| *x == id) {
            flags.insert(d);
        }
    }

    /// 取走某窗口的脏标志（帧循环消费一次）。未注册窗口返回 `EMPTY`。
    pub fn take_dirty(&self, id: WindowId) -> Dirty {
        match self.inner.windows.borrow_mut().iter_mut().find(|(x, _)| *x == id) {
            Some((_, flags)) => std::mem::take(flags),
            None => Dirty::empty(),
        }
    }

    /// 只看不取（调试 / 测试用）
    pub fn peek_dirty(&self, id: WindowId) -> Dirty {
        self.inner
            .windows
            .borrow()
            .iter()
            .find(|(x, _)| *x == id)
            .map(|(_, f)| *f)
            .unwrap_or(Dirty::empty())
    }

    /// 记录窗口的逻辑像素尺寸（帧驱动每帧调用；用户代码只读）
    pub(crate) fn set_window_size(&self, id: WindowId, size: lieui_geom::Size) {
        let mut sizes = self.inner.window_sizes.borrow_mut();
        match sizes.iter_mut().find(|(x, _)| *x == id) {
            Some(slot) => slot.1 = size,
            None => sizes.push((id, size)),
        }
    }

    /// 窗口尺寸（**上一帧**的值；首次布局前为 `None`）。
    ///
    /// 给 `view()` 里"按视口算尺寸"的需求用（图片适应窗口、虚拟列表可见行数……）：
    /// `view()` 在布局之前跑，拿不到实测值，用上一帧的尺寸是通行做法（晚一帧不影响交互）。
    pub fn window_size(&self, id: WindowId) -> Option<lieui_geom::Size> {
        self.inner
            .window_sizes
            .borrow()
            .iter()
            .find(|(x, _)| *x == id)
            .map(|(_, s)| *s)
    }
}

// ── view() 值守（防止在渲染函数里改状态）──

/// `view()` 值守守卫（RAII）。
///
/// **为什么必须是 RAII 而不是 `begin_view()` / `end_view()` 成对调用**：
/// `view()` 是**用户代码**，它一旦 panic，手写的 `end_view()` 永远不会执行 ⇒ `in_view`
/// 永久停在 `Some(..)` ⇒ 此后该窗口所有 `Signal::set` 都会被 `assert_not_in_view` 拦下
/// （debug panic / release 静默自激），**Runtime 被永久毒化**。
/// 换成 `Drop` 之后，panic / `?` / 提前 return 都能正确恢复。
pub(crate) struct ViewGuard<'a>(&'a Cell<Option<WindowId>>);

impl Drop for ViewGuard<'_> {
    fn drop(&mut self) {
        self.0.set(None);
    }
}

impl Runtime {
    /// 进入 `view()` 阶段，返回的值守守卫（`Drop` 时自动退出）。
    ///
    /// 调用方**必须**持有返回值直到 `view()` 结束：
    /// `let _guard = rt.begin_view(id); ...`
    pub(crate) fn begin_view(&self, id: WindowId) -> ViewGuard<'_> {
        debug_assert!(
            self.inner.in_view.get().is_none(),
            "Runtime::begin_view 嵌套调用（应为每个窗口串行执行 view()）"
        );
        self.inner.in_view.set(Some(id));
        ViewGuard(&self.inner.in_view)
    }

    pub fn is_in_view(&self) -> bool {
        self.inner.in_view.get().is_some()
    }
}

/// 共享可变状态槽（≈ Vue `ref` / WinUI 可观察属性）。
///
/// - `Signal` 本身是 `Clone`（`Rc` 句柄），**不要求 `T: Clone`**；
/// - `get()` 需要 `T: Clone`（返回副本），大对象用 `with(|v| ...)` 免克隆；
/// - `set/update` 只置位脏标志，不立刻重跑 `view()`。
pub struct Signal<T: 'static> {
    inner: Rc<SignalInner<T>>,
    rt: Runtime,
}

struct SignalInner<T> {
    value: RefCell<T>,
}

impl<T: 'static> Signal<T> {
    /// 用运行时句柄创建（`App::new(rt)` 之前建好、再 clone 进各窗口的 ViewModel）。
    pub fn new(rt: &Runtime, value: T) -> Self {
        Self {
            inner: Rc::new(SignalInner {
                value: RefCell::new(value),
            }),
            rt: rt.clone(),
        }
    }

    /// 免克隆读取。
    ///
    /// ⚠️ 回调内**不要**调 `set/update`（`RefCell` 借用冲突会 panic）；
    /// 需要在读的同时改，请先 `get()` 出副本（见 `docs/architecture-v3.md` §4 借用纪律 1）。
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.inner.value.borrow())
    }

    /// 写入并置脏。
    ///
    /// **禁止在 `view()` 内调用**（debug 下 panic）：那会造成自我触发循环。
    /// 状态变更只应在事件闭包 / `on_tick` / `on_external` 里做。
    pub fn set(&self, value: T) {
        self.assert_not_in_view();
        *self.inner.value.borrow_mut() = value;
        self.rt.mark_all(Dirty::VIEW);
    }

    /// 就地修改并置脏（语义同 `set`）
    pub fn update<F: FnOnce(&mut T)>(&self, f: F) {
        self.assert_not_in_view();
        f(&mut self.inner.value.borrow_mut());
        self.rt.mark_all(Dirty::VIEW);
    }

    /// 禁止在 `view()` 内改状态。
    ///
    /// **always-on，不带 `debug_assertions` 门禁**（D8）。此前 release 下这条保护整个消失，
    /// 用户在 `view()` 里误调 `Signal::set` 会变成"每帧 view → set → 再 view"的
    /// **永久满帧自激**：100% CPU、不报错、没有日志。代价只是读一个 `Cell<Option<WindowId>>`，
    /// 换来 release 下也能 fail-fast —— 设计 §四把它列为"违反会panic 或死循环"的硬纪律，
    /// 就该无条件执行。
    #[track_caller]
    fn assert_not_in_view(&self) {
        if self.rt.is_in_view() {
            panic!(
                "Signal::set/update 不能在 view() 内调用（会自我触发循环）：\
                 请把状态变更放到事件闭包或 on_tick/on_external 里"
            );
        }
    }
}

impl<T: Clone + 'static> Signal<T> {
    /// 读取副本
    pub fn get(&self) -> T {
        self.inner.value.borrow().clone()
    }
}

impl<T: Default + 'static> Signal<T> {
    /// 取出并留下 `T::default()`（同样置脏）
    pub fn take(&self) -> T {
        self.assert_not_in_view();
        let out = std::mem::take(&mut *self.inner.value.borrow_mut());
        self.rt.mark_all(Dirty::VIEW);
        out
    }
}

/// 手动实现（不能 derive：否则会要求 `T: Clone`）
impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
            rt: self.rt.clone(),
        }
    }
}

impl<T: std::fmt::Debug + 'static> std::fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Signal").field(&*self.inner.value.borrow()).finish()
    }
}

/// 把「`Rc<VM>` + 它的某个 `&self` 方法」变成一个无捕获噪音的 `Fn()` 闭包。
///
/// ```ignore
/// r.button("+1").on_tap(act(self, Self::inc));
/// ```
pub fn act<T: 'static>(vm: &Rc<T>, f: fn(&T)) -> impl Fn() + 'static {
    let me = Rc::clone(vm);
    move || f(&me)
}

/// 同上，但保留 `Rc<VM>` 之外的额外参数（如列表项 id）。
pub fn act1<T: 'static, A: Clone + 'static>(vm: &Rc<T>, f: fn(&T, A), arg: A) -> impl Fn() + 'static {
    let me = Rc::clone(vm);
    move || f(&me, arg.clone())
}

#[cfg(test)]
#[path = "reactive_tests.rs"]
mod tests;
