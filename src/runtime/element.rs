//! Element — 运行时实体及其树结构（含交互状态）

use std::cell::Cell;
use slotmap::SlotMap;
use crate::core::ElementId;
use crate::layout::box_model::{ComputedLayout, IntrinsicSize};
use crate::view::node::{PropMap, ViewNode};

/// 元素交互状态（由鼠标事件更新）
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ElementState { pub hovered: bool, pub pressed: bool, pub focused: bool }

pub struct ElementEntry {
    pub type_name: &'static str,
    pub props: PropMap,
    pub intrinsic: Cell<IntrinsicSize>,
    pub layout: Cell<ComputedLayout>,
    pub dirty: Cell<bool>,
    pub parent: Option<ElementId>,
    pub children: Vec<ElementId>,
    pub key: Option<String>,
    pub interact: Cell<ElementState>,
}

pub struct ElementTree { entries: SlotMap<ElementId, ElementEntry>, root: Option<ElementId> }

impl ElementTree {
    pub fn new() -> Self { Self { entries: SlotMap::with_key(), root: None } }

    pub fn create(&mut self, tn: &'static str, props: PropMap) -> ElementId {
        self.entries.insert(ElementEntry { type_name: tn, props, intrinsic: Cell::new(IntrinsicSize::zero()),
            layout: Cell::new(ComputedLayout::default()), dirty: Cell::new(true),
            parent: None, children: Vec::new(), key: None, interact: Cell::new(ElementState::default()) })
    }
    pub fn create_from_node(&mut self, n: &ViewNode) -> ElementId {
        self.entries.insert(ElementEntry { type_name: n.type_name, props: n.props.clone(),
            intrinsic: Cell::new(IntrinsicSize::zero()), layout: Cell::new(ComputedLayout::default()),
            dirty: Cell::new(true), parent: None, children: Vec::new(), key: n.key.clone(),
            interact: Cell::new(ElementState::default()) })
    }

    // ---- 交互状态 ----
    pub fn state(&self, id: ElementId) -> ElementState { self.entries.get(id).map(|e| e.interact.get()).unwrap_or_default() }
    pub fn set_state(&self, id: ElementId, st: ElementState) { if let Some(e) = self.entries.get(id) { e.interact.set(st); } }

    // ---- 原有方法（剪短以节约token） ----
    pub fn set_root(&mut self, id: ElementId) { self.root = Some(id); }
    pub fn root(&self) -> Option<ElementId> { self.root }
    pub fn parent_of(&self, id: ElementId) -> Option<ElementId> { self.entries.get(id).and_then(|e| e.parent) }
    pub fn children_of(&self, id: ElementId) -> Vec<ElementId> { self.entries.get(id).map(|e| e.children.clone()).unwrap_or_default() }
    pub fn type_name(&self, id: ElementId) -> Option<&'static str> { self.entries.get(id).map(|e| e.type_name) }
    pub fn props(&self, id: ElementId) -> Option<&PropMap> { self.entries.get(id).map(|e| &e.props) }
    pub fn set_props(&mut self, id: ElementId, p: PropMap) { if let Some(e) = self.entries.get_mut(id) { e.props = p; e.dirty.set(true); } }
    pub fn mark_dirty(&self, id: ElementId) { if let Some(e) = self.entries.get(id) { e.dirty.set(true); } }
    pub fn is_dirty(&self, id: ElementId) -> bool { self.entries.get(id).map(|e| e.dirty.get()).unwrap_or(false) }
    pub fn set_intrinsic(&self, id: ElementId, i: IntrinsicSize) { if let Some(e) = self.entries.get(id) { e.intrinsic.set(i); } }
    pub fn intrinsic(&self, id: ElementId) -> IntrinsicSize { self.entries.get(id).map(|e| e.intrinsic.get()).unwrap_or(IntrinsicSize::zero()) }
    pub fn set_layout(&self, id: ElementId, l: ComputedLayout) { if let Some(e) = self.entries.get(id) { e.layout.set(l); } }
    pub fn layout(&self, id: ElementId) -> ComputedLayout { self.entries.get(id).map(|e| e.layout.get()).unwrap_or_default() }
    pub fn contains(&self, id: ElementId) -> bool { self.entries.contains_key(id) }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn key(&self, id: ElementId) -> Option<String> { self.entries.get(id).and_then(|e| e.key.clone()) }
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
        if let Some(e) = self.entries.get(id) { if let Some(pid) = e.parent { if let Some(p) = self.entries.get_mut(pid) { p.children.retain(|c| *c != id); } } }
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
    pub fn traverse<F: FnMut(ElementId, &ElementEntry)>(&self, mut f: F) {
        if let Some(r) = self.root { self.tr(r, &mut f); }
    }
    fn tr<F: FnMut(ElementId, &ElementEntry)>(&self, id: ElementId, f: &mut F) {
        if let Some(e) = self.entries.get(id) { f(id, e); for c in &e.children { self.tr(*c, f); } }
    }
}
impl Default for ElementTree { fn default() -> Self { Self::new() } }
