//! VelloRenderer — 基于 liecharts PixmapRenderer 模式
//!
//! RenderContext + set_paint + fill_rect/fill_path + glyph_run + render_to_pixmap
//! 文本渲染使用 parley TextLayout 中的 glyph 数据 + vello_cpu glyph_run

use vello_cpu::kurbo::{Affine, BezPath, Circle, Rect, Shape, Stroke as KurboStroke};
use vello_cpu::peniko::color::AlphaColor;
use vello_cpu::{
    CompositeMode, Pixmap, RasterizerSettings, RenderContext, RenderSettings, Resources,
};

use crate::render::renderer::Renderer;
use crate::render::visual::{LayeredElement, Stroke, VisualElement};
use crate::view::paint::ImageFit;

/// 光栅化工作线程数。
///
/// - `LIEUI_RENDER_THREADS` 未设置 → `0`（由 vello 自动决定：核数-1，上限 8）。
/// - 设为 `1` 可强制单线程（小场景下规避线程池调度开销）。
/// - 未启用 `parallel` feature 时恒为 `0`（`RenderSettings::num_threads` 不生效）。
pub fn render_threads_from_env() -> u16 {
    std::env::var("LIEUI_RENDER_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok())
        .unwrap_or(0)
}

pub struct VelloRenderer {
    ctx: RenderContext,
    resources: Resources,
    width: u16,
    height: u16,
    /// 跨帧复用的像素缓冲：避免每帧 `Pixmap::new` 的分配与清零。
    ///
    /// 同时是"局部光栅化"的前提——脏区外的像素在帧间被保留。
    pixmap: Pixmap,
    /// 光栅化工作线程数（`0` = 自动）。
    num_threads: u16,
    /// 上一帧各元素的 `(签名, 包围盒)`，用于帧间比较得出 UI 脏区。
    prev_signatures: Vec<(u64, Rect)>,
    /// 本帧 UI 脏区（由 `update_ui_dirty` 计算）。
    ui_dirty: Vec<crate::geometry::Rect>,
    /// 本帧 UI 是否整屏脏（退化为全量更新）。
    ui_full: bool,
}

/// 是否启用 UI 脏区探测（`LIEUI_UI_DIRTY=0` 关闭，等价旧行为）。
fn ui_dirty_enabled() -> bool {
    std::env::var("LIEUI_UI_DIRTY").map_or(true, |v| v.trim() != "0")
}

/// kurbo 矩形 → lieui 矩形。
fn krect_to_rect(r: Rect) -> crate::geometry::Rect {
    crate::geometry::Rect::new(
        r.x0 as f32,
        r.y0 as f32,
        r.width() as f32,
        r.height() as f32,
    )
}

impl VelloRenderer {
    pub fn new(w: u16, h: u16) -> Self {
        Self::with_threads(w, h, render_threads_from_env())
    }

    /// 用指定的光栅化线程数创建渲染器（`0` = 由 vello 自动决定）。
    pub fn with_threads(w: u16, h: u16, num_threads: u16) -> Self {
        Self {
            ctx: Self::make_context(w, h, num_threads),
            resources: Resources::new(),
            width: w,
            height: h,
            pixmap: Pixmap::new(w, h),
            num_threads,
            prev_signatures: Vec::new(),
            ui_dirty: Vec::new(),
            ui_full: true,
        }
    }

    /// 构造渲染上下文：`parallel` feature 下把线程数传给 vello。
    fn make_context(w: u16, h: u16, num_threads: u16) -> RenderContext {
        #[cfg(feature = "parallel")]
        {
            RenderContext::new_with(
                w,
                h,
                RenderSettings {
                    num_threads,
                    ..Default::default()
                },
            )
        }
        #[cfg(not(feature = "parallel"))]
        {
            let _ = num_threads;
            RenderContext::new(w, h)
        }
    }

    pub fn resize(&mut self, w: u16, h: u16) {
        self.width = w;
        self.height = h;
        self.ctx = Self::make_context(w, h, self.num_threads);
        if self.pixmap.width() != w || self.pixmap.height() != h {
            self.pixmap.resize(w, h);
        }
        // 尺寸变化后所有历史签名失效，下一帧整屏重来。
        self.prev_signatures.clear();
        self.ui_full = true;
        self.ui_dirty.clear();
    }
    pub fn width(&self) -> u16 {
        self.width
    }
    pub fn height(&self) -> u16 {
        self.height
    }

    /// 光栅化工作线程数（`0` = 自动）。
    pub fn num_threads(&self) -> u16 {
        self.num_threads
    }

    /// 比较本帧与上一帧的元素签名，算出 **UI 通道的脏区**。
    ///
    /// - `ui_dirty_full() == true`：结构变化（数量/顺序差异大、Group 变化），整屏重来。
    /// - 否则 `ui_dirty()` 给出变化元素的矩形并集（含上一帧的旧矩形，用于擦除）。
    ///
    /// 保守原则：任何"无法定位矩形"的变化（如 Group、无边界元素）一律退化为整屏，
    /// 宁可多画也不可漏更新。可用 `LIEUI_UI_DIRTY=0` 关闭（等价旧行为：整屏脏）。
    pub fn update_ui_dirty(&mut self, elements: &[LayeredElement]) {
        if !ui_dirty_enabled() {
            self.ui_full = true;
            self.ui_dirty.clear();
            self.prev_signatures.clear();
            return;
        }
        let mut full = elements.len() != self.prev_signatures.len();
        let mut dirty: Vec<crate::geometry::Rect> = Vec::new();
        for (i, el) in elements.iter().enumerate() {
            let sig = el.signature();
            let cur = el.element.bounding_rect();
            let changed = self
                .prev_signatures
                .get(i)
                .is_none_or(|(prev_sig, _)| *prev_sig != sig);
            if !changed {
                continue;
            }
            match cur {
                Some(r) => dirty.push(krect_to_rect(r)),
                // 无法定位矩形（Group 等）：退化为整屏，避免漏更新。
                None => full = true,
            }
            // 上一帧该位置的旧矩形也要重画（内容消失/移动后需要擦除）。
            if let Some((_, prev_rect)) = self.prev_signatures.get(i) {
                dirty.push(krect_to_rect(*prev_rect));
            }
        }
        // 上一帧多出来的元素（本帧消失）→ 其区域需重画。
        for (_, prev_rect) in self.prev_signatures.iter().skip(elements.len()) {
            dirty.push(krect_to_rect(*prev_rect));
        }

        self.prev_signatures = elements
            .iter()
            .map(|el| {
                (
                    el.signature(),
                    el.element.bounding_rect().unwrap_or(Rect::ZERO),
                )
            })
            .collect();
        self.ui_full = full;
        self.ui_dirty = if full { Vec::new() } else { dirty };
    }

    /// UI 通道本帧是否整屏脏。
    pub fn ui_dirty_full(&self) -> bool {
        self.ui_full
    }

    /// UI 通道本帧的脏区（仅在 `ui_dirty_full() == false` 时有意义）。
    pub fn ui_dirty(&self) -> &[crate::geometry::Rect] {
        &self.ui_dirty
    }

    /// 上一帧光栅化结果的像素（premul RGBA8，行优先）。
    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }

    /// 上一帧光栅化结果的字节视图，供合屏直接消费（免逐像素转换）。
    pub fn pixmap_bytes(&self) -> &[u8] {
        self.pixmap.data_as_u8_slice()
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
    /// 光栅化并返回**内部持久 pixmap** 的引用（跨帧复用，避免每帧重新分配）。
    fn render(&mut self, elements: &[LayeredElement]) -> &Pixmap {
        let w = self.width;
        let h = self.height;
        // 跨帧复用缓冲：仅尺寸变化时才重建（pixmap 同时是局部光栅化的载体）。
        if self.pixmap.width() != w || self.pixmap.height() != h {
            self.pixmap.resize(w, h);
        }

        // 背景填充（最低层，之后每层都以 SrcOver 合成到其上）。
        self.ctx
            .set_paint(AlphaColor::from_rgba8(240, 240, 240, 255));
        self.ctx.fill_rect(&Rect::new(0.0, 0.0, w as f64, h as f64));
        self.ctx.flush();
        self.ctx.render(&mut self.pixmap, &mut self.resources);
        // 清空命令列表，避免后续层级重复绘制背景。
        self.ctx.reset();

        // 关键：Image 与矢量元素必须按真实 z 顺序合成。
        // 若把所有 Image 后处理 blit 到最顶层，会遮挡 Content 之上更高 z 的
        // Modal / Overlay 等内容（例如 pdfkit 预览图盖住退出确认弹窗）。
        // 因此按 z 升序分组：每一层先绘制矢量元素，再 blit 该层图像，逐层叠加。
        let mut sorted: Vec<&LayeredElement> = elements.iter().collect();
        sorted.sort_by_key(|e| e.z_index());

        // 同 z 层内的 Image 及其裁剪区域。
        let mut images: Vec<(LayeredElement, Option<Rect>)> = Vec::new();
        let mut idx = 0;
        while idx < sorted.len() {
            let z = sorted[idx].z_index();
            images.clear();
            // 收集并绘制当前 z 层级的全部元素（矢量元素直接入 ctx，Image 记录待 blit）。
            while idx < sorted.len() && sorted[idx].z_index() == z {
                self.render_element(sorted[idx], &mut images, None);
                idx += 1;
            }
            // 将该层矢量元素以 SrcOver 合成到 pix，再 blit 该层图像。
            self.ctx.flush();
            self.ctx.render_with(
                &mut self.pixmap,
                &mut self.resources,
                RasterizerSettings {
                    composite_mode: CompositeMode::SrcOver,
                    ..Default::default()
                },
            );
            for (img, clip) in &images {
                Self::blit_image(&mut self.pixmap, img, *clip);
            }
            // 清空命令列表，下一 z 层只携带自身的绘制命令。
            self.ctx.reset();
        }
        &self.pixmap
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
            _ => {
                // 视口剔除：元素完全在裁剪区外时不提交绘制。
                if let Some(clip) = current_clip
                    && let Some(bbox) = el.element.bounding_rect()
                    && (clip.x1 <= bbox.x0
                        || clip.x0 >= bbox.x1
                        || clip.y1 <= bbox.y0
                        || clip.y0 >= bbox.y1)
                {
                    if crate::perf::enabled() {
                        eprintln!(
                            "[clip] skip  bbox=({:.1},{:.1})-({:.1},{:.1}) \
                                     clip=({:.1},{:.1})-({:.1},{:.1})",
                            bbox.x0, bbox.y0, bbox.x1, bbox.y1, clip.x0, clip.y0, clip.x1, clip.y1,
                        );
                    }
                    return; // 完全在裁剪区外，跳过
                }
                self.draw(&el.element)
            }
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
                    (
                        dw,
                        dh,
                        bounds.x0 + (bw - dw) / 2.0,
                        bounds.y0 + (bh - dh) / 2.0,
                    )
                }
                ImageFit::Cover => {
                    let s = (bw / img_w).max(bh / img_h);
                    let dw = img_w * s;
                    let dh = img_h * s;
                    (
                        dw,
                        dh,
                        bounds.x0 + (bw - dw) / 2.0,
                        bounds.y0 + (bh - dh) / 2.0,
                    )
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
                            let ccx = if (dx as f64) < dx0 + r {
                                dx0 + r
                            } else {
                                dx0 + dw - r
                            };
                            let ccy = if (dy as f64) < dy0 + r {
                                dy0 + r
                            } else {
                                dy0 + dh - r
                            };
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
                        let rr = vello_cpu::kurbo::RoundedRect::from_rect(*rect, *r);
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
                self.ctx
                    .fill_blurred_rounded_rect(rect, *radius as f32, *std_dev as f32, false);
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
            VisualElement::SharedSurface { .. } => {
                // 共享表面由 compositor 单独合屏，不走 vello 光栅化。
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

    /// 构造一组覆盖多种图元的元素，用于渲染器级别的一致性/复用测试。
    fn sample_elements() -> Vec<LayeredElement> {
        vec![
            LayeredElement::new(
                VisualElement::Rect {
                    rect: KRect::new(2.0, 2.0, 30.0, 30.0),
                    style: FillStrokeStyle::new().with_fill(Color::RED),
                },
                0,
            ),
            LayeredElement::new(
                VisualElement::RoundedRect {
                    rect: KRect::new(10.0, 10.0, 40.0, 40.0),
                    radius: 6.0,
                    style: FillStrokeStyle::new()
                        .with_fill(Color::new(0, 0, 255))
                        .with_stroke(Color::new(0, 0, 0), 2.0),
                },
                1,
            ),
            LayeredElement::new(
                VisualElement::Group {
                    children: vec![LayeredElement::new(
                        VisualElement::Rect {
                            rect: KRect::new(0.0, 0.0, 60.0, 60.0),
                            style: FillStrokeStyle::new().with_fill(Color::new(0, 255, 0)),
                        },
                        0,
                    )],
                    transform: None,
                    clip_rect: Some(KRect::new(0.0, 0.0, 20.0, 20.0)),
                },
                2,
            ),
        ]
    }

    /// 持久 pixmap：跨帧复用同一块缓冲（避免每帧分配 + 清零）。
    #[test]
    fn pixmap_buffer_is_reused_across_frames() {
        let mut renderer = VelloRenderer::new(64, 64);
        let elements = sample_elements();
        renderer.render(&elements);
        let first = renderer.pixmap().data().as_ptr();
        renderer.render(&elements);
        let second = renderer.pixmap().data().as_ptr();
        assert_eq!(
            first, second,
            "pixmap buffer should be reused instead of reallocated every frame"
        );
    }

    /// 多线程光栅化必须与单线程结果逐像素一致。
    #[test]
    fn thread_count_does_not_change_output() {
        let elements = sample_elements();
        let mut single = VelloRenderer::with_threads(64, 64, 1);
        let mut multi = VelloRenderer::with_threads(64, 64, 4);
        let a = single.render(&elements).data().to_vec();
        let b = multi.render(&elements).data().to_vec();
        assert_eq!(a.len(), b.len());
        assert!(
            a.iter().zip(&b).all(|(x, y)| x == y),
            "single-threaded and multi-threaded rasterization must match"
        );
    }

    /// 帧间无变化 → UI 不产生脏区（可整帧跳过上屏）。
    #[test]
    fn ui_dirty_is_empty_when_nothing_changed() {
        let mut r = VelloRenderer::new(64, 64);
        let els = sample_elements();
        r.update_ui_dirty(&els);
        assert!(r.ui_dirty_full(), "first frame must be a full update");
        r.update_ui_dirty(&els);
        assert!(!r.ui_dirty_full());
        assert!(r.ui_dirty().is_empty(), "unchanged frame has no dirty rect");
    }

    /// 单个元素变化 → 脏区覆盖该元素（新旧矩形都要覆盖，便于擦除）。
    #[test]
    fn ui_dirty_covers_changed_element() {
        let mut r = VelloRenderer::new(64, 64);
        let mut els = sample_elements();
        r.update_ui_dirty(&els);
        // 改第一个矩形的颜色。
        els[0] = LayeredElement::new(
            VisualElement::Rect {
                rect: KRect::new(2.0, 2.0, 30.0, 30.0),
                style: FillStrokeStyle::new().with_fill(Color::new(10, 20, 30)),
            },
            0,
        );
        r.update_ui_dirty(&els);
        assert!(!r.ui_dirty_full());
        let dirty = r.ui_dirty();
        assert!(!dirty.is_empty());
        assert!(
            dirty
                .iter()
                .any(|d| d.x <= 2.0 && d.y <= 2.0 && d.width >= 28.0),
            "dirty should cover the changed rect, got {dirty:?}"
        );
    }

    /// 元素数量变化 / Group 变化 → 退化为整屏（宁可多画不可漏更新）。
    #[test]
    fn ui_dirty_degrades_to_full_on_structural_change() {
        let mut r = VelloRenderer::new(64, 64);
        let els = sample_elements();
        r.update_ui_dirty(&els);
        // 1) 数量变化
        r.update_ui_dirty(&els[..1]);
        assert!(r.ui_dirty_full(), "element count change must force full");

        // 2) Group（无包围盒）内容变化
        let mut els2 = sample_elements();
        r.update_ui_dirty(&els2);
        els2[2] = LayeredElement::new(
            VisualElement::Group {
                children: vec![LayeredElement::new(
                    VisualElement::Rect {
                        rect: KRect::new(0.0, 0.0, 55.0, 55.0),
                        style: FillStrokeStyle::new().with_fill(Color::new(0, 255, 0)),
                    },
                    0,
                )],
                transform: None,
                clip_rect: Some(KRect::new(0.0, 0.0, 20.0, 20.0)),
            },
            2,
        );
        r.update_ui_dirty(&els2);
        assert!(
            r.ui_dirty_full(),
            "group change without bounding box must force full"
        );
    }

    /// 线程数配置应被记录（便于排查并行是否生效）。
    #[test]
    fn thread_count_is_configurable() {
        assert_eq!(VelloRenderer::with_threads(8, 8, 3).num_threads(), 3);
        assert_eq!(VelloRenderer::with_threads(8, 8, 0).num_threads(), 0);
    }

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
