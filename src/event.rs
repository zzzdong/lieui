//! 事件模型：类型 + 路由策略 + 两段式分发。
//!
//! 对齐 WinUI `UIElement` 的裁剪版（设计文档 §3.5 / §3.13）：
//! - 一种事件种类表 + 三态路由（`Tunnel` / `Bubble` / `Direct`）；
//! - 处理器统一是闭包 `Rc<dyn Fn(&mut Ctx)>`，**只拿 `&mut Ctx`**；
//! - `handled` + `handledEventsToo` 取代旧实现的 `.builtin()` 排序 hack；
//! - **`Ctx` 不提供对保留树的访问**：处理器只能改状态 / 攒 `Cmd` / 请求重绘，
//!   结构变化一律靠下一帧 `view()` 重跑（这就是"闭包不改结构"的落地约束）。
//!
//! 分发是**两段式**的（借用安全的唯一路径）：
//! ① 只读遍历 → 收集 `HandlerSlot`（`Rc` clone）；② 借用释放后逐个调用。

use std::rc::Rc;

use lieui_geom::Point;

use crate::cmd::{Cmd, CmdBuf};
use crate::reactive::{Dirty, Runtime};
use crate::track::{FocusState, NodeId, Track};
use crate::window::WindowId;

/// 指针身份（鼠标写死 0，触控留口）
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct PointerId(pub u32);

/// 指针按键
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum PointerButton {
    #[default]
    Left,
    Right,
    Middle,
    Other(u16),
}

/// 路由策略
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Routing {
    /// 预览（隧道）阶段：外 → 内，先于 Bubble
    Tunnel,
    /// 冒泡阶段：内 → 外
    Bubble,
    /// 只发给目标节点，不传播
    Direct,
}

/// 命名键（M1 只列常用；`Other` 保留原始码）
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum NamedKey {
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Insert,
    Space,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Shift,
    Control,
    Alt,
    Meta,
    Other(u32),
}

/// 键码
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum KeyCode {
    Named(NamedKey),
    Char(char),
}

/// 修饰键状态（手写位集；键盘编辑需要 Shift 扩选、Ctrl 全选/词跳）
#[derive(Copy, Clone, PartialEq, Eq, Default, Debug)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const EMPTY: Self = Self(0);
    pub const SHIFT: Self = Self(1);
    pub const CTRL: Self = Self(1 << 1);
    pub const ALT: Self = Self(1 << 2);
    pub const META: Self = Self(1 << 3);

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn shift(self) -> bool {
        self.contains(Self::SHIFT)
    }

    pub const fn ctrl(self) -> bool {
        self.contains(Self::CTRL)
    }

    pub const fn alt(self) -> bool {
        self.contains(Self::ALT)
    }

    pub const fn meta(self) -> bool {
        self.contains(Self::META)
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.insert(rhs);
    }
}

/// 事件种类（裁剪自 WinUI `UIElement` 的 32 个 `*Event` + `FrameworkElement` 的若干）
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum EventKind {
    // ── 指针 ──
    PointerEntered,
    PointerExited,
    PointerMoved,
    PointerPressed,
    PointerReleased,
    PointerWheelChanged,
    PointerCanceled,
    PointerCaptureLost,
    // ── 点击 / 手势 / 上下文 ──
    Tapped,
    DoubleTapped,
    RightTapped,
    Holding,
    ContextRequested,
    ContextCanceled,
    // ── 键盘 / 字符 ──
    KeyDown,
    KeyUp,
    PreviewKeyDown,
    PreviewKeyUp,
    CharacterReceived,
    // ── 焦点 ──
    GettingFocus,
    GotFocus,
    LosingFocus,
    LostFocus,
    NoFocusCandidateFound,
    // ── 文本 / IME ──
    TextCompositionStarted,
    TextCompositionChanged,
    TextCompositionEnded,
    // ── 生命周期 / 布局 ──
    Loaded,
    Unloaded,
    SizeChanged,
    EffectiveViewportChanged,
    ScrollChanged,
    BringIntoViewRequested,
    // ── 拖放 ──
    DragStarting,
    DragEnter,
    DragOver,
    DragLeave,
    Drop,
    DropCompleted,
    // ── 框架内 ──
    /// 浮层被"点击外部"关闭：发给该层根（由 `App` 在命中测试之外派发）
    Dismissed,
    /// 逐帧 tick（动画）——框架自有
    Tick,
}

impl EventKind {
    /// 路由策略（设计 §3.5：`Preview*` 走隧道，捕获丢失/生命周期类只发给目标）
    pub const fn routing(self) -> Routing {
        use EventKind::*;
        match self {
            PreviewKeyDown | PreviewKeyUp => Routing::Tunnel,
            PointerCaptureLost
            | Loaded
            | Unloaded
            | SizeChanged
            | EffectiveViewportChanged
            | ScrollChanged
            | BringIntoViewRequested
            | Dismissed
            | Tick
            | NoFocusCandidateFound => Routing::Direct,
            _ => Routing::Bubble,
        }
    }

    /// 是否为指针类事件（分发时要按 `PointerId` 找捕获）
    pub const fn is_pointer(self) -> bool {
        use EventKind::*;
        matches!(
            self,
            PointerEntered
                | PointerExited
                | PointerMoved
                | PointerPressed
                | PointerReleased
                | PointerWheelChanged
                | PointerCanceled
                | PointerCaptureLost
                | Tapped
                | DoubleTapped
                | RightTapped
                | Holding
        )
    }

    /// 调试名
    pub const fn name(self) -> &'static str {
        use EventKind::*;
        match self {
            PointerEntered => "PointerEntered",
            PointerExited => "PointerExited",
            PointerMoved => "PointerMoved",
            PointerPressed => "PointerPressed",
            PointerReleased => "PointerReleased",
            PointerWheelChanged => "PointerWheelChanged",
            PointerCanceled => "PointerCanceled",
            PointerCaptureLost => "PointerCaptureLost",
            Tapped => "Tapped",
            DoubleTapped => "DoubleTapped",
            RightTapped => "RightTapped",
            Holding => "Holding",
            ContextRequested => "ContextRequested",
            ContextCanceled => "ContextCanceled",
            KeyDown => "KeyDown",
            KeyUp => "KeyUp",
            PreviewKeyDown => "PreviewKeyDown",
            PreviewKeyUp => "PreviewKeyUp",
            CharacterReceived => "CharacterReceived",
            GettingFocus => "GettingFocus",
            GotFocus => "GotFocus",
            LosingFocus => "LosingFocus",
            LostFocus => "LostFocus",
            NoFocusCandidateFound => "NoFocusCandidateFound",
            TextCompositionStarted => "TextCompositionStarted",
            TextCompositionChanged => "TextCompositionChanged",
            TextCompositionEnded => "TextCompositionEnded",
            Loaded => "Loaded",
            Unloaded => "Unloaded",
            SizeChanged => "SizeChanged",
            EffectiveViewportChanged => "EffectiveViewportChanged",
            ScrollChanged => "ScrollChanged",
            BringIntoViewRequested => "BringIntoViewRequested",
            DragStarting => "DragStarting",
            DragEnter => "DragEnter",
            DragOver => "DragOver",
            DragLeave => "DragLeave",
            Drop => "Drop",
            DropCompleted => "DropCompleted",
            Dismissed => "Dismissed",
            Tick => "Tick",
        }
    }
}

/// 事件（M1 的裁剪版；IME / 拖放 / 手势的 payload 随后续里程碑补）
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Pointer {
        kind: EventKind,
        pointer: PointerId,
        pos: Point,
        button: PointerButton,
    },
    Wheel {
        pos: Point,
        delta: (f32, f32),
    },
    Key {
        kind: EventKind,
        code: KeyCode,
        /// 字符输入（`CharacterReceived` 的 payload；IME 提交也走这里）
        text: Option<String>,
        repeat: bool,
        modifiers: Modifiers,
    },
    /// 焦点 / 生命周期 / 框架内事件（无 payload）
    Simple {
        kind: EventKind,
    },
    /// IME 预编辑（输入法未提交的组合文本；`cursor` 是字节区间）
    ImePreedit {
        text: String,
        cursor: Option<(u32, u32)>,
    },
    /// 滚动变化（发给滚动容器自身；`offset` 是新偏移）
    Scroll {
        offset: (f32, f32),
    },
    Tick {
        now_ms: u64,
    },
}

/// 事件的**只读摘要**（`Copy`）：处理器通过 `Ctx` 读它，避免每处理器 clone 整个 payload
#[derive(Clone, Copy, Debug)]
pub struct EventView {
    pub kind: EventKind,
    pub pos: Point,
    pub pointer: PointerId,
    pub button: PointerButton,
    pub wheel: (f32, f32),
    /// 滚动容器的新偏移（`ScrollChanged` 专用）
    pub scroll: (f32, f32),
    pub key: Option<KeyCode>,
    pub repeat: bool,
    pub modifiers: Modifiers,
    pub now_ms: u64,
}

impl Event {
    /// 指针类事件
    pub fn pointer(kind: EventKind, pointer: PointerId, pos: Point, button: PointerButton) -> Self {
        Event::Pointer {
            kind,
            pointer,
            pos,
            button,
        }
    }

    /// 滚轮（`delta` 正 = 向上：滚轮上推 / 触控板上滑 ⇒ offset 减小，见
    /// `input::default_wheel_scroll`）
    pub fn wheel(pos: Point, delta: (f32, f32)) -> Self {
        Event::Wheel { pos, delta }
    }

    /// 无 payload 事件（焦点 / 生命周期 / 框架内）
    pub fn simple(kind: EventKind) -> Self {
        Event::Simple { kind }
    }

    /// 键盘事件
    pub fn key(kind: EventKind, code: KeyCode) -> Self {
        Self::key_with(kind, code, Modifiers::EMPTY)
    }

    /// 键盘事件（带修饰键）
    pub fn key_with(kind: EventKind, code: KeyCode, modifiers: Modifiers) -> Self {
        Event::Key {
            kind,
            code,
            text: None,
            repeat: false,
            modifiers,
        }
    }

    /// 字符输入（`CharacterReceived`）：IME 提交也走这条，一个字符一条
    pub fn char_received(ch: char) -> Self {
        Event::Key {
            kind: EventKind::CharacterReceived,
            code: KeyCode::Char(ch),
            text: Some(ch.to_string()),
            repeat: false,
            modifiers: Modifiers::EMPTY,
        }
    }

    pub fn kind(&self) -> EventKind {
        match self {
            Event::Pointer { kind, .. } | Event::Key { kind, .. } | Event::Simple { kind } => *kind,
            Event::Wheel { .. } => EventKind::PointerWheelChanged,
            Event::ImePreedit { .. } => EventKind::TextCompositionChanged,
            Event::Scroll { .. } => EventKind::ScrollChanged,
            Event::Tick { .. } => EventKind::Tick,
        }
    }

    pub fn summary(&self) -> EventView {
        match self {
            Event::Pointer {
                kind,
                pointer,
                pos,
                button,
            } => EventView {
                kind: *kind,
                pos: *pos,
                pointer: *pointer,
                button: *button,
                ..EventView::empty(*kind)
            },
            Event::Wheel { pos, delta } => EventView {
                kind: EventKind::PointerWheelChanged,
                pos: *pos,
                wheel: *delta,
                ..EventView::empty(EventKind::PointerWheelChanged)
            },
            Event::Key {
                kind,
                code,
                repeat,
                modifiers,
                ..
            } => EventView {
                kind: *kind,
                key: Some(*code),
                repeat: *repeat,
                modifiers: *modifiers,
                ..EventView::empty(*kind)
            },
            Event::Simple { kind } => EventView::empty(*kind),
            Event::ImePreedit { .. } => EventView::empty(EventKind::TextCompositionChanged),
            Event::Scroll { offset } => EventView {
                kind: EventKind::ScrollChanged,
                scroll: *offset,
                ..EventView::empty(EventKind::ScrollChanged)
            },
            Event::Tick { now_ms } => EventView {
                kind: EventKind::Tick,
                now_ms: *now_ms,
                ..EventView::empty(EventKind::Tick)
            },
        }
    }

    pub fn pointer_pos(&self) -> Option<Point> {
        match self {
            Event::Pointer { pos, .. } | Event::Wheel { pos, .. } => Some(*pos),
            _ => None,
        }
    }
}

impl EventView {
    fn empty(kind: EventKind) -> Self {
        Self {
            kind,
            pos: Point::zero(),
            pointer: PointerId(0),
            button: PointerButton::Left,
            wheel: (0.0, 0.0),
            scroll: (0.0, 0.0),
            key: None,
            repeat: false,
            modifiers: Modifiers::EMPTY,
            now_ms: 0,
        }
    }

    /// 无 payload 的事件（焦点 / 生命周期 / 框架内）
    pub fn simple(kind: EventKind) -> Self {
        Self::empty(kind)
    }

    /// 逐帧钩子的"伪事件"（`on_tick` 的 `Ctx` 也拿得到事件摘要，保持统一）
    pub fn tick() -> Self {
        Self::empty(EventKind::Tick)
    }

    /// 外部数据的"伪事件"
    pub fn external() -> Self {
        Self::empty(EventKind::Unloaded)
    }
}

/// 事件处理器：只拿 `&mut Ctx`（改状态 / 攒 `Cmd` / 请求重绘），**不拿保留树**
pub type Handler = Rc<dyn Fn(&mut Ctx)>;

/// 节点上的一个处理器槽位
#[derive(Clone)]
pub struct HandlerSlot {
    pub kind: EventKind,
    pub handler: Handler,
    /// `true` = 即便事件已被处理也要调用（≈ WinUI `handledEventsToo`）；
    /// 框架内置行为用它，取代旧实现的 `.builtin()` 排序 hack。
    pub handled_events_too: bool,
}

impl std::fmt::Debug for HandlerSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandlerSlot")
            .field("kind", &self.kind)
            .field("handled_events_too", &self.handled_events_too)
            .finish()
    }
}

/// 处理器上下文。
///
/// 约束（§3.5）：用户闭包**不能**直接改视图树，只能
/// ① 改 `Signal`（自动置 `VIEW`）；② 用 `Cmd` 请求框架动作；③ 显式请求失效/重绘。
pub struct Ctx {
    rt: Runtime,
    window: WindowId,
    event: EventView,
    handled: bool,
    cmds: CmdBuf,
}

impl Ctx {
    pub fn new(rt: &Runtime, window: WindowId, event: EventView) -> Self {
        Self {
            rt: rt.clone(),
            window,
            event,
            handled: false,
            cmds: CmdBuf::new(),
        }
    }

    pub fn window(&self) -> WindowId {
        self.window
    }

    /// 运行时句柄（事件里要建新 signal / 新窗口时用）
    pub fn runtime(&self) -> Runtime {
        self.rt.clone()
    }

    /// 当前事件的只读摘要
    pub fn event(&self) -> EventView {
        self.event
    }

    /// 滚动容器的新偏移（`ScrollChanged` 事件专用；其它事件返回 `(0,0)`）
    pub fn scroll_offset(&self) -> (f32, f32) {
        self.event.scroll
    }

    // ── handled ──

    pub fn handled(&self) -> bool {
        self.handled
    }

    /// 标记"已处理"：后续 `handled_events_too == false` 的处理器将被跳过
    pub fn mark_handled(&mut self) {
        self.handled = true;
    }

    pub fn set_handled(&mut self, v: bool) {
        self.handled = v;
    }

    // ── 失效 / 重绘 ──

    /// 非 `Signal` 路径的状态变更 → 下帧重跑 `view()` + `align`
    pub fn invalidate(&self) {
        self.rt.mark(self.window, Dirty::VIEW);
    }

    /// 请求重绘（不重跑 `view()`、不重排）：**已有脏区**里那一块会被光栅化。
    ///
    /// 适合"状态在节点属性里、脏区已由树登记"的场景（光标闪烁、外部像素流）。
    /// 自绘节点改了内部状态（不经过树）时要用 [`Self::damage_all`] 或 [`Self::damage`]，
    /// 否则没有脏区 ⇒ 什么都不画。
    pub fn request_repaint(&self) {
        self.rt.mark(self.window, Dirty::PAINT | Dirty::PRESENT);
    }

    /// **整窗标脏**（下一帧整窗重绘）。
    ///
    /// 动画帧里最常用：自绘节点（`CustomNode`）的相位不经过保留树，
    /// 没有脏矩形可登记 ⇒ 用整窗重绘保证画出来（精修脏区可用 [`Self::damage`]）。
    pub fn damage_all(&mut self) {
        self.cmds.damage_all();
    }

    // ── 定时器 / 动画帧（见 `crate::timer`）──

    /// 延迟 `dur` 后回调一次（在 **UI 线程**执行，可拿 `&mut Ctx`）
    pub fn set_timeout<F: FnMut(&mut Ctx) + 'static>(
        &self,
        dur: std::time::Duration,
        f: F,
    ) -> crate::timer::TimerHandle {
        self.rt.set_timeout(self.window, dur, f)
    }

    /// 每 `dur` 回调一次（直到 `TimerHandle::cancel` 或窗口关闭）
    pub fn set_interval<F: FnMut(&mut Ctx) + 'static>(
        &self,
        dur: std::time::Duration,
        f: F,
    ) -> crate::timer::TimerHandle {
        self.rt.set_interval(self.window, dur, f)
    }

    /// 请求下一帧回调 `ViewModel::on_animation`（想继续动画就在回调里再请求一次）
    pub fn request_animation(&self) {
        self.rt.request_animation(self.window);
    }

    // ── 跨线程投递 / loading 遮罩（见 `crate::task`）──

    /// 拿一份**可 `Send` 的投递句柄** —— 交给工作线程用。
    ///
    /// ★ 框架**不提供"任务"概念**：不替你开线程、不定取消协议、不管线程池。
    /// 你只需要把结果**投递**回来：
    ///
    /// ```ignore
    /// let poster = cx.poster();              // `Ctx` 是 `!Send`，但句柄能进线程
    /// std::thread::spawn(move || {
    ///     poster.post(win, heavy_work());    // 投递 + 唤醒
    /// });
    /// ```
    pub fn poster(&self) -> crate::task::Poster {
        self.rt.poster()
    }

    /// 开一个「忙碌」段（loading 遮罩）—— **与任务无关**。
    ///
    /// 遮罩何时收起由你决定：[`crate::task::BusyToken::finish`] / `drop` /
    /// [`crate::task::BusyToken::dismiss_after`]（定时兜底）。
    pub fn begin_busy(&self, label: impl Into<String>) -> crate::task::BusyToken {
        self.rt.begin_busy(self.window, label)
    }

    /// 平台唤醒器（`None` = 无头 / 事件循环未起）。
    ///
    /// 一般用 [`Self::poster`]（两种模式都能投递）；只有需要**显式区分**
    /// "有没有事件循环"时才直接用这个。
    pub fn waker(&self) -> Option<std::sync::Arc<dyn crate::task::Waker>> {
        self.rt.waker()
    }

    // ── 命令（延迟写入通道）──

    /// 提交一个待处理请求（**框架控制消息**：开窗/关窗等；载荷类型由 `app` 层解释）。
    ///
    /// 用法见 [`crate::app::open_window`] / [`crate::app::close_self`]。
    /// 业务事件请用 [`Self::emit`]（走 `on_external`，跨线程一致）。
    pub fn request<T: 'static>(&self, v: T) {
        self.rt.requests().push(v);
    }

    /// 投递一条**自定义事件**给本窗口（下一个 tick 由 `ViewModel::on_external` 收到）。
    ///
    /// 这是 `Signal` 之外的"消息"通道：适合枚举型业务事件（`MyEvent::SaveFinished`）、
    /// 或者"不想为它建 Signal"的一次性通知。同一个 `T` 也可以跨线程投递
    /// （[`crate::task::Poster::post`] 与 [`Emitter`] 是同一条管道）。
    pub fn emit<T: Send + 'static>(&self, msg: T) -> bool {
        self.rt.emit(self.window, msg)
    }

    pub fn cmd(&mut self, c: Cmd) {
        self.cmds.push(c);
    }

    pub fn cmds(&self) -> &[Cmd] {
        self.cmds.as_slice()
    }

    pub fn take_cmds(&mut self) -> Vec<Cmd> {
        self.cmds.take()
    }

    pub fn damage(&mut self, id: NodeId) {
        self.cmds.damage(id);
    }

    pub fn focus(&mut self, id: NodeId) {
        self.cmds.focus(id, FocusState::Programmatic);
    }

    pub fn capture_pointer(&mut self, pointer: PointerId, id: NodeId) {
        self.cmds.capture(pointer, id);
    }

    pub fn release_pointer(&mut self, pointer: PointerId) {
        self.cmds.release(pointer);
    }

    pub fn scroll_to(&mut self, id: NodeId, offset: (f32, f32)) {
        self.cmds.scroll_to(id, offset);
    }
}

// ───────────────────────── 自定义事件（`emit` / `Emitter`）─────────────────────────

impl Runtime {
    /// 投递一条自定义事件到某个窗口：UI 线程在下一个 tick 的
    /// `ViewModel::on_external` 里收到（`data.downcast::<T>()`）。
    ///
    /// 与 [`crate::task::Poster::post`] 是同一条管道：有平台时走
    /// `EventLoopProxy`，无头时进本地队列（`App::frame_all` 消费）。返回 `false`
    /// 表示事件循环已退出。
    pub fn emit<T: Send + 'static>(&self, window: WindowId, msg: T) -> bool {
        // `post_external`（而不是 `post`）：载荷已经是 `ExternalData`，
        // 再走 `post` 会被**二次包装**，落地侧 `downcast::<T>()` 就取不到。
        self.inner
            .poster
            .post_external(window, crate::app::ExternalData::new(msg))
    }

    /// **广播**一条自定义事件给所有已注册窗口（载荷用 `Arc` 共享，免得逐个克隆）。
    ///
    /// ```ignore
    /// rt.emit_global(Arc::new(AppEvent::ThemePackChanged));
    /// // 窗口侧：data.downcast::<Arc<AppEvent>>()
    /// ```
    pub fn emit_global<T: Send + Sync + 'static>(&self, msg: std::sync::Arc<T>) -> bool {
        let windows = self.windows();
        let mut ok = false;
        for w in windows {
            ok |= self.emit(w, std::sync::Arc::clone(&msg));
        }
        ok
    }
}

/// 类型化的**事件发射器**：`Clone + Send`，可以挂在业务层里、也可以 move 进工作线程。
///
/// 就是 [`crate::task::Poster`] 的**类型化糖**（固定窗口 + 固定载荷类型）：
/// poster 只关心"发事件"，不带任何任务语义。适合把 UI 无关的业务模块
/// （网络层、文件监听、设备）接进来。
///
/// ```ignore
/// struct Net { tx: Emitter<NetEvent> }               // 业务层持有
/// impl Net {
///     fn on_bytes(&self, b: Vec<u8>) { self.tx.emit(NetEvent::Bytes(b)); }
/// }
/// // UI 侧：ViewModel::on_external 里 downcast::<NetEvent>()
/// ```
///
/// 内部只存 [`Poster`](crate::task::Poster)（`Arc`，`Send + Sync`）而**不是**
/// `Runtime`（`Rc`，`!Send`）—— 这正是它天然能跨线程、无需 unsafe 的原因；
/// 也正因为存的是**共享的投递中心**，平台 waker 后装入它也立刻可见
/// （见 `Poster` 文档里的 A6）。
pub struct Emitter<T> {
    window: WindowId,
    poster: crate::task::Poster,
    _marker: std::marker::PhantomData<fn() -> T>,
}

impl<T> Clone for Emitter<T> {
    fn clone(&self) -> Self {
        Self {
            window: self.window,
            poster: self.poster.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T: Send + 'static> Emitter<T> {
    /// 造一个发射器
    pub fn new(rt: &Runtime, window: WindowId) -> Self {
        Self {
            window,
            poster: rt.poster(),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn window(&self) -> WindowId {
        self.window
    }

    /// 发一条事件（UI 线程在 `on_external` 里收到）
    pub fn emit(&self, msg: T) -> bool {
        self.poster.post(self.window, msg)
    }
}

impl<T> std::fmt::Debug for Emitter<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Emitter")
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "event_tests.rs"]
mod tests;
/// 一次分发收集到的任务清单
#[derive(Default, Debug)]
pub struct RoutePlan {
    pub target: NodeId,
    pub items: Vec<HandlerSlot>,
}

fn collect_from(track: &Track, id: NodeId, kind: EventKind, items: &mut Vec<HandlerSlot>) {
    if let Some(n) = track.get(id) {
        // 禁用节点**不参与路由**（对齐 WinUI `IsEnabled=false`：输入整体不达）。
        // 内置行为另有拦截（`widgets::handle` 开头就查 `enabled`），此前这里没查 ——
        // 于是 `button(..).enabled(false).on_tap(..)` 照样触发（菜单里"禁用项"最典型：
        // 剪切/删除的禁用态看着是灰的、点下去却真的执行）。
        if !n.interaction.enabled {
            return;
        }
        for slot in &n.handlers {
            if slot.kind == kind {
                items.push(slot.clone());
            }
        }
    }
}

/// ① 只读阶段：按路由策略沿命中链收集处理器。
///
/// `path` 约定：`path[0]` = 最外层，`path.last()` = 命中目标（由 M2 的命中测试产出）。
pub fn collect_route(track: &Track, path: &[NodeId], kind: EventKind) -> RoutePlan {
    let Some(target) = path.last().copied() else {
        return RoutePlan::default();
    };

    let mut items = Vec::new();
    match kind.routing() {
        // 隧道：外 → 内
        Routing::Tunnel => {
            for id in path {
                collect_from(track, *id, kind, &mut items);
            }
        }
        // 冒泡：内 → 外
        Routing::Bubble => {
            for id in path.iter().rev() {
                collect_from(track, *id, kind, &mut items);
            }
        }
        Routing::Direct => collect_from(track, target, kind, &mut items),
    }

    RoutePlan { target, items }
}

/// 分发结果
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DispatchOutcome {
    pub handled: bool,
    pub invoked: usize,
    pub skipped: usize,
}

/// ② 可写阶段：按序调用处理器。
///
/// 借用说明：`track` 只在这一阶段**只读**用来收集；收集完成后就不再借用它，
/// 处理器只拿到 `&mut Ctx`（内部只有 `Runtime` / `CmdBuf`），所以不会与树冲突。
/// 处理器攒下的 `Cmd` 由调用方在**分发结束后**统一 `apply_cmds` 落树。
pub fn dispatch(
    rt: &Runtime,
    window: WindowId,
    track: &Track,
    path: &[NodeId],
    ev: &Event,
    extra_cmds: &mut CmdBuf,
) -> DispatchOutcome {
    let kind = ev.kind();
    let plan = collect_route(track, path, kind);
    if plan.items.is_empty() {
        return DispatchOutcome::default();
    }

    let mut cx = Ctx::new(rt, window, ev.summary());
    let mut out = DispatchOutcome::default();

    for slot in &plan.items {
        if cx.handled && !slot.handled_events_too {
            out.skipped += 1;
            continue;
        }
        (slot.handler)(&mut cx);
        out.invoked += 1;
    }

    out.handled = cx.handled();
    extra_cmds.extend(cx.take_cmds());
    out
}
