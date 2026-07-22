//! Runtime — v2 核心"服务端"

pub mod element;
pub mod reconciler;

pub use element::ElementTree;

use crate::core::layers::{LayerType, Layers};
use crate::geometry::Size;
use crate::layout::node::LayoutContext;
use crate::render::visual::LayeredElement;
use crate::runtime::reconciler::Reconciler;
use crate::state;
use crate::view::node::ViewNode;

pub struct Runtime {
    pub layers: Layers,
    viewport: Size,
    needs_layout: bool,
    needs_render: bool,
    pending_view_tree: Option<ViewNode>,
    pub debug_stats: DebugStats,
}

#[derive(Debug, Default)]
pub struct DebugStats { pub reconciler: crate::runtime::reconciler::ReconcilerStats, pub element_count: usize }

fn id_as_u64(id: crate::core::ElementId) -> u64 { id.as_ffi() }

impl Runtime {
    pub fn new(viewport: Size) -> Self {
        Self { layers: Layers::new(), viewport, needs_layout: true, needs_render: true, pending_view_tree: None, debug_stats: DebugStats::default() }
    }
    pub fn set_viewport(&mut self, viewport: Size) { self.viewport = viewport; self.needs_layout = true; self.needs_render = true; }
    pub fn submit_view_tree(&mut self, view_tree: ViewNode) { self.pending_view_tree = Some(view_tree); self.needs_render = true; }

    pub fn frame(&mut self) -> Vec<LayeredElement> {
        if state::take_rebuild_requested() || self.pending_view_tree.is_some() {
            if let Some(view_tree) = self.pending_view_tree.take() {
                let mut reconciler = Reconciler::new();
                if self.layers.layer_root(LayerType::Base).is_none() {
                    let id = self.layers.tree.create_from_node(&view_tree);
                    self.layers.tree.set_root(id);
                    self.layers.set_base_root(id);
                    let mut patches = Vec::new();
                    reconciler.diff(&view_tree, id, &self.layers.tree, &mut patches);
                    reconciler.apply(patches, &mut self.layers.tree);
                } else {
                    let root_id = self.layers.layer_root(LayerType::Base).unwrap();
                    let mut patches = Vec::new();
                    reconciler.diff(&view_tree, root_id, &self.layers.tree, &mut patches);
                    reconciler.apply(patches, &mut self.layers.tree);
                    self.debug_stats.reconciler = reconciler.stats;
                }
                self.needs_layout = true;
            }
        }
        if self.needs_layout { self.perform_layout(); self.needs_layout = false; }
        if self.needs_render { self.needs_render = false; self.debug_stats.element_count = self.layers.tree.len(); self.build_render_tree() }
        else { Vec::new() }
    }

    fn perform_layout(&mut self) {
        let mut ctx = LayoutContext::new();
        ctx.compute(self.viewport, &self.layers.tree);
        self.layers.with_layout_mut(LayerType::Base, |layout| *layout = ctx.clone());
    }

    fn build_render_tree(&self) -> Vec<LayeredElement> {
        let mut elements = Vec::new();
        self.layers.with_layout(LayerType::Base, |layout| {
            if let Some(ref root) = layout.root { Self::collect_visuals(root, &self.layers, &mut elements); }
        });
        elements.sort_by_key(|e| e.z_index);
        elements
    }

    fn collect_visuals(node: &crate::layout::node::LayoutNode, layers: &Layers, elements: &mut Vec<LayeredElement>) {
        let id = node.id;
        if let Some(type_name) = layers.tree.type_name(id) {
            let props = layers.tree.props(id);
            let rect = node.bounds();
            use crate::geometry::Color;
            use crate::render::visual::{FillStrokeStyle, LayeredElement as LE, VisualElement};
            use crate::render::visual::KRect;
            match type_name {
                "text" => {
                    if let Some(content) = props.and_then(|p| p.get_str("content")) {
                        let color = props.and_then(|p| p.get_color("color")).unwrap_or(Color::BLACK);
                        let font_size = props.and_then(|p| p.get_f64("font_size")).unwrap_or(16.0);
                        // 使用 parley 创建文本布局
                        let layout = crate::text::create_text_layout(content, font_size, color, None);
                        let pos = kurbo::Point::new(rect.x as f64, rect.y as f64);
                        elements.push(LE::new(
                            VisualElement::TextRun {
                                text: content.to_string(), position: pos, color, font_size,
                                font_family: "sans-serif".to_string(), rotation: 0.0,
                                max_width: None, layout: Some(Box::new(layout)),
                            }, 0,
                        ).with_id(id_as_u64(id)));
                    }
                }
                "button" => {
                    let r = KRect::new(rect.x as f64, rect.y as f64, (rect.x + rect.width) as f64, (rect.y + rect.height) as f64);
                    elements.push(LE::new(
                        VisualElement::RoundedRect { rect: r, radius: 4.0, style: FillStrokeStyle::new().with_fill(Color::new(220, 220, 220)) }, 0,
                    ).with_id(id_as_u64(id)));
                    if let Some(label) = props.and_then(|p| p.get_str("label")) {
                        let layout = crate::text::create_text_layout(label, 14.0, Color::BLACK, None);
                        elements.push(LE::new(
                            VisualElement::TextRun {
                                text: label.to_string(), position: kurbo::Point::new((rect.x + 12.0) as f64, (rect.y + 8.0) as f64),
                                color: Color::BLACK, font_size: 14.0, font_family: "sans-serif".to_string(),
                                rotation: 0.0, max_width: None, layout: Some(Box::new(layout)),
                            }, 0,
                        ).with_id(id_as_u64(id)));
                    }
                }
                "image" => {
                    if let Some(data) = props.and_then(|p| p.get_bytes("data")) {
                        let img_w = props.and_then(|p| p.get_u32("img_w")).unwrap_or(rect.width as u32);
                        let img_h = props.and_then(|p| p.get_u32("img_h")).unwrap_or(rect.height as u32);
                        elements.push(LE::new(
                            VisualElement::Image { bounds: KRect::new(rect.x as f64, rect.y as f64, (rect.x + rect.width) as f64, (rect.y + rect.height) as f64),
                                data: std::sync::Arc::new(data.to_vec()), width: img_w, height: img_h, opacity: None }, 0,
                        ).with_id(id_as_u64(id)));
                    }
                }
                "checkbox" => {
                    let checked = props.and_then(|p| p.get_bool("checked")).unwrap_or(false);
                    let fill = if checked { Color::new(60, 120, 220) } else { Color::new(200, 200, 200) };
                    let r = KRect::new(rect.x as f64, rect.y as f64, (rect.x + 16.0) as f64, (rect.y + 16.0) as f64);
                    elements.push(LE::new(VisualElement::Rect { rect: r, style: FillStrokeStyle::new().with_fill(fill).with_stroke(Color::new(150,150,150), 1.0) }, 0).with_id(id_as_u64(id)));
                }
                _ => {}
            }
        }
        for child in &node.children { Self::collect_visuals(child, layers, elements); }
    }
}
