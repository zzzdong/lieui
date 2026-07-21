//! Image Widget - 位图显示（RGBA8，行优先）

use std::sync::Arc;

use kurbo::Rect as KurboRect;

use crate::core::WidgetId;
use crate::geometry::Size;
use crate::layout::{IntrinsicSize, LayoutNode};
use crate::render::visual::{LayeredElement, VisualElement};
use crate::widget::Widget;

/// 简单位图 widget：持有 RGBA8 像素数据，在内容框内等比缩放并居中绘制。
///
/// 通过 `flex_grow = 1` 配合父容器的 `AlignItems::Stretch` 填满可用区域。
pub struct Image {
    visible: bool,
    data: Arc<Vec<u8>>,
    width: u32,
    height: u32,
    dirty: bool,
}

impl Image {
    pub fn new() -> Self {
        Self {
            visible: true,
            data: Arc::new(Vec::new()),
            width: 0,
            height: 0,
            dirty: false,
        }
    }

    /// 设置像素数据（RGBA8，行优先）
    pub fn set_pixels(&mut self, data: Vec<u8>, width: u32, height: u32) {
        self.data = Arc::new(data);
        self.width = width;
        self.height = height;
        self.dirty = true;
    }

    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }
}

impl Default for Image {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Image {
    crate::impl_widget_any!(Image);

    fn type_name(&self) -> &'static str {
        "Image"
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        if !self.visible {
            return LayoutNode::new(id).with_fixed_size(Size::new(0.0, 0.0));
        }
        // 固有尺寸为 0，依靠 flex_grow(1) 与父容器 Stretch 填满可用区域；
        // 实际绘制时在内容上按宽高比缩放居中。
        let mut node = LayoutNode::new(id);
        node.intrinsic_size = IntrinsicSize::Fixed(Size::new(0.0, 0.0));
        node.flex_grow = 1.0;
        node
    }

    fn render(&mut self, layout: &LayoutNode, _ctx: &crate::core::ViewContext) -> Vec<LayeredElement> {
        if !self.visible || self.data.is_empty() {
            return Vec::new();
        }
        let cb = layout.computed.content_box;
        let iw = self.width as f64;
        let ih = self.height as f64;
        let cw = cb.width as f64;
        let ch = cb.height as f64;
        if iw <= 0.0 || ih <= 0.0 || cw <= 0.0 || ch <= 0.0 {
            return Vec::new();
        }
        let scale = (cw / iw).min(ch / ih);
        let dw = iw * scale;
        let dh = ih * scale;
        let x = cb.x as f64 + (cw - dw) / 2.0;
        let y = cb.y as f64 + (ch - dh) / 2.0;
        vec![LayeredElement::default_layer(VisualElement::Image {
            bounds: KurboRect::new(x, y, x + dw, y + dh),
            data: self.data.clone(),
            width: self.width,
            height: self.height,
            opacity: None,
        })]
    }
}
