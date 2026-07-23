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
            // 调试：LIEUI_DUMP_LAYOUT=1 时打印布局树
            if cfg!(debug_assertions) {
                use std::sync::OnceLock;
                static DUMPED: OnceLock<bool> = OnceLock::new();
                if DUMPED.set(true).is_ok() {
                    eprintln!("=== LIEUI Layout Tree ===");
                    for lt in [LayerType::Base, LayerType::Overlay, LayerType::Modal] {
                        let label = format!("{:?}", lt);
                        self.layers.with_layout(lt, |ctx| {
                            if ctx.root.is_some() {
                                eprintln!("--- Layer: {} ---", label);
                                ctx.dump_tree();
                            }
                        });
                    }
                    // 打印 ElementTree 父子关系
                    eprintln!("--- ElementTree parent-child ---");
                    if let Some(rid) = self.layers.layer_root(LayerType::Base) {
                        fn dump_tree_et(tree: &element::ElementTree, id: crate::core::ElementId, depth: usize) {
                            let indent = "  ".repeat(depth);
                            let nt = tree.get_node_ref(id).map(|n| n.node_type_name()).unwrap_or("?");
                            eprintln!("{}{:?} [{}] < {}", indent, id, nt, tree.parent_of(id).map(|p| format!("{:?}", p)).unwrap_or("root".into()));
                            for cid in tree.children_of(id) {
                                dump_tree_et(tree, cid, depth + 1);
                            }
                        }
                        dump_tree_et(&self.layers.tree, rid, 0);
                    }
                }
            }
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
                // 通过 with_layout 获取上帧的 LayoutContext 引用，用于增量复用
                self.layers.with_layout(lt, |prev| {
                    ctx.collect(rid, &self.layers.tree, prev.root.as_ref());
                });
                ctx.compute(self.viewport, &self.layers.tree);
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
        let node_ref = match layers.tree.get_node_ref(id) {
            Some(n) => n,
            None => return,
        };
        let state = if node.node_type == NodeType::Listener {
            layers.tree.state(id)
        } else {
            inherited_state
        };

        // 在 cv 中内联渲染（替代 ViewNode::render() 间接调用）
        if let crate::view::node::ViewNode::Text {
            content,
            font_size,
            color,
            ..
        } = node_ref
        {
            let r = node.computed.rect();
            // 尝试从缓存复用 TextLayout，避免重复布局
            let lay = match layers.tree.text_layout_cache(id) {
                Some(cached) => cached,
                None => Box::new(crate::text::create_text_layout(content, *font_size, *color, None)),
            };
            elements.push(
                crate::render::visual::LayeredElement::new(
                    crate::render::visual::VisualElement::TextRun {
                        text: std::sync::Arc::from(content.as_str()),
                        position: kurbo::Point::new(r.x as f64, r.y as f64),
                        color: *color,
                        font_size: *font_size,
                        font_family: "sans-serif".to_string(),
                        rotation: 0.0,
                        max_width: None,
                        layout: Some(lay),
                    },
                    z_index,
                )
                .with_id(id.as_ffi()),
            );
            // 缓存新的 TextLayout 供下次使用（update_node 时会清除缓存）
            layers.tree.set_text_layout_cache(
                id,
                Box::new(crate::text::create_text_layout(content, *font_size, *color, None)),
            );
        } else if let crate::view::node::ViewNode::Image {
            data, w, h, ..
        } = node_ref
        {
            let r = node.computed.rect();
            elements.push(
                crate::render::visual::LayeredElement::new(
                    crate::render::visual::VisualElement::Image {
                        bounds: crate::render::visual::KRect::new(
                            r.x as f64,
                            r.y as f64,
                            (r.x + r.width) as f64,
                            (r.y + r.height) as f64,
                        ),
                        data: std::sync::Arc::new(data.clone()),
                        width: *w,
                        height: *h,
                        opacity: None,
                    },
                    z_index,
                )
                .with_id(id.as_ffi()),
            );
        } else if let crate::view::node::ViewNode::Box { style, .. } = node_ref {
            let bg = if state.pressed && style.pressed_background.is_some() {
                style.pressed_background
            } else if state.hovered && style.hover_background.is_some() {
                style.hover_background
            } else {
                style.background_color
            };
            if let Some(bg) = bg {
                let r = node.computed.rect();
                elements.push(
                    crate::render::visual::LayeredElement::new(
                        crate::render::visual::VisualElement::RoundedRect {
                            rect: crate::render::visual::KRect::new(
                                r.x as f64,
                                r.y as f64,
                                (r.x + r.width) as f64,
                                (r.y + r.height) as f64,
                            ),
                            radius: style.border_radius as f64,
                            style: crate::render::visual::FillStrokeStyle::new().with_fill(bg),
                        },
                        z_index,
                    )
                    .with_id(id.as_ffi()),
                );
            }
        }
        // Flex / Listener 不产生渲染元素

        for child in &node.children {
            Self::cv(child, z_index, layers, elements, state);
        }
    }
}
