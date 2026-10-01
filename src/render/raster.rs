//! 纯 CPU 光栅化：**持久 `Pixmap` + 按脏区做"行带"局部渲染**。
//!
//! 这是 v3 相对现状的关键差异（设计 §3.8）：旧实现每帧 `Pixmap::new(w, h)` + 全量光栅化；
//! 这里 pixmap 跨帧存活，只重画脏区所在的**行带**（全宽 × 若干行），其余像素原样保留。
//!
//! 三个实现要点：
//! 1. 行带而不是任意矩形：`PixmapMut` 要求缓冲区按 `width × 4` 紧密排列，
//!    全宽行带才能直接用持久 pixmap 的字节切片做视图（**无需 unsafe，也不用中间拷贝**）。
//! 2. `RasterizerSettings::offset` 在 vello_cpu 0.2 里是 `(u16, u16)`（不能为负），
//!    所以行带的"上移"由**场景变换**完成：`shift = translate(0, -band.y)` 与每个 op 自身的
//!    变换组合后交给 `set_transform`。
//! 3. 颜色契约：`Pixmap` 是 **premultiplied RGBA8**；`lieui::Color` 是直链（straight），
//!    转换统一走 [`to_vello`]（`AlphaColor::from_rgba8`）。
//!
//! 归 M4：把 pixmap 交给 softbuffer（`present_with_damage`）——那是窗口层的事，
//! 本模块只负责"把场景画进持久 pixmap"。

use lieui_geom::{Color, Rect, Size};
use vello_cpu::kurbo::{
    Affine as KAffine, BezPath, Rect as KRect, RoundedRect, Shape as _, Stroke as KStroke,
};
use vello_cpu::peniko::color::{AlphaColor, Srgb};
use vello_cpu::{
    CompositeMode, Pixmap, PixmapMut, RasterizerSettings, RenderContext, Resources,
};

use crate::render::scene::{Op, Scene};
use crate::transform::Affine;

/// `lieui::Color`（straight RGBA8）→ vello 颜色
pub fn to_vello(c: Color) -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(c.r, c.g, c.b, c.a)
}

fn krect(r: Rect) -> KRect {
    KRect::new(r.x as f64, r.y as f64, r.right() as f64, r.bottom() as f64)
}

/// 逻辑尺寸 × scale → 物理像素尺寸（至少 1×1）
fn physical_of(logical: Size, scale: f32) -> (u16, u16) {
    let w = (logical.width * scale).round().max(1.0) as u16;
    let h = (logical.height * scale).round().max(1.0) as u16;
    (w, h)
}

fn kaffine(a: Affine) -> KAffine {
    let [a1, b1, c1, d1, e1, f1] = a.m;
    KAffine::new([
        a1 as f64, b1 as f64, c1 as f64, d1 as f64, e1 as f64, f1 as f64,
    ])
}

fn rounded_path(r: Rect, radius: f32) -> BezPath {
    let rr = radius
        .max(0.0)
        .min((r.width.min(r.height) * 0.5).max(0.0));
    RoundedRect::from_rect(krect(r), rr as f64).to_path(0.05)
}

/// 一次光栅化的统计
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RasterStats {
    /// 本次渲染的批次（脏矩形）数
    pub batches: usize,
    /// 提交的绘制原语数（= op 数 × 批次数）
    pub ops: usize,
    /// 实际光栅化的像素数（批次面积之和）
    pub pixels: u64,
    /// 是否真的做了光栅化（没有要画的东西 ⇒ false）
    pub rasterized: bool,
}

/// 脏区 → **光栅化批次**（纯函数，可单测）
///
/// - `damage_all`、脏区面积超过窗口 45%、或碎片过多（> 8 块）⇒ 直接整窗一块；
/// - 脏区为空且非 `damage_all` ⇒ 返回空（没有任何要画的）；
/// - 否则把每个脏区**取整到整像素**、裁到窗口内、**去重**后原样返回。
///
/// 为什么不合并成"全宽行带"（曾经的做法）：行带的像素数 = 全宽 × 行数，
/// 一个 37×77 的文本脏区会变成 400×77 —— 脏区收益直接减半。
/// 现在每个矩形单独渲染进复用 `scratch`，再按行拷回持久 pixmap，
/// 光栅化开销 ∝ **脏区面积**。
pub fn damage_batches(size: Size, damage: &[Rect], damage_all: bool) -> Vec<Rect> {
    if size.width <= 0.0 || size.height <= 0.0 {
        return Vec::new();
    }
    let full = Rect::new(0.0, 0.0, size.width, size.height);
    if damage_all {
        return vec![full];
    }

    let mut out: Vec<Rect> = Vec::new();
    let mut area = 0.0f64;
    for d in damage {
        let Some(c) = d.intersect(&full) else { continue };
        let x0 = c.x.floor().max(0.0);
        let y0 = c.y.floor().max(0.0);
        let x1 = c.right().ceil().min(size.width);
        let y1 = c.bottom().ceil().min(size.height);
        if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
            continue;
        }
        let r = Rect::new(x0, y0, x1 - x0, y1 - y0);
        if out.contains(&r) {
            continue;
        }
        area += f64::from(r.width) * f64::from(r.height);
        out.push(r);
    }
    if out.is_empty() {
        return Vec::new();
    }

    let total = f64::from(size.width) * f64::from(size.height);
    if area > total * 0.45 || out.len() > 8 {
        return vec![full];
    }
    out
}

/// 光栅器：持有持久 pixmap、复用 scratch 与 vello 上下文。
///
/// **坐标系**：场景（op）坐标是**逻辑**像素（布局坐标系），pixmap 是**物理**像素。
/// 两者的关系是 [`Rasterizer::scale`]（DPI）：`physical = logical × scale`。
/// 批次渲染时把 `scale` 组合进场景变换，脏区先乘 `scale` 变成物理像素。
pub struct Rasterizer {
    ctx: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
    /// 批次渲染的临时画布（尺寸随批次变化，容量跨帧复用）
    scratch: Pixmap,
    /// 逻辑尺寸（= 布局的窗口尺寸）
    logical: Size,
    scale: f32,
}

impl Rasterizer {
    pub fn new(logical: Size) -> Self {
        Self::with_scale(logical, 1.0)
    }

    pub fn with_scale(logical: Size, scale: f32) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let logical = Size::new(logical.width.max(1.0), logical.height.max(1.0));
        let (w, h) = physical_of(logical, scale);
        Self {
            ctx: RenderContext::new(w, h),
            resources: Resources::new(),
            pixmap: Pixmap::new(w, h),
            scratch: Pixmap::new(w, h),
            logical,
            scale,
        }
    }

    /// 逻辑尺寸（布局坐标系）
    pub fn logical_size(&self) -> Size {
        self.logical
    }

    /// 物理尺寸（pixmap 尺寸）
    pub fn size(&self) -> Size {
        Size::new(f32::from(self.pixmap.width()), f32::from(self.pixmap.height()))
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// 持久 pixmap（M4 直接把它 blit 给 softbuffer；测试直接读像素）
    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }

    pub fn pixmap_mut(&mut self) -> &mut Pixmap {
        &mut self.pixmap
    }

    /// 逻辑尺寸变化：重建 pixmap 与上下文（旧内容作废，调用方应同时整窗标脏）
    pub fn resize(&mut self, logical: Size) {
        self.rebuild(Size::new(logical.width.max(1.0), logical.height.max(1.0)), self.scale);
    }

    /// DPI 变化：只改光栅分辨率，不动布局（逻辑尺寸不变）
    pub fn set_scale(&mut self, scale: f32) {
        if !scale.is_finite() || scale <= 0.0 || (scale - self.scale).abs() < 1e-6 {
            return;
        }
        self.rebuild(self.logical, scale);
    }

    fn rebuild(&mut self, logical: Size, scale: f32) {
        let (w, h) = physical_of(logical, scale);
        self.logical = logical;
        self.scale = scale;
        self.pixmap = Pixmap::new(w, h);
        self.scratch = Pixmap::new(w, h);
        self.ctx = RenderContext::new(w, h);
        self.resources = Resources::new();
    }

    /// 按脏区把 `scene` 光栅化进持久 pixmap。**未落在脏区里的像素保持上一帧的结果。**
    ///
    /// 约定：`scene` 的第一条原语应当是不透明底色（`SceneBuilder` 保证），
    /// 否则批次里的旧像素不会被盖掉。
    pub fn rasterize(&mut self, scene: &Scene, damage: &[Rect], damage_all: bool) -> RasterStats {
        // 脏区是**逻辑**坐标 ⇒ 先乘 scale 变成物理像素
        let physical_size = self.size();
        let scaled: Vec<Rect> = damage
            .iter()
            .map(|d| Rect::new(
                d.x * self.scale,
                d.y * self.scale,
                d.width * self.scale,
                d.height * self.scale,
            ))
            .collect();
        let batches = damage_batches(physical_size, &scaled, damage_all);
        let mut stats = RasterStats {
            batches: batches.len(),
            rasterized: !batches.is_empty(),
            ..Default::default()
        };
        if batches.is_empty() {
            return stats;
        }

        let scale = self.scale;
        for batch in &batches {
            let bw = batch.width.round().clamp(1.0, f32::from(u16::MAX)) as u16;
            let bh = batch.height.round().clamp(1.0, f32::from(u16::MAX)) as u16;
            let (x0, y0) = (batch.x.max(0.0) as usize, batch.y.max(0.0) as usize);

            // 批次画布（复用容量）；场景坐标先按 DPI 放大，再平移到批次原点
            self.ctx.reset_and_resize(bw, bh);
            let shift = KAffine::translate((-(batch.x as f64), -(batch.y as f64)))
                * KAffine::scale(f64::from(scale));
            // 图片 op 不走 vello（手动 blit，见 blit_images）；与 vello 原语分桶
            let mut image_ops: Vec<&Op> = Vec::new();
            for op in scene.ops() {
                if matches!(op, Op::Image { .. }) {
                    image_ops.push(op);
                } else {
                    self.submit(op, shift);
                }
            }
            self.ctx.flush();

            if self.scratch.width() != bw || self.scratch.height() != bh {
                self.scratch.resize(bw, bh);
            }
            {
                let data = self.scratch.data_as_u8_slice_mut();
                if let Some(target) = PixmapMut::new(bw, bh, data) {
                    let settings = RasterizerSettings {
                        composite_mode: CompositeMode::SrcOver,
                        ..Default::default()
                    };
                    self.ctx.render_with(target, &mut self.resources, settings);
                }
            }
            self.ctx.reset();

            // 图片 blit（SrcOver，写进批次画布）：
            // 已知限制：blit 在该批次的所有 vello 原语之后 ⇒ 同批次内图片总在最上层
            if !image_ops.is_empty() {
                for op in &image_ops {
                    if let Op::Image { image, rect, transform } = op {
                        // 逻辑 rect → 批次内物理坐标
                        let tb = transform.bounding_box(*rect);
                        let dst = Rect::new(
                            (tb.x * scale) - batch.x,
                            (tb.y * scale) - batch.y,
                            tb.width * scale,
                            tb.height * scale,
                        );
                        self.blit_image(image, dst);
                    }
                }
            }
            self.ctx.reset();

            // 拷回持久 pixmap（逐行；两侧都是 `PremulRgba8`，无需 unsafe）
            let pw = usize::from(self.pixmap.width());
            let ph = usize::from(self.pixmap.height());
            let n = usize::from(bw);
            for row in 0..usize::from(bh) {
                let dy = y0 + row;
                if dy >= ph {
                    break;
                }
                let dst = dy * pw + x0;
                if dst + n > pw * ph {
                    break;
                }
                let src = row * n;
                let (from, to) = (self.scratch.data(), self.pixmap.data_mut());
                to[dst..dst + n].copy_from_slice(&from[src..src + n]);
            }

            stats.ops += scene.len();
            stats.pixels += u64::from(bw) * u64::from(bh);
        }

        stats
    }

    /// 手动 blit：把 RGBA8（**直通 alpha**）图片按 contain 方式缩放进 `dst`
    /// （批次内物理像素矩形），最近邻采样，SrcOver 写入批次画布（premultiplied）。
    fn blit_image(&mut self, image: &crate::track::ImageData, dst: Rect) {
        if dst.width <= 0.0 || dst.height <= 0.0 || image.width == 0 || image.height == 0 {
            return;
        }
        let s = (dst.width / image.width as f32).min(dst.height / image.height as f32);
        let dw = image.width as f32 * s;
        let dh = image.height as f32 * s;
        let dx = dst.x + (dst.width - dw) * 0.5;
        let dy = dst.y + (dst.height - dh) * 0.5;

        let cw = usize::from(self.scratch.width());
        let chh = usize::from(self.scratch.height());
        let x0 = dx.floor().max(0.0) as usize;
        let y0 = dy.floor().max(0.0) as usize;
        let x1 = ((dx + dw).ceil() as usize).min(cw);
        let y1 = ((dy + dh).ceil() as usize).min(chh);
        if x0 >= x1 || y0 >= y1 {
            return;
        }

        let iw = image.width as usize;
        let ihh = image.height as usize;
        let canvas = self.scratch.data_as_u8_slice_mut();
        for py in y0..y1 {
            let fy = (py as f32 + 0.5 - dy) / s;
            let sy = ((fy.floor() as usize).min(ihh - 1)) * iw * 4;
            for px in x0..x1 {
                let fx = (px as f32 + 0.5 - dx) / s;
                let sx = (fx.floor() as usize).min(iw - 1);
                let si = sy + sx * 4;
                let a = image.rgba[si + 3] as u32;
                let di = (py * cw + px) * 4;
                // SrcOver：premultiplied src + premultiplied dst
                let ia = 255 - a;
                let sr = image.rgba[si] as u32 * a / 255;
                let sg = image.rgba[si + 1] as u32 * a / 255;
                let sb = image.rgba[si + 2] as u32 * a / 255;
                let dr = canvas[di] as u32;
                let dg = canvas[di + 1] as u32;
                let db = canvas[di + 2] as u32;
                let da = canvas[di + 3] as u32;
                canvas[di] = (sr + dr * ia / 255) as u8;
                canvas[di + 1] = (sg + dg * ia / 255) as u8;
                canvas[di + 2] = (sb + db * ia / 255) as u8;
                canvas[di + 3] = (a + da * ia / 255) as u8;
            }
        }
    }

    fn submit(&mut self, op: &Op, shift: KAffine) {
        match op {
            // 图片不走 vello：在 rasterize 里分桶后手动 blit
            Op::Image { .. } => {}
            Op::Rect {
                rect,
                radius,
                color,
                transform,
            } => {
                self.ctx.set_transform(shift * kaffine(*transform));
                self.ctx.set_paint(to_vello(*color));
                if *radius > 0.5 {
                    let path = rounded_path(*rect, *radius);
                    self.ctx.fill_path(&path);
                } else {
                    self.ctx.fill_rect(&krect(*rect));
                }
            }

            Op::Shadow {
                rect,
                radius,
                std_dev,
                color,
                transform,
            } => {
                self.ctx.set_transform(shift * kaffine(*transform));
                self.ctx.set_paint(to_vello(*color));
                self.ctx
                    .fill_blurred_rounded_rect(&krect(*rect), *radius, *std_dev, false);
            }

            Op::Border {
                rect,
                radius,
                width,
                color,
                transform,
            } => {
                self.ctx.set_transform(shift * kaffine(*transform));
                self.ctx.set_paint(to_vello(*color));
                self.ctx.set_stroke(KStroke::new(f64::from(*width)));
                if *radius > 0.5 {
                    let path = rounded_path(*rect, *radius);
                    self.ctx.stroke_path(&path);
                } else {
                    self.ctx.stroke_rect(&krect(*rect));
                }
            }

            Op::Text {
                layout,
                origin,
                color,
                transform,
            } => {
                // glyph 位置相对排版原点；`origin` 通过 glyph_transform 施加
                self.ctx.set_transform(shift * kaffine(*transform));
                let glyph_transform = KAffine::translate((origin.x as f64, origin.y as f64));
                for line in layout.lines() {
                    for item in line.items() {
                        let parley::layout::PositionedLayoutItem::GlyphRun(run) = item else {
                            continue;
                        };
                        let r = run.run();
                        let glyphs: Vec<vello_cpu::Glyph> = run
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
                        self.ctx.set_paint(to_vello(*color));
                        self.ctx
                            .glyph_run(&mut self.resources, r.font())
                            .font_size(r.font_size())
                            .glyph_transform(glyph_transform)
                            .fill_glyphs(glyphs.into_iter());
                    }
                }
            }

            Op::PushClip { rect, transform } => {
                self.ctx.set_transform(shift * kaffine(*transform));
                // 轴对齐矩形用极小容差转路径；圆角裁剪由上层（滚动容器/圆角）另行表达
                let path = krect(*rect).to_path(0.01);
                self.ctx.push_clip_path(&path);
            }
            Op::PopClip => self.ctx.pop_clip_path(),

            Op::PushOpacity { opacity } => self.ctx.push_opacity_layer(*opacity),
            Op::PopOpacity => self.ctx.pop_layer(),
        }
    }
}

impl std::fmt::Debug for Rasterizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rasterizer")
            .field("logical", &self.logical)
            .field("scale", &self.scale)
            .field("size", &self.size())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::scene::Scene;
    use vello_cpu::color::PremulRgba8;

    const W: f32 = 120.0;
    const H: f32 = 80.0;
    const BG: Color = Color::new(255, 255, 255);
    const RED: Color = Color::new(255, 0, 0);
    const BLUE: Color = Color::new(0, 0, 255);

    fn size() -> Size {
        Size::new(W, H)
    }

    /// 用 op 列表直接拼一个场景（跳过保留树，专注光栅层）
    fn scene(ops: Vec<Op>) -> Scene {
        let mut s = Scene::default();
        s.push(Op::Rect {
            rect: Rect::new(0.0, 0.0, W, H),
            radius: 0.0,
            color: BG,
            transform: Affine::IDENTITY,
        });
        for op in ops {
            s.push(op);
        }
        s
    }

    fn full(ops: Vec<Op>) -> Scene {
        scene(ops)
    }

    fn raster(ops: Vec<Op>) -> (Rasterizer, RasterStats) {
        let mut r = Rasterizer::new(size());
        let s = full(ops);
        let stats = r.rasterize(&s, &[], true);
        (r, stats)
    }

    fn px(r: &Rasterizer, x: u16, y: u16) -> PremulRgba8 {
        let pix = r.pixmap();
        pix.data()[usize::from(y) * usize::from(pix.width()) + usize::from(x)]
    }

    fn expect(p: PremulRgba8, c: Color) {
        // 直链 → 预乘（不透明时相同）
        let a = u16::from(c.a);
        let premul = |v: u8| ((u16::from(v) * a) / 255) as u8;
        assert_eq!(
            (p.r, p.g, p.b, p.a),
            (premul(c.r), premul(c.g), premul(c.b), c.a),
            "像素不匹配：{p:?}"
        );
    }

    #[test]
    fn image_op_blits_pixels_contained() {
        // 2×2 四色图放进 4×4 的目标矩形（contain：正好铺满 4×4）
        let img = crate::track::ImageData {
            width: 2,
            height: 2,
            rgba: vec![
                255, 0, 0, 255, // 红
                0, 255, 0, 255, // 绿
                0, 0, 255, 255, // 蓝
                255, 255, 0, 255, // 黄
            ],
        };
        let (r, _) = raster(vec![Op::Image {
            image: std::sync::Arc::new(img),
            rect: Rect::new(2.0, 2.0, 4.0, 4.0),
            transform: Affine::IDENTITY,
        }]);

        // contain 缩放 2 倍：左上象限 = 红、右下象限 = 黄
        assert_eq!(px(&r, 3, 3), PremulRgba8::from_u8_array([255, 0, 0, 255]));
        assert_eq!(px(&r, 5, 5), PremulRgba8::from_u8_array([255, 255, 0, 255]));
        // 矩形外仍是背景
        assert_eq!(px(&r, 1, 1), PremulRgba8::from_u8_array([255, 255, 255, 255]));
        assert_eq!(px(&r, 7, 7), PremulRgba8::from_u8_array([255, 255, 255, 255]));
    }

    #[test]
    fn background_is_filled() {
        let (r, stats) = raster(vec![]);
        assert!(stats.rasterized);
        assert_eq!(stats.batches, 1);
        assert_eq!(stats.pixels, u64::from(W as u16) * u64::from(H as u16));
        for &(x, y) in &[(0u16, 0u16), (60, 40), (119, 79)] {
            expect(px(&r, x, y), BG);
        }
    }

    #[test]
    fn rect_is_painted_at_its_position_with_antialiased_edges() {
        let (r, _) = raster(vec![Op::Rect {
            rect: Rect::new(10.0, 10.0, 20.0, 20.0),
            radius: 0.0,
            color: RED,
            transform: Affine::IDENTITY,
        }]);

        expect(px(&r, 20, 20), RED);
        expect(px(&r, 5, 5), BG);
        expect(px(&r, 50, 50), BG);
    }

    #[test]
    fn rounded_rect_leaves_the_corner_untouched() {
        let (r, _) = raster(vec![Op::Rect {
            rect: Rect::new(10.0, 10.0, 40.0, 40.0),
            radius: 10.0,
            color: RED,
            transform: Affine::IDENTITY,
        }]);
        expect(px(&r, 30, 30), RED);
        expect(px(&r, 11, 11), BG);
    }

    /// M3 的核心承诺：脏区只重画自己那几行，其余像素**原样保留**
    #[test]
    fn damage_band_keeps_the_rest_of_the_frame() {
        let mut r = Rasterizer::new(size());

        // 第一帧：画一块大红
        let s1 = full(vec![Op::Rect {
            rect: Rect::new(0.0, 0.0, W, H),
            radius: 0.0,
            color: RED,
            transform: Affine::IDENTITY,
        }]);
        r.rasterize(&s1, &[], true);
        expect(px(&r, 60, 40), RED);

        // 第二帧：只把 (10,10,20,20) 弄成蓝色，脏区只报告那一块
        let s2 = full(vec![
            Op::Rect {
                rect: Rect::new(0.0, 0.0, W, H),
                radius: 0.0,
                color: RED,
                transform: Affine::IDENTITY,
            },
            Op::Rect {
                rect: Rect::new(10.0, 10.0, 20.0, 20.0),
                radius: 0.0,
                color: BLUE,
                transform: Affine::IDENTITY,
            },
        ]);
        let stats = r.rasterize(&s2, &[Rect::new(10.0, 10.0, 20.0, 20.0)], false);

        assert_eq!(stats.batches, 1);
        assert_eq!(stats.pixels, 400, "只画 20×20");
        expect(px(&r, 20, 20), BLUE);
        expect(px(&r, 60, 40), RED);
    }

    #[test]
    fn several_damage_rects_become_separate_batches() {
        let mut r = Rasterizer::new(size());
        let s = full(vec![]);
        let stats = r.rasterize(
            &s,
            &[
                Rect::new(0.0, 0.0, 10.0, 10.0),
                Rect::new(0.0, 60.0, 10.0, 10.0),
            ],
            false,
        );
        assert_eq!(stats.batches, 2);
        assert_eq!(stats.pixels, 200);
    }

    #[test]
    fn clip_crops_the_primitive() {
        let (r, _) = raster(vec![
            Op::PushClip {
                rect: Rect::new(0.0, 0.0, 40.0, 40.0),
                transform: Affine::IDENTITY,
            },
            Op::Rect {
                rect: Rect::new(0.0, 0.0, W, H),
                radius: 0.0,
                color: RED,
                transform: Affine::IDENTITY,
            },
            Op::PopClip,
        ]);
        expect(px(&r, 20, 20), RED);
        expect(px(&r, 80, 60), BG);
    }

    #[test]
    fn opacity_layer_blends_with_the_background() {
        let (r, _) = raster(vec![
            Op::PushOpacity { opacity: 0.5 },
            Op::Rect {
                rect: Rect::new(0.0, 0.0, 40.0, 40.0),
                radius: 0.0,
                color: Color::new(0, 0, 0),
                transform: Affine::IDENTITY,
            },
            Op::PopOpacity,
        ]);
        let p = px(&r, 20, 20);
        // 半透明黑盖白底 ⇒ 中灰
        assert!((110..=145).contains(&p.r), "期望中灰，得到 {p:?}");
        assert_eq!(p.a, 255);
    }

    #[test]
    fn transform_is_applied() {
        let (r, _) = raster(vec![Op::Rect {
            rect: Rect::new(0.0, 0.0, 20.0, 20.0),
            radius: 0.0,
            color: RED,
            transform: Affine::translate(50.0, 30.0),
        }]);
        expect(px(&r, 10, 10), BG, );
        expect(px(&r, 60, 40), RED);
    }

    #[test]
    fn text_paints_glyphs() {
        let spec = lieui_text::TextSpec {
            font_size: 24.0,
            ..Default::default()
        };
        let layout = std::sync::Arc::new(lieui_text::create_text_layout("Hg", &spec, Color::BLACK));
        assert!(layout.width() > 0.0, "字体可用");

        let (r, _) = raster(vec![Op::Text {
            layout,
            origin: lieui_geom::Point::new(4.0, 4.0),
            color: Color::BLACK,
            transform: Affine::IDENTITY,
        }]);

        // 文本区域里应出现非背景像素
        let mut painted = 0;
        for y in 0..28u16 {
            for x in 0..40u16 {
                let p = px(&r, x, y);
                if p.r < 200 {
                    painted += 1;
                }
            }
        }
        assert!(painted > 5, "应画出字形，实际 {painted} 像素");
    }

    #[test]
    fn shadow_paints_around_the_rect() {
        let (r, _) = raster(vec![Op::Shadow {
            rect: Rect::new(40.0, 30.0, 40.0, 20.0),
            radius: 4.0,
            std_dev: 4.0,
            color: Color::rgba(0, 0, 0, 120),
            transform: Affine::IDENTITY,
        }]);
        let p = px(&r, 60, 40);
        assert!(p.r < 240, "矩形处被阴影压暗：{p:?}");
        // 远处不受影响
        expect(px(&r, 5, 5), BG);
    }

    #[test]
    fn resize_rebuilds_the_pixmap() {
        let mut r = Rasterizer::new(Size::new(10.0, 10.0));
        assert_eq!(r.size(), Size::new(10.0, 10.0));
        r.resize(Size::new(30.0, 20.0));
        assert_eq!(r.pixmap().width(), 30);
        assert_eq!(r.pixmap().height(), 20);
    }

    // ── 纯函数：行带计算 ──

    #[test]
    fn batches_dedupe_and_clamp() {
        let b = damage_batches(
            size(),
            &[
                Rect::new(10.0, 10.0, 5.0, 5.0),
                Rect::new(10.0, 10.0, 5.0, 5.0), // 完全重复 ⇒ 丢弃
                Rect::new(-20.0, -20.0, 30.0, 30.0), // 裁到窗口内 ⇒ (0,0,10,10)
            ],
            false,
        );
        assert_eq!(b.len(), 2, "只丢掉完全重复的那个：{b:?}");
        assert_eq!(b[0], Rect::new(10.0, 10.0, 5.0, 5.0));
        assert_eq!(b[1], Rect::new(0.0, 0.0, 10.0, 10.0));
    }

    #[test]
    fn batches_round_to_whole_pixels() {
        let b = damage_batches(size(), &[Rect::new(10.2, 10.2, 4.0, 5.1)], false);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0], Rect::new(10.0, 10.0, 5.0, 6.0), "10.2..14.2 / 10.2..15.3");
    }

    #[test]
    fn batches_fall_back_to_the_full_window_when_damage_is_large_or_fragmented() {
        assert_eq!(
            damage_batches(size(), &[Rect::new(0.0, 0.0, W, H * 0.5)], false),
            vec![Rect::new(0.0, 0.0, W, H)],
            "面积过半 ⇒ 整窗"
        );
        assert_eq!(
            damage_batches(size(), &[], true),
            vec![Rect::new(0.0, 0.0, W, H)]
        );

        // 碎片过多（> 8）⇒ 整窗（免去多次上下文重建）
        let many: Vec<Rect> = (0..9)
            .map(|i| Rect::new(i as f32, i as f32, 1.0, 1.0))
            .collect();
        assert_eq!(damage_batches(size(), &many, false).len(), 1);
    }

    #[test]
    fn no_damage_means_no_work() {
        assert!(damage_batches(size(), &[], false).is_empty());
        // 完全在窗口外的脏区被丢弃
        assert!(damage_batches(size(), &[Rect::new(-50.0, -50.0, 10.0, 10.0)], false).is_empty());

        let mut r = Rasterizer::new(size());
        let s = full(vec![]);
        let stats = r.rasterize(&s, &[], false);
        assert!(!stats.rasterized);
        assert_eq!(stats.batches, 0);
    }

    #[test]
    fn narrow_damage_only_rasterizes_its_own_area() {
        let mut r = Rasterizer::new(size());
        let s = full(vec![]);
        // 一个 2×3 的小脏区：只应光栅化 6 个像素（不是整行宽度）
        let stats = r.rasterize(&s, &[Rect::new(100.0, 50.0, 2.0, 3.0)], false);
        assert_eq!(stats.pixels, 6);
    }

    #[test]
    fn zero_sized_window_is_a_no_op() {
        let mut r = Rasterizer::new(Size::new(0.0, 0.0));
        let s = full(vec![]);
        let stats = r.rasterize(&s, &[], true);
        assert!(!stats.rasterized || stats.pixels > 0);
        // 不 panic 即可（内部按 1×1 兜底）
        assert!(r.pixmap().width() >= 1);
    }
}
