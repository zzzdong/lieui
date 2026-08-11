//! ViewNode — immutable UI 描述原语
//!
//! 内核层只保留三种原语：
//! - Text：文本节点
//! - Image：图片节点
//! - Div：通用容器，通过 `FlexStyle` 表达块级或 Flex 布局
//!
//! 每个原语都可以附带 `listeners: Vec<Listener>` 来响应多种事件。
//! `ViewNode` 对上层 Widget 完全不可知，只通过 `key` 为事件状态提供稳定索引。
//!
//! 架构分层：
//! - ViewNode 层：只保存确定好的内联样式（layout + paint），不做类 CSS 的继承/选择器。
//! - Layout 层：Taitank 风格的 FlexNode 引擎，直接消费 `ViewNode::layout`。
//! - Widget 层：把类 CSS 的高级语义编译成 ViewNode 的内联样式。

use crate::event::EventType;
use crate::layout::box_model::{IntrinsicSize, LayoutConstraint};
use crate::layout::measurable::{EmptyMeasure, FixedMeasure, Measurable, TextMeasure};
use crate::layout::style::FlexStyle;
use crate::view::paint::{ImageStyle, PaintStyle, TextStyle};
use std::rc::Rc;
use std::sync::Arc;

/// 事件处理回调。
#[derive(Clone)]
pub enum Callback {
    /// 无上下文的简单回调。
    Simple(Rc<dyn Fn()>),
    /// 带 EventContext 的回调（可访问 phase、stop_propagation 等）。
    WithCtx(Rc<dyn Fn(&mut crate::event::EventContext)>),
}

impl std::fmt::Debug for Callback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Callback").finish()
    }
}

impl PartialEq for Callback {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Callback::Simple(a), Callback::Simple(b)) => Rc::ptr_eq(a, b),
            (Callback::WithCtx(a), Callback::WithCtx(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for Callback {}

/// 事件监听器：事件类型 + 回调。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    /// 监听的事件类型。已导入了 `crate::event::EventType`。
    pub event: EventType,
    /// 事件发生时的回调。
    pub callback: Callback,
    /// 事件来源：内置行为 vs 用户自定义回调。
    ///
    /// 组件内部行为（Slider 拖拽、Input 聚焦/输入、Draggable 拖拽接线等）标记为
    /// [`ListenerKind::BuiltIn`]，通过公开 API 注册的回调（`.on_click(...)` 等）
    /// 标记为 [`ListenerKind::User`]。分发时同一节点上内置回调先于用户回调执行，
    /// 用户回调调用 `stop_propagation()` 不会阻止同节点已执行的内置行为。
    pub kind: ListenerKind,
}

/// 事件监听器来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ListenerKind {
    /// 组件内部行为所需的回调（由组件构建时标记）。
    BuiltIn,
    /// 用户通过组件公开 API 注册的回调。
    #[default]
    User,
}

impl Listener {
    pub fn on_click(cb: Rc<dyn Fn()>) -> Self {
        Self {
            event: EventType::Click,
            callback: Callback::Simple(Rc::clone(&cb)),
            kind: ListenerKind::User,
        }
    }

    pub fn on_click_with_ctx(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::Click,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_mouse_down(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::MouseDown,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_mouse_up(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::MouseUp,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_mouse_move(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::MouseMove,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_mouse_wheel(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::MouseWheel,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_mouse_enter(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::MouseEnter,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_mouse_leave(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::MouseLeave,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_drag_start(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::DragStart,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_drag_move(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::DragMove,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_drag_end(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::DragEnd,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_key_down(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::KeyDown,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_key_up(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::KeyUp,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_focus_in(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::FocusIn,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_focus_out(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::FocusOut,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_ime_preedit(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::ImePreedit,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_ime_commit(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::ImeCommit,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    pub fn on_ime_disabled(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::ImeDisabled,
            callback: Callback::WithCtx(cb),
            kind: ListenerKind::User,
        }
    }

    /// 标记为内置行为回调（组件内部接线使用）。
    pub fn builtin(mut self) -> Self {
        self.kind = ListenerKind::BuiltIn;
        self
    }

    /// 事件来源。
    pub fn kind(&self) -> ListenerKind {
        self.kind
    }
}

/// 比较两组监听器的「签名」是否相同：只看数量、事件类型与回调形态，
/// 不比较回调指针。builder 每次 rebuild 都会新建闭包，`Rc::ptr_eq` 永远
/// 不相等，若直接参与 config 比较会导致所有带监听器的节点每帧都被
/// Update（清空排版缓存 + 全量重排）。回调本体由 Reconciler 通过
/// 轻量的 UpdateListeners 补丁单独刷新。
pub fn listeners_sig_eq(a: &[Listener], b: &[Listener]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(x, y)| {
            x.event == y.event
                && x.kind == y.kind
                && matches!(
                    (&x.callback, &y.callback),
                    (Callback::Simple(_), Callback::Simple(_))
                        | (Callback::WithCtx(_), Callback::WithCtx(_))
                )
        })
}

/// 节点类型标记
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Text,
    Image,
    SharedSurface,
    Div,
}

#[derive(Debug, Clone)]
pub enum ViewNode {
    Text {
        content: String,
        style: TextStyle,
        layout: FlexStyle,
        key: Option<String>,
        listeners: Vec<Listener>,
    },
    Image {
        data: Arc<Vec<u8>>,
        style: ImageStyle,
        layout: FlexStyle,
        key: Option<String>,
        listeners: Vec<Listener>,
    },
    /// 共享像素表面：由高频组件（terminal）自持 buffer，通过脏区增量提交，
    /// 不经 rebuild 全链路。Compositor 只合屏其脏区。
    SharedSurface {
        surface: std::rc::Rc<crate::render::surface::SharedSurface>,
        layout: FlexStyle,
        key: Option<String>,
        listeners: Vec<Listener>,
    },
    Div {
        layout: FlexStyle,
        paint: PaintStyle,
        key: Option<String>,
        children: Vec<ViewNode>,
        listeners: Vec<Listener>,
    },
}

impl ViewNode {
    pub fn node_type(&self) -> NodeType {
        match self {
            ViewNode::Text { .. } => NodeType::Text,
            ViewNode::Image { .. } => NodeType::Image,
            ViewNode::SharedSurface { .. } => NodeType::SharedSurface,
            ViewNode::Div { .. } => NodeType::Div,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            ViewNode::Text { .. } => "text",
            ViewNode::Image { .. } => "image",
            ViewNode::SharedSurface { .. } => "shared_surface",
            ViewNode::Div { .. } => "div",
        }
    }

    pub fn key(&self) -> Option<&str> {
        match self {
            ViewNode::Text { key, .. }
            | ViewNode::Image { key, .. }
            | ViewNode::SharedSurface { key, .. }
            | ViewNode::Div { key, .. } => key.as_deref(),
        }
    }

    pub fn set_key(&mut self, k: String) {
        match self {
            ViewNode::Text { key, .. }
            | ViewNode::Image { key, .. }
            | ViewNode::SharedSurface { key, .. }
            | ViewNode::Div { key, .. } => *key = Some(k),
        }
    }

    pub fn add_listener(&mut self, l: Listener) {
        match self {
            ViewNode::Text { listeners, .. }
            | ViewNode::Image { listeners, .. }
            | ViewNode::SharedSurface { listeners, .. }
            | ViewNode::Div { listeners, .. } => listeners.push(l),
        }
    }

    pub fn with_listener(mut self, l: Listener) -> Self {
        self.add_listener(l);
        self
    }

    pub fn children(&self) -> &[ViewNode] {
        match self {
            ViewNode::Div { children, .. } => children,
            _ => &[],
        }
    }

    /// 返回节点的 Taitank 布局样式引用。
    pub fn layout(&self) -> &FlexStyle {
        match self {
            ViewNode::Text { layout, .. }
            | ViewNode::Image { layout, .. }
            | ViewNode::SharedSurface { layout, .. }
            | ViewNode::Div { layout, .. } => layout,
        }
    }

    /// 在约束下测量自身固有尺寸。
    pub fn measure(&self, constraint: &LayoutConstraint) -> IntrinsicSize {
        match self {
            ViewNode::Text { content, style, .. } => {
                TextMeasure::new(content.clone(), style.font_size).measure(constraint)
            }
            ViewNode::Image { style, .. } => {
                FixedMeasure::new(IntrinsicSize::new(style.width as f32, style.height as f32))
                    .measure(constraint)
            }
            ViewNode::SharedSurface { surface, .. } => {
                FixedMeasure::new(IntrinsicSize::new(surface.width() as f32, surface.height() as f32))
                    .measure(constraint)
            }
            ViewNode::Div { layout, .. } => {
                use crate::layout::types::{Dimension as LayoutDimension, is_defined};
                let w = layout.dim[LayoutDimension::Width as usize];
                let h = layout.dim[LayoutDimension::Height as usize];
                if is_defined(w) || is_defined(h) {
                    FixedMeasure::new(IntrinsicSize::new(
                        if is_defined(w) { w } else { 0.0 },
                        if is_defined(h) { h } else { 0.0 },
                    ))
                    .measure(constraint)
                } else {
                    EmptyMeasure.measure(constraint)
                }
            }
        }
    }

    /// 比较"配置"部分是否相等（排除 children，因为孩子由树结构管理）。
    pub fn config_eq(&self, other: &Self) -> bool {
        use ViewNode::*;
        match (self, other) {
            (
                Text {
                    content: a,
                    style: b,
                    layout: l1,
                    listeners: l2,
                    ..
                },
                Text {
                    content: x,
                    style: y,
                    layout: r1,
                    listeners: r2,
                    ..
                },
            ) => a == x && b == y && l1 == r1 && listeners_sig_eq(l2, r2),
            (
                Image {
                    data: a,
                    style: b,
                    layout: l1,
                    listeners: l2,
                    ..
                },
                Image {
                    data: x,
                    style: y,
                    layout: r1,
                    listeners: r2,
                    ..
                },
            ) => a == x && b == y && l1 == r1 && listeners_sig_eq(l2, r2),
            (
                SharedSurface {
                    surface: a,
                    layout: l1,
                    listeners: l2,
                    ..
                },
                SharedSurface {
                    surface: x,
                    layout: r1,
                    listeners: r2,
                    ..
                },
            ) => Rc::ptr_eq(a, x) && l1 == r1 && listeners_sig_eq(l2, r2),
            (
                Div {
                    layout: l1,
                    paint: p1,
                    listeners: l2,
                    ..
                },
                Div {
                    layout: r1,
                    paint: p2,
                    listeners: r2,
                    ..
                },
            ) => l1 == r1 && p1 == p2 && listeners_sig_eq(l2, r2),
            _ => false,
        }
    }

    pub fn is_container(&self) -> bool {
        matches!(self, ViewNode::Div { .. })
    }

    /// 整树相等性判断（用于 ViewNodeTree 缓存短路）。
    /// 与 `config_eq` 不同：对 Image 数据使用 `Arc::ptr_eq`，
    /// 避免大图片缓冲区的逐字节比较。
    pub fn tree_eq(&self, other: &Self) -> bool {
        if !self.node_config_eq_cached(other) {
            return false;
        }
        let a = self.children();
        let b = other.children();
        if a.len() != b.len() {
            return false;
        }
        a.iter().zip(b.iter()).all(|(x, y)| x.tree_eq(y))
    }

    /// tree_eq 使用的节点级比较（不含 children），Image 用指针相等。
    fn node_config_eq_cached(&self, other: &Self) -> bool {
        use ViewNode::*;
        match (self, other) {
            (
                Text {
                    content: a,
                    style: b,
                    layout: l1,
                    listeners: l2,
                    ..
                },
                Text {
                    content: x,
                    style: y,
                    layout: r1,
                    listeners: r2,
                    ..
                },
            ) => a == x && b == y && l1 == r1 && listeners_sig_eq(l2, r2),
            (
                Image {
                    data: a,
                    style: b,
                    layout: l1,
                    listeners: l2,
                    ..
                },
                Image {
                    data: x,
                    style: y,
                    layout: r1,
                    listeners: r2,
                    ..
                },
            ) => Arc::ptr_eq(a, x) && b == y && l1 == r1 && listeners_sig_eq(l2, r2),
            (
                SharedSurface {
                    surface: a,
                    layout: l1,
                    listeners: l2,
                    ..
                },
                SharedSurface {
                    surface: x,
                    layout: r1,
                    listeners: r2,
                    ..
                },
            ) => Rc::ptr_eq(a, x) && l1 == r1 && listeners_sig_eq(l2, r2),
            (
                Div {
                    layout: l1,
                    paint: p1,
                    listeners: l2,
                    ..
                },
                Div {
                    layout: r1,
                    paint: p2,
                    listeners: r2,
                    ..
                },
            ) => l1 == r1 && p1 == p2 && listeners_sig_eq(l2, r2),
            _ => false,
        }
    }

    /// 返回所有监听器。
    pub fn listeners(&self) -> &[Listener] {
        match self {
            ViewNode::Text { listeners, .. }
            | ViewNode::Image { listeners, .. }
            | ViewNode::SharedSurface { listeners, .. }
            | ViewNode::Div { listeners, .. } => listeners,
        }
    }

    /// 返回节点上所有 EventType::Click 的监听器。
    pub fn click_listeners(&self) -> Vec<&Listener> {
        self.listeners()
            .iter()
            .filter(|l| l.event == EventType::Click)
            .collect()
    }

    /// 节点是否声明了交互视觉（hover / pressed 样式）。
    ///
    /// 与 listener 无关：IconButton、Button 等组件即使没有注册任何回调，
    /// 只要配置了 hover/pressed 背景或文字颜色，就应参与 hover/pressed
    /// 状态管理与渲染（工具栏按钮最常见的形态）。
    pub fn is_interactive(&self) -> bool {
        match self {
            ViewNode::Div { paint, .. } => {
                paint.hover_background.is_some() || paint.pressed_background.is_some()
            }
            ViewNode::Text { style, .. } => {
                style.hover_color.is_some() || style.pressed_color.is_some()
            }
            ViewNode::Image { .. } => false,
            ViewNode::SharedSurface { .. } => false,
        }
    }
}
