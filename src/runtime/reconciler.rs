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
    /// 将已有 Element 移动到目标父节点下的指定位置。
    /// 用于复用已有节点实例（保留交互状态），仅调整树结构。
    Move {
        id: ElementId,
        parent: ElementId,
        position: usize,
    },
    /// 仅刷新监听器回调（builder 每次 rebuild 都会新建闭包）。
    /// 不标记 dirty、不清排版缓存，避免回调指针变化触发全量重排。
    UpdateListeners {
        id: ElementId,
        listeners: Vec<crate::view::node::Listener>,
    },
}

#[derive(Debug, Default)]
pub struct ReconcilerStats {
    pub created: usize,
    pub updated: usize,
    pub removed: usize,
    pub moved: usize,
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
        let existing = tree.children_ref(parent_id);
        let mut matched = Vec::new();
        let mut used = std::collections::HashSet::new();

        // 预建 key / type 索引，将子节点匹配从 O(n^2) 降到 O(n)。
        let mut by_key: std::collections::HashMap<String, Vec<ElementId>> =
            std::collections::HashMap::new();
        let mut by_type: std::collections::HashMap<&'static str, Vec<ElementId>> =
            std::collections::HashMap::new();
        for &id in existing {
            if let Some(node) = tree.get_node_ref(id) {
                if let Some(key) = node.key() {
                    by_key.entry(key.to_owned()).or_default().push(id);
                }
                by_type.entry(node.type_name()).or_default().push(id);
            }
        }

        for (pos, child_node) in view_node.children().iter().enumerate() {
            match self.find_match(child_node, pos, existing, &used, &by_key, &by_type, tree) {
                Ok(id) => {
                    used.insert(id);
                    matched.push(id);
                    // 若匹配到的节点当前不在期望位置，生成 Move 补丁以复用实例。
                    if let Some(cur_pos) = existing.iter().position(|c| *c == id) {
                        if cur_pos != pos {
                            patches.push(Patch::Move {
                                id,
                                parent: parent_id,
                                position: pos,
                            });
                            self.stats.moved += 1;
                        }
                    }
                    if !tree.config_eq(id, child_node) {
                        patches.push(Patch::Update {
                            id,
                            node: child_node.clone(),
                        });
                        self.stats.updated += 1;
                    } else if !child_node.listeners().is_empty() {
                        // config 未变但闭包是新建的：只换回调，不触发重排。
                        patches.push(Patch::UpdateListeners {
                            id,
                            listeners: child_node.listeners().to_vec(),
                        });
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
        for child_id in existing {
            if !matched.contains(child_id) && tree.contains(*child_id) {
                patches.push(Patch::Remove { id: *child_id });
                self.stats.removed += 1;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn find_match(
        &self,
        view_node: &ViewNode,
        position: usize,
        existing: &[ElementId],
        used: &std::collections::HashSet<ElementId>,
        by_key: &std::collections::HashMap<String, Vec<ElementId>>,
        by_type: &std::collections::HashMap<&'static str, Vec<ElementId>>,
        tree: &ElementTree,
    ) -> Result<ElementId, ()> {
        // 1. 优先按 key 匹配
        if let Some(key) = view_node.key() {
            if let Some(candidates) = by_key.get(key) {
                for id in candidates {
                    if !used.contains(id) {
                        return Ok(*id);
                    }
                }
            }
        }
        // 2. 位置优化：同位置 type_name 匹配，避免尾部追加时重排
        if position < existing.len()
            && !used.contains(&existing[position])
            && tree.type_name(existing[position]) == Some(view_node.type_name())
        {
            return Ok(existing[position]);
        }
        // 3. key 未命中时按 type_name 回退（跨序查找）
        if let Some(candidates) = by_type.get(view_node.type_name()) {
            for id in candidates {
                if !used.contains(id) {
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
                Patch::Move {
                    id,
                    parent,
                    position,
                } => {
                    tree.move_child(id, parent, position);
                }
                Patch::UpdateListeners { id, listeners } => {
                    tree.set_listeners(id, listeners);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Color;
    use crate::layout::style::FlexStyle;
    use crate::view::node::ViewNode;
    use crate::view::paint::{PaintStyle, TextStyle};

    fn div(children: Vec<ViewNode>) -> ViewNode {
        ViewNode::Div {
            layout: FlexStyle::default(),
            paint: PaintStyle::default(),
            key: None,
            children,
            listeners: Vec::new(),
        }
    }

    fn text_with_key(content: &str, key: &str) -> ViewNode {
        ViewNode::Text {
            content: content.to_string(),
            style: TextStyle::default(),
            layout: FlexStyle::default(),
            key: Some(key.to_string()),
            listeners: Vec::new(),
        }
    }

    fn text_with_key_and_color(content: &str, key: &str, color: Color) -> ViewNode {
        ViewNode::Text {
            content: content.to_string(),
            style: TextStyle {
                color,
                ..TextStyle::default()
            },
            layout: FlexStyle::default(),
            key: Some(key.to_string()),
            listeners: Vec::new(),
        }
    }

    fn build_tree(view: &ViewNode) -> (ElementTree, ElementId) {
        let mut tree = ElementTree::new();
        let root = tree.create_from_node(view);
        tree.set_root(root);
        let mut r = Reconciler::new();
        let mut patches = Vec::new();
        r.diff(view, root, &tree, &mut patches);
        r.apply(patches, &mut tree);
        (tree, root)
    }

    #[test]
    fn move_reorders_children_without_recreate() {
        let old = div(vec![
            text_with_key("a", "a"),
            text_with_key("b", "b"),
            text_with_key("c", "c"),
        ]);
        let new = div(vec![
            text_with_key("c", "c"),
            text_with_key("a", "a"),
            text_with_key("b", "b"),
        ]);

        let (mut tree, root) = build_tree(&old);
        let before: Vec<ElementId> = tree.children_of(root);

        let mut r = Reconciler::new();
        let mut patches = Vec::new();
        r.diff(&new, root, &tree, &mut patches);
        assert_eq!(patches.len(), 3, "expected 3 Move patches");
        assert!(patches.iter().all(|p| matches!(p, Patch::Move { .. })));
        assert_eq!(r.stats.moved, 3);
        assert_eq!(r.stats.created, 0);
        assert_eq!(r.stats.removed, 0);
        assert_eq!(r.stats.updated, 0);

        r.apply(patches, &mut tree);
        let after: Vec<ElementId> = tree.children_of(root);
        assert_eq!(after.len(), 3);
        // 顺序应变为 c, a, b，且 id 复用。
        assert_eq!(after[0], before[2]);
        assert_eq!(after[1], before[0]);
        assert_eq!(after[2], before[1]);
        // 集合相同，无重建。
        let mut before_sorted = before.clone();
        let mut after_sorted = after.clone();
        before_sorted.sort();
        after_sorted.sort();
        assert_eq!(before_sorted, after_sorted);
    }

    #[test]
    fn move_combined_with_update() {
        let old = div(vec![
            text_with_key_and_color("a", "a", Color::RED),
            text_with_key_and_color("b", "b", Color::new(0, 0, 255)),
        ]);
        let new = div(vec![
            text_with_key_and_color("b", "b", Color::new(0, 255, 0)),
            text_with_key_and_color("a", "a", Color::RED),
        ]);

        let (mut tree, root) = build_tree(&old);
        let before = tree.children_of(root);

        let mut r = Reconciler::new();
        let mut patches = Vec::new();
        r.diff(&new, root, &tree, &mut patches);
        assert_eq!(r.stats.moved, 2);
        assert_eq!(r.stats.updated, 1);
        assert_eq!(r.stats.created, 0);
        assert_eq!(r.stats.removed, 0);

        r.apply(patches, &mut tree);
        let after = tree.children_of(root);
        assert_eq!(after.len(), 2);
        assert_eq!(after[0], before[1]); // b 移到首位
        assert_eq!(after[1], before[0]); // a 移到位1
    }

    #[test]
    fn no_move_when_order_unchanged() {
        let old = div(vec![text_with_key("a", "a"), text_with_key("b", "b")]);
        let new = div(vec![text_with_key("a", "a"), text_with_key("b", "b")]);

        let (tree, root) = build_tree(&old);
        let mut r = Reconciler::new();
        let mut patches = Vec::new();
        r.diff(&new, root, &tree, &mut patches);
        assert!(patches.iter().all(|p| !matches!(p, Patch::Move { .. })));
        assert_eq!(r.stats.moved, 0);
    }
}
