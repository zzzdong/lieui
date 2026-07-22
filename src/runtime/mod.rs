//! Runtime — v2 核心"服务端"
pub mod element;
pub mod reconciler;
use crate::core::layers::{LayerType, Layers};
use crate::geometry::Size;
use crate::layout::context::LayoutContext;
use crate::render::visual::LayeredElement;
use crate::runtime::reconciler::Reconciler;
use crate::state;
use crate::view::node::ViewNode;
pub use element::ElementTree;

pub struct Runtime {
    pub layers: Layers,
    pub(crate) viewport: Size,
    needs_layout: bool,
    needs_render: bool,
    pending_view_tree: Option<ViewNode>,
    pub debug_stats: DebugStats,
}
#[derive(Debug, Default)]
pub struct DebugStats {
    pub reconciler: crate::runtime::reconciler::ReconcilerStats,
    pub element_count: usize,
}
impl Runtime {
    pub fn new(viewport: Size) -> Self {
        Self {
            layers: Layers::new(),
            viewport,
            needs_layout: true,
            needs_render: true,
            pending_view_tree: None,
            debug_stats: DebugStats::default(),
        }
    }
    pub fn set_viewport(&mut self, vp: Size) {
        self.viewport = vp;
        self.needs_layout = true;
        self.needs_render = true;
    }
    pub fn submit_view_tree(&mut self, vt: ViewNode) {
        self.pending_view_tree = Some(vt);
        self.needs_render = true;
    }

    pub fn frame(&mut self) -> Vec<LayeredElement> {
        if state::take_rebuild_requested() || self.pending_view_tree.is_some() {
            if let Some(vt) = self.pending_view_tree.take() {
                let mut r = Reconciler::new();
                if self.layers.layer_root(LayerType::Base).is_none() {
                    let id = self.layers.tree.create_from_node(&vt);
                    self.layers.tree.set_root(id);
                    self.layers.set_base_root(id);
                    let mut p = Vec::new();
                    r.diff(&vt, id, &self.layers.tree, &mut p);
                    r.apply(p, &mut self.layers.tree);
                } else {
                    let rid = self.layers.layer_root(LayerType::Base).unwrap();
                    let mut p = Vec::new();
                    r.diff(&vt, rid, &self.layers.tree, &mut p);
                    r.apply(p, &mut self.layers.tree);
                    self.debug_stats.reconciler = r.stats;
                }
                self.needs_layout = true;
            }
        }

        // 处理 Modal / Overlay 的显示/隐藏请求
        if let Some(pending) = state::take_pending_modal() {
            match pending {
                Some(view) => {
                    let id = self.layers.tree.create_from_node(&view);
                    self.layers.show_modal(id);
                }
                None => {
                    if let Some(rid) = self.layers.layer_root(LayerType::Modal) {
                        self.layers.tree.remove(rid);
                    }
                    self.layers.hide_modal();
                }
            }
            self.needs_layout = true;
        }
        if let Some(pending) = state::take_pending_overlay() {
            match pending {
                Some(view) => {
                    let id = self.layers.tree.create_from_node(&view);
                    self.layers.show_overlay(id);
                }
                None => {
                    if let Some(rid) = self.layers.layer_root(LayerType::Overlay) {
                        self.layers.tree.remove(rid);
                    }
                    self.layers.hide_overlay();
                }
            }
            self.needs_layout = true;
        }
        if self.needs_layout {
            self.perform_layout();
            self.needs_layout = false;
        }
        if self.needs_render {
            self.needs_render = false;
            self.debug_stats.element_count = self.layers.tree.len();
            self.build_render_tree()
        } else {
            Vec::new()
        }
    }

    /// 仅重新生成渲染树（不跑 builder/reconciliation/layout）
    /// 用于 hover/pressed 等视觉交互更新
    pub fn frame_render_only(&self) -> Vec<LayeredElement> {
        self.build_render_tree()
    }

    fn perform_layout(&mut self) {
        for lt in [LayerType::Base, LayerType::Overlay, LayerType::Modal] {
            if let Some(rid) = self.layers.layer_root(lt) {
                let mut ctx = LayoutContext::new();
                ctx.collect(rid, &self.layers.tree);
                ctx.compute(self.viewport);
                self.layers.set_layer_layout(lt, ctx);
            } else {
                self.layers.set_layer_layout(lt, LayoutContext::new());
            }
        }
    }

    fn build_render_tree(&self) -> Vec<LayeredElement> {
        let mut e = Vec::new();
        for lt in [LayerType::Base, LayerType::Overlay, LayerType::Modal] {
            let z = lt.z_index();
            self.layers.with_layout(lt, |l| {
                if let Some(ref r) = l.root {
                    Self::cv(
                        r,
                        z,
                        &self.layers,
                        &mut e,
                        crate::core::state::ElementState::default(),
                    );
                }
            });
        }
        e.sort_by_key(|x| x.z_index);
        e
    }

    fn cv(
        node: &crate::layout::node::LayoutNode,
        z_index: i32,
        layers: &Layers,
        elements: &mut Vec<LayeredElement>,
        inherited_state: crate::core::state::ElementState,
    ) {
        use crate::view::node::NodeType;
        let id = node.id;
        let node_ref = layers.tree.get_node(id);
        // Listener 作为交互边界，使用并传播自身状态；其他节点继承父状态
        let state = if node.node_type == NodeType::Listener {
            layers.tree.state(id)
        } else {
            inherited_state
        };
        node_ref.render(&node.computed, state, elements, z_index, id);

        for child in &node.children {
            Self::cv(child, z_index, layers, elements, state);
        }
    }
}
