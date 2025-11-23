use std::borrow::Cow;

use parley::{
    Alignment, AlignmentOptions, FontContext, FontData, FontFamily, FontStack, FontWeight, GenericFamily, Glyph, InlineBox, Layout, LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty
};


pub struct PaintContext {
    render_cx: vello_cpu::RenderContext,
    font_cx: FontContext,
    layout_cx: LayoutContext<()>,
}

impl PaintContext {
    pub fn new(width: u16, height: u16) -> Self {
        let render_cx = vello_cpu::RenderContext::new(width, height);
        let font_cx = FontContext::new();
        let layout_cx = LayoutContext::new();
        Self {
            render_cx,
            font_cx,
            layout_cx,
        }
    }

    pub fn paint_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        style: StyleProperty<()>,
        width: Option<u32>,
        height: Option<u32>,
    ) {
        let layout = self.layout_glyph_run(text, style, width, height);

        let width = layout.width();
        let height = layout.height();

        let mut glyphs = Vec::new();
        let mut font = None;



        for line in layout.lines() {
            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(run) => {
                        font = Some(run.run().font());
                        // Render the glyph run
                        for glyph in run.positioned_glyphs() {
                            glyphs.push(glyph);
                        }
                        
                    }
                    PositionedLayoutItem::InlineBox(inline_box) => {
                        // Render the inline box
                    }
                };
            }
        }

        


    }

    fn layout_glyph_run<'a>(
        &'a mut self,
        text: &str,
        style: StyleProperty<()>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Layout<()> {
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, true);
        builder.push_default(style);

        let mut layout: Layout<()> = builder.build(text);

        let max_width = width.map(|w| w as f32);

        match (width, height) {
            (Some(width), Some(height)) => {
                let mut break_lines = layout.break_lines();

                let mut box_width = 0.0;
                let mut box_height = 0.0;

                while let Some((w, h)) = break_lines.break_next(width as f32) {
                    if box_height + h > height as f32 {
                        break_lines.revert();
                        break;
                    }
                    box_width = w.max(box_width);
                    box_height += h;
                }

                break_lines.finish();
            }
            (width, None) => {
                layout.break_all_lines(max_width);
            }
            (None, Some(_)) => {
                // FIXME: maybe height is not enough
                layout.break_all_lines(None);
            }
        }

        layout.align(max_width, Alignment::Start, AlignmentOptions::default());

        layout
    }
}

