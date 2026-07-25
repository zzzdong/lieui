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
}

impl Listener {
    pub fn on_click(cb: Rc<dyn Fn()>) -> Self {
        Self {
            event: EventType::Click,
            callback: Callback::Simple(Rc::clone(&cb)),
        }
    }

    pub fn on_click_with_ctx(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::Click,
            callback: Callback::WithCtx(cb),
        }
    }

    pub fn on_mouse_down(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::MouseDown,
            callback: Callback::WithCtx(cb),
        }
    }

    pub fn on_mouse_up(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::MouseUp,
            callback: Callback::WithCtx(cb),
        }
    }

    pub fn on_mouse_move(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::MouseMove,
            callback: Callback::WithCtx(cb),
        }
    }

    pub fn on_mouse_wheel(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::MouseWheel,
            callback: Callback::WithCtx(cb),
        }
    }

    pub fn on_key_down(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::KeyDown,
            callback: Callback::WithCtx(cb),
        }
    }

    pub fn on_key_up(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::KeyUp,
            callback: Callback::WithCtx(cb),
        }
    }

    pub fn on_focus_in(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::FocusIn,
            callback: Callback::WithCtx(cb),
        }
    }

    pub fn on_focus_out(cb: Rc<dyn Fn(&mut crate::event::EventContext)>) -> Self {
        Self {
            event: EventType::FocusOut,
            callback: Callback::WithCtx(cb),
        }
    }
}

/// 节点类型标记
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Text,
    Image,
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
            ViewNode::Div { .. } => NodeType::Div,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            ViewNode::Text { .. } => "text",
            ViewNode::Image { .. } => "image",
            ViewNode::Div { .. } => "div",
        }
    }

    pub fn key(&self) -> Option<&str> {
        match self {
            ViewNode::Text { key, .. }
            | ViewNode::Image { key, .. }
            | ViewNode::Div { key, .. } => key.as_deref(),
        }
    }

    pub fn set_key(&mut self, k: String) {
        match self {
            ViewNode::Text { key, .. }
            | ViewNode::Image { key, .. }
            | ViewNode::Div { key, .. } => *key = Some(k),
        }
    }

    pub fn add_listener(&mut self, l: Listener) {
        match self {
            ViewNode::Text { listeners, .. }
            | ViewNode::Image { listeners, .. }
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
            ViewNode::Div { layout, .. } => {
                use crate::layout::types::{is_defined, Dimension as LayoutDimension};
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
            ) => a == x && b == y && l1 == r1 && l2 == r2,
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
            ) => a == x && b == y && l1 == r1 && l2 == r2,
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
            ) => l1 == r1 && p1 == p2 && l2 == r2,
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
            ) => a == x && b == y && l1 == r1 && l2 == r2,
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
            ) => Arc::ptr_eq(a, x) && b == y && l1 == r1 && l2 == r2,
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
            ) => l1 == r1 && p1 == p2 && l2 == r2,
            _ => false,
        }
    }

    /// 返回所有监听器。
    pub fn listeners(&self) -> &[Listener] {
        match self {
            ViewNode::Text { listeners, .. }
            | ViewNode::Image { listeners, .. }
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
}
