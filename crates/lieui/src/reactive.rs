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
    inner: Rc<RuntimeInner>,
}

#[derive(Default)]
struct RuntimeInner {
    /// 每窗口一份脏标志（`Vec` 而非 `HashMap`：窗口数量是个位数，顺序即注册顺序）
    windows: RefCell<Vec<(WindowId, Dirty)>>,
    /// 当前正在执行 `view()` 的窗口；`view()` 期间禁止 `Signal::set`
    in_view: Cell<Option<WindowId>>,
    /// 待处理请求（开窗 / 关窗 / 外部数据），由 `app` 层解释载荷类型
    requests: RequestQueue,
    /// 全局主题（设计 §3.10：`Theme` 是 App 的普通字段，无 thread_local）
    theme: RefCell<crate::theme::Theme>,
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
        f.debug_struct("RequestQueue")
            .field("pending", &self.len())
            .finish()
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
        self.inner.theme.borrow().clone()
    }

    /// 切换主题：所有窗口重跑 `view()`（描述里的颜色随新主题重新烘焙）+ 整窗重绘。
    ///
    /// 同值调用是 no-op（避免无意义的全窗重绘）。
    pub fn set_theme(&self, t: crate::theme::Theme) {
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
        match self
            .inner
            .windows
            .borrow_mut()
            .iter_mut()
            .find(|(x, _)| *x == id)
        {
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

    // ── view() 值守（防止在渲染函数里改状态）──

    /// 进入 `view()` 阶段（由 M1 的帧循环调用；当前只有单测在用）
    #[allow(dead_code)]
    pub(crate) fn begin_view(&self, id: WindowId) {
        debug_assert!(
            self.inner.in_view.get().is_none(),
            "Runtime::begin_view 嵌套调用（应为每个窗口串行执行 view()）"
        );
        self.inner.in_view.set(Some(id));
    }

    /// 退出 `view()` 阶段
    #[allow(dead_code)]
    pub(crate) fn end_view(&self) {
        self.inner.in_view.set(None);
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

    #[track_caller]
    fn assert_not_in_view(&self) {
        if cfg!(debug_assertions) && self.rt.is_in_view() {
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
mod tests {
    use super::*;

    #[test]
    fn set_marks_all_windows_and_take_clears() {
        let rt = Runtime::new();
        let a = WindowId::new(1);
        let b = WindowId::new(2);
        rt.register_window(a);
        rt.register_window(b);

        let s = Signal::new(&rt, 0);
        s.set(1);

        assert!(rt.take_dirty(a).contains(Dirty::VIEW));
        assert!(rt.take_dirty(b).contains(Dirty::VIEW));
        // 取走即清空
        assert!(rt.take_dirty(a).is_empty());
        assert!(rt.peek_dirty(b).is_empty());
    }

    #[test]
    fn update_writes_and_marks() {
        let rt = Runtime::new();
        let w = WindowId::new(1);
        rt.register_window(w);

        let s = Signal::new(&rt, 1);
        s.update(|v| *v += 41);
        assert_eq!(s.get(), 42);
        assert!(rt.take_dirty(w).contains(Dirty::VIEW));
    }

    #[test]
    fn batching_is_free_one_flag_for_n_sets() {
        let rt = Runtime::new();
        let w = WindowId::new(1);
        rt.register_window(w);

        let a = Signal::new(&rt, 0);
        let b = Signal::new(&rt, 0);
        // 一次"事件"里改 N 个 signal
        a.set(1);
        b.set(2);
        a.update(|v| *v += 1);

        // 脏标志就一位，帧循环只消费一次视图重跑
        assert_eq!(rt.take_dirty(w), Dirty::VIEW);
    }

    #[test]
    fn signal_is_clone_even_if_t_is_not() {
        struct NotClone(u32);
        let rt = Runtime::new();
        let s = Signal::new(&rt, NotClone(1));
        let s2 = s.clone();
        s2.set(NotClone(2));
        assert_eq!(s.with(|v| v.0), 2);
    }

    #[test]
    fn with_avoids_clone() {
        let rt = Runtime::new();
        let s = Signal::new(&rt, vec![1, 2, 3]);
        assert_eq!(s.with(|v| v.len()), 3);
    }

    #[test]
    fn take_resets_to_default() {
        let rt = Runtime::new();
        let w = WindowId::new(1);
        rt.register_window(w);
        let s: Signal<String> = Signal::new(&rt, "hi".into());
        assert_eq!(s.take(), "hi");
        assert_eq!(s.with(|v| v.clone()), "");
        assert!(rt.take_dirty(w).contains(Dirty::VIEW));
    }

    #[test]
    fn unregistered_window_gets_nothing_and_does_not_panic() {
        let rt = Runtime::new();
        let w = WindowId::new(7);
        rt.register_window(w);
        let s = Signal::new(&rt, 0);
        s.set(1);
        rt.unregister_window(w);

        assert!(rt.take_dirty(w).is_empty());
        rt.mark_all(Dirty::PAINT); // 没有窗口，也不应 panic
        assert!(rt.take_dirty(w).is_empty());
    }

    #[test]
    fn register_is_idempotent() {
        let rt = Runtime::new();
        let w = WindowId::new(1);
        rt.register_window(w);
        rt.register_window(w);
        assert_eq!(rt.windows(), vec![w]);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "view()")]
    fn set_inside_view_panics() {
        let rt = Runtime::new();
        let w = WindowId::new(1);
        rt.register_window(w);
        let s = Signal::new(&rt, 0);

        rt.begin_view(w); // 模拟帧循环里的 view() 阶段
        s.set(1); // 应当 panic
    }

    #[test]
    fn get_inside_view_is_fine_and_no_dirty_is_marked() {
        let rt = Runtime::new();
        let w = WindowId::new(1);
        rt.register_window(w);
        let s = Signal::new(&rt, 7);

        rt.begin_view(w);
        assert_eq!(s.get(), 7);
        rt.end_view();

        // 只读不置脏
        assert!(rt.take_dirty(w).is_empty());
    }

    #[test]
    fn act_calls_the_method() {
        use std::cell::Cell;
        struct Vm {
            hits: Cell<u32>,
        }
        impl Vm {
            fn inc(&self) {
                self.hits.set(self.hits.get() + 1);
            }
        }
        let vm = Rc::new(Vm { hits: Cell::new(0) });
        let f = act(&vm, Vm::inc);
        f();
        f();
        assert_eq!(vm.hits.get(), 2);
    }

    #[test]
    fn act1_passes_the_argument() {
        use std::cell::Cell;
        struct Vm {
            last: Cell<u32>,
        }
        impl Vm {
            fn pick(&self, id: u32) {
                self.last.set(id);
            }
        }
        let vm = Rc::new(Vm { last: Cell::new(0) });
        let f = act1(&vm, Vm::pick, 42);
        f();
        assert_eq!(vm.last.get(), 42);
    }
}
