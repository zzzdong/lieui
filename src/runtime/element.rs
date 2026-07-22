//! Element — 运行时实体及其树结构

use std::cell::Cell;
use slotmap::SlotMap;
use crate::core::ElementId;
use crate::layout::box_model::{ComputedLayout, IntrinsicSize};
use crate::view::node::{PropMap, ViewNode};

pub struct ElementEntry {
    pub type_name: &'static str,
    pub props: PropMap,
    pub intrinsic: Cell<IntrinsicSize>,
    pub layout: Cell<ComputedLayout>,
    pub dirty: Cell<bool>,
    pub parent: Option<ElementId>,
    pub children: Vec<ElementId>,
    pub key: Option<String>,
}

pub struct ElementTree {
    entries: SlotMap<ElementId, ElementEntry>,
    root: Option<ElementId>,
}

impl ElementTree {
    pub fn new() -> Self {
        Self { entries: SlotMap::with_key(), root: None }
    }

    pub fn create(&mut self, type_name: &'static str, props: PropMap) -> ElementId {
        self.entries.insert(ElementEntry {
            type_name, props, intrinsic: Cell::new(IntrinsicSize::zero()),
            layout: Cell::new(ComputedLayout::default()), dirty: Cell::new(true),
            parent: None, children: Vec::new(), key: None,
        })
    }

    pub fn create_from_node(&mut self, node: &ViewNode) -> ElementId {
        self.entries.insert(ElementEntry {
            type_name: node.type_name, props: node.props.clone(),
            intrinsic: Cell::new(IntrinsicSize::zero()),
            layout: Cell::new(ComputedLayout::default()), dirty: Cell::new(true),
            parent: None, children: Vec::new(), key: node.key.clone(),
        })
    }

    pub fn set_root(&mut self, id: ElementId) { self.root = Some(id); }
    pub fn root(&self) -> Option<ElementId> { self.root }
    pub fn parent_of(&self, id: ElementId) -> Option<ElementId> {
        self.entries.get(id).and_then(|e| e.parent)
    }
    pub fn children_of(&self, id: ElementId) -> Vec<ElementId> {
        self.entries.get(id).map(|e| e.children.clone()).unwrap_or_default()
    }
    pub fn type_name(&self, id: ElementId) -> Option<&'static str> {
        self.entries.get(id).map(|e| e.type_name)
    }
    pub fn props(&self, id: ElementId) -> Option<&PropMap> {
        self.entries.get(id).map(|e| &e.props)
    }
    pub fn set_props(&mut self, id: ElementId, props: PropMap) {
        if let Some(e) = self.entries.get_mut(id) { e.props = props; e.dirty.set(true); }
    }
    pub fn mark_dirty(&self, id: ElementId) {
        if let Some(e) = self.entries.get(id) { e.dirty.set(true); }
    }
    pub fn is_dirty(&self, id: ElementId) -> bool {
        self.entries.get(id).map(|e| e.dirty.get()).unwrap_or(false)
    }
    pub fn set_intrinsic(&self, id: ElementId, i: IntrinsicSize) {
        if let Some(e) = self.entries.get(id) { e.intrinsic.set(i); }
    }
    pub fn intrinsic(&self, id: ElementId) -> IntrinsicSize {
        self.entries.get(id).map(|e| e.intrinsic.get()).unwrap_or(IntrinsicSize::zero())
    }
    pub fn set_layout(&self, id: ElementId, l: ComputedLayout) {
        if let Some(e) = self.entries.get(id) { e.layout.set(l); }
    }
    pub fn layout(&self, id: ElementId) -> ComputedLayout {
        self.entries.get(id).map(|e| e.layout.get()).unwrap_or_default()
    }
    pub fn contains(&self, id: ElementId) -> bool { self.entries.contains_key(id) }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn key(&self, id: ElementId) -> Option<String> {
        self.entries.get(id).and_then(|e| e.key.clone())
    }
    pub fn add_child(&mut self, parent_id: ElementId, child_id: ElementId) {
        if let Some(c) = self.entries.get_mut(child_id) { c.parent = Some(parent_id); }
        if let Some(p) = self.entries.get_mut(parent_id) { p.children.push(child_id); }
    }
    pub fn insert_child(&mut self, parent_id: ElementId, position: usize, child_id: ElementId) {
        if let Some(c) = self.entries.get_mut(child_id) { c.parent = Some(parent_id); }
        if let Some(p) = self.entries.get_mut(parent_id) {
            let pos = position.min(p.children.len());
            p.children.insert(pos, child_id);
        }
    }
    pub fn remove(&mut self, id: ElementId) -> bool {
        if !self.entries.contains_key(id) { return false; }
        let subtree = self.collect_subtree(id);
        if let Some(e) = self.entries.get(id) {
            if let Some(pid) = e.parent {
                if let Some(p) = self.entries.get_mut(pid) { p.children.retain(|c| *c != id); }
            }
        }
        if self.root == Some(id) { self.root = None; }
        for rid in subtree.into_iter().rev() { self.entries.remove(rid); }
        true
    }
    fn collect_subtree(&self, root_id: ElementId) -> Vec<ElementId> {
        let mut ids = vec![root_id];
        if let Some(e) = self.entries.get(root_id) {
            for cid in &e.children { ids.extend(self.collect_subtree(*cid)); }
        }
        ids
    }
    pub fn path_to(&self, target_id: ElementId) -> Vec<ElementId> {
        let mut path = Vec::new();
        let mut current = target_id;
        while let Some(e) = self.entries.get(current) {
            path.push(current);
            current = match e.parent { Some(p) => p, None => break };
        }
        path.reverse();
        path
    }
    pub fn traverse<F: FnMut(ElementId, &ElementEntry)>(&self, mut f: F) {
        if let Some(root_id) = self.root { self.traverse_rec(root_id, &mut f); }
    }
    fn traverse_rec<F: FnMut(ElementId, &ElementEntry)>(&self, id: ElementId, f: &mut F) {
        if let Some(e) = self.entries.get(id) {
            f(id, e);
            for cid in &e.children { self.traverse_rec(*cid, f); }
        }
    }
}
impl Default for ElementTree { fn default() -> Self { Self::new() } }
