//! Element — 运行时实体及其树结构（含交互状态）
//! 每个 Element 存储其 ViewNode（不含孩子）用于类型安全访问

use std::cell::Cell;
use slotmap::SlotMap;
use crate::core::ElementId;
use crate::layout::box_model::{ComputedLayout, IntrinsicSize};
use crate::view::node::ViewNode;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ElementState { pub hovered: bool, pub pressed: bool, pub focused: bool }

/// 运行时元素条目：存储 ViewNode（不含孩子）+ 布局/交互状态
pub struct ElementEntry {
    pub node: ViewNode,          // 类型安全的视图配置（孩子由 ElementTree.children 管理）
    pub intrinsic: Cell<IntrinsicSize>,
    pub layout: Cell<ComputedLayout>,
    pub dirty: Cell<bool>,
    pub parent: Option<ElementId>,
    pub children: Vec<ElementId>,
    pub interact: Cell<ElementState>,
}

pub struct ElementTree { entries: SlotMap<ElementId, ElementEntry>, root: Option<ElementId> }

impl ElementTree {
    pub fn new() -> Self { Self { entries: SlotMap::with_key(), root: None } }

    /// 从 ViewNode 创建 Element（容器变体的 children 被清空，由 tree 管理）
    pub fn create_from_node(&mut self, n: &ViewNode) -> ElementId {
        let node = without_children(n);
        self.entries.insert(ElementEntry {
            node,
            intrinsic: Cell::new(IntrinsicSize::zero()),
            layout: Cell::new(ComputedLayout::default()),
            dirty: Cell::new(true),
            parent: None,
            children: Vec::new(),
            interact: Cell::new(ElementState::default()),
        })
    }

    // ---- 兼容方法（基于 type_name）----
    pub fn type_name(&self, id: ElementId) -> Option<&'static str> {
        self.entries.get(id).map(|e| e.node.type_name())
    }
    pub fn get_node(&self, id: ElementId) -> ViewNode {
        self.entries.get(id).map(|e| e.node.clone()).unwrap_or(ViewNode::Divider { key: None })
    }
    pub fn on_click(&self, id: ElementId) -> Option<u64> {
        match self.entries.get(id)?.node.clone() {
            ViewNode::Button { on_click, .. } => on_click,
            ViewNode::Checkbox { on_click, .. } => on_click,
            _ => None,
        }
    }

    // ---- 交互状态 ----
    pub fn state(&self, id: ElementId) -> ElementState {
        self.entries.get(id).map(|e| e.interact.get()).unwrap_or_default()
    }
    pub fn set_state(&self, id: ElementId, st: ElementState) {
        if let Some(e) = self.entries.get(id) { e.interact.set(st); }
    }

    // ---- 树操作 ----
    pub fn set_root(&mut self, id: ElementId) { self.root = Some(id); }
    pub fn root(&self) -> Option<ElementId> { self.root }
    pub fn parent_of(&self, id: ElementId) -> Option<ElementId> { self.entries.get(id).and_then(|e| e.parent) }
    pub fn children_of(&self, id: ElementId) -> Vec<ElementId> { self.entries.get(id).map(|e| e.children.clone()).unwrap_or_default() }
    pub fn contains(&self, id: ElementId) -> bool { self.entries.contains_key(id) }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn set_intrinsic(&self, id: ElementId, i: IntrinsicSize) { if let Some(e) = self.entries.get(id) { e.intrinsic.set(i); } }
    pub fn intrinsic(&self, id: ElementId) -> IntrinsicSize { self.entries.get(id).map(|e| e.intrinsic.get()).unwrap_or(IntrinsicSize::zero()) }
    pub fn set_layout(&self, id: ElementId, l: ComputedLayout) { if let Some(e) = self.entries.get(id) { e.layout.set(l); } }
    pub fn layout(&self, id: ElementId) -> ComputedLayout { self.entries.get(id).map(|e| e.layout.get()).unwrap_or_default() }
    pub fn update_node(&self, id: ElementId, new: &ViewNode) {
        if let Some(e) = self.entries.get(id) {
            e.node = without_children(new);
            e.dirty.set(true);
        }
    }

    pub fn add_child(&mut self, pid: ElementId, cid: ElementId) {
        if let Some(c) = self.entries.get_mut(cid) { c.parent = Some(pid); }
        if let Some(p) = self.entries.get_mut(pid) { p.children.push(cid); }
    }
    pub fn insert_child(&mut self, pid: ElementId, pos: usize, cid: ElementId) {
        if let Some(c) = self.entries.get_mut(cid) { c.parent = Some(pid); }
        if let Some(p) = self.entries.get_mut(pid) { p.children.insert(pos.min(p.children.len()), cid); }
    }
    pub fn remove(&mut self, id: ElementId) -> bool {
        if !self.entries.contains_key(id) { return false; }
        let sub = self.collect_subtree(id);
        if let Some(e) = self.entries.get(id) {
            if let Some(pid) = e.parent {
                if let Some(p) = self.entries.get_mut(pid) { p.children.retain(|c| *c != id); }
            }
        }
        if self.root == Some(id) { self.root = None; }
        for rid in sub.into_iter().rev() { self.entries.remove(rid); }
        true
    }
    fn collect_subtree(&self, rid: ElementId) -> Vec<ElementId> {
        let mut ids = vec![rid]; if let Some(e) = self.entries.get(rid) { for c in &e.children { ids.extend(self.collect_subtree(*c)); } } ids
    }
    pub fn path_to(&self, tgt: ElementId) -> Vec<ElementId> {
        let mut p = Vec::new(); let mut c = tgt;
        while let Some(e) = self.entries.get(c) { p.push(c); c = match e.parent { Some(x) => x, None => break }; } p.reverse(); p
    }

    pub fn config_eq(&self, id: ElementId, other: &ViewNode) -> bool {
        self.entries.get(id).map(|e| e.node.config_eq(other)).unwrap_or(false)
    }
}
impl Default for ElementTree { fn default() -> Self { Self::new() } }

/// 从 ViewNode 克隆但清空 children（用于 ElementEntry 存储）
fn without_children(n: &ViewNode) -> ViewNode {
    match n {
        ViewNode::Column { justify, align, spacing, expand, key, .. } =>
            ViewNode::Column { justify: *justify, align: *align, spacing: *spacing, expand: *expand, key: key.clone(), children: vec![] },
        ViewNode::Row { justify, align, spacing, expand, key, .. } =>
            ViewNode::Row { justify: *justify, align: *align, spacing: *spacing, expand: *expand, key: key.clone(), children: vec![] },
        ViewNode::Container { expand, key, .. } =>
            ViewNode::Container { expand: *expand, key: key.clone(), children: vec![] },
        ViewNode::Custom { type_name, props, key, .. } =>
            ViewNode::Custom { type_name, props: props.clone(), key: key.clone(), children: vec![] },
        _ => n.clone(),
    }
}
