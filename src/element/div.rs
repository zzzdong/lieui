use color::Srgb;
use taffy::{Layout, Style};
use vello_cpu::{RenderContext, kurbo::Rect};

use crate::element::{IElement, world::ElementRef};

pub struct DivElement<State> {
    pub style: Style,
    pub children: Vec<ElementRef<State>>,
}

impl<State> IElement<State> for DivElement<State> {
    fn paint(&mut self, cx: &mut RenderContext, layout: &Layout) {
        cx.set_paint(color::AlphaColor::<Srgb>::WHITE);
        cx.fill_rect(&Rect::new(0.0, 0.0, 100.0, 100.0));
    }

    fn measure(
        &mut self,
        size: taffy::Size<Option<f32>>,
        available: taffy::Size<taffy::AvailableSpace>,
        style: &Style,
    ) -> taffy::Size<f32> {
        taffy::Size::ZERO
    }
}
