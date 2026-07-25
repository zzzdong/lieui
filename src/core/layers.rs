//! Layers — 三层架构 (Base / Overlay / Modal)
//!
//! 所有 Element 存在共享的 ElementTree 中，每层只维护自己的 root。
//! 布局结果直接保存在 ElementEntry::layout 中，命中测试也直接遍历 ElementTree。
//! 所有层均用 RefCell 包裹，使 EventContext（持有 &Layers）可直接修改各层。
use crate::core::ElementId;
use crate::event::EventManager;
use crate::geometry::Point;
use crate::runtime::element::ElementTree;
use std::cell::RefCell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerType {
    Base,
    Overlay,
    Modal,
}

impl LayerType {
    pub fn z_index(&self) -> i32 {
        match self {
            LayerType::Base => 0,
            LayerType::Overlay => 1000,
            LayerType::Modal => 2000,
        }
    }

    /// 事件派发顺序：从高 z 到低 z
    pub fn dispatch_order() -> [LayerType; 3] {
        [LayerType::Modal, LayerType::Overlay, LayerType::Base]
    }
}

struct LayerInfo {
    root: Option<ElementId>,
}
impl LayerInfo {
    fn new() -> Self {
        Self { root: None }
    }
}

pub struct Layers {
    pub tree: ElementTree,
    base: RefCell<LayerInfo>,
    overlay: RefCell<LayerInfo>,
    modal: RefCell<LayerInfo>,
    pub event_manager: RefCell<EventManager>,
}
impl Layers {
    pub fn new() -> Self {
        Self {
            tree: ElementTree::new(),
            base: RefCell::new(LayerInfo::new()),
            overlay: RefCell::new(LayerInfo::new()),
            modal: RefCell::new(LayerInfo::new()),
            event_manager: RefCell::new(EventManager::new()),
        }
    }

    // ========== 各层 root 设置 ==========

    pub fn set_base_root(&mut self, id: ElementId) {
        self.base.borrow_mut().root = Some(id);
    }

    pub fn show_overlay(&self, root_id: ElementId) {
        self.overlay.borrow_mut().root = Some(root_id);
    }

    pub fn hide_overlay(&self) {
        self.overlay.borrow_mut().root = None;
    }

    pub fn show_modal(&self, root_id: ElementId) {
        self.modal.borrow_mut().root = Some(root_id);
    }

    pub fn hide_modal(&self) {
        self.modal.borrow_mut().root = None;
    }

    // ========== 查询 ==========

    pub fn layer_root(&self, lt: LayerType) -> Option<ElementId> {
        match lt {
            LayerType::Base => self.base.borrow().root,
            LayerType::Overlay => self.overlay.borrow().root,
            LayerType::Modal => self.modal.borrow().root,
        }
    }

    pub fn layer_has_content(&self, lt: LayerType) -> bool {
        self.layer_root(lt).is_some()
    }

    // ========== 命中测试 ==========

    pub fn layer_hit_test(&self, lt: LayerType, point: Point) -> Option<ElementId> {
        self.layer_root(lt)
            .and_then(|rid| Self::hit_test_rec(&self.tree, rid, point.x, point.y))
    }

    fn hit_test_rec(tree: &ElementTree, id: ElementId, px: f32, py: f32) -> Option<ElementId> {
        let layout = tree.layout(id);
        if !layout.contains(px, py) {
            return None;
        }
        // 优先命中更内层的节点，以支持嵌套监听（例如行可点击，行内的 Checkbox 也可点击）。
        for cid in tree.children_of(id).iter().rev() {
            if let Some(hit) = Self::hit_test_rec(tree, *cid, px, py) {
                return Some(hit);
            }
        }
        Some(id)
    }

    /// 跨所有层做命中测试，返回命中的层与元素（按 dispatch_order 优先高 z）
    pub fn hit_test_top(&self, point: Point) -> Option<(LayerType, ElementId)> {
        for lt in LayerType::dispatch_order() {
            if let Some(id) = self.layer_hit_test(lt, point) {
                return Some((lt, id));
            }
        }
        None
    }

    // ========== 跨层查找 ==========

    pub fn path_to(&self, target: ElementId) -> Vec<ElementId> {
        self.tree.path_to(target)
    }
}
impl Default for Layers {
    fn default() -> Self {
        Self::new()
    }
}
