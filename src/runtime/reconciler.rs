//! Reconciler — ViewTree ↔ ElementTree diff 引擎

use crate::core::ElementId;
use crate::runtime::element::ElementTree;
use crate::view::node::{PropMap, PropValue, ViewNode};

#[derive(Debug)]
pub enum Patch {
    Create { parent: ElementId, position: usize, node: ViewNode },
    Update { id: ElementId, props: PropMap },
    Remove { id: ElementId },
}

#[derive(Debug, Default)]
pub struct ReconcilerStats { pub created: usize, pub updated: usize, pub removed: usize }

pub struct Reconciler { pub stats: ReconcilerStats }

impl Reconciler {
    pub fn new() -> Self { Self { stats: ReconcilerStats::default() } }

    pub fn diff(&mut self, view_node: &ViewNode, parent_id: ElementId, tree: &ElementTree, patches: &mut Vec<Patch>) {
        let existing = tree.children_of(parent_id);
        let mut matched = Vec::new();
        for (pos, child_node) in view_node.children.iter().enumerate() {
            match self.find_match(child_node, &existing, tree) {
                Ok(id) => {
                    matched.push(id);
                    if !props_equal(&child_node.props, tree.props(id).unwrap_or(&PropMap::new())) {
                        patches.push(Patch::Update { id, props: child_node.props.clone() });
                        self.stats.updated += 1;
                    }
                    self.diff(child_node, id, tree, patches);
                }
                Err(_) => {
                    patches.push(Patch::Create { parent: parent_id, position: pos, node: child_node.clone() });
                    self.stats.created += 1;
                }
            }
        }
        for child_id in &existing {
            if !matched.contains(child_id) && tree.contains(*child_id) {
                patches.push(Patch::Remove { id: *child_id });
                self.stats.removed += 1;
            }
        }
    }

    fn find_match(&self, view_node: &ViewNode, existing: &[ElementId], tree: &ElementTree) -> Result<ElementId, ()> {
        if let Some(ref key) = view_node.key {
            for id in existing {
                if let Some(ek) = tree.key(*id) { if ek == *key { return Ok(*id); } }
            }
        }
        for id in existing {
            if let Some(tn) = tree.type_name(*id) { if tn == view_node.type_name { return Ok(*id); } }
        }
        Err(())
    }

    pub fn apply(&mut self, patches: Vec<Patch>, tree: &mut ElementTree) {
        for patch in patches {
            match patch {
                Patch::Create { parent, position, node } => {
                    let id = tree.create_from_node(&node);
                    tree.insert_child(parent, position, id);
                    self.apply_children(id, &node, tree);
                }
                Patch::Update { id, props } => { tree.set_props(id, props); }
                Patch::Remove { id } => { tree.remove(id); }
            }
        }
    }

    fn apply_children(&mut self, parent_id: ElementId, node: &ViewNode, tree: &mut ElementTree) {
        for child in &node.children {
            let id = tree.create_from_node(child);
            tree.add_child(parent_id, id);
            self.apply_children(id, child, tree);
        }
    }
}

fn props_equal(a: &PropMap, b: &PropMap) -> bool {
    if a.len() != b.len() { return false; }
    for (k, v) in a.iter() {
        match b.get(k) {
            Some(bv) => {
                match (v, bv) {
                    (PropValue::Str(a), PropValue::Str(b)) => if a != b { return false; },
                    (PropValue::F32(a), PropValue::F32(b)) => if (a - b).abs() > 0.001 { return false; },
                    (PropValue::F64(a), PropValue::F64(b)) => if (a - b).abs() > 0.001 { return false; },
                    _ => if std::mem::discriminant(v) != std::mem::discriminant(bv) { return false; },
                }
            }
            None => return false,
        }
    }
    true
}
