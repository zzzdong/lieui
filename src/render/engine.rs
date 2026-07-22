//! VelloRenderer — 基于 liecharts PixmapRenderer 模式
//!
//! RenderContext + set_paint + fill_rect/fill_path + glyph_run + render_to_pixmap
//! 文本渲染使用 parley TextLayout 中的 glyph 数据 + vello_cpu glyph_run

use vello_cpu::{Pixmap, RenderContext, Resources};
use vello_cpu::kurbo::{Affine, BezPath, Circle, Rect, Shape, Stroke as KurboStroke};
use vello_cpu::peniko::color::AlphaColor;

use crate::render::renderer::Renderer;
use crate::render::visual::{LayeredElement, Stroke, VisualElement};

pub struct VelloRenderer {
    ctx: RenderContext,
    resources: Resources,
    width: u16,
    height: u16,
}

impl VelloRenderer {
    pub fn new(w: u16, h: u16) -> Self {
        Self {
            ctx: RenderContext::new(w, h),
            resources: Resources::new(),
            width: w, height: h,
        }
    }
    pub fn resize(&mut self, w: u16, h: u16) {
        self.width = w; self.height = h;
        self.ctx = RenderContext::new(w, h);
    }
    pub fn width(&self) -> u16 { self.width }
    pub fn height(&self) -> u16 { self.height }

    fn cv(c: &crate::geometry::Color) -> AlphaColor<vello_cpu::color::Srgb> {
        c.as_vello()
    }

    fn apply_fill(&mut self, fill: &crate::geometry::Color, rect: &Rect) {
        self.ctx.set_paint(Self::cv(fill));
        self.ctx.fill_rect(rect);
    }

    fn apply_stroke(&mut self, stroke: &Stroke) {
        self.ctx.set_paint(Self::cv(&stroke.color));
        self.ctx.set_stroke(KurboStroke::new(stroke.width));
    }
}

impl Renderer for VelloRenderer {
    fn render(&mut self, elements: &[LayeredElement]) -> Pixmap {
        // 背景
        self.ctx.set_paint(AlphaColor::from_rgba8(240, 240, 240, 255));
        self.ctx.fill_rect(&Rect::new(0.0, 0.0, self.width as f64, self.height as f64));

        for e in elements { self.draw(&e.element); }

        self.ctx.flush();
        let mut pix = Pixmap::new(self.width, self.height);
        self.ctx.render_to_pixmap(&mut self.resources, &mut pix);
        pix
    }
}

impl VelloRenderer {
    fn draw(&mut self, el: &VisualElement) {
        match el {
            VisualElement::Rect { rect, style } => {
                if let Some(f) = &style.fill { self.apply_fill(f, rect); }
                if let Some(s) = &style.stroke { self.apply_stroke(s); self.ctx.stroke_rect(rect); }
            }
            VisualElement::RoundedRect { rect, radius: r, style } => {
                if let Some(f) = &style.fill {
                    self.ctx.set_paint(Self::cv(f));
                    self.ctx.fill_blurred_rounded_rect(rect, *r as f32, 0.0);
                }
            }
            VisualElement::Circle { center, radius, style } => {
                let circle = Circle::new(*center, *radius);
                let path = circle.to_path(0.1);
                if let Some(f) = &style.fill { self.ctx.set_paint(Self::cv(f)); self.ctx.fill_path(&path); }
                if let Some(s) = &style.stroke { self.apply_stroke(s); self.ctx.stroke_path(&path); }
            }
            VisualElement::Line { start, end, style } => {
                let mut path = BezPath::new();
                path.move_to(*start); path.line_to(*end);
                self.ctx.set_paint(Self::cv(&style.color));
                self.ctx.set_stroke(KurboStroke::new(style.width));
                self.ctx.stroke_path(&path);
            }
            VisualElement::Path { path, style } => {
                if let Some(f) = &style.fill { self.ctx.set_paint(Self::cv(f)); self.ctx.fill_path(path); }
                if let Some(s) = &style.stroke { self.apply_stroke(s); self.ctx.stroke_path(path); }
            }
            VisualElement::TextRun { text, position, color, font_size: _, rotation, layout, .. } => {
                let layout = match layout { Some(l) => l, None => return };
                let transform = Affine::translate((position.x, position.y)) * Affine::rotate(*rotation);

                for line in layout.lines() {
                    for item in line.items() {
                        if let parley::layout::PositionedLayoutItem::GlyphRun(glyph_run) = item {
                            let run = glyph_run.run();
                            let font_data = run.font();
                            let run_font_size = run.font_size();

                            let glyphs: Vec<vello_cpu::Glyph> = glyph_run
                                .positioned_glyphs()
                                .map(|g| vello_cpu::Glyph { id: g.id, x: g.x, y: g.y })
                                .collect();

                            if glyphs.is_empty() { continue; }

                            self.ctx.set_paint(Self::cv(color));
                            self.ctx.glyph_run(&mut self.resources, font_data)
                                .font_size(run_font_size)
                                .glyph_transform(transform)
                                .fill_glyphs(glyphs.into_iter());
                        }
                    }
                }
                // suppress unused warning for `text`
                let _ = text;
            }
            VisualElement::Image { bounds, .. } => {
                self.ctx.set_paint(AlphaColor::from_rgba8(200, 200, 200, 128));
                self.ctx.fill_rect(bounds);
            }
            VisualElement::Group { children, .. } => {
                for c in children { self.draw(&c.element); }
            }
        }
    }
}
