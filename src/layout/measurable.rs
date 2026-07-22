//! Measurable trait

use crate::layout::box_model::{IntrinsicSize, LayoutConstraint};
use crate::view::node::PropMap;

pub trait Measurable {
    fn measure(&self, props: &PropMap, constraint: &LayoutConstraint) -> IntrinsicSize;
}

pub struct DefaultMeasurer;
impl Measurable for DefaultMeasurer {
    fn measure(&self, props: &PropMap, constraint: &LayoutConstraint) -> IntrinsicSize {
        let mut w = 0.0f32;
        let mut h = 0.0f32;
        if let Some(content) = props.get_str("content") {
            let font_size = props.get_f32("font_size").unwrap_or(16.0) as f64;
            let max_w = if constraint.max_width < f32::MAX { Some(constraint.max_width as f64) } else { None };
            let (mw, mh) = crate::text::TextEngine::measure_text(content, font_size, max_w);
            w = mw as f32;
            h = mh as f32;
        }
        if let Some(label) = props.get_str("label") {
            let (mw, _mh) = crate::text::TextEngine::measure_text(label, 14.0, None);
            w = w.max(mw as f32 + 24.0);
            h = h.max(32.0);
        }
        // checkbox: 方框(20px) + 标签文字
        if props.get_str("checked").is_some() || props.get_bool("checked").is_some() {
            let label_w = props.get_str("label").map(|l| {
                crate::text::TextEngine::measure_text(l, 14.0, None).0 as f32
            }).unwrap_or(0.0);
            w = w.max(20.0 + label_w + 8.0);
            h = h.max(24.0);
        }
        if let Some(fw) = props.get_f32("width") { w = fw; }
        if let Some(fh) = props.get_f32("height") { h = fh; }
        w = w.clamp(constraint.min_width, constraint.max_width);
        h = h.clamp(constraint.min_height, constraint.max_height);
        IntrinsicSize::new(w, h)
    }
}
