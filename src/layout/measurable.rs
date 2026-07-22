//! Measurable trait — match 在类型变体上直接读字段

use crate::layout::box_model::{IntrinsicSize, LayoutConstraint};
use crate::view::node::ViewNode;

pub trait Measurable {
    fn measure(&self, node: &ViewNode, constraint: &LayoutConstraint) -> IntrinsicSize;
}

pub struct DefaultMeasurer;
impl Measurable for DefaultMeasurer {
    fn measure(&self, node: &ViewNode, constraint: &LayoutConstraint) -> IntrinsicSize {
        let (mut w, mut h) = (0.0f32, 0.0f32);

        match node {
            ViewNode::Text { content, font_size, .. } => {
                let max_w = if constraint.max_width < f32::MAX { Some(constraint.max_width as f64) } else { None };
                let (mw, mh) = crate::text::TextEngine::measure_text(content, *font_size, max_w);
                w = mw as f32; h = mh as f32;
            }
            ViewNode::Button { label, .. } => {
                let (mw, _) = crate::text::TextEngine::measure_text(label, 14.0, None);
                w = mw as f32 + 24.0; h = 32.0;
            }
            ViewNode::Checkbox { label, .. } => {
                let (mw, _) = if !label.is_empty() {
                    crate::text::TextEngine::measure_text(label, 14.0, None)
                } else { (0.0, 0.0) };
                w = 20.0 + mw as f32 + 8.0; h = 24.0;
            }
            ViewNode::Image { w: iw, h: ih, .. } => { w = *iw as f32; h = *ih as f32; }
            ViewNode::Divider { .. } => { w = 0.0; h = 0.0; }
            // 容器由 layout context 处理，这里返回 0
            ViewNode::Column { .. } | ViewNode::Row { .. } | ViewNode::Container { .. } | ViewNode::Custom { .. } => {}
        }

        w = w.clamp(constraint.min_width, constraint.max_width);
        h = h.clamp(constraint.min_height, constraint.max_height);
        IntrinsicSize::new(w, h)
    }
}
