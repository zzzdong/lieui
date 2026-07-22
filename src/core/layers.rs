//! Layers — 三层架构 (Base/Overlay/Modal)
use std::cell::RefCell;
use crate::core::ElementId;
use crate::event::EventManager;
use crate::layout::context::LayoutContext;
use crate::runtime::element::ElementTree;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerType { Base, Overlay, Modal }
impl LayerType {
    pub fn z_index(&self) -> i32 { match self { LayerType::Base => 0, LayerType::Overlay => 1000, LayerType::Modal => 2000 } }
}

struct LayerInfo { root: Option<ElementId>, layout: LayoutContext }
impl LayerInfo { fn new() -> Self { Self { root: None, layout: LayoutContext::new() } } }

pub struct Layers {
    pub tree: ElementTree,
    base: RefCell<LayerInfo>,
    _overlay: RefCell<LayerInfo>,
    _modal: RefCell<LayerInfo>,
    pub event_manager: RefCell<EventManager>,
}
impl Layers {
    pub fn new() -> Self { Self { tree: ElementTree::new(), base: RefCell::new(LayerInfo::new()), _overlay: RefCell::new(LayerInfo::new()), _modal: RefCell::new(LayerInfo::new()), event_manager: RefCell::new(EventManager::new()) } }
    pub fn set_base_root(&mut self, id: ElementId) { self.base.borrow_mut().root = Some(id); }
    pub fn layer_root(&self, lt: LayerType) -> Option<ElementId> {
        match lt { LayerType::Base => self.base.borrow().root, _ => None }
    }
    pub fn with_layout<R>(&self, lt: LayerType, f: impl FnOnce(&LayoutContext) -> R) -> R {
        match lt { LayerType::Base => f(&self.base.borrow().layout), _ => unreachable!() }
    }
    pub fn with_layout_mut<R>(&self, lt: LayerType, f: impl FnOnce(&mut LayoutContext) -> R) -> R {
        match lt { LayerType::Base => f(&mut self.base.borrow_mut().layout), _ => unreachable!() }
    }
}
impl Default for Layers { fn default() -> Self { Self::new() } }
