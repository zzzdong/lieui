//! VelloRenderer — 基于 liecharts PixmapRenderer 模式
//!
//! RenderContext + set_paint + fill_rect/fill_path + glyph_run + render_to_pixmap
//! 文本渲染使用 parley TextLayout 中的 glyph 数据 + vello_cpu glyph_run

use vello_cpu::kurbo::{Affine, BezPath, Circle, Rect, Shape, Stroke as KurboStroke};
use vello_cpu::peniko::color::AlphaColor;
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::render::renderer::Renderer;
use crate::render::visual::{LayeredElement, Stroke, VisualElement};
use crate::view::paint::ImageFit;

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
            width: w,
            height: h,
        }
    }
    pub fn resize(&mut self, w: u16, h: u16) {
        self.width = w;
        self.height = h;
        self.ctx = RenderContext::new(w, h);
    }
    pub fn width(&self) -> u16 {
        self.width
    }
    pub fn height(&self) -> u16 {
        self.height
    }

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
        self.ctx
            .set_paint(AlphaColor::from_rgba8(240, 240, 240, 255));
        self.ctx
            .fill_rect(&Rect::new(0.0, 0.0, self.width as f64, self.height as f64));

        // 递归绘制非 Image 元素，并收集 Image 及其当前裁剪区域。
        // Image 在 vello_cpu 渲染完成后通过 pixmap 后处理 blit，因此需要单独记录 clip。
        let mut images: Vec<(LayeredElement, Option<Rect>)> = Vec::new();
        for e in elements {
            self.render_element(e, &mut images, None);
        }

        self.ctx.flush();
        let mut pix = Pixmap::new(self.width, self.height);
        self.ctx.render_to_pixmap(&mut self.resources, &mut pix);

        // 后处理：在 pixmap 上直接 blit 图像
        for (img, clip) in &images {
            Self::blit_image(&mut pix, img, *clip);
        }
        pix
    }
}

impl VelloRenderer {
    /// 递归渲染元素。Image 被收集到 `images` 中；Group 的 clip_rect 会作为当前裁剪区域传给子元素。
    fn render_element(
        &mut self,
        el: &LayeredElement,
        images: &mut Vec<(LayeredElement, Option<Rect>)>,
        current_clip: Option<Rect>,
    ) {
        match &el.element {
            VisualElement::Image { .. } => {
                images.push((el.clone(), current_clip));
            }
            VisualElement::Group {
                children,
                clip_rect,
                ..
            } => {
                let next_clip = match (current_clip, *clip_rect) {
                    (Some(parent), Some(child)) => Some(parent.intersect(child)),
                    (Some(parent), None) => Some(parent),
                    (None, Some(child)) => Some(child),
                    (None, None) => None,
                };
                if let Some(clip) = clip_rect {
                    self.ctx.push_clip_path(&clip.to_path(0.1));
                    for c in children {
                        self.render_element(c, images, next_clip);
                    }
                    self.ctx.pop_clip_path();
                } else {
                    for c in children {
                        self.render_element(c, images, next_clip);
                    }
                }
            }
            _ => self.draw(&el.element),
        }
    }

    /// 在 Pixmap 上直接 blit RGBA 图像数据，可选按 `clip` 矩形裁剪。
    fn blit_image(pix: &mut vello_cpu::Pixmap, img: &LayeredElement, clip: Option<Rect>) {
        if let VisualElement::Image {
            bounds,
            data,
            width,
            height,
            opacity,
            fit,
            border_radius,
            ..
        } = &img.element
        {
            let pw = pix.width() as usize;
            let ph = pix.height() as usize;
            let img_w = *width as f64;
            let img_h = *height as f64;
            if img_w <= 0.0 || img_h <= 0.0 || data.len() < 4 {
                return;
            }
            let bw = bounds.width();
            let bh = bounds.height();
            if bw <= 0.0 || bh <= 0.0 {
                return;
            }

            // 依据 fit 计算图像在容器中的绘制矩形（绘制坐标，左上角原点）。
            let (dw, dh, dx0, dy0) = match fit {
                ImageFit::None => (img_w, img_h, bounds.x0, bounds.y0),
                ImageFit::Fill => (bw, bh, bounds.x0, bounds.y0),
                ImageFit::Contain => {
                    let s = (bw / img_w).min(bh / img_h);
                    let dw = img_w * s;
                    let dh = img_h * s;
                    (dw, dh, bounds.x0 + (bw - dw) / 2.0, bounds.y0 + (bh - dh) / 2.0)
                }
                ImageFit::Cover => {
                    let s = (bw / img_w).max(bh / img_h);
                    let dw = img_w * s;
                    let dh = img_h * s;
                    (dw, dh, bounds.x0 + (bw - dw) / 2.0, bounds.y0 + (bh - dh) / 2.0)
                }
            };

            // 把外层 clip 与绘制矩形相交，减少逐像素判断。
            let clip = match clip {
                Some(c) => {
                    let x0 = c.x0.max(dx0);
                    let y0 = c.y0.max(dy0);
                    let x1 = c.x1.min(dx0 + dw);
                    let y1 = c.y1.min(dy0 + dh);
                    if x1 > x0 && y1 > y0 {
                        Some(Rect::new(x0, y0, x1, y1))
                    } else {
                        None
                    }
                }
                None => None,
            };

            let op = opacity.unwrap_or(1.0).clamp(0.0, 1.0);
            let radius = *border_radius;
            let iw = *width as usize;
            let ih = *height as usize;
            let d = pix.data_mut();

            let cols = dw.ceil() as usize;
            let rows = dh.ceil() as usize;
            for py in 0..rows {
                let dy = (dy0 + py as f64) as usize;
                if dy >= ph {
                    break;
                }
                for px in 0..cols {
                    let dx = (dx0 + px as f64) as usize;
                    if dx >= pw {
                        break;
                    }
                    if let Some(c) = clip {
                        let fx = dx as f64 + 0.5;
                        let fy = dy as f64 + 0.5;
                        if fx < c.x0 || fx >= c.x1 || fy < c.y0 || fy >= c.y1 {
                            continue;
                        }
                    }
                    // 圆角裁剪：超出圆角矩形外的像素跳过。
                    if radius > 0.5 {
                        let r = (radius as f64).min(dw / 2.0).min(dh / 2.0);
                        let in_corner_x = (dx as f64) < dx0 + r || (dx as f64) > dx0 + dw - r;
                        let in_corner_y = (dy as f64) < dy0 + r || (dy as f64) > dy0 + dh - r;
                        let inside = if in_corner_x && in_corner_y {
                            let ccx = if (dx as f64) < dx0 + r { dx0 + r } else { dx0 + dw - r };
                            let ccy = if (dy as f64) < dy0 + r { dy0 + r } else { dy0 + dh - r };
                            let ddx = dx as f64 - ccx;
                            let ddy = dy as f64 - ccy;
                            ddx * ddx + ddy * ddy <= r * r
                        } else {
                            true
                        };
                        if !inside {
                            continue;
                        }
                    }
                    // 由目标像素反算源像素（最近邻采样）。
                    let mut sx = ((px as f64 / dw) * img_w) as usize;
                    let mut sy = ((py as f64 / dh) * img_h) as usize;
                    if sx >= iw {
                        sx = iw - 1;
                    }
                    if sy >= ih {
                        sy = ih - 1;
                    }
                    let si = (sy * iw + sx) * 4;
                    if si + 3 >= data.len() {
                        continue;
                    }
                    let r = data[si];
                    let g = data[si + 1];
                    let b = data[si + 2];
                    let a = data[si + 3];
                    if a == 0 {
                        continue;
                    }
                    let final_a = (a as f32 * op) as u8;
                    let alpha = final_a as f32 / 255.0;
                    let r = (r as f32 * alpha) as u8;
                    let g = (g as f32 * alpha) as u8;
                    let b = (b as f32 * alpha) as u8;
                    d[dy * pw + dx] =
                        vello_cpu::color::PremulRgba8::from_u8_array([r, g, b, final_a]);
                }
            }
        }
    }

    fn draw(&mut self, el: &VisualElement) {
        match el {
            VisualElement::Rect { rect, style } => {
                if let Some(f) = &style.fill {
                    self.apply_fill(f, rect);
                }
                if let Some(s) = &style.stroke {
                    self.apply_stroke(s);
                    self.ctx.stroke_rect(rect);
                }
            }
            VisualElement::RoundedRect {
                rect,
                radius: r,
                style,
            } => {
                if let Some(f) = &style.fill {
                    if *r < 0.5 {
                        // radius 为 0 时直接矩形填充
                        self.apply_fill(f, rect);
                    } else {
                        // 普通圆角矩形路径填充。
                        // 注意不要用 fill_blurred_rounded_rect(std_dev=0)：
                        // 那是高斯模糊专用路径，每像素代价比路径填充高一个量级。
                        let rr = vello_cpu::kurbo::RoundedRect::from_rect(*rect, *r as f64);
                        self.ctx.set_paint(Self::cv(f));
                        self.ctx.fill_path(&rr.to_path(0.1));
                    }
                }
                if let Some(s) = &style.stroke {
                    self.apply_stroke(s);
                    if *r < 0.5 {
                        self.ctx.stroke_rect(rect);
                    } else {
                        let rr = vello_cpu::kurbo::RoundedRect::from_rect(*rect, *r);
                        self.ctx.stroke_path(&rr.to_path(0.1));
                    }
                }
            }
            VisualElement::ShadowRoundedRect {
                rect,
                radius,
                std_dev,
                color,
            } => {
                self.ctx.set_paint(Self::cv(color));
                self.ctx.fill_blurred_rounded_rect(rect, *radius as f32, *std_dev as f32);
            }
            VisualElement::Circle {
                center,
                radius,
                style,
            } => {
                let circle = Circle::new(*center, *radius);
                let path = circle.to_path(0.1);
                if let Some(f) = &style.fill {
                    self.ctx.set_paint(Self::cv(f));
                    self.ctx.fill_path(&path);
                }
                if let Some(s) = &style.stroke {
                    self.apply_stroke(s);
                    self.ctx.stroke_path(&path);
                }
            }
            VisualElement::Line { start, end, style } => {
                let mut path = BezPath::new();
                path.move_to(*start);
                path.line_to(*end);
                self.ctx.set_paint(Self::cv(&style.color));
                self.ctx.set_stroke(KurboStroke::new(style.width));
                self.ctx.stroke_path(&path);
            }
            VisualElement::Path { path, style } => {
                if let Some(f) = &style.fill {
                    self.ctx.set_paint(Self::cv(f));
                    self.ctx.fill_path(path);
                }
                if let Some(s) = &style.stroke {
                    self.apply_stroke(s);
                    self.ctx.stroke_path(path);
                }
            }
            VisualElement::TextRun {
                text,
                position,
                color,
                font_size: _,
                rotation,
                layout,
                ..
            } => {
                let layout = match layout {
                    Some(l) => l,
                    None => return,
                };
                let transform =
                    Affine::translate((position.x, position.y)) * Affine::rotate(*rotation);

                for line in layout.lines() {
                    for item in line.items() {
                        if let parley::layout::PositionedLayoutItem::GlyphRun(glyph_run) = item {
                            let run = glyph_run.run();
                            let font_data = run.font();
                            let run_font_size = run.font_size();

                            let glyphs: Vec<vello_cpu::Glyph> = glyph_run
                                .positioned_glyphs()
                                .map(|g| vello_cpu::Glyph {
                                    id: g.id,
                                    x: g.x,
                                    y: g.y,
                                })
                                .collect();

                            if glyphs.is_empty() {
                                continue;
                            }

                            self.ctx.set_paint(Self::cv(color));
                            self.ctx
                                .glyph_run(&mut self.resources, font_data)
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
                self.ctx
                    .set_paint(AlphaColor::from_rgba8(200, 200, 200, 128));
                self.ctx.fill_rect(bounds);
            }
            VisualElement::Group { .. } => {
                // Group 由 render_element 处理，这里不应直接遇到。
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Color;
    use crate::render::renderer::Renderer;
    use crate::render::visual::{FillStrokeStyle, KRect, LayeredElement, VisualElement};
    use std::sync::Arc;

    #[test]
    fn group_clip_rect_crops_children() {
        let mut renderer = VelloRenderer::new(100, 100);
        // 一个红色大矩形，但被 Group 的 clip_rect (0,0,50,50) 裁剪。
        let elements = vec![LayeredElement::new(
            VisualElement::Group {
                children: vec![LayeredElement::new(
                    VisualElement::Rect {
                        rect: KRect::new(0.0, 0.0, 100.0, 100.0),
                        style: FillStrokeStyle::new().with_fill(Color::RED),
                    },
                    0,
                )],
                transform: None,
                clip_rect: Some(KRect::new(0.0, 0.0, 50.0, 50.0)),
            },
            0,
        )];

        let pix = renderer.render(&elements);
        let data = pix.data();
        let w = pix.width() as usize;

        // clip 区域内应为红色
        let inside = data[25 * w + 25];
        assert_eq!(
            inside,
            vello_cpu::color::PremulRgba8::from_u8_array([255, 0, 0, 255])
        );

        // clip 区域外应保持默认背景色 (240,240,240)
        let outside = data[75 * w + 75];
        assert_eq!(
            outside,
            vello_cpu::color::PremulRgba8::from_u8_array([240, 240, 240, 255])
        );
    }

    #[test]
    fn blit_image_premultiplies_straight_rgba() {
        let mut pix = Pixmap::new(2, 2);
        // 直链 RGBA：半透明红色，alpha=128，r=255
        let img_data = Arc::new(vec![
            255, 0, 0, 128, 255, 0, 0, 128, 255, 0, 0, 128, 255, 0, 0, 128,
        ]);
        let img = LayeredElement::new(
            VisualElement::Image {
                bounds: KRect::new(0.0, 0.0, 2.0, 2.0),
                data: img_data,
                width: 2,
                height: 2,
                opacity: Some(1.0),
                fit: ImageFit::Fill,
                border_radius: 0.0,
            },
            0,
        );

        VelloRenderer::blit_image(&mut pix, &img, None);

        let data = pix.data();
        // 预乘后 r 应被 alpha 缩放：255 * 128 / 255 = 128
        let expected = vello_cpu::color::PremulRgba8::from_u8_array([128, 0, 0, 128]);
        for p in data {
            assert_eq!(*p, expected);
        }
    }

    #[test]
    fn blit_image_keeps_opaque_rgba_unchanged() {
        let mut pix = Pixmap::new(1, 1);
        let img_data = Arc::new(vec![255, 128, 64, 255]);
        let img = LayeredElement::new(
            VisualElement::Image {
                bounds: KRect::new(0.0, 0.0, 1.0, 1.0),
                data: img_data,
                width: 1,
                height: 1,
                opacity: Some(1.0),
                fit: ImageFit::Fill,
                border_radius: 0.0,
            },
            0,
        );

        VelloRenderer::blit_image(&mut pix, &img, None);

        assert_eq!(
            pix.data()[0],
            vello_cpu::color::PremulRgba8::from_u8_array([255, 128, 64, 255])
        );
    }
}
