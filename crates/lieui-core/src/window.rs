//! 窗口状态与 P2 LAYOUT 阶段
//!
//! ## 重排边界（M1 任务 1）
//! 属性变化 → `mark_layout_dirty(node)` 沿树向上，止步于最近的**重排边界**。
//! 边界的定义：自身尺寸不依赖后代——即窗口根，或宽高都已确定的节点。
//! P2 只重算这些边界子树，因此「改一个文本节点」不会触发整窗重排。
//!
//! 边界子树重算需要两个上一帧的量（都记在 `LayoutStore` 里）：
//! - `parent_origin` —— 父内容盒原点，用于还原子树坐标系；
//! - `avail` —— 父级给的可用宽高，用于复现完全相同的父约束（百分比尺寸也正确）。

use lieui_layout::{LayoutEngine, LayoutStats, VALUE_UNDEFINED, viewport_avail};
use lieui_text::TextService;

use crate::id::{ElementTypeId, NodeId, WindowId};
use crate::layout::{LayoutHost, LayoutStore};
use crate::props::keys as K;
use crate::props::{PropColumn, PropertyStore};
use crate::tree::{ItemKey, Tree};

/// 单帧最多跑几轮布局（第二轮只用于修正首帧的百分比尺寸）
const MAX_LAYOUT_ROUNDS: usize = 2;

pub struct Window {
    pub id: WindowId,
    pub tree: Tree,
    pub props: PropertyStore,
    pub layout: LayoutStore,
    engine: LayoutEngine,
    viewport: (f32, f32),
    /// 待重排的边界（P2 消费）
    boundaries: Vec<NodeId>,
}

impl Window {
    pub fn new(id: WindowId, viewport: (f32, f32)) -> Self {
        let mut w = Self {
            id,
            tree: Tree::new(),
            props: PropertyStore::new(),
            layout: LayoutStore::default(),
            engine: LayoutEngine::new(),
            viewport,
            boundaries: Vec::new(),
        };
        w.boundaries.push(w.tree.root());
        w
    }

    #[inline]
    pub fn root(&self) -> NodeId {
        self.tree.root()
    }

    #[inline]
    pub fn stats(&self) -> LayoutStats {
        self.engine.stats()
    }

    pub fn reset_stats(&mut self) {
        self.engine.reset_stats();
    }

    pub fn set_viewport(&mut self, w: f32, h: f32) {
        if self.viewport != (w, h) {
            self.viewport = (w, h);
            self.boundaries.push(self.tree.root());
        }
    }
    pub fn viewport(&self) -> (f32, f32) {
        self.viewport
    }

    /// 待重排边界数（诊断 / 测试用）
    pub fn pending_boundaries(&self) -> usize {
        self.boundaries.len()
    }

    /// 树中存活节点数（示例 / 诊断用）
    pub fn tree_node_count(&self) -> usize {
        self.tree.len()
    }

    // ── 结构 ──

    pub fn create_node(
        &mut self,
        parent: NodeId,
        type_id: ElementTypeId,
        key: Option<ItemKey>,
    ) -> NodeId {
        let n = self.tree.create(type_id, key);
        self.tree.append_child(parent, n);
        self.update_boundary(n);
        self.mark_layout_dirty(n);
        n
    }

    pub fn destroy_subtree(&mut self, node: NodeId) {
        let all: Vec<NodeId> = self.tree.descendants(node).collect();
        for n in &all {
            self.props.destroy_node(*n);
            self.layout.remove(*n);
        }
        self.tree.detach(node);
        self.tree.destroy_subtree(node);
        // 父容器尺寸可能因此变化
        if let Some(parent) = all
            .first()
            .and_then(|n| self.tree.get(*n).and_then(|x| x.parent))
        {
            self.mark_layout_dirty(parent);
        }
    }

    // ── 属性 ──

    /// 写属性（LOCAL 层），并按需触发重排 / 重绘标记。
    pub fn set_prop<T: PropColumn>(
        &mut self,
        node: NodeId,
        key: crate::props::PropKey<T>,
        value: T,
    ) {
        self.props.set(&mut self.tree, node, key, value);
        self.after_prop_change(node, key.slot(), key.flags());
    }

    fn after_prop_change(&mut self, node: NodeId, slot: u16, flags: crate::props::PropFlags) {
        // 宽高变化 → 重排边界资格可能变化
        if slot == K::WIDTH.slot() || slot == K::HEIGHT.slot() {
            self.update_boundary(node);
        }
        if flags.contains(crate::props::PropFlags::AFFECT_LAYOUT) {
            self.mark_layout_dirty(node);
        }
    }

    /// 重算节点的「是否为重排边界」
    pub fn update_boundary(&mut self, node: NodeId) {
        let w = self.props.resolve(&self.tree, node, K::WIDTH.id()).as_dim();
        let h = self
            .props
            .resolve(&self.tree, node, K::HEIGHT.id())
            .as_dim();
        let is_root = self.tree.get(node).is_some_and(|n| n.parent.is_none());
        let definite = w.is_some_and(|d| d.is_definite()) && h.is_some_and(|d| d.is_definite());
        if let Some(n) = self.tree.get_mut(node) {
            n.is_layout_boundary = is_root || definite;
        }
    }

    /// 标记重排：向上冒泡直到最近的边界
    pub fn mark_layout_dirty(&mut self, node: NodeId) {
        let mut cur = Some(node);
        while let Some(n) = cur {
            if !self.tree.is_alive(n) {
                break;
            }
            let is_boundary = self.tree.get(n).is_none_or(|x| x.is_layout_boundary);
            if is_boundary {
                if !self.boundaries.contains(&n) {
                    self.boundaries.push(n);
                }
                return;
            }
            if let Some(x) = self.tree.get_mut(n) {
                x.layout_dirty = true;
            }
            cur = self.tree.get(n).and_then(|x| x.parent);
        }
        // 兜底（理论上不可达：根恒为边界）
        let root = self.tree.root();
        if !self.boundaries.contains(&root) {
            self.boundaries.push(root);
        }
    }

    // ── P2 LAYOUT ──

    /// 执行布局阶段。消费 `boundaries`，把结果写进 `LayoutStore`。
    pub fn run_layout(&mut self, text: &mut TextService) {
        if self.boundaries.is_empty() {
            return;
        }
        self.normalize_boundaries();
        let mut queue = std::mem::take(&mut self.boundaries);

        for _ in 0..MAX_LAYOUT_ROUNDS {
            let mut retry = Vec::new();
            for b in queue.drain(..) {
                if !self.layout_boundary(text, b) {
                    // 百分比缺少历史可用尺寸，下一轮用本轮结果修正
                    retry.push(b);
                }
            }
            if retry.is_empty() {
                break;
            }
            queue = retry;
        }
    }

    /// 单个边界子树的布局。返回 false 表示需要再跑一轮。
    fn layout_boundary(&mut self, text: &mut TextService, boundary: NodeId) -> bool {
        if !self.tree.is_alive(boundary) {
            return true;
        }
        let root = self.tree.root();
        let (origin, avail) = if boundary == root {
            ((0.0, 0.0), viewport_avail(self.viewport.0, self.viewport.1))
        } else {
            match self.layout.get(boundary) {
                Some(l) => (l.parent_origin(), (l.avail_w, l.avail_h)),
                // 尚未布局过（新节点）→ 按内容自然尺寸先算一轮
                None => ((0.0, 0.0), (VALUE_UNDEFINED, VALUE_UNDEFINED)),
            }
        };

        // ★ 先把结果拷成自有数据：host 还借着 `self.layout`（供百分比解析），
        //   必须等它释放后才能写回 LayoutStore。
        let (unresolved, entries) = {
            let mut host = LayoutHost::new(
                &self.tree,
                &mut self.props,
                &mut *text,
                &self.layout,
                self.viewport,
            );
            let out = self
                .engine
                .layout(&mut host, boundary.to_u64(), avail, origin);
            let entries: Vec<(u64, lieui_layout::ComputedLayout)> = out.iter().collect();
            (host.unresolved_percent, entries)
        };
        for (id, l) in entries {
            self.layout.set(id, l);
        }
        !unresolved
    }

    /// 归一化边界列表：去重 + 去掉被祖先边界覆盖的后代 + 父先于子
    fn normalize_boundaries(&mut self) {
        let mut list = std::mem::take(&mut self.boundaries);
        list.retain(|n| self.tree.is_alive(*n));
        list.sort_by_key(|n| self.tree.depth(*n));
        let mut out: Vec<NodeId> = Vec::with_capacity(list.len());
        for b in list {
            let covered = out.iter().any(|o| self.is_ancestor(*o, b));
            if !covered && !out.contains(&b) {
                out.push(b);
            }
        }
        self.boundaries = out;
    }

    fn is_ancestor(&self, ancestor: NodeId, node: NodeId) -> bool {
        self.tree.ancestors(node).any(|x| x == ancestor)
    }

    // ── 查询 ──

    /// 节点的布局结果（未布局过时返回 None）
    pub fn layout_of(&self, node: NodeId) -> Option<lieui_layout::ComputedLayout> {
        self.layout.get(node)
    }

    /// 命中测试：从后往前找包含点的最深节点
    pub fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        let mut hit = None;
        for node in self.tree.descendants(self.tree.root()) {
            if let Some(l) = self.layout.get(node)
                && x >= l.x
                && x <= l.x + l.width
                && y >= l.y
                && y <= l.y + l.height
            {
                hit = Some(node);
            }
        }
        hit
    }
}
