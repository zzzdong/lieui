//! 渲染层：**draw list 展开** + **纯 CPU 光栅化**（设计 §3.8）。
//!
//! ```text
//! Track（保留树）
//!   └─ SceneBuilder::build  ──▶ Scene（扁平 op 列表，含视图态配色、变换、裁剪、不透明度）
//!                                └─ Rasterizer::rasterize ──▶ 持久 Pixmap（只重画脏区行带）
//!                                                              └─ M4：present_with_damage → softbuffer
//! ```
//!
//! 与现状的差别：旧实现每帧走 `cv()` 递归生成 `VisualElement` 嵌套树、再全量 `Pixmap::new` 光栅化；
//! 这里展开成扁平列表（顺序即绘制序），并按脏区只重画受影响的行带。

pub mod raster;
pub mod scene;

pub use raster::{RasterStats, Rasterizer, damage_batches, damage_batches_bands, damage_batches_union, to_vello};
pub use scene::{Op, Scene, SceneBuilder, SceneOptions, SceneStats, TextCache, kind_name};

use lieui_geom::{Color, Rect, Size};
use vello_cpu::Pixmap;

use crate::track::Track;

/// 一次渲染的统计
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderStats {
    pub scene: SceneStats,
    pub raster: RasterStats,
    /// 提交的 op 数
    pub ops: usize,
}

/// 一个窗口的渲染器：复用文本排版缓存与持久 pixmap
pub struct Renderer {
    scene: SceneBuilder,
    raster: Rasterizer,
    opts: SceneOptions,
}

impl Renderer {
    pub fn new(size: Size, background: Color) -> Self {
        Self {
            scene: SceneBuilder::new(),
            raster: Rasterizer::new(size),
            opts: SceneOptions {
                window: size,
                background,
                focus_ring: true,
                theme: crate::theme::Theme::default(),
            },
        }
    }

    /// 逻辑尺寸（布局坐标系）
    pub fn size(&self) -> Size {
        self.opts.window
    }

    /// 物理尺寸（pixmap 尺寸 = 逻辑 × DPI）
    pub fn physical_size(&self) -> Size {
        self.raster.size()
    }

    pub fn scale(&self) -> f32 {
        self.raster.scale()
    }

    /// 窗口逻辑尺寸变化（同时重建 pixmap）
    pub fn resize(&mut self, size: Size) {
        self.opts.window = size;
        self.raster.resize(size);
    }

    /// DPI 变化：只改光栅分辨率，不动布局（调用方应同时整窗标脏）
    pub fn set_scale(&mut self, scale: f32) {
        self.raster.set_scale(scale);
    }

    pub fn background(&self) -> Color {
        self.opts.background
    }

    /// 换底色（主题切换；调用方应同时整窗标脏）
    pub fn set_background(&mut self, c: Color) {
        self.opts.background = c;
    }

    pub fn options(&self) -> &SceneOptions {
        &self.opts
    }

    pub fn options_mut(&mut self) -> &mut SceneOptions {
        &mut self.opts
    }

    /// 当前帧的图像（M4 直接 blit 给 softbuffer；测试直接读像素）
    pub fn pixmap(&self) -> &Pixmap {
        self.raster.pixmap()
    }

    pub fn text_cache_len(&self) -> usize {
        self.scene.text_cache().len()
    }

    /// 一帧：展开 + 按脏区光栅化。
    ///
    /// `damage` 为空且非整窗脏时**按整窗处理**（保守：宁可多画，不可漏画——
    /// 例如"只标了 `PAINT` 但没有矩形"的情况）。
    ///
    /// 关键约束：**场景剔除与光栅批次必须同源**。`damage_batches` 在碎片 >8 或
    /// 面积过半时会把批次退化成整窗；若场景仍按细碎脏区剔除，整窗重画的就是
    /// "只剩脏区元素"的场景，页面其他元素会被擦成底色（gallery 嵌套滚动区
    /// 一滚整页消失就是这个坑）。所以批次先算，退化整窗 ⇒ 场景也不剔除；
    /// 剔除矩形用批次反推的逻辑矩形（含取整外扩），保证重画区内的原语不缺。
    pub fn render(&mut self, track: &Track, damage: &[Rect], damage_all: bool) -> RenderStats {
        let all = damage_all || damage.is_empty();
        let scale = self.raster.scale();
        let physical = self.raster.size();
        let scaled: Vec<Rect> = damage
            .iter()
            .map(|d| Rect::new(d.x * scale, d.y * scale, d.width * scale, d.height * scale))
            .collect();
        let batches = damage_batches(physical, &scaled, all);
        if batches.is_empty() {
            // 脏区完全在窗外 ⇒ 无可画
            return RenderStats::default();
        }
        let full = Rect::new(0.0, 0.0, physical.width, physical.height);
        let fallback = batches.len() == 1 && batches[0] == full;
        let scene_all = all || fallback;
        let cull: Vec<Rect> = batches
            .iter()
            .map(|b| Rect::new(b.x / scale, b.y / scale, b.width / scale, b.height / scale))
            .collect();
        let scene = self.scene.build(track, &self.opts, &cull, scene_all);
        let ops = scene.len();
        let raster = self.raster.rasterize(&scene, damage, scene_all);
        RenderStats {
            scene: scene.stats,
            raster,
            ops,
        }
    }
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("size", &self.opts.window)
            .field("background", &self.opts.background)
            .finish()
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
