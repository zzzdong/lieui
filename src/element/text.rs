use std::{borrow::Cow, marker::PhantomData};

use parley::{Font, FontFamily, StyleProperty, TextStyle};
use taffy::{AvailableSpace, Layout};
use vello_cpu::{RenderContext, kurbo::Rect, peniko::Color};

use crate::{
    element::{IElement, style::Style, world::ElementRef},
    paint::{PaintContext, TextColor, TextEngine},
};

pub struct TextElement {
    pub text: String,
    pub style: TextStyle<'static, TextColor>,
    layout: Option<parley::Layout<TextColor>>,
}

impl TextElement {
    pub fn new(text: String) -> Self {
        let mut style = TextStyle::default();

        style.font_size = 16.0;
        style.brush = Color::BLACK.into();

        Self {
            text,
            style,
            layout: None,
        }
    }
}

impl IElement for TextElement {
    fn paint(&mut self, cx: &mut PaintContext) {
        match &self.layout {
            Some(layout) => {
                self.layout = Some(layout.clone());
            }
            None => {
                let layout = TextEngine::layout_text(&self.text, &self.style, None, None);
                self.layout = Some(layout);
            }
        }
        let old_paint = cx.painter.paint().clone();

        cx.painter.set_paint(cx.style.background_color);
        cx.painter.fill_rect(&cx.rect);

        cx.painter.set_paint(cx.style.color);

        TextEngine::paint_text(cx, self.layout.as_ref().unwrap());

        cx.painter.set_paint(old_paint);
    }

    fn measure(
        &mut self,
        constraint: taffy::Size<Option<f32>>,
        available: taffy::Size<taffy::AvailableSpace>,
        style: &Style,
    ) -> taffy::Size<f32> {
        // println!("text.measure: constraint: {constraint:?}, available: {available:?}");

        self.style.font_size = style.font_size as f32;
        self.style.brush = style.color.into();
        self.style.font_stack = parley::FontStack::List(Cow::Owned(vec![FontFamily::Named(
            style.font_family.clone().into(),
        )]));

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
