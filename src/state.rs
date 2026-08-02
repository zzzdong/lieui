//! 全局状态更新信号
//!
//! 本模块只负责在单线程内协调“是否需要 rebuild/redraw”。
//! Widget state 的持久化由 `widget::BuildContext` 负责。

use crate::core::layers::{Anchor, FocusPolicy, LayerKind, LayerOptions};
use crate::view::node::ViewNode;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

thread_local! {
    static REBUILD_REQUESTED: Cell<bool> = const { Cell::new(false) };
    static REDRAW_REQUESTED: Cell<bool> = const { Cell::new(false) };
    static PENDING_LAYER: RefCell<Vec<LayerCmd>> = const { RefCell::new(Vec::new()) };
    static WINDOW_CLOSE_REQUESTED: Cell<bool> = const { Cell::new(false) };
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
}

/// 待处理的层命令：显示（含已构建的 widget tree）或隐藏（单实例层，如 Modal/Overlay）。
pub(crate) enum LayerCmd {
    Show {
        kind: LayerKind,
        spec: LayerSpec,
        view: Box<ViewNode>,
    },
    /// 移除指定单实例层（Modal / Overlay）。
    Hide { kind: LayerKind },
}

/// 请求全量重建（builder + layout + render）
pub fn request_rebuild() {
    REBUILD_REQUESTED.with(|r| r.set(true));
}

/// 检查并清除重建标记
pub(crate) fn take_rebuild_requested() -> bool {
    REBUILD_REQUESTED.with(|r| r.replace(false))
}

/// 公开版本：检查并清除重建标记。
/// 供嵌入式驱动（自定义事件循环）与性能测试使用。
pub fn take_rebuild_requested_pub() -> bool {
    take_rebuild_requested()
}

/// 请求仅重绘（不跑 builder/layout，只更新交互状态渲染）
pub fn request_redraw() {
    REDRAW_REQUESTED.with(|r| r.set(true));
}

/// 检查并清除重绘标记
pub(crate) fn take_redraw_requested() -> bool {
    REDRAW_REQUESTED.with(|r| r.replace(false))
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
) {
    let mut ctx =
        crate::widget::BuildContext::new(Rc::new(RefCell::new(crate::widget::StateMap::new())));
    let widget = builder(&mut ctx);
    let view = widget.build(&mut ctx);
    PENDING_LAYER.with(|c| {
        c.borrow_mut().push(LayerCmd::Show {
            kind,
            spec,
            view: Box::new(view),
        });
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
    PENDING_LAYER.with(|c| {
        c.borrow_mut().push(LayerCmd::Hide {
            kind: LayerKind::Modal,
        });
    });
    request_rebuild();
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
    PENDING_LAYER.with(|c| {
        c.borrow_mut().push(LayerCmd::Hide {
            kind: LayerKind::Overlay,
        });
    });
    request_rebuild();
}

/// 取走待处理的层命令（Runtime 内部使用）。
pub(crate) fn take_pending_layers() -> Vec<LayerCmd> {
    PENDING_LAYER.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

/// 请求真正关闭当前窗口。
///
/// 该调用**绕过关闭守卫**：集成方在自行实现的确认弹窗被用户确认后调用本函数
/// （例如按钮 `on_click` 中），下一帧事件循环会直接关闭窗口，不再触发
/// `CloseAction::Cancel`，从而避免重复弹窗。与关闭守卫回调中的 `&dyn Fn()` 相比，
/// 本函数可被存到跨帧存活的回调（如 `on_click`）中异步调用。
pub fn request_window_close() {
    WINDOW_CLOSE_REQUESTED.with(|c| c.set(true));
}

/// 检查并清除「请求关闭窗口」标记（Runtime 内部使用）。
pub(crate) fn take_window_close_requested() -> bool {
    WINDOW_CLOSE_REQUESTED.with(|c| c.replace(false))
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
