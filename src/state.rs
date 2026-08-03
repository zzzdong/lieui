//! 全局状态更新信号
//!
//! 本模块只负责在单线程内协调“是否需要 rebuild/redraw”。
//! Widget state 的持久化由 `widget::BuildContext` 负责。

use crate::core::layers::{Anchor, FocusPolicy, LayerHandle, LayerKind, LayerOptions};
use crate::view::node::ViewNode;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use winit::window::WindowId;

/// 每个窗口各自的重建/重绘/关闭标志。
#[derive(Default, Clone, Copy)]
struct Flags {
    rebuild: bool,
    redraw: bool,
    close: bool,
}

thread_local! {
    /// 当前正在处理的窗口。所有 `request_*` 调用都会路由到该窗口，
    /// 从而把全局标志变成 per-window，避免多窗口下的信号互相吞噬/重复。
    static CURRENT_WID: Cell<Option<WindowId>> = const { Cell::new(None) };
}

/// 所有已注册窗口的标志表。键为窗口 id，值是该窗口独有的重建/重绘/关闭标记。
static FLAGS: std::sync::LazyLock<std::sync::Mutex<HashMap<WindowId, Flags>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// 注册一个窗口，使其在标志表中拥有独立条目。
///
/// 由 `Application` 在创建窗口时调用；独立的性能测试/嵌入式驱动也可调用
/// 以接入 per-window 标志路由（详见 `request_rebuild` / `take_rebuild_requested`）。
pub fn register_window(wid: WindowId) {
    FLAGS.lock().unwrap().entry(wid).or_default();
}

/// 注销一个窗口并清理其标志（窗口关闭时调用）。
pub fn unregister_window(wid: WindowId) {
    FLAGS.lock().unwrap().remove(&wid);
}

/// 设置「当前窗口」。在 `window_event` 进入某窗口处理时调用，
/// 之后的 `request_*` 都会作用于该窗口。
pub(crate) fn set_current_window(wid: WindowId) {
    CURRENT_WID.with(|c| c.set(Some(wid)));
}

/// 清除「当前窗口」。窗口处理结束时调用。
pub(crate) fn clear_current_window() {
    CURRENT_WID.with(|c| c.set(None));
}

/// 取得当前窗口的已注册窗口 id（若存在）。
fn current_wid() -> Option<WindowId> {
    CURRENT_WID.with(|c| c.get())
}

/// 修改指定窗口的标志。
fn set_flag<F: FnOnce(&mut Flags)>(wid: WindowId, f: F) {
    let mut g = FLAGS.lock().unwrap();
    f(g.entry(wid).or_default());
}

/// 若无「当前窗口」（例如全局动画 tick），则广播到所有已注册窗口，
/// 保持与单窗口时代一致的语义。
fn set_flag_current_or_all<F: Fn(&mut Flags) + Copy>(f: F) {
    match current_wid() {
        Some(wid) => set_flag(wid, f),
        None => {
            for fl in FLAGS.lock().unwrap().values_mut() {
                f(fl);
            }
        }
    }
}

thread_local! {
    static PENDING_LAYER: RefCell<Vec<LayerCmd>> = const { RefCell::new(Vec::new()) };
}

/// 一次「以 widget tree 显示某层」的规格：锚点 + 焦点/阻塞策略 + 层选项。
#[derive(Debug, Clone, Copy)]
pub struct LayerSpec {
    pub anchor: Anchor,
    pub focus: FocusPolicy,
    pub opts: LayerOptions,
}

impl LayerSpec {
    pub fn new(anchor: Anchor, focus: FocusPolicy) -> Self {
        Self {
            anchor,
            focus,
            opts: LayerOptions::default(),
        }
    }

    /// 带 backdrop 的规格（Modal 半透明遮罩等）。
    pub fn with_backdrop(mut self, color: crate::geometry::Color) -> Self {
        self.opts.backdrop = Some(color);
        self
    }

    /// 直接指定完整层选项（如 `LayerOptions::popup()` 启用防溢出翻转）。
    pub fn with_opts(mut self, opts: LayerOptions) -> Self {
        self.opts = opts;
        self
    }
}

/// 待处理的层命令：显示（含已构建的 widget tree）或隐藏（单实例层，如 Modal/Overlay）。
pub(crate) enum LayerCmd {
    Show {
        kind: LayerKind,
        spec: LayerSpec,
        view: Box<ViewNode>,
        handle: LayerHandle,
    },
    /// 移除指定单实例层（Modal / Overlay）。
    Hide { kind: LayerKind },
}

/// 请求全量重建（builder + layout + render）
///
/// 作用于「当前窗口」；若当前不在任何窗口上下文（如全局动画 tick），则广播到所有窗口。
pub fn request_rebuild() {
    set_flag_current_or_all(|f| f.rebuild = true);
}

/// 检查并清除指定窗口的重建标记
pub(crate) fn take_rebuild_requested(wid: WindowId) -> bool {
    let mut g = FLAGS.lock().unwrap();
    match g.get_mut(&wid) {
        Some(f) => std::mem::replace(&mut f.rebuild, false),
        None => false,
    }
}

/// 公开版本：检查并清除指定窗口的重建标记。
/// 供嵌入式驱动（自定义事件循环）与性能测试使用。
pub fn take_rebuild_requested_pub(wid: WindowId) -> bool {
    take_rebuild_requested(wid)
}

/// 请求仅重绘（不跑 builder/layout，只更新交互状态渲染）
///
/// 作用于「当前窗口」；若当前不在任何窗口上下文（如全局动画 tick），则广播到所有窗口。
pub fn request_redraw() {
    set_flag_current_or_all(|f| f.redraw = true);
}

/// 检查并清除指定窗口的重绘标记
pub(crate) fn take_redraw_requested(wid: WindowId) -> bool {
    let mut g = FLAGS.lock().unwrap();
    match g.get_mut(&wid) {
        Some(f) => std::mem::replace(&mut f.redraw, false),
        None => false,
    }
}

/// 以 **widget tree** 方式显示任意层（window 之外的浮层：Modal / Overlay / Popup /
/// Tooltip / System）。
///
/// `builder` 返回 `Box<dyn Widget>`（与主内容 builder 形态一致），内部创建独立的
/// [`BuildContext`] 并把 widget tree build 成 `ViewNode` 后挂载到指定层。
///
/// 集成方可以用 widget（`Button` / `IconButton` / `Column` / `Container` 等）声明层内容，
/// 而不必手写 `ViewNode`。注意：每次调用都会新建独立 `BuildContext`，
/// 因此基于 `use_state` 的 hook 状态不会跨层会话持久；需要持久状态时，
/// 应把状态放到外层（如 `AppState`）并在每次重建时重新 `show_layer`。
pub fn show_layer(
    kind: LayerKind,
    spec: LayerSpec,
    builder: impl FnOnce(&mut crate::widget::BuildContext) -> Box<dyn crate::widget::Widget> + 'static,
) -> LayerHandle {
    let mut ctx =
        crate::widget::BuildContext::new(Rc::new(RefCell::new(crate::widget::StateMap::new())));
    let widget = builder(&mut ctx);
    let view = widget.build(&mut ctx);
    // 预先生成句柄，经由全局命令携带到目标窗口的 LayerStack。
    let handle = NEXT_POPUP_HANDLE.with(|c| {
        let h = LayerHandle(c.get());
        c.set(h.0 + 1);
        h
    });
    PENDING_LAYER.with(|c| {
        c.borrow_mut().push(LayerCmd::Show {
            kind,
            spec,
            view: Box::new(view),
            handle,
        });
    });
    request_rebuild();
    handle
}

/// 隐藏指定单实例层（Modal / Overlay），按 `LayerKind` 查找当前存在该 kind 的默认条目。
pub fn hide_layer(kind: LayerKind) {
    PENDING_LAYER.with(|c| {
        c.borrow_mut().push(LayerCmd::Hide { kind });
    });
    request_rebuild();
}

/// 以 widget tree 显示 Modal 层（阻塞式 + 半透明遮罩 + 屏幕居中）。
pub fn show_modal(
    builder: impl FnOnce(&mut crate::widget::BuildContext) -> Box<dyn crate::widget::Widget> + 'static,
) {
    show_layer(
        LayerKind::Modal,
        LayerSpec::new(Anchor::ScreenCenter, FocusPolicy::BlockBelow)
            .with_backdrop(crate::geometry::Color::rgba(0, 0, 0, 80)),
        builder,
    );
}

/// 隐藏默认 Modal 层。
pub fn hide_modal() {
    hide_layer(LayerKind::Modal);
}

/// 以 widget tree 显示 Overlay 层（不阻塞下层）。
pub fn show_overlay(
    builder: impl FnOnce(&mut crate::widget::BuildContext) -> Box<dyn crate::widget::Widget> + 'static,
) {
    show_layer(
        LayerKind::Overlay,
        LayerSpec::new(Anchor::None, FocusPolicy::Transparent),
        builder,
    );
}

/// 隐藏默认 Overlay 层。
pub fn hide_overlay() {
    hide_layer(LayerKind::Overlay);
}

/// 弹出浮层相对触发区域的首选方向。
#[derive(Debug, Clone, Copy)]
pub enum PopupPlacement {
    /// 触发器下方展开（默认，若底部空间不足则自动翻转到上方）
    Below,
    /// 触发器上方展开（若顶部空间不足则翻转到下方）
    Above,
    /// 触发器右侧展开（若右侧空间不足则翻转到左侧）
    RightOf,
    /// 触发器左侧展开（若左侧空间不足则翻转到右侧）
    LeftOf,
    /// 固定在屏幕坐标 (x, y)，不相对触发器
    Fixed { x: f32, y: f32 },
}

impl PopupPlacement {
    /// 转换为 `Anchor`，`trigger` 为触发区域，`gap` 为浮层与触发器的间距。
    fn to_anchor(self, trigger: crate::geometry::Rect, gap: f32) -> Anchor {
        match self {
            PopupPlacement::Below => Anchor::Below {
                anchor: trigger,
                gap,
            },
            PopupPlacement::Above => Anchor::Above {
                anchor: trigger,
                gap,
            },
            PopupPlacement::RightOf => Anchor::RightOf {
                anchor: trigger,
                gap,
            },
            PopupPlacement::LeftOf => Anchor::LeftOf {
                anchor: trigger,
                gap,
            },
            PopupPlacement::Fixed { x, y } => Anchor::Fixed { x, y },
        }
    }
}

/// 弹出浮层句柄：用于 `hide_popup` 精确关闭该浮层实例。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PopupHandle(pub LayerHandle);

// 全局自增句柄源，保证跨窗口 Popup 句柄唯一（句柄仅在创建窗口内消费）。
thread_local! {
    static NEXT_POPUP_HANDLE: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// 以 widget tree 显示一个 Popup 浮层（如右键菜单、下拉菜单）。
///
/// 浮层会落在 `LayerKind::Popup` 层，自动设置 `FocusPolicy::Dismissable`
/// （点击浮层外部自动关闭），并启用防溢出翻转：当按首选方向定位会超出窗口
/// 视口时自动翻转到对侧，保证菜单始终完整可见（方案 A：纯自绘、不超出窗口）。
///
/// 返回 `PopupHandle`，可传给 `hide_popup` 精确关闭（菜单项点击后通常需手动关闭）。
pub fn show_popup(
    trigger: crate::geometry::Rect,
    placement: PopupPlacement,
    builder: impl FnOnce(&mut crate::widget::BuildContext) -> Box<dyn crate::widget::Widget> + 'static,
) -> PopupHandle {
    show_popup_with(trigger, placement, move |_handle, ctx| builder(ctx))
}

/// `show_popup` 的变体：builder 可拿到本 Popup 的 `PopupHandle`，
/// 便于在菜单项点击回调里调用 `hide_popup(handle)` 精确关闭。例如：
/// ```ignore
/// let h = show_popup_with(rect, PopupPlacement::Below, |handle, ctx| {
///     Box::new(Menu::new().item(MenuItem::new("项").close_with(handle)))
/// });
/// ```
pub fn show_popup_with(
    trigger: crate::geometry::Rect,
    placement: PopupPlacement,
    builder: impl FnOnce(
        PopupHandle,
        &mut crate::widget::BuildContext,
    ) -> Box<dyn crate::widget::Widget>
    + 'static,
) -> PopupHandle {
    let mut ctx =
        crate::widget::BuildContext::new(Rc::new(RefCell::new(crate::widget::StateMap::new())));
    let handle = NEXT_POPUP_HANDLE.with(|c| {
        let h = LayerHandle(c.get());
        c.set(h.0 + 1);
        h
    });
    let widget = builder(PopupHandle(handle), &mut ctx);
    let view = widget.build(&mut ctx);
    let spec = LayerSpec::new(placement.to_anchor(trigger, 4.0), FocusPolicy::Dismissable)
        .with_opts(LayerOptions::popup());
    PENDING_LAYER.with(|c| {
        c.borrow_mut().push(LayerCmd::Show {
            kind: LayerKind::Popup,
            spec,
            view: Box::new(view),
            handle,
        });
    });
    request_rebuild();
    PopupHandle(handle)
}

/// 关闭指定 Popup 浮层（按句柄精确移除）。
///
/// 说明：Popup 层通过 `PENDING_POPUP_HIDE` 按 handle 精确移除；`LayerCmd::Hide`
/// 仅用于单实例的 Modal/Overlay，对 Popup 无意义，故不推送（避免冗余命令）。
pub fn hide_popup(handle: PopupHandle) {
    PENDING_POPUP_HIDE.with(|c| {
        c.borrow_mut().push(handle.0.0);
    });
    request_rebuild();
}

// 待关闭的 Popup 句柄队列（与 PENDING_LAYER 同帧消费，用于按 handle 精确移除）。
thread_local! {
    static PENDING_POPUP_HIDE: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// 取走待处理的层命令（Runtime 内部使用）。
pub(crate) fn take_pending_layers() -> Vec<LayerCmd> {
    PENDING_LAYER.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

/// 取走待关闭的 Popup 句柄队列（Runtime 内部使用）。
pub(crate) fn take_popup_hides() -> Vec<u64> {
    PENDING_POPUP_HIDE.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

/// 请求真正关闭当前窗口。
///
/// 该调用**绕过关闭守卫**：集成方在自行实现的确认弹窗被用户确认后调用本函数
/// （例如按钮 `on_click` 中），下一帧事件循环会直接关闭窗口，不再触发
/// `CloseAction::Cancel`，从而避免重复弹窗。与关闭守卫回调中的 `&dyn Fn()` 相比，
/// 本函数可被存到跨帧存活的回调（如 `on_click`）中异步调用。
///
/// 作用于「当前窗口」。若调用时不在任何窗口上下文（异常情况），会广播到所有窗口
/// 并打印告警，避免静默丢失关闭请求。
pub fn request_window_close() {
    match current_wid() {
        Some(wid) => set_flag(wid, |f| f.close = true),
        None => {
            eprintln!(
                "[lieui] request_window_close called outside any window context; \
                 broadcasting close to all windows"
            );
            for fl in FLAGS.lock().unwrap().values_mut() {
                fl.close = true;
            }
        }
    }
}

/// 检查并清除指定窗口的「请求关闭窗口」标记（Runtime 内部使用）。
pub(crate) fn take_window_close_requested(wid: WindowId) -> bool {
    let mut g = FLAGS.lock().unwrap();
    match g.get_mut(&wid) {
        Some(f) => std::mem::replace(&mut f.close, false),
        None => false,
    }
}

/// 线程局部的共享状态。
///
/// 作为起点，`State<T>` 通过 thread-local 标志触发 rebuild。后续可以迁移到
/// `BuildContext::use_state`，但当前实现与现有示例兼容。
pub struct State<T> {
    inner: std::rc::Rc<std::cell::RefCell<T>>,
}

impl<T> State<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: std::rc::Rc::new(std::cell::RefCell::new(value)),
        }
    }

    pub fn get(&self) -> std::cell::Ref<'_, T> {
        self.inner.borrow()
    }

    pub fn set(&self, value: T) {
        *self.inner.borrow_mut() = value;
        request_rebuild();
    }

    pub fn update<F: FnOnce(&mut T)>(&self, f: F) {
        f(&mut self.inner.borrow_mut());
        request_rebuild();
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for State<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("value", &*self.get())
            .finish()
    }
}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self {
            inner: std::rc::Rc::clone(&self.inner),
        }
    }
}
