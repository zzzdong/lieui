use std::f32;

use taffy::{
    AvailableSpace, Dimension, Layout, Size as TaffySize, Style, prelude::TaffyMaxContent,
};
use vello_cpu::{RenderContext, kurbo::Rect, peniko::Color};

use crate::element::{ElementId, IElement, world::ElementRef};

pub struct DivElement {
    pub style: Style,
    pub children: Vec<ElementId>,
    pub background_color: Color,
}

impl DivElement {
    pub fn new() -> Self {
        Self {
            style: Style::default(),
            children: Vec::new(),
            background_color: Color::WHITE,
        }
    }

    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.style.size.width = Dimension::length(width);
        self
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.style.size.height = Dimension::length(height);
        self
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = color;
        self
    }

    pub fn layout_style(&self) -> Style {
        self.style.clone()
    }
}

impl IElement for DivElement {
    fn paint(&mut self, cx: &mut RenderContext, layout: &Layout) {
        let rect = Rect::new(
            layout.location.x.into(),
            layout.location.y.into(),
            (layout.location.x + layout.size.width) as f64,
            (layout.location.y + layout.size.height) as f64,
        );

        cx.set_paint(self.background_color);

        cx.fill_rect(&rect);
    }

        fn measure(
        &mut self,
        constraint: taffy::Size<Option<f32>>,
        available: taffy::Size<taffy::AvailableSpace>,
        _style: &Style,
    ) -> taffy::Size<f32> {
        // 1. 解码 Dimension（无 match，用 is_auto / into_option / value）
        let own_width = match self.style.size.width.tag() {
            taffy::style::CompactLength::LENGTH_TAG => Some(self.style.size.width.value()),
            taffy::style::CompactLength::PERCENT_TAG => {
                available.width.into_option().map(|w| w * self.style.size.width.value())
            }
            _ => None, // AUTO_TAG / MIN_CONTENT / MAX_CONTENT
        };

        let own_height = match self.style.size.height.tag() {
            taffy::style::CompactLength::LENGTH_TAG => Some(self.style.size.height.value()),
            taffy::style::CompactLength::PERCENT_TAG => {
                available.height.into_option().map(|h| h * self.style.size.height.value())
            }
            _ => None,
        };

        // 2. 父硬约束 → 可用空间兜底
        let w = own_width
            .or(constraint.width)
            .or_else(|| available.width.into_option())
            .unwrap_or(0.0)
            .max(0.0);
        let h = own_height
            .or(constraint.height)
            .or_else(|| available.height.into_option())
            .unwrap_or(0.0)
            .max(0.0);

        taffy::Size { width: w, height: h }
    }
}
