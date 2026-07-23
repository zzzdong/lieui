//! Layers — 三层架构 (Base / Overlay / Modal)
//!
//! 所有 Element 存在共享的 ElementTree 中，每层只维护自己的 root 和 LayoutContext。
//! 所有层均用 RefCell 包裹，使 EventContext（持有 &Layers）可直接修改各层。
use crate::core::ElementId;
use crate::event::EventManager;
use crate::geometry::Point;
use crate::layout::context::LayoutContext;
use crate::layout::node::LayoutNode;
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
    layout: LayoutContext,
}
impl LayerInfo {
    fn new() -> Self {
        Self {
            root: None,
            layout: LayoutContext::new(),
        }
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
        let mut overlay = self.overlay.borrow_mut();
        overlay.root = None;
        overlay.layout = LayoutContext::new();
    }

    pub fn show_modal(&self, root_id: ElementId) {
        self.modal.borrow_mut().root = Some(root_id);
    }

    pub fn hide_modal(&self) {
        let mut modal = self.modal.borrow_mut();
        modal.root = None;
        modal.layout = LayoutContext::new();
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

    pub fn with_layout<R>(&self, lt: LayerType, f: impl FnOnce(&LayoutContext) -> R) -> R {
        match lt {
            LayerType::Base => f(&self.base.borrow().layout),
            LayerType::Overlay => f(&self.overlay.borrow().layout),
            LayerType::Modal => f(&self.modal.borrow().layout),
        }
    }

    pub fn with_layout_mut<R>(&self, lt: LayerType, f: impl FnOnce(&mut LayoutContext) -> R) -> R {
        match lt {
            LayerType::Base => f(&mut self.base.borrow_mut().layout),
            LayerType::Overlay => f(&mut self.overlay.borrow_mut().layout),
            LayerType::Modal => f(&mut self.modal.borrow_mut().layout),
        }
    }

    pub fn set_layer_layout(&self, lt: LayerType, ctx: LayoutContext) {
        match lt {
            LayerType::Base => self.base.borrow_mut().layout = ctx,
            LayerType::Overlay => self.overlay.borrow_mut().layout = ctx,
            LayerType::Modal => self.modal.borrow_mut().layout = ctx,
        }
    }

    pub fn layer_layout_root(&self, lt: LayerType) -> Option<LayoutNode> {
        self.with_layout(lt, |l| l.root.clone())
    }



    // ========== 命中测试 ==========

    pub fn layer_hit_test(&self, lt: LayerType, point: Point) -> Option<ElementId> {
        self.with_layout(lt, |l| l.root.as_ref()?.hit_test_rec(point.x, point.y))
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
