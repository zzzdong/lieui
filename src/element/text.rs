use parley::{Font, FontFamily, StyleProperty, TextStyle};
use taffy::{AvailableSpace, Layout, Style};
use vello_cpu::RenderContext;

use crate::{
    element::{IElement, world::ElementRef},
    paint::{TextColor, TextEngine},
};

pub struct TextElement {
    pub text: String,
    pub style: TextStyle<'static, TextColor>,
    layout: Option<parley::Layout<TextColor>>,
}

impl TextElement {
    pub fn new(text: String) -> Self {
        let mut style = TextStyle::default();

        style.font_size = 20.0;

        Self {
            text,
            style,
            layout: None,
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

        TextEngine::paint_text(cx, 0.0, 0.0, self.layout.as_ref().unwrap())
    }

    fn measure(
        &mut self,
        size: taffy::Size<Option<f32>>,
        available: taffy::Size<taffy::AvailableSpace>,
        style: &Style,
    ) -> taffy::Size<f32> {
        let width = match size.width {
            Some(w) => Some(w),
            None => available.width.into_option(),
        };
        let height = match size.height {
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
