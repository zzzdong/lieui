use std::marker::PhantomData;

use parley::{Font, FontFamily, StyleProperty, TextStyle};
use taffy::{AvailableSpace, Layout, Style};
use vello_cpu::{RenderContext, kurbo::Rect, peniko::Color};

use crate::{
    element::{IElement, world::ElementRef},
    paint::{TextColor, TextEngine},
};

pub struct TextElement {
    pub text: String,
    pub style: TextStyle<'static, TextColor>,
    layout: Option<parley::Layout<TextColor>>,
    background_color: Color,
    front_color: Color,
}

impl TextElement {
    pub fn new(text: String) -> Self {
        let mut style = TextStyle::default();

        style.font_size = 20.0;
        style.brush = Color::BLACK.into();

        Self {
            text,
            style,
            layout: None,
            background_color: Color::WHITE,
            front_color: Color::BLACK,
        }
    }
}

impl IElement for TextElement {
    fn paint(&mut self, cx: &mut RenderContext, layout: &Layout) {
        match &self.layout {
            Some(layout) => {
                self.layout = Some(layout.clone());
            }
            None => {
                let layout = TextEngine::layout_text(&self.text, &self.style, None, None);
                self.layout = Some(layout);
            }
        }

        let rect = Rect::new(
            layout.location.x.into(),
            layout.location.y.into(),
            (layout.location.x + layout.size.width) as f64,
            (layout.location.y + layout.size.height) as f64,
        );

        let paint = cx.paint().clone();

        cx.set_paint(self.background_color);

        cx.fill_rect(&rect);

        println!("text.paint: rect: {:?}", rect);

        TextEngine::paint_text(cx, rect.x0 as f32, rect.y0 as f32, self.layout.as_ref().unwrap());

        cx.set_paint(paint);
    }

    fn measure(
        &mut self,
        constraint: taffy::Size<Option<f32>>,
        available: taffy::Size<taffy::AvailableSpace>,
        style: &Style,
    ) -> taffy::Size<f32> {
        println!("text.measure: constraint: {constraint:?}, available: {available:?}");

        let width = match constraint.width {
            Some(w) => Some(w),
            None => available.width.into_option(),
        };
        let height = match constraint.height {
            Some(h) => Some(h),
            None => available.height.into_option(),
        };

        let layout = TextEngine::layout_text(&self.text, &self.style, width, height);

        let size = taffy::Size {
            width: layout.width(),
            height: layout.height(),
        };

        // cache layout
        self.layout = Some(layout);

        size
    }
}
