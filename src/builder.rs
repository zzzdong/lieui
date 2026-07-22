//! Builder 模式：声明式 UI 构建 + Slot 位置感知的增量重建
//!
//! 核心机制：
//! - 通过 `BuildContext` 声明式描述 UI 结构
//! - **每个父容器维护独立的子节点 slot 列表**（按 parent WidgetId 分组）
//! - 重建时在对应父容器下按 (位置索引 + 类型标签) 匹配，复用已存在的 Widget
//! - 支持显式 Key（用于动态列表如 Checkbox 的 for 循环），匹配失败时自动增删
//!
//! 使用方式：
//! ```ignore
//! let mut vc = ViewContext::new(Size::new(800.0, 600.0));
//! let count = vc.state(0);
//!
//! vc.set_build_fn(move |bctx| {
//!     bctx.column(|bctx| {
//!         bctx.text(&format!("Count: {}", *count.get()));
//!         bctx.button("+", |ectx| {
//!             count.update(|v| *v += 1);
//!             ectx.request_rebuild();
//!         });
//!     });
//! });
//! vc.build();
//! vc.run_blocking();
//! ```

use std::cell::{Cell, RefMut};
use std::collections::HashMap;

use crate::core::{ViewContext, WidgetId};
use crate::event::EventContext;
use crate::text::TextStyle;
use crate::widget::Widget;
use crate::widgets::*;

/// 重建状态：存储上一帧的 widget 树结构
///
/// - `root`: 根 widget ID（用于设置 Base 层 root）
/// - `children`: 每个父容器下的子节点列表
#[derive(Clone, Default)]
pub struct BuildSnapshot {
    /// 根 widget（第一个创建的顶层容器）
    root: Option<WidgetId>,
    /// `parent_id → [(child_id, type_tag)]`
    children: HashMap<WidgetId, Vec<(WidgetId, String)>>,
}

impl BuildSnapshot {
    /// 返回 Builder 创建的根 widget 的 ID（即 Base 层的 root）
    pub fn first_widget_id(&self) -> Option<WidgetId> {
        self.root
    }
}

/// 声明式 UI 构建上下文
pub struct BuildContext<'a> {
    pub(crate) vc: &'a mut ViewContext,

    /// 当前父容器
    parent: Option<WidgetId>,

    /// 上一帧各父容器的子节点列表
    old_children: HashMap<WidgetId, Vec<(WidgetId, String)>>,

    /// 当前帧各父容器的子节点列表
    new_children: HashMap<WidgetId, Vec<(WidgetId, String)>>,

    /// 当前父容器下的子节点索引（每个 parent 共享同一个计数器）
    slot_idx: Cell<usize>,

    /// 父级状态栈：用于 exit_container 时恢复 parent + slot_idx
    saved_stack: Vec<(Option<WidgetId>, usize)>,

    /// 根顶层 widget ID（第一个 parent=None 时创建的 widget）
    root_id: Option<WidgetId>,
}

impl<'a> BuildContext<'a> {
    pub(crate) fn new(vc: &'a mut ViewContext, snapshot: BuildSnapshot) -> Self {
        Self {
            vc,
            parent: None,
            old_children: snapshot.children,
            new_children: HashMap::new(),
            slot_idx: Cell::new(0),
            saved_stack: Vec::new(),
            root_id: snapshot.root,
        }
    }

    pub(crate) fn finalize(self) -> BuildSnapshot {
        BuildSnapshot {
            root: self.root_id,
            children: self.new_children,
        }
    }

    // ========================================================================
    // 内部：slot 匹配与创建
    // ========================================================================

    fn slot_or_create<W: crate::widget::Widget + 'static>(
        &mut self,
        explicit_key: Option<&str>,
        create: impl FnOnce() -> W,
    ) -> WidgetId {
        let idx = self.slot_idx.get();
        self.slot_idx.set(idx + 1);

        let type_name = std::any::type_name::<W>();
        let tag = explicit_key
            .map(|k| format!("key:{}", k))
            .unwrap_or_else(|| format!("type:{}", type_name));

        // 在旧数据中按 parent 匹配
        // parent=None 的顶层 widget 用 WidgetId::default() 作为 sentinel key
        let parent_key = self.parent.unwrap_or(WidgetId::default());
        let matched_id = self
            .old_children
            .get(&parent_key)
            .and_then(|slots| slots.get(idx))
            .filter(|(_, old_tag)| *old_tag == tag)
            .map(|(id, _)| *id);

        if let Some(old_id) = matched_id {
            self.record_child(old_id, tag);
            return old_id;
        }

        let widget = create();
        let id = self.vc.create(widget);
        if let Some(parent) = self.parent {
            self.vc.add_child(parent, id);
        } else {
            self.root_id = Some(id);
        }
        self.record_child(id, tag);
        id
    }

    /// 记录当前父容器下的子节点
    /// parent=None 的顶层 widget 用 WidgetId::default() 作为 sentinel key
    fn record_child(&mut self, id: WidgetId, tag: String) {
        let parent_key = self.parent.unwrap_or(WidgetId::default());
        self.new_children
            .entry(parent_key)
            .or_default()
            .push((id, tag));
    }

    /// 进入子容器：保存 (parent, slot_idx)，设置新 parent，slot_idx 归零
    fn enter_container(&mut self, id: WidgetId) {
        self.saved_stack
            .push((self.parent, self.slot_idx.get()));
        self.parent = Some(id);
        self.slot_idx.set(0);
    }

    /// 退出子容器：恢复上一级的 (parent, slot_idx)
    fn exit_container(&mut self) {
        if let Some((saved_parent, saved_idx)) = self.saved_stack.pop() {
            self.parent = saved_parent;
            self.slot_idx.set(saved_idx);
        }
    }

    // ========================================================================
    // 声明式 API
    // ========================================================================

    pub fn text(&mut self, content: &str) -> WidgetId {
        let id = self.slot_or_create::<Text>(None, || Text::new(content));
        if let Some(mut t) = self.vc.get_mut::<Text>(id) {
            t.set_content(content);
        }
        id
    }

    pub fn text_with_style(&mut self, content: &str, style: &TextStyle) -> WidgetId {
        let style_c = style.clone();
        let id = self.slot_or_create::<Text>(None, || {
            let mut t = Text::new(content);
            t.set_style(&style_c);
            t
        });
        if let Some(mut t) = self.vc.get_mut::<Text>(id) {
            t.set_content(content);
            t.set_style(style);
        }
        id
    }

    pub fn button<F>(&mut self, label: &str, on_click: F) -> WidgetId
    where
        F: Fn(&EventContext) + 'static,
    {
        let id = self
            .slot_or_create::<Button>(None, || Button::new(label).on_click(on_click));
        if let Some(mut b) = self.vc.get_mut::<Button>(id) {
            b.set_text(label);
        }
        id
    }

    pub fn checkbox<F>(&mut self, tag: &str, label: &str, checked: bool, on_change: F) -> WidgetId
    where
        F: Fn(bool) + 'static,
    {
        let id = self.slot_or_create::<Checkbox>(Some(tag), || {
            Checkbox::new(label).checked(checked).on_changed(on_change)
        });
        if let Some(mut cb) = self.vc.get_mut::<Checkbox>(id) {
            cb.set_checked(checked);
        }
        id
    }

    pub fn image(&mut self) -> WidgetId {
        self.slot_or_create::<Image>(None, Image::new)
    }

    // ---- 容器类（自带 enter_container / exit_container）----

    pub fn container(&mut self, f: impl FnOnce(&mut BuildContext)) -> WidgetId {
        let id = self.slot_or_create::<Container>(None, Container::new);
        self.enter_container(id);
        f(self);
        self.exit_container();
        id
    }

    pub fn container_bg(&mut self, bg: &str, f: impl FnOnce(&mut BuildContext)) -> WidgetId {
        let id =
            self.slot_or_create::<Container>(None, || Container::new().background(bg));
        self.enter_container(id);
        f(self);
        self.exit_container();
        id
    }

    pub fn column(&mut self, f: impl FnOnce(&mut BuildContext)) -> WidgetId {
        let id = self.slot_or_create::<Column>(None, || Column::new().expand(true).spacing(4.0));
        self.enter_container(id);
        f(self);
        self.exit_container();
        id
    }

    pub fn column_start(&mut self, spacing: f32, f: impl FnOnce(&mut BuildContext)) -> WidgetId {
        let id = self.slot_or_create::<Column>(None, || {
            use crate::layout::{AlignItems, JustifyContent};
            Column::new()
                .spacing(spacing)
                .justify(JustifyContent::Start)
                .align(AlignItems::Start)
        });
        self.enter_container(id);
        f(self);
        self.exit_container();
        id
    }

    pub fn row(&mut self, f: impl FnOnce(&mut BuildContext)) -> WidgetId {
        let id = self.slot_or_create::<Row>(None, || Row::new().expand(true).spacing(4.0));
        self.enter_container(id);
        f(self);
        self.exit_container();
        id
    }

    // ========================================================================
    // 通用配置 API
    // ========================================================================

    pub fn get_mut<W: Widget + 'static>(&self, id: WidgetId) -> Option<RefMut<'_, W>> {
        self.vc.get_mut::<W>(id)
    }
}
