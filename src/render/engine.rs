//! VelloRenderer — 像素操作渲染引擎
use vello_cpu::color::PremulRgba8;
use vello_cpu::Pixmap;
use crate::render::renderer::Renderer;
use crate::render::visual::{FillStrokeStyle, LayeredElement, VisualElement};

pub struct VelloRenderer { width: u16, height: u16 }

impl VelloRenderer {
    pub fn new(w: u16, h: u16) -> Self { Self { width: w, height: h } }
    pub fn resize(&mut self, w: u16, h: u16) { self.width = w; self.height = h; }
    pub fn width(&self) -> u16 { self.width }
    pub fn height(&self) -> u16 { self.height }
    fn prgba(r: u8, g: u8, b: u8, a: u8) -> PremulRgba8 { PremulRgba8::from_u8_array([r, g, b, a]) }
    fn c(&self, c: &crate::geometry::Color) -> PremulRgba8 {
        Self::prgba((c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, (c.a * 255.0) as u8)
    }
}

impl Renderer for VelloRenderer {
    fn render(&mut self, elements: &[LayeredElement]) -> Pixmap {
        let mut pix = Pixmap::new(self.width, self.height);
        let bg = Self::prgba(245, 245, 245, 255);
        for p in pix.data_mut() { *p = bg; }
        for e in elements { self.draw(&mut pix, &e.element); }
        pix
    }
}

impl VelloRenderer {
    fn draw(&self, pix: &mut Pixmap, ele: &VisualElement) {
        match ele {
            VisualElement::Rect { rect, style } => {
                self.fill(pix, rect.x0, rect.y0, rect.x1 - rect.x0, rect.y1 - rect.y0, style);
            }
            VisualElement::RoundedRect { rect, .. } => {
                self.fill(pix, rect.x0, rect.y0, rect.x1 - rect.x0, rect.y1 - rect.y0, &FillStrokeStyle::new().with_fill(crate::geometry::Color::from_rgb8(220, 220, 220)));
            }
            VisualElement::TextRun { position, color, .. } => {
                self.dot(pix, position.x, position.y, color);
            }
            VisualElement::Group { children, .. } => {
                for c in children { self.draw(pix, &c.element); }
            }
            _ => {}
        }
    }

    fn fill(&self, pix: &mut Pixmap, x: f64, y: f64, w: f64, h: f64, style: &FillStrokeStyle) {
        let fill = match &style.fill { Some(f) => f, None => return };
        let col = self.c(fill);
        let pw = pix.width() as usize;
        let ph = pix.height() as usize;
        let x0 = (x.max(0.0) as usize).min(pw);
        let y0 = (y.max(0.0) as usize).min(ph);
        let x1 = ((x + w) as usize).min(pw);
        let y1 = ((y + h) as usize).min(ph);
        let data = pix.data_mut();
        for row in y0..y1 {
            for col_idx in x0..x1 {
                data[row * pw + col_idx] = col;
            }
        }
    }

    fn dot(&self, pix: &mut Pixmap, x: f64, y: f64, col: &crate::geometry::Color) {
        let pw = pix.width() as usize;
        let ph = pix.height() as usize;
        let px = (x as usize).min(pw - 1);
        let py = (y as usize).min(ph - 1);
        pix.data_mut()[py * pw + px] = self.c(col);
    }
}
