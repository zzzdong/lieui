//! 节点树 —— 保留式（retained），基于 sibling 指针，遍历零分配
//!
//! ★ 不提供 `query_selector`：禁止字符串全局查询（ADR-1）。
//! 调试/测试遍历走 `#[cfg(debug_assertions)]` 的调试树。

use crate::arena::GenerationalArena;
use crate::id::{ElementTypeId, NodeId};

bitflags::bitflags! {
    /// 节点位标志
    #[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
    pub struct NodeFlags: u32 {
        const VISIBLE        = 1 << 0;
        /// 继承缓存需重解析
        const INHERIT_DIRTY  = 1 << 1;
        const LAYOUT_DIRTY   = 1 << 2;
        const PAINT_DIRTY    = 1 << 3;
        const NEEDS_A11Y     = 1 << 4;
        const FOCUSABLE      = 1 << 5;
        const CLIPS          = 1 << 6;
        /// 处于回收池中（虚拟化列表）
        const RECYCLED       = 1 << 7;
        /// 该节点已挂载到树上
        const MOUNTED        = 1 << 8;
    }
}

impl NodeFlags {
    /// 新建节点的初始标志
    pub const INITIAL: Self = Self::VISIBLE
        .union(Self::LAYOUT_DIRTY)
        .union(Self::PAINT_DIRTY)
        .union(Self::INHERIT_DIRTY);
}

/// 伪状态集合（hover / active / focus / disabled …），供样式 states 查表
pub type PseudoClassSet = u32;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ItemKey {
    Str(std::sync::Arc<str>),
    I64(i64),
    U64(u64),
}

impl From<&str> for ItemKey {
    fn from(s: &str) -> Self {
        ItemKey::Str(s.into())
    }
}
impl From<String> for ItemKey {
    fn from(s: String) -> Self {
        ItemKey::Str(s.into())
    }
}
impl From<i64> for ItemKey {
    fn from(v: i64) -> Self {
        ItemKey::I64(v)
    }
}
impl From<u64> for ItemKey {
    fn from(v: u64) -> Self {
        ItemKey::U64(v)
    }
}

pub struct Node {
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub prev_sibling: Option<NodeId>,
    pub next_sibling: Option<NodeId>,

    pub type_id: ElementTypeId,
    pub key: Option<ItemKey>,
    pub child_count: u32,

    pub flags: NodeFlags,
    /// 可继承属性写入时自增，供继承路径压缩比对
    pub value_epoch: u32,
    pub pseudo: PseudoClassSet,

    pub layout_dirty: bool,
    /// 是否为重排边界：自身尺寸不依赖后代 → 子树重排不必波及祖先
    pub is_layout_boundary: bool,
}

pub struct Tree {
    nodes: GenerationalArena<Node>,
    root: NodeId,
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    pub fn new() -> Self {
        let mut nodes: GenerationalArena<Node> = GenerationalArena::with_capacity(64);
        let (idx, generation) = nodes.insert(Node {
            parent: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
            type_id: ElementTypeId::ROOT,
            key: None,
            child_count: 0,
            flags: NodeFlags::INITIAL | NodeFlags::MOUNTED,
            value_epoch: 0,
            pseudo: 0,
            layout_dirty: true,
            is_layout_boundary: true, // 根恒为边界
        });
        let root = NodeId::from_parts(idx, generation);
        Self { nodes, root }
    }

    #[inline]
    pub fn root(&self) -> NodeId {
        self.root
    }

    #[inline]
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.index(), id.generation())
    }

    #[inline]
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id.index(), id.generation())
    }

    /// 未存活或空句柄返回 None
    #[inline]
    pub fn is_alive(&self, id: NodeId) -> bool {
        self.nodes.is_alive(id.index(), id.generation())
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.len() == 0
    }

    // ── 结构修改 ──

    /// 创建节点；`parent = None` 表示尚未挂载（稍后 append_child）
    pub fn create(&mut self, type_id: ElementTypeId, key: Option<ItemKey>) -> NodeId {
        let (idx, generation) = self.nodes.insert(Node {
            parent: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
            type_id,
            key,
            child_count: 0,
            flags: NodeFlags::INITIAL,
            value_epoch: 0,
            pseudo: 0,
            layout_dirty: true,
            is_layout_boundary: false,
        });
        NodeId::from_parts(idx, generation)
    }

    /// 追加到 `parent` 的子节点末尾
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        if parent == child {
            return;
        }
        self.detach(child);
        // ★ 必须在改写 last_child 之前取旧尾节点
        let prev_last = self.get(parent).and_then(|p| p.last_child);
        if let Some(p) = self.get_mut(parent) {
            p.child_count += 1;
            if p.first_child.is_none() {
                p.first_child = Some(child);
            }
            p.last_child = Some(child);
        }
        if let Some(prev) = prev_last
            && let Some(pv) = self.get_mut(prev)
        {
            pv.next_sibling = Some(child);
        }
        if let Some(c) = self.get_mut(child) {
            c.parent = Some(parent);
            c.prev_sibling = prev_last;
            c.next_sibling = None;
            c.flags |= NodeFlags::MOUNTED;
        }
    }

    /// 从父节点摘下（不销毁），用于移动
    pub fn detach(&mut self, node: NodeId) {
        let (parent, prev, next) = match self.get(node) {
            Some(n) => (n.parent, n.prev_sibling, n.next_sibling),
            None => return,
        };
        if let Some(p) = parent
            && let Some(pn) = self.get_mut(p)
        {
            if pn.first_child == Some(node) {
                pn.first_child = next;
            }
            if pn.last_child == Some(node) {
                pn.last_child = prev;
            }
            if pn.child_count > 0 {
                pn.child_count -= 1;
            }
        }
        if let Some(prev) = prev
            && let Some(pv) = self.get_mut(prev)
        {
            pv.next_sibling = next;
        }
        if let Some(next) = next
            && let Some(nx) = self.get_mut(next)
        {
            nx.prev_sibling = prev;
        }
        if let Some(n) = self.get_mut(node) {
            n.parent = None;
            n.prev_sibling = None;
            n.next_sibling = None;
            n.flags &= !NodeFlags::MOUNTED;
        }
    }

    /// 一次性重排 `parent` 的子节点顺序。`children` 必须恰好是当前子节点集合。
    pub fn set_children(&mut self, parent: NodeId, children: &[NodeId]) {
        // 先全部摘下，再按序挂回
        let old: Vec<NodeId> = self.children(parent).collect();
        for c in old {
            self.detach(c);
        }
        for c in children {
            self.append_child(parent, *c);
        }
    }

    /// 递归销毁子树（不含 `node` 自身从父节点的摘除）
    pub fn destroy_subtree(&mut self, node: NodeId) -> usize {
        let kids: Vec<NodeId> = self.children(node).collect();
        let mut n = 0;
        for k in kids {
            n += self.destroy_subtree(k);
        }
        self.nodes.remove(node.index(), node.generation());
        n + 1
    }

    // ── 遍历 ──

    pub fn children(&self, node: NodeId) -> ChildIter<'_> {
        ChildIter {
            tree: self,
            next: self.get(node).and_then(|n| n.first_child),
        }
    }

    /// 从自身到根
    pub fn ancestors(&self, node: NodeId) -> AncestorIter<'_> {
        AncestorIter {
            tree: self,
            cur: Some(node),
        }
    }

    /// 深度优先，含自身
    pub fn descendants(&self, node: NodeId) -> DescendantIter<'_> {
        DescendantIter::new(self, node)
    }

    /// 节点深度（根 = 0）
    pub fn depth(&self, node: NodeId) -> u32 {
        self.ancestors(node).count() as u32 - 1
    }
}

pub struct ChildIter<'a> {
    tree: &'a Tree,
    next: Option<NodeId>,
}

impl Iterator for ChildIter<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let cur = self.next?;
        self.next = self.tree.get(cur).and_then(|n| n.next_sibling);
        Some(cur)
    }
}

pub struct AncestorIter<'a> {
    tree: &'a Tree,
    cur: Option<NodeId>,
}

impl Iterator for AncestorIter<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let cur = self.cur?;
        self.cur = self.tree.get(cur).and_then(|n| n.parent);
        Some(cur)
    }
}

pub struct DescendantIter<'a> {
    tree: &'a Tree,
    stack: Vec<NodeId>,
}

impl<'a> DescendantIter<'a> {
    fn new(tree: &'a Tree, root: NodeId) -> Self {
        Self {
            tree,
            stack: vec![root],
        }
    }
}

impl Iterator for DescendantIter<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let cur = self.stack.pop()?;
        // 逆序压栈以保持正序输出
        let kids: Vec<NodeId> = self.tree.children(cur).collect();
        for k in kids.into_iter().rev() {
            self.stack.push(k);
        }
        Some(cur)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_traverse() {
        let mut t = Tree::new();
        let a = t.create(ElementTypeId::BOX, None);
        let b = t.create(ElementTypeId::BOX, None);
        let c = t.create(ElementTypeId::BOX, None);
        t.append_child(t.root(), a);
        t.append_child(a, b);
        t.append_child(a, c);

        assert_eq!(t.children(a).collect::<Vec<_>>(), vec![b, c]);
        assert_eq!(t.descendants(t.root()).collect::<Vec<_>>().len(), 4);
        assert_eq!(t.ancestors(c).collect::<Vec<_>>(), vec![c, a, t.root()]);
        assert_eq!(t.depth(c), 2);
    }

    #[test]
    fn detach_relinks_siblings() {
        let mut t = Tree::new();
        let (a, b, c) = (
            t.create(ElementTypeId::BOX, None),
            t.create(ElementTypeId::BOX, None),
            t.create(ElementTypeId::BOX, None),
        );
        t.append_child(t.root(), a);
        t.append_child(t.root(), b);
        t.append_child(t.root(), c);
        t.detach(b);
        assert_eq!(t.children(t.root()).collect::<Vec<_>>(), vec![a, c]);
        assert_eq!(t.get(a).unwrap().next_sibling, Some(c));
        assert_eq!(t.get(c).unwrap().prev_sibling, Some(a));
        assert_eq!(t.get(t.root()).unwrap().child_count, 2);
    }

    #[test]
    fn destroy_invalidates_handles() {
        let mut t = Tree::new();
        let a = t.create(ElementTypeId::BOX, None);
        t.append_child(t.root(), a);
        assert!(t.is_alive(a));
        let n = t.destroy_subtree(a);
        assert_eq!(n, 1);
        assert!(!t.is_alive(a));
        assert!(t.get(a).is_none());
    }

    #[test]
    fn set_children_reorders() {
        let mut t = Tree::new();
        let (a, b, c) = (
            t.create(ElementTypeId::BOX, None),
            t.create(ElementTypeId::BOX, None),
            t.create(ElementTypeId::BOX, None),
        );
        for x in [a, b, c] {
            t.append_child(t.root(), x);
        }
        t.set_children(t.root(), &[c, a, b]);
        assert_eq!(t.children(t.root()).collect::<Vec<_>>(), vec![c, a, b]);
    }
}
