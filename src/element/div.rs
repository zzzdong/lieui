use std::f32;

use taffy::{
    AlignItems, AvailableSpace, Dimension, Display, JustifyContent, Layout, Size as TaffySize, Style as TaffyStyle, prelude::TaffyMaxContent
};
use vello_cpu::{RenderContext, kurbo::Rect, peniko::Color};

use crate::{element::{ElementId, IElement, style::Style, world::ElementRef}, paint::PaintContext};

pub struct DivElement {
    pub style: TaffyStyle,
    pub children: Vec<ElementId>,
}

impl DivElement {
    pub fn new() -> Self {
        Self {
            style: TaffyStyle::default(),
            children: Vec::new(),
        }
    }

    pub fn with_style(mut self, style: TaffyStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_display(mut self, display: Display) -> Self {
        self.style.display = display;
        self
    }

    pub fn with_width(mut self, width: Dimension) -> Self {
        self.style.size.width = width;
        self
    }

    pub fn with_height(mut self, height: Dimension) -> Self {
        self.style.size.height = height;
        self
    }

    pub fn with_justify_content(mut self, justify_content: JustifyContent) -> Self {
        self.style.justify_content = Some(justify_content);
        self
    }

    pub fn with_align_items(mut self, align_items: AlignItems) -> Self {
        self.style.align_items = Some(align_items);
        self
    }

    pub fn layout_style(&self) -> TaffyStyle {
        self.style.clone()
    }
}

impl IElement for DivElement {

    fn paint(&mut self, cx: &mut PaintContext) {
        cx.painter.set_paint(cx.style.background_color);
        cx.painter.fill_rect(&cx.rect);
    }

    fn measure(
        &mut self,
        constraint: taffy::Size<Option<f32>>,
        available: taffy::Size<taffy::AvailableSpace>,
        _style: &Style,
    ) -> taffy::Size<f32> {
        // println!("div.measure: constraint: {constraint:?}, available: {available:?}");

        if let taffy::Size {
            width: Some(width),
            height: Some(height),
        } = constraint
        {
            return taffy::Size { width, height };
        }

        taffy::Size::ZERO
    }
}
