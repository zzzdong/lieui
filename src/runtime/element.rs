//! Element — 运行时实体及其树结构（含交互状态）
//! 每个 Element 存储其 ViewNode（不含孩子）用于类型安全访问

use crate::core::{ElementId, ElementState};
use crate::layout::box_model::{ComputedLayout, IntrinsicSize};
use crate::text::TextLayout;
use crate::view::node::{LayoutStyle, NodeType, ViewNode};
use slotmap::SlotMap;
use std::cell::{Cell, RefCell};

/// 运行时元素条目：存储 ViewNode（不含孩子）+ 布局/交互状态
pub struct ElementEntry {
    pub node: ViewNode, // 类型安全的视图配置（孩子由 ElementTree.children 管理）
    pub intrinsic: Cell<IntrinsicSize>,
    pub layout: Cell<ComputedLayout>,
    pub dirty: Cell<bool>,
    pub parent: Option<ElementId>,
    pub children: Vec<ElementId>,
    pub interact: Cell<ElementState>,
    /// 文本布局缓存（Text 节点专用），update_node 时清除，render 时复用
    pub text_layout_cache: RefCell<Option<Box<TextLayout>>>,
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
        self.entries.insert(ElementEntry {
            node,
            intrinsic: Cell::new(IntrinsicSize::zero()),
            layout: Cell::new(ComputedLayout::default()),
            dirty: Cell::new(true),
            parent: None,
            children: Vec::new(),
            interact: Cell::new(ElementState::default()),
            text_layout_cache: RefCell::new(None),
        })
    }

    // ---- 兼容方法（基于 type_name）----
    pub fn type_name(&self, id: ElementId) -> Option<&'static str> {
        self.entries.get(id).map(|e| e.node.type_name())
    }
    pub fn get_node(&self, id: ElementId) -> ViewNode {
        self.entries
            .get(id)
            .map(|e| e.node.clone())
            .unwrap_or(ViewNode::Text {
                content: String::new(),
                font_size: 0.0,
                color: crate::geometry::Color::TRANSPARENT,
                key: None,
                listener: None,
            })
    }

    pub fn get_node_ref(&self, id: ElementId) -> Option<&ViewNode> {
        self.entries.get(id).map(|e| &e.node)
    }
    pub fn on_click(&self, id: ElementId) -> Option<u64> {
        self.entries.get(id)?.node.on_click_id()
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
    pub fn update_node(&mut self, id: ElementId, new: &ViewNode) {
        if let Some(e) = self.entries.get_mut(id) {
            e.node = without_children(new);
            e.dirty.set(true);
            e.text_layout_cache = RefCell::new(None); // 清除文本布局缓存
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
        for rid in sub.into_iter().rev() {
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

    /// 快速获取节点的布局样式（通过 ViewNode.layout_style() 提取）
    pub fn layout_style(&self, id: ElementId) -> LayoutStyle {
        self.entries
            .get(id)
            .map(|e| e.node.layout_style())
            .unwrap_or_default()
    }

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
        self.children_of(id)
            .iter()
            .any(|c| self.subtree_has_dirty(*c))
    }

    // ---- 文本布局缓存 ----

    /// 读取文本布局缓存（Text 节点专用）。
    /// 仅克隆返回给调用方使用，**不移除**缓存，避免每帧重建渲染树时
    /// take→clone→set 的额外开销（parley::Layout 在 debug 下 clone 极贵）。
    pub fn peek_text_layout_cache(&self, id: ElementId) -> Option<Box<TextLayout>> {
        self.entries
            .get(id)
            .and_then(|e| e.text_layout_cache.borrow().clone())
    }

    /// 设置文本布局缓存
    pub fn set_text_layout_cache(&self, id: ElementId, layout: Box<TextLayout>) {
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
            style,
            flex,
            display,
            key,
            listener,
            ..
        } => ViewNode::Div {
            style: style.clone(),
            flex: flex.clone(),
            display: *display,
            key: key.clone(),
            children: vec![],
            listener: *listener,
        },
        ViewNode::Text {
            content,
            font_size,
            color,
            key,
            listener,
        } => ViewNode::Text {
            content: content.clone(),
            font_size: *font_size,
            color: *color,
            key: key.clone(),
            listener: *listener,
        },
        ViewNode::Image {
            data,
            w,
            h,
            key,
            listener,
        } => ViewNode::Image {
            data: std::sync::Arc::clone(data),
            w: *w,
            h: *h,
            key: key.clone(),
            listener: *listener,
        },
        ViewNode::Canvas { key, listener } => ViewNode::Canvas {
            key: key.clone(),
            listener: *listener,
        },
    }
}
