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
use vello_cpu::kurbo::{Affine as KAffine, BezPath, Rect as KRect, RoundedRect, Shape as _, Stroke as KStroke};
use vello_cpu::peniko::color::{AlphaColor, Srgb};
use vello_cpu::{Pixmap, PixmapMut, RasterizerSettings, RenderContext, Resources, TargetInit};

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
    KAffine::new([a1 as f64, b1 as f64, c1 as f64, d1 as f64, e1 as f64, f1 as f64])
}

fn rounded_path(r: Rect, radius: f32) -> BezPath {
    let rr = radius.max(0.0).min((r.width.min(r.height) * 0.5).max(0.0));
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
        out.push(r);
    }
    if out.is_empty() {
        return Vec::new();
    }

    // ★ C2：判定退化**之前**先合并（D21）。
    //
    //   实测（`tests/perf_regression.rs` 基线）：**一次滚动产生 302 个碎片**，
    //   "标脏 30 个节点"也有 30 个 —— 而阈值是 8 块⇒ **几乎任何 ≥9 节点变化
    //   都退化为整窗光栅 + 全树 Scene 重建**。也就是说局部渲染在"单控件交互"
    //   之外的所有场景下**都拿不到收益**（这不是"滚动场景的优化"，是普遍失效）。
    //
    //   而这些碎片高度重叠（列表滚动登记的是每个移动行的"旧 ∪ 新"两条带），
    //   先合并即可把块数压到个位数。
    let out = merge_rects(out);

    let area: f64 = out.iter().map(|r| f64::from(r.width) * f64::from(r.height)).sum();
    let total = f64::from(size.width) * f64::from(size.height);
    if area > total * AREA_FALLBACK_RATIO || out.len() > MAX_BATCHES {
        return vec![full];
    }
    out
}

/// 退化阈值：块数上限（C2：合并后仍超过它就整窗重画）
const MAX_BATCHES: usize = 32;
/// 退化阈值：合并后面积占比上限
const AREA_FALLBACK_RATIO: f64 = 0.45;
/// 合并间距（px）：两个矩形间隙 ≤ 此值就并起来。
///
/// 取值 > 0 是必要的：滚动碎片的旧位置与新位置之间**有间隙**（不重合），
/// 只合并"真正相交"的压不动。该值由 `tests/perf_regression.rs` 的基线测试调定。
const MERGE_GAP: f32 = 2.0;

/// 把相交（或间距 ≤ [`MERGE_GAP`]）的脏矩形求并，显著减少块数。
///
/// **只做相交/近邻合并，不做"全部并成一个"** —— 后者会让"分散的 4 个小更新"
/// 退化成接近整窗（那正是 [`damage_batches_union`] 的问题）。
///
/// 面积**只会因合并而变大**（并集 ⊇ 原来），所以仍然保守：
/// 重画面积 ⊇ 真正需要重画的区域，绝不会漏画。
///
/// 复杂度 O(n²)，n 是单帧脏区数（实测滚动一次 302）；合并后块数收敛到个位数，
/// 第二阶段的 `out.contains` 去重也随之变快。
pub fn merge_rects(rects: Vec<Rect>) -> Vec<Rect> {
    if rects.len() <= 1 {
        return rects;
    }
    let mut cur: Vec<Rect> = Vec::with_capacity(rects.len());
    for r in rects {
        let mut acc = r;
        let mut i = 0;
        while i < cur.len() {
            let c = cur[i];
            let near = c.x <= acc.right() + MERGE_GAP
                && acc.x <= c.right() + MERGE_GAP
                && c.y <= acc.bottom() + MERGE_GAP
                && acc.y <= c.bottom() + MERGE_GAP;
            if near {
                acc = acc.union(&c);
                cur.remove(i); // n 小，且合并后迅速收敛
            } else {
                i += 1;
            }
        }
        cur.push(acc);
    }
    cur
}

/// 脏区 → **单个包围盒**批次（最简策略）。
///
/// 把全部脏区合并成一个 union 矩形（取整、裁边）。正确性与 [`damage_batches`] 等价
/// （重画面积 ⊇ 脏区），实现只有几行，但"少量且分散的更新"会退化成接近整窗
/// —— 见 `examples/damage_bench.rs` 里的策略对比。
pub fn damage_batches_union(size: Size, damage: &[Rect], damage_all: bool) -> Vec<Rect> {
    if size.width <= 0.0 || size.height <= 0.0 {
        return Vec::new();
    }
    let full = Rect::new(0.0, 0.0, size.width, size.height);
    if damage_all {
        return vec![full];
    }
    let mut acc: Option<Rect> = None;
    for d in damage {
        let Some(c) = d.intersect(&full) else { continue };
        acc = Some(match acc {
            None => c,
            Some(a) => a.union(&c),
        });
    }
    match acc {
        Some(r) => {
            let r = pixel_snap(r, size);
            if r.width >= 1.0 && r.height >= 1.0 {
                vec![r]
            } else {
                Vec::new()
            }
        }
        None => Vec::new(),
    }
}

/// 脏区 → **水平行带**批次（介于"精确碎片"与"单包围盒"之间）。
///
/// 按 y 区间把脏区合并成若干**互不重叠**的水平带（每带取 x 的 union，相邻同 x 的带再合并）。
///
/// 相比精确碎片模式（[`damage_batches`]）：
/// - 批次数少 ⇒ 批次间**不重叠** ⇒ 同一像素不会被重复合成（精确模式里重叠的碎片会）；
/// - "要重画的矩形集合"由一条纯函数确定 ⇒ 场景剔除与批次**天然同源**（少一类 bug）；
/// - 分散在多行的更新（列表多处变化）仍能保持局部，不会像单包围盒那样吃掉整窗。
///
/// 代价：同一行内左右分开的两块会合并成整行宽度（多画中间那段）。
pub fn damage_batches_bands(size: Size, damage: &[Rect], damage_all: bool) -> Vec<Rect> {
    if size.width <= 0.0 || size.height <= 0.0 {
        return Vec::new();
    }
    let full = Rect::new(0.0, 0.0, size.width, size.height);
    if damage_all {
        return vec![full];
    }

    // ① 裁剪 + 取整 + 去重
    let mut rects: Vec<Rect> = Vec::new();
    for d in damage {
        let Some(c) = d.intersect(&full) else { continue };
        let r = pixel_snap(c, size);
        if r.width >= 1.0 && r.height >= 1.0 && !rects.contains(&r) {
            rects.push(r);
        }
    }
    if rects.is_empty() {
        return Vec::new();
    }

    // ② 所有 y 边界（排序去重）⇒ 相邻边界构成互不重叠的水平区间
    let mut ys: Vec<f32> = Vec::with_capacity(rects.len() * 2);
    for r in &rects {
        ys.push(r.y);
        ys.push(r.bottom());
    }
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ys.dedup_by(|a, b| (*a - *b).abs() < 0.5);

    // ③ 每个 y 区间：取覆盖它的矩形们的 x union
    let mut out: Vec<Rect> = Vec::new();
    for win in ys.windows(2) {
        let (y0, y1) = (win[0], win[1]);
        if y1 - y0 < 1.0 {
            continue;
        }
        let mut xs: Option<(f32, f32)> = None;
        for r in &rects {
            if r.y <= y0 + 0.01 && r.bottom() >= y1 - 0.01 {
                xs = Some(match xs {
                    None => (r.x, r.right()),
                    Some((a, b)) => (a.min(r.x), b.max(r.right())),
                });
            }
        }
        if let Some((x0, x1)) = xs
            && x1 - x0 >= 1.0
        {
            out.push(Rect::new(x0, y0, x1 - x0, y1 - y0));
        }
    }

    // ④ 相邻且 x 范围一致的带合并（减少批次数）
    let mut merged: Vec<Rect> = Vec::with_capacity(out.len());
    for r in out {
        match merged.last_mut() {
            Some(p)
                if (p.x - r.x).abs() < 0.01 && (p.width - r.width).abs() < 0.01 && (p.bottom() - r.y).abs() < 0.01 =>
            {
                p.height = r.bottom() - p.y;
            }
            _ => merged.push(r),
        }
    }
    merged
}

/// 取整到整像素并裁到窗口内（三种策略共用的规范化）
fn pixel_snap(c: Rect, size: Size) -> Rect {
    let x0 = c.x.floor().max(0.0);
    let y0 = c.y.floor().max(0.0);
    let x1 = c.right().ceil().min(size.width);
    let y1 = c.bottom().ceil().min(size.height);
    Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
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
    /// 累计"字形渲染失败"次数（vello_cpu 0.3 起 `fill_glyphs` 会返回 `Result`）。
    ///
    /// 失败只意味着**那一段文字没画出来**（例如字形 id 失效），不该让整帧崩 ⇒
    /// 这里计数而不是 `unwrap`；非零时 `Debug` 输出里能看到（排查字体问题用）。
    glyph_errors: u32,
}

impl Rasterizer {
    pub fn new(logical: Size) -> Self {
        Self::with_scale(logical, 1.0)
    }

    pub fn with_scale(logical: Size, scale: f32) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        let logical = Size::new(logical.width.max(1.0), logical.height.max(1.0));
        let (w, h) = physical_of(logical, scale);
        Self {
            ctx: RenderContext::new(w, h),
            resources: Resources::new(),
            pixmap: Pixmap::new(w, h),
            scratch: Pixmap::new(w, h),
            logical,
            scale,
            glyph_errors: 0,
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
    /// 脏区（**逻辑**坐标）→ 物理像素批次（默认策略，见 [`damage_batches`]）。
    ///
    /// 批次决策与执行分离（[`Self::rasterize_batches`]）：这样剔除与光栅化可以
    /// **共用同一个批次列表**（同源不再靠约定），也便于基准对比不同策略。
    pub fn batches_for(&self, damage: &[Rect], damage_all: bool) -> Vec<Rect> {
        let scaled: Vec<Rect> = damage
            .iter()
            .map(|d| {
                Rect::new(
                    d.x * self.scale,
                    d.y * self.scale,
                    d.width * self.scale,
                    d.height * self.scale,
                )
            })
            .collect();
        damage_batches(self.size(), &scaled, damage_all)
    }

    pub fn rasterize(&mut self, scene: &Scene, damage: &[Rect], damage_all: bool) -> RasterStats {
        let batches = self.batches_for(damage, damage_all);
        self.rasterize_batches(scene, &batches)
    }

    /// 按**给定的物理像素批次**光栅化（批次必须互不重叠，否则重叠区会被重复合成）
    pub fn rasterize_batches(&mut self, scene: &Scene, batches: &[Rect]) -> RasterStats {
        let mut stats = RasterStats {
            batches: batches.len(),
            rasterized: !batches.is_empty(),
            ..Default::default()
        };
        if batches.is_empty() {
            return stats;
        }

        let scale = self.scale;
        for batch in batches {
            let bw = batch.width.round().clamp(1.0, f32::from(u16::MAX)) as u16;
            let bh = batch.height.round().clamp(1.0, f32::from(u16::MAX)) as u16;
            let (x0, y0) = (batch.x.max(0.0) as usize, batch.y.max(0.0) as usize);

            // 批次画布（复用容量）；场景坐标先按 DPI 放大，再平移到批次原点
            self.ctx.reset_and_resize(bw, bh);
            let shift = KAffine::translate((-(batch.x as f64), -(batch.y as f64))) * KAffine::scale(f64::from(scale));
            if self.scratch.width() != bw || self.scratch.height() != bh {
                self.scratch.resize(bw, bh);
            }

            // **按 op 顺序就地合成**：图片 op 绕开 vello（手动 blit），遇到它时必须先
            // 把之前累积的原语渲染掉，否则同批次内的图片会被画到所有原语之上 ——
            // 表现为"后画的浮层/遮罩被图片盖住"（PDF 预览盖住 loading 遮罩就是这个）。
            let mut pending = false;
            // ★ C3：软件裁剪栈（批次坐标系）。
            //
            //   vello 原语走 `PushClip`/`PopClip` 的原生裁剪栈，但**图片是手动 blit**
            //   （`Op::Image` 在 `submit` 里被跳过），所以它此前**完全不受裁剪约束**
            //   ⇒ 滚动容器 / 圆角裁剪里的图片会**溢出到裁剪区外**（可见渲染错误）。
            //
            //   这里在 op 循环里并行维护一份裁剪栈（同样按 DPI 缩放 + 批次原点平移），
            //   `blit_image` 拿它对目标矩形求交。
            let batch_bounds = Rect::new(0.0, 0.0, f32::from(bw), f32::from(bh));
            let mut clip_stack: Vec<Rect> = Vec::new();

            for op in scene.ops() {
                match op {
                    Op::Image { image, rect, transform } => {
                        self.flush_segment(&mut pending, bw, bh, &clip_stack);
                        // 逻辑 rect → 批次内物理坐标
                        let tb = transform.bounding_box(*rect);
                        let dst = Rect::new(
                            (tb.x * scale) - batch.x,
                            (tb.y * scale) - batch.y,
                            tb.width * scale,
                            tb.height * scale,
                        );
                        // 与当前裁剪求交（栈空 ⇒ 用批次边界，即"不额外裁剪"）
                        let clip = clip_stack.last().copied().unwrap_or(batch_bounds);
                        // 整块被裁掉 ⇒ 无事可做（也省掉一次采样循环）
                        if let Some(clipped) = clip.intersect(&dst) {
                            self.blit_image(image, clipped);
                        }
                    }
                    Op::PushClip { rect, transform } => {
                        let tb = transform.bounding_box(*rect);
                        let r = Rect::new(
                            (tb.x * scale) - batch.x,
                            (tb.y * scale) - batch.y,
                            tb.width * scale,
                            tb.height * scale,
                        );
                        // 嵌套裁剪取**交集**（与 vello 裁剪栈语义一致）
                        let clipped = match clip_stack.last() {
                            Some(prev) => prev.intersect(&r).unwrap_or(r),
                            None => r,
                        };
                        clip_stack.push(clipped);
                        self.submit(op, shift);
                        pending = true;
                    }
                    Op::PopClip => {
                        clip_stack.pop();
                        self.submit(op, shift);
                        pending = true;
                    }
                    other => {
                        self.submit(other, shift);
                        pending = true;
                    }
                }
            }
            self.flush_segment(&mut pending, bw, bh, &clip_stack);

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

    /// 把当前累积的 vello 原语渲染进批次画布（`pending` 复位）。
    ///
    /// 只在**图片边界**与**批次结尾**调用：这样图片与原语严格按 op 顺序合成。
    /// 没有待渲染原语时是 no-op（连续多张图片不会白跑一遍）。
    ///
    /// ★ `clip_stack` 参数是**必需的**（C3）：`ctx.reset()` 会**清空 vello 的裁剪栈**，
    /// 而 `PopClip` 按 op 顺序到来 ⇒ reset 之后再遇到 `PopClip` 就会
    /// **"clip stack underflowed" panic**。这个坑只在"裁剪区内有图片"时
    /// 才会触发（只有图片会调用本函数），所以它此前一直潜伏着。
    /// 修法：reset 之后按软件栈**重建**一层裁剪（栈顶已是各层交集，一层就够）。
    fn flush_segment(&mut self, pending: &mut bool, bw: u16, bh: u16, clip_stack: &[Rect]) {
        if !*pending {
            return;
        }
        *pending = false;
        self.ctx.flush();
        {
            let data = self.scratch.data_as_u8_slice_mut();
            if let Some(target) = PixmapMut::new(bw, bh, data) {
                let settings = RasterizerSettings {
                    // vello_cpu 0.3 起 `CompositeMode` 被删掉，混合语义改由
                    // **`TargetInit`** 表达（"目标画布怎么开始"），而且**默认值是
                    // `Clear(透明)`** —— 直接用默认会把批次画布上已有的内容（尤其是
                    // 前面手工 blit 进去的图片）整片擦掉。
                    // 我们要的正是"画在已有内容之上"：`SrcOver` 保住了内容，也保住了
                    // 目标的透明度提示（分段渲染 / 图片 z 序修复依赖这一点）。
                    target_init: TargetInit::SrcOver,
                    ..Default::default()
                };
                self.ctx.render_with(target, &mut self.resources, settings);
            }
        }
        self.ctx.reset();
        // ★ 重建裁剪栈（`reset` 清空过）：栈顶已是各层交集，推一层即可恢复约束。
        //   不重建的话，后面到来的 `PopClip` 会让 vello "clip stack underflowed" panic。
        if let Some(c) = clip_stack.last() {
            let path = krect(*c).to_path(0.01);
            self.ctx.push_clip_path(&path);
        }
    }

    /// 手动 blit：把 RGBA8（**直通 alpha**）图片按 contain 方式缩放进 `dst`
    /// （批次内物理像素矩形），最近邻采样，SrcOver 写入批次画布（premultiplied）。
    ///
    /// 已知限制：不走 vello ⇒ **不参与 `PushClip` 裁剪栈**（滚动容器里的图片不会被裁），
    /// 采样是最近邻。z 序已按 op 顺序处理（见 `flush_segment`）。
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
                        // vello_cpu 0.3：`fill_glyphs` 返回 `Result`（此前是无声无息）。
                        // 失败 = 这一小段文字没画出来（如字形 id 失效），不该让整帧崩 ⇒
                        // 计数（`Debug` 里能看到），不 `unwrap`、也不逐次打日志刷屏。
                        if self
                            .ctx
                            .glyph_run(&mut self.resources, r.font())
                            .font_size(r.font_size())
                            .glyph_transform(glyph_transform)
                            .fill_glyphs(glyphs.into_iter())
                            .is_err()
                        {
                            self.glyph_errors += 1;
                        }
                    }
                }
            }

            Op::PushClip { rect, transform } => {
                self.ctx.set_transform(shift * kaffine(*transform));
                // 轴对齐矩形用极小容差转路径；圆角裁剪由上层（滚动容器/圆角）另行表达
                let path = krect(*rect).to_path(0.01);
                self.ctx.push_clip_path(&path);
            }
            // 0.3：glifo 的 `DrawSink::pop_clip_path` 只是转发到内部固有的 `pop_clip`
            // （用固有方法即可，不必为了一个别名把 glifo 拉成依赖）
            Op::PopClip => self.ctx.pop_clip(),

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
            .field("glyph_errors", &self.glyph_errors)
            .finish()
    }
}

#[cfg(test)]
#[path = "raster_tests.rs"]
mod tests;
