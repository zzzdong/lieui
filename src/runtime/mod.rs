//! Runtime — v2 核心"服务端"
pub mod element;
pub mod reconciler;
pub use element::ElementTree;
use crate::core::layers::{LayerType, Layers};
use crate::geometry::Size;
use crate::layout::context::LayoutContext;
use crate::render::visual::LayeredElement;
use crate::runtime::reconciler::Reconciler;
use crate::state;
use crate::view::node::ViewNode;

pub struct Runtime {
    pub layers: Layers, viewport: Size, needs_layout: bool, needs_render: bool,
    pending_view_tree: Option<ViewNode>, pub debug_stats: DebugStats,
    pub hovered_id: Option<crate::core::ElementId>,
    pub pressed_id: Option<crate::core::ElementId>,
}
#[derive(Debug, Default)]
pub struct DebugStats { pub reconciler: crate::runtime::reconciler::ReconcilerStats, pub element_count: usize }
fn id_as_u64(id: crate::core::ElementId) -> u64 { id.as_ffi() }

impl Runtime {
    pub fn new(viewport: Size) -> Self {
        Self { layers: Layers::new(), viewport, needs_layout: true, needs_render: true,
            pending_view_tree: None, debug_stats: DebugStats::default(),
            hovered_id: None, pressed_id: None }
    }
    pub fn set_viewport(&mut self, vp: Size) { self.viewport = vp; self.needs_layout = true; self.needs_render = true; }
    pub fn submit_view_tree(&mut self, vt: ViewNode) { self.pending_view_tree = Some(vt); self.needs_render = true; }

    pub fn frame(&mut self) -> Vec<LayeredElement> {
        if state::take_rebuild_requested() || self.pending_view_tree.is_some() {
            if let Some(vt) = self.pending_view_tree.take() {
                let mut r = Reconciler::new();
                if self.layers.layer_root(LayerType::Base).is_none() {
                    let id = self.layers.tree.create_from_node(&vt);
                    self.layers.tree.set_root(id); self.layers.set_base_root(id);
                    let mut p = Vec::new(); r.diff(&vt, id, &self.layers.tree, &mut p); r.apply(p, &mut self.layers.tree);
                } else {
                    let rid = self.layers.layer_root(LayerType::Base).unwrap();
                    let mut p = Vec::new(); r.diff(&vt, rid, &self.layers.tree, &mut p); r.apply(p, &mut self.layers.tree);
                    self.debug_stats.reconciler = r.stats;
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
        if let Some(rid) = self.layers.tree.root() { ctx.collect(rid, &self.layers.tree); ctx.compute(self.viewport); }
        self.layers.with_layout_mut(LayerType::Base, |l| *l = ctx.clone());
    }

    fn build_render_tree(&self) -> Vec<LayeredElement> {
        let mut e = Vec::new();
        let hov = self.hovered_id; let pre = self.pressed_id;
        self.layers.with_layout(LayerType::Base, |l| {
            if let Some(ref r) = l.root { Self::cv(r, &self.layers, &mut e, hov, pre); }
        });
        e.sort_by_key(|x| x.z_index); e
    }

    #[allow(clippy::too_many_arguments)]
    fn cv(node: &crate::layout::node::LayoutNode, layers: &Layers, elements: &mut Vec<LayeredElement>,
          hovered: Option<crate::core::ElementId>, pressed: Option<crate::core::ElementId>) {
        let id = node.id;
        let tn = node.type_name;
        if !tn.is_empty() {
            let p = layers.tree.props(id);
            let r = node.bounds();
            use crate::geometry::Color;
            use crate::render::visual::{FillStrokeStyle, LayeredElement as LE, VisualElement};
            use crate::render::visual::KRect;
            match tn {
                "text" => {
                    if let Some(c) = p.and_then(|x| x.get_str("content")) {
                        let cl = p.and_then(|x| x.get_color("color")).unwrap_or(Color::BLACK);
                        let fs = p.and_then(|x| x.get_f64("font_size")).unwrap_or(16.0);
                        let lay = crate::text::create_text_layout(c, fs, cl, None);
                        elements.push(LE::new(VisualElement::TextRun {
                            text: c.to_string(), position: kurbo::Point::new(r.x as f64, r.y as f64),
                            color: cl, font_size: fs, font_family: "sans-serif".to_string(),
                            rotation: 0.0, max_width: None, layout: Some(Box::new(lay)),
                        }, 0).with_id(id_as_u64(id)));
                    }
                }
                "button" => {
                    let (hh, pp) = (Some(id) == hovered, Some(id) == pressed);
                    let bg = if pp { Color::new(180,200,220) } else if hh { Color::new(200,215,230) } else { Color::new(220,220,220) };
                    let k = KRect::new(r.x as f64, r.y as f64, (r.x+r.width) as f64, (r.y+r.height) as f64);
                    elements.push(LE::new(VisualElement::RoundedRect { rect: k, radius: 4.0, style: FillStrokeStyle::new().with_fill(bg) }, 0).with_id(id_as_u64(id)));
                    if let Some(lb) = p.and_then(|x| x.get_str("label")) {
                        let lay = crate::text::create_text_layout(lb, 14.0, Color::BLACK, None);
                        elements.push(LE::new(VisualElement::TextRun {
                            text: lb.to_string(), position: kurbo::Point::new((r.x+12.0) as f64,(r.y+8.0) as f64),
                            color: Color::BLACK, font_size: 14.0, font_family: "sans-serif".to_string(),
                            rotation: 0.0, max_width: None, layout: Some(Box::new(lay)),
                        }, 0).with_id(id_as_u64(id)));
                    }
                }
                "image" => {
                    if let Some(d) = p.and_then(|x| x.get_bytes("data")) {
                        let iw = p.and_then(|x| x.get_u32("img_w")).unwrap_or(r.width as u32);
                        let ih = p.and_then(|x| x.get_u32("img_h")).unwrap_or(r.height as u32);
                        elements.push(LE::new(VisualElement::Image {
                            bounds: KRect::new(r.x as f64,r.y as f64,(r.x+r.width) as f64,(r.y+r.height) as f64),
                            data: std::sync::Arc::new(d.to_vec()), width: iw, height: ih, opacity: None,
                        }, 0).with_id(id_as_u64(id)));
                    }
                }
                "checkbox" => {
                    let ck = p.and_then(|x| x.get_bool("checked")).unwrap_or(false);
                    let bs = r.height.min(24.0);
                    let cb = KRect::new(r.x as f64,r.y as f64,(r.x+bs) as f64,(r.y+bs) as f64);
                    let fl = if ck { Color::new(60,120,220) } else { Color::new(200,200,200) };
                    elements.push(LE::new(VisualElement::Rect { rect: cb, style: FillStrokeStyle::new().with_fill(fl).with_stroke(Color::new(150,150,150),1.0) }, 0).with_id(id_as_u64(id)));
                    if let Some(lb) = p.and_then(|x| x.get_str("label")) {
                        let lay = crate::text::create_text_layout(lb, 14.0, Color::BLACK, None);
                        elements.push(LE::new(VisualElement::TextRun {
                            text: lb.to_string(), position: kurbo::Point::new((r.x+bs+6.0)as f64,(r.y+4.0)as f64),
                            color: Color::BLACK, font_size: 14.0, font_family: "sans-serif".to_string(),
                            rotation: 0.0, max_width: None, layout: Some(Box::new(lay)),
                        }, 0).with_id(id_as_u64(id)));
                    }
                }
                _ => {}
            }
        }
        for child in &node.children { Self::cv(child, layers, elements, hovered, pressed); }
    }
}
