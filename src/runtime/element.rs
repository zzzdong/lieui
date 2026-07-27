//! Element — 运行时实体及其树结构（含交互状态）
//! 每个 Element 存储其 ViewNode（不含孩子）用于类型安全访问

use crate::core::{ElementId, ElementState};
use crate::layout::box_model::{ComputedLayout, IntrinsicSize};
use crate::text::TextLayout;
use crate::view::node::{Listener, NodeType, ViewNode};
use crate::view::paint::TextStyle;
use slotmap::SlotMap;
use std::cell::{Cell, RefCell};
use std::sync::Arc;

/// 运行时元素条目：存储 ViewNode（不含孩子）+ 布局/交互状态
pub struct ElementEntry {
    pub node: ViewNode, // 类型安全的视图配置（孩子由 ElementTree.children 管理）
    pub intrinsic: Cell<IntrinsicSize>,
    pub layout: Cell<ComputedLayout>,
    pub dirty: Cell<bool>,
    pub parent: Option<ElementId>,
    pub children: Vec<ElementId>,
    pub interact: Cell<ElementState>,
    /// 滚动容器当前滚动偏移（内容画布相对视口的位移）。运行时状态，
    /// 不进入 Style/eq，故滚动不会触发 rebuild/重测量，仅驱动重排+重绘。
    pub scroll_offset: Cell<(f32, f32)>,
    /// 滚动容器内容尺寸（由布局阶段计算并写入，用于钳制滚动范围）。
    pub content_size: Cell<(f32, f32)>,
    /// 节点上附带的监听器列表（生命周期与 Element 绑定）
    pub listeners: RefCell<Vec<Listener>>,
    /// 文本布局缓存（Text 节点专用），update_node 时清除，render 时复用
    pub text_layout_cache: RefCell<Option<Arc<TextLayout>>>,
}

pub struct ElementTree {
    entries: SlotMap<ElementId, ElementEntry>,
    root: Option<ElementId>,
}

impl ElementTree {
    pub fn new() -> Self {
        Self {
            entries: SlotMap::with_key(),
            root: None,
        }
    }

    /// 从 ViewNode 创建 Element（容器变体的 children 被清空，由 tree 管理）
    pub fn create_from_node(&mut self, n: &ViewNode) -> ElementId {
        let node = without_children(n);
        let listeners = node.listeners().to_vec();
        self.entries.insert(ElementEntry {
            node,
            intrinsic: Cell::new(IntrinsicSize::zero()),
            layout: Cell::new(ComputedLayout::default()),
            dirty: Cell::new(true),
            parent: None,
            children: Vec::new(),
            interact: Cell::new(ElementState::default()),
            scroll_offset: Cell::new((0.0, 0.0)),
            content_size: Cell::new((0.0, 0.0)),
            listeners: RefCell::new(listeners),
            text_layout_cache: RefCell::new(None),
        })
    }

    // ---- 兼容方法（基于 type_name）----
    pub fn type_name(&self, id: ElementId) -> Option<&'static str> {
        self.entries.get(id).map(|e| e.node.type_name())
    }
    pub fn get_node(&self, id: ElementId) -> ViewNode {
        use crate::layout::style::FlexStyle;
        self.entries
            .get(id)
            .map(|e| e.node.clone())
            .unwrap_or(ViewNode::Text {
                content: String::new(),
                style: TextStyle {
                    font_size: 0.0,
                    color: crate::geometry::Color::TRANSPARENT,
                    ..TextStyle::default()
                },
                layout: FlexStyle::default(),
                key: None,
                listeners: Vec::new(),
            })
    }

    pub fn get_node_ref(&self, id: ElementId) -> Option<&ViewNode> {
        self.entries.get(id).map(|e| &e.node)
    }
    /// 返回节点上附带的监听器列表。
    pub fn listeners(&self, id: ElementId) -> Vec<Listener> {
        self.entries
            .get(id)
            .map(|e| e.listeners.borrow().clone())
            .unwrap_or_default()
    }

    /// 检查节点是否有关注的监听器（用于交互状态继承判断）。
    pub fn has_any_listener(&self, id: ElementId) -> bool {
        self.entries
            .get(id)
            .is_some_and(|e| !e.listeners.borrow().is_empty())
    }

    /// 检查节点是否监听 IME 事件。
    pub fn has_ime_listener(&self, id: ElementId) -> bool {
        self.entries.get(id).is_some_and(|e| {
            e.listeners.borrow().iter().any(|l| {
                matches!(
                    l.event,
                    crate::event::EventType::ImePreedit
                        | crate::event::EventType::ImeCommit
                        | crate::event::EventType::ImeDisabled
                )
            })
        })
    }

    /// 在直接子节点中查找指定 key 的节点。
    pub fn find_child_by_key(&self, id: ElementId, key: &str) -> Option<ElementId> {
        self.entries.get(id).and_then(|e| {
            e.children
                .iter()
                .find(|&&c| self.key_of(c) == Some(key))
                .copied()
        })
    }

    /// 获取节点的 key。
    pub fn key_of(&self, id: ElementId) -> Option<&str> {
        self.entries.get(id).and_then(|e| e.node.key())
    }

    // ---- 交互状态 ----
    pub fn state(&self, id: ElementId) -> ElementState {
        self.entries
            .get(id)
            .map(|e| e.interact.get())
            .unwrap_or_default()
    }
    pub fn set_state(&self, id: ElementId, st: ElementState) {
        if let Some(e) = self.entries.get(id) {
            e.interact.set(st);
        }
    }

    // ---- 树操作 ----
    pub fn set_root(&mut self, id: ElementId) {
        self.root = Some(id);
    }
    pub fn root(&self) -> Option<ElementId> {
        self.root
    }
    pub fn parent_of(&self, id: ElementId) -> Option<ElementId> {
        self.entries.get(id).and_then(|e| e.parent)
    }
    pub fn children_of(&self, id: ElementId) -> Vec<ElementId> {
        self.entries
            .get(id)
            .map(|e| e.children.clone())
            .unwrap_or_default()
    }
    /// 零拷贝访问子节点列表（热路径专用，避免 children_of 的 Vec 克隆）。
    pub fn children_ref(&self, id: ElementId) -> &[ElementId] {
        self.entries
            .get(id)
            .map(|e| e.children.as_slice())
            .unwrap_or(&[])
    }
    pub fn contains(&self, id: ElementId) -> bool {
        self.entries.contains_key(id)
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn set_intrinsic(&self, id: ElementId, i: IntrinsicSize) {
        if let Some(e) = self.entries.get(id) {
            e.intrinsic.set(i);
        }
    }
    pub fn intrinsic(&self, id: ElementId) -> IntrinsicSize {
        self.entries
            .get(id)
            .map(|e| e.intrinsic.get())
            .unwrap_or(IntrinsicSize::zero())
    }
    pub fn set_layout(&self, id: ElementId, l: ComputedLayout) {
        if let Some(e) = self.entries.get(id) {
            e.layout.set(l);
        }
    }
    pub fn layout(&self, id: ElementId) -> ComputedLayout {
        self.entries
            .get(id)
            .map(|e| e.layout.get())
            .unwrap_or_default()
    }

    // ---- 滚动状态（运行时，keyed by id）----

    /// 读取滚动容器当前滚动偏移（内容画布相对视口的位移）。
    pub fn scroll_offset(&self, id: ElementId) -> (f32, f32) {
        self.entries
            .get(id)
            .map(|e| e.scroll_offset.get())
            .unwrap_or((0.0, 0.0))
    }
    /// 写入滚动偏移。
    pub fn set_scroll_offset(&self, id: ElementId, v: (f32, f32)) {
        if let Some(e) = self.entries.get(id) {
            e.scroll_offset.set(v);
        }
    }
    /// 读取滚动容器内容尺寸（由布局阶段写入）。
    pub fn content_size(&self, id: ElementId) -> (f32, f32) {
        self.entries
            .get(id)
            .map(|e| e.content_size.get())
            .unwrap_or((0.0, 0.0))
    }
    /// 写入内容尺寸。
    pub fn set_content_size(&self, id: ElementId, v: (f32, f32)) {
        if let Some(e) = self.entries.get(id) {
            e.content_size.set(v);
        }
    }

    /// 读取滚动容器绑定的 scroll_state（如果存在）。
    /// 引擎在更新内部滚动偏移时会同步写入此 State。
    pub fn scroll_state(&self, id: ElementId) -> Option<crate::state::State<(f32, f32)>> {
        self.entries.get(id).and_then(|e| match &e.node {
            crate::view::node::ViewNode::Div { layout, .. } => layout.scroll_state.clone(),
            _ => None,
        })
    }
    pub fn update_node(&mut self, id: ElementId, new: &ViewNode) {
        if let Some(e) = self.entries.get_mut(id) {
            // 文本内容与样式未变时保留排版缓存（仅布局/监听器变化不影响排版）。
            let keep_text_cache = matches!(
                (&e.node, new),
                (
                    ViewNode::Text {
                        content: a,
                        style: sa,
                        ..
                    },
                    ViewNode::Text {
                        content: b,
                        style: sb,
                        ..
                    },
                ) if a == b && sa == sb
            );
            e.node = without_children(new);
            e.dirty.set(true);
            if !keep_text_cache {
                e.text_layout_cache = RefCell::new(None); // 清除文本布局缓存
            }
            *e.listeners.borrow_mut() = e.node.listeners().to_vec();
        }
    }

    /// 仅替换监听器回调，不标记 dirty、不清排版缓存。
    /// 用于 rebuild 后刷新闭包（闭包每次都是新建的，但不影响布局/绘制）。
    pub fn set_listeners(&mut self, id: ElementId, listeners: Vec<Listener>) {
        if let Some(e) = self.entries.get_mut(id) {
            match &mut e.node {
                ViewNode::Div { listeners: l, .. }
                | ViewNode::Text { listeners: l, .. }
                | ViewNode::Image { listeners: l, .. } => *l = listeners.clone(),
            }
            *e.listeners.borrow_mut() = listeners;
        }
    }

    pub fn add_child(&mut self, pid: ElementId, cid: ElementId) {
        if let Some(c) = self.entries.get_mut(cid) {
            c.parent = Some(pid);
        }
        if let Some(p) = self.entries.get_mut(pid) {
            p.children.push(cid);
        }
    }
    pub fn insert_child(&mut self, pid: ElementId, pos: usize, cid: ElementId) {
        if let Some(c) = self.entries.get_mut(cid) {
            c.parent = Some(pid);
        }
        if let Some(p) = self.entries.get_mut(pid) {
            p.children.insert(pos.min(p.children.len()), cid);
        }
    }
    /// 将已有节点移动到目标父节点的指定位置，复用 Element 实例。
    /// 仅从原父节点的 children 列表中移除，不删除子树 entries。
    pub fn move_child(&mut self, id: ElementId, new_parent: ElementId, position: usize) {
        if let Some(old_parent) = self.parent_of(id) {
            if let Some(p) = self.entries.get_mut(old_parent) {
                p.children.retain(|c| *c != id);
            }
        }
        if let Some(c) = self.entries.get_mut(id) {
            c.parent = Some(new_parent);
            c.dirty.set(true);
        }
        if let Some(p) = self.entries.get_mut(new_parent) {
            p.children.insert(position.min(p.children.len()), id);
        }
    }
    pub fn remove(&mut self, id: ElementId) -> bool {
        if !self.entries.contains_key(id) {
            return false;
        }
        let sub = self.collect_subtree(id);
        if let Some(e) = self.entries.get(id) {
            if let Some(pid) = e.parent {
                if let Some(p) = self.entries.get_mut(pid) {
                    p.children.retain(|c| *c != id);
                }
            }
        }
        if self.root == Some(id) {
            self.root = None;
        }
        for rid in sub.iter().rev().copied() {
            self.entries.remove(rid);
        }
        true
    }
    fn collect_subtree(&self, rid: ElementId) -> Vec<ElementId> {
        let mut ids = vec![rid];
        if let Some(e) = self.entries.get(rid) {
            for c in &e.children {
                ids.extend(self.collect_subtree(*c));
            }
        }
        ids
    }
    pub fn path_to(&self, tgt: ElementId) -> Vec<ElementId> {
        let mut p = Vec::new();
        let mut c = tgt;
        while let Some(e) = self.entries.get(c) {
            p.push(c);
            c = match e.parent {
                Some(x) => x,
                None => break,
            };
        }
        p.reverse();
        p
    }

    pub fn config_eq(&self, id: ElementId, other: &ViewNode) -> bool {
        self.entries
            .get(id)
            .map(|e| e.node.config_eq(other))
            .unwrap_or(false)
    }

    // ---- 布局辅助 ----

    /// 获取节点类型
    pub fn node_type_of(&self, id: ElementId) -> Option<NodeType> {
        self.entries.get(id).map(|e| e.node.node_type())
    }

    /// 检查 dirty 标记
    pub fn is_dirty(&self, id: ElementId) -> bool {
        self.entries.get(id).map(|e| e.dirty.get()).unwrap_or(false)
    }

    /// 递归检查子树是否有 dirty 节点
    pub fn subtree_has_dirty(&self, id: ElementId) -> bool {
        if self.is_dirty(id) {
            return true;
        }
        self.children_ref(id)
            .iter()
            .any(|c| self.subtree_has_dirty(*c))
    }

    /// 检查整棵树是否存在 dirty 节点（遍历所有 entries，O(n)）。
    pub fn has_dirty_node(&self) -> bool {
        self.entries.iter().any(|(_, e)| e.dirty.get())
    }

    /// 将所有节点标记为 dirty（例如 viewport 变化时需要全量重排）。
    pub fn mark_dirty_all(&mut self) {
        for (_, e) in self.entries.iter_mut() {
            e.dirty.set(true);
        }
    }

    /// 清除所有 dirty 标记（布局计算完成后调用）。
    pub fn clear_dirty(&mut self) {
        for (_, e) in self.entries.iter_mut() {
            e.dirty.set(false);
        }
    }

    // ---- 文本布局缓存 ----

    /// 读取文本布局缓存（Text 节点专用）。
    /// 仅克隆返回给调用方使用，**不移除**缓存，避免每帧重建渲染树时
    /// take→clone→set 的额外开销（parley::Layout 在 debug 下 clone 极贵）。
    pub fn peek_text_layout_cache(&self, id: ElementId) -> Option<Arc<TextLayout>> {
        self.entries
            .get(id)
            .and_then(|e| e.text_layout_cache.borrow().clone())
    }

    /// 设置文本布局缓存
    pub fn set_text_layout_cache(&self, id: ElementId, layout: Arc<TextLayout>) {
        if let Some(e) = self.entries.get(id) {
            *e.text_layout_cache.borrow_mut() = Some(layout);
        }
    }
}
impl Default for ElementTree {
    fn default() -> Self {
        Self::new()
    }
}

/// 从 ViewNode 克隆但清空 children（用于 ElementEntry 存储）
fn without_children(n: &ViewNode) -> ViewNode {
    match n {
        ViewNode::Div {
            layout,
            paint,
            key,
            listeners,
            ..
        } => ViewNode::Div {
            layout: layout.clone(),
            paint: paint.clone(),
            key: key.clone(),
            children: vec![],
            listeners: listeners.clone(),
        },
        ViewNode::Text {
            content,
            style,
            layout,
            key,
            listeners,
        } => ViewNode::Text {
            content: content.clone(),
            style: style.clone(),
            layout: layout.clone(),
            key: key.clone(),
            listeners: listeners.clone(),
        },
        ViewNode::Image {
            data,
            style,
            layout,
            key,
            listeners,
        } => ViewNode::Image {
            data: std::sync::Arc::clone(data),
            style: *style,
            layout: layout.clone(),
            key: key.clone(),
            listeners: listeners.clone(),
        },
    }
}
