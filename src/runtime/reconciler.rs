//! Reconciler — ViewTree ↔ ElementTree diff 引擎

use crate::core::ElementId;
use crate::runtime::element::ElementTree;
use crate::view::node::ViewNode;

#[derive(Debug)]
pub enum Patch {
    Create {
        parent: ElementId,
        position: usize,
        node: ViewNode,
    },
    Update {
        id: ElementId,
        node: ViewNode,
    },
    Remove {
        id: ElementId,
    },
}

#[derive(Debug, Default)]
pub struct ReconcilerStats {
    pub created: usize,
    pub updated: usize,
    pub removed: usize,
}

pub struct Reconciler {
    pub stats: ReconcilerStats,
}

impl Default for Reconciler {
    fn default() -> Self {
        Self::new()
    }
}

impl Reconciler {
    pub fn new() -> Self {
        Self {
            stats: ReconcilerStats::default(),
        }
    }

    pub fn diff(
        &mut self,
        view_node: &ViewNode,
        parent_id: ElementId,
        tree: &ElementTree,
        patches: &mut Vec<Patch>,
    ) {
        let existing = tree.children_of(parent_id);
        let mut matched = Vec::new();
        let mut used = std::collections::HashSet::new();
        for (pos, child_node) in view_node.children().iter().enumerate() {
            match self.find_match(child_node, &existing, &used, tree) {
                Ok(id) => {
                    used.insert(id);
                    matched.push(id);
                    if !tree.config_eq(id, child_node) {
                        patches.push(Patch::Update {
                            id,
                            node: child_node.clone(),
                        });
                        self.stats.updated += 1;
                    }
                    self.diff(child_node, id, tree, patches);
                }
                Err(_) => {
                    patches.push(Patch::Create {
                        parent: parent_id,
                        position: pos,
                        node: child_node.clone(),
                    });
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

    fn find_match(
        &self,
        view_node: &ViewNode,
        existing: &[ElementId],
        used: &std::collections::HashSet<ElementId>,
        tree: &ElementTree,
    ) -> Result<ElementId, ()> {
        // 1. 优先按 key 匹配
        if let Some(key) = view_node.key() {
            for id in existing {
                if used.contains(id) {
                    continue;
                }
                if let Some(node) = tree.get_node_ref(*id) {
                    if node.key() == Some(key) {
                        return Ok(*id);
                    }
                }
            }
        }
        // 2. key 未命中时按 type_name 回退
        for id in existing {
            if used.contains(id) {
                continue;
            }
            if let Some(tn) = tree.type_name(*id) {
                if tn == view_node.type_name() {
                    return Ok(*id);
                }
            }
        }
        Err(())
    }

    pub fn apply(&mut self, patches: Vec<Patch>, tree: &mut ElementTree) {
        for patch in patches {
            match patch {
                Patch::Create {
                    parent,
                    position,
                    node,
                } => {
                    let id = tree.create_from_node(&node);
                    tree.insert_child(parent, position, id);
                    self.apply_children(id, &node, tree);
                }
                Patch::Update { id, node } => {
                    tree.update_node(id, &node);
                }
                Patch::Remove { id } => {
                    tree.remove(id);
                }
            }
        }
    }

    fn apply_children(&mut self, parent_id: ElementId, node: &ViewNode, tree: &mut ElementTree) {
        for child in node.children() {
            let id = tree.create_from_node(child);
            tree.add_child(parent_id, id);
            self.apply_children(id, child, tree);
        }
    }
}
