// src/widget/tree.rs
//! Widget 树管理 - 使用 SlotMap + RefCell 存储
//!
//! 设计原则：
//! - SlotMap 提供连续内存存储和 generational key
//! - 每个 Widget 有独立的 RefCell，支持跨 Widget 可变借用（事件回调场景）
//! - 父子关系集中管理在 WidgetEntry 中，不再分散在各 Widget 内部

use std::cell::{Ref, RefCell, RefMut};

use slotmap::SlotMap;

use crate::core::WidgetId;
use crate::widget::Widget;

/// Widget 存储条目
///
/// 包含：
/// - widget: RefCell 包装的 Widget，支持独立借用
/// - parent: 反向引用，支持 O(depth) path_to
/// - children: 子节点列表，集中管理
pub struct WidgetEntry {
    pub widget: RefCell<Box<dyn Widget>>,
    pub parent: Option<WidgetId>,
    pub children: Vec<WidgetId>,
}

/// Widget 树
///
/// 使用 SlotMap 存储，提供：
/// - 连续内存布局（缓存友好）
/// - Generational key（防止悬垂指针）
/// - 集中父子关系管理
pub struct WidgetTree {
    /// 所有 Widget 的存储
    entries: SlotMap<WidgetId, WidgetEntry>,
    /// 根节点 ID
    root: Option<WidgetId>,
}

impl WidgetTree {
    /// 创建空的 Widget 树
    pub fn new() -> Self {
        Self {
            entries: SlotMap::with_key(),
            root: None,
        }
    }

    /// 创建并添加 Widget 到树中
    pub fn create<W: Widget>(&mut self, widget: W) -> WidgetId {
        let entry = WidgetEntry {
            widget: RefCell::new(Box::new(widget)),
            parent: None,
            children: Vec::new(),
        };
        self.entries.insert(entry)
    }

    /// 设置根节点
    pub fn set_root(&mut self, root_id: WidgetId) {
        self.root = Some(root_id);
    }

    /// 清除根节点（用于隐藏 Layer）
    pub fn clear_root(&mut self) {
        self.root = None;
    }

    /// 获取根节点 ID
    pub fn root(&self) -> Option<WidgetId> {
        self.root
    }

    /// 添加子节点关系
    ///
    /// 父子关系由 WidgetEntry 集中管理
    pub fn add_child(&mut self, parent_id: WidgetId, child_id: WidgetId) {
        // 设置 child 的 parent
        if let Some(child_entry) = self.entries.get_mut(child_id) {
            child_entry.parent = Some(parent_id);
        }
        // 添加到 parent 的 children
        if let Some(parent_entry) = self.entries.get_mut(parent_id) {
            parent_entry.children.push(child_id);
        }
    }

    /// 移除子节点关系
    pub fn remove_child(&mut self, parent_id: WidgetId, child_id: WidgetId) {
        // 清除 child 的 parent
        if let Some(child_entry) = self.entries.get_mut(child_id) {
            child_entry.parent = None;
        }
        // 从 parent 的 children 中移除
        if let Some(parent_entry) = self.entries.get_mut(parent_id) {
            parent_entry.children.retain(|id| *id != child_id);
        }
    }

    /// 获取指定类型的 Widget 不可变引用
    pub fn get<W: Widget>(&self, id: WidgetId) -> Option<Ref<'_, W>> {
        let entry = self.entries.get(id)?;
        let widget_ref = entry.widget.borrow();
        Ref::filter_map(widget_ref, |w| w.as_any().downcast_ref::<W>()).ok()
    }

    /// 获取指定类型的 Widget 可变引用
    pub fn get_mut<W: Widget>(&self, id: WidgetId) -> Option<RefMut<'_, W>> {
        let entry = self.entries.get(id)?;
        let widget_ref = entry.widget.borrow_mut();
        RefMut::filter_map(widget_ref, |w| w.as_any_mut().downcast_mut::<W>()).ok()
    }

    /// 获取 Widget 可变引用（类型擦除）
    pub fn get_widget(&self, id: WidgetId) -> Option<RefMut<'_, Box<dyn Widget>>> {
        self.entries.get(id).map(|e| e.widget.borrow_mut())
    }

    /// 获取 Widget 不可变引用（类型擦除）
    pub fn get_widget_immut(&self, id: WidgetId) -> Option<Ref<'_, Box<dyn Widget>>> {
        self.entries.get(id).map(|e| e.widget.borrow())
    }

    /// 检查是否存在
    pub fn contains(&self, id: WidgetId) -> bool {
        self.entries.contains_key(id)
    }

    /// 获取子节点列表（直接从 WidgetEntry 读取）
    pub fn children_of(&self, id: WidgetId) -> Vec<WidgetId> {
        self.entries
            .get(id)
            .map(|e| e.children.clone())
            .unwrap_or_default()
    }

    /// 获取父节点
    pub fn parent_of(&self, id: WidgetId) -> Option<WidgetId> {
        self.entries.get(id).and_then(|e| e.parent)
    }

    /// 遍历树（前序遍历）
    pub fn traverse<F>(&self, mut f: F)
    where
        F: FnMut(WidgetId, &dyn Widget),
    {
        if let Some(root_id) = self.root {
            self.traverse_recursive(root_id, &mut f);
        }
    }

    fn traverse_recursive<F>(&self, id: WidgetId, f: &mut F)
    where
        F: FnMut(WidgetId, &dyn Widget),
    {
        if let Some(entry) = self.entries.get(id) {
            let widget = entry.widget.borrow();
            f(id, widget.as_ref());
            let children = entry.children.clone();
            drop(widget);
            for child_id in children {
                self.traverse_recursive(child_id, f);
            }
        }
    }

    /// 获取从根到指定节点的路径（O(depth) 沿 parent 链上溯）
    pub fn path_to(&self, target_id: WidgetId) -> Vec<WidgetId> {
        let mut path = Vec::new();
        let mut current = target_id;

        while let Some(entry) = self.entries.get(current) {
            path.push(current);
            current = match entry.parent {
                Some(p) => p,
                None => break,
            };
        }

        // 反转：从根到目标
        path.reverse();
        path
    }

    /// 获取节点数量
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for WidgetTree {
    fn default() -> Self {
        Self::new()
    }
}
