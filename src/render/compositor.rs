//! Compositor — 进程内脏区合屏器
//!
//! 负责把 UI 通道与 SharedSurface 通道上报的脏区合并为少量大矩形，
//! 仅对变化区域做像素合成，避免每帧全量 blit。
//!
//! 对应重构思路：
//! - **widget 级脏区 → 矩形合并**：把分散的脏矩形合拢成少量不重叠大矩形，
//!   减少拷贝/合成次数（想法 1）。
//! - **部分上屏**：只把脏区从各 surface 合成到 backing framebuffer，
//!   软缓冲无 damage 能力时整块 present，但内容已增量更新（想法 4）。

use crate::geometry::Rect;
use crate::render::surface::{SharedSurface, SurfaceId};

/// 一个 compositor 需要合成的脏区集合。
#[derive(Debug, Default)]
pub struct Compositor {
    /// 本帧所有脏区（尚未合并）。
    pub dirty_rects: Vec<Rect>,
    /// 本帧是否有脏区（决定是否需要合屏）。
    pub frame_dirty: bool,
    /// 最终 RGBA8 帧缓冲（与窗口尺寸一致，跨帧复用）。
    pub backing: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Compositor {
    /// 新建一个指定窗口尺寸的 compositor，backing 缓冲以 `bg` 填充。
    pub fn new(width: u32, height: u32, bg: [u8; 4]) -> Self {
        let len = (width as usize) * (height as usize) * 4;
        let mut backing = Vec::with_capacity(len);
        backing.resize(len, 0);
        Self::fill_rect(&mut backing, width, height, &Rect::new(0.0, 0.0, width as f32, height as f32), bg);
        Self {
            dirty_rects: Vec::new(),
            frame_dirty: false,
            backing,
            width,
            height,
        }
    }

    /// 上报一个脏区（UI 通道或 SharedSurface 通道）。
    pub fn add_dirty(&mut self, rect: Rect) {
        let x0 = rect.x.max(0.0).min(self.width as f32);
        let y0 = rect.y.max(0.0).min(self.height as f32);
        let x1 = (rect.x + rect.width).max(0.0).min(self.width as f32);
        let y1 = (rect.y + rect.height).max(0.0).min(self.height as f32);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        self.dirty_rects.push(Rect::new(x0, y0, x1 - x0, y1 - y0));
        self.frame_dirty = true;
    }

    /// 合并脏区：把分散矩形按行合并成"尽可能少的水平条带"。
    ///
    /// 简单实现：对每个 dirty 矩形，把与之有 y 重叠（或相邻）的矩形按 x 取并集，
    /// 合并成一个水平条带。这样同一行的多个小脏区一次拷贝，兼顾合并效果与实现简单。
    pub fn merge(&mut self) -> Vec<Rect> {
        if self.dirty_rects.is_empty() {
            return Vec::new();
        }
        // 按 y0 排序，便于按行合并。
        self.dirty_rects.sort_by(|a, b| {
            a.y
                .partial_cmp(&b.y)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut out: Vec<Rect> = Vec::new();
        for r in self.dirty_rects.drain(..) {
            let mut merged = false;
            // 与已有条带尝试合并：y 范围重叠时水平扩展。
            for band in out.iter_mut() {
                let overlaps_y = r.y < band.y + band.height && r.y + r.height > band.y;
                if overlaps_y {
                    // 扩展当前条带的 x / y 范围。
                    let nx0 = band.x.min(r.x);
                    let ny0 = band.y.min(r.y);
                    let nx1 = (band.x + band.width).max(r.x + r.width);
                    let ny1 = (band.y + band.height).max(r.y + r.height);
                    *band = Rect::new(nx0, ny0, nx1 - nx0, ny1 - ny0);
                    merged = true;
                    break;
                }
            }
            if !merged {
                out.push(r);
            }
        }
        self.dirty_rects = Vec::new();
        out
    }

    /// 把一个 SharedSurface 的脏区合成到 backing。
    ///
    /// `origin` 是表面在窗口中的位置。只处理 dirty 覆盖的区域，未覆盖像素不动。
    pub fn composite_surface(&mut self, surface: &SharedSurface, origin: (f32, f32)) {
        let dirty = surface.take_dirty();
        if dirty.is_empty() {
            return;
        }
        let (ox, oy) = origin;
        let sw = surface.width() as f32;
        let sh = surface.height() as f32;
        let buf = surface.buffer.lock().unwrap();
        for rect in &dirty {
            // 表面局部坐标 → 窗口坐标（加上 origin）。
            let wx0 = rect.x + ox;
            let wy0 = rect.y + oy;
            let wx1 = rect.x + rect.width + ox;
            let wy1 = rect.y + rect.height + oy;
            // 与窗口尺寸求交。
            let cx0 = wx0.max(0.0);
            let cy0 = wy0.max(0.0);
            let cx1 = wx1.min(self.width as f32);
            let cy1 = wy1.min(self.height as f32);
            if cx1 <= cx0 || cy1 <= cy0 {
                continue;
            }
            // 只拷贝交叠区域，并做防御性边界 clamp（避免 surface 与 backing 尺寸
            // 在 resize 的半帧错位时越界）。
            let bw = self.width as usize;
            let bh = self.height as usize;
            let sx0 = (cx0 - ox).max(0.0) as usize;
            let sy0 = (cy0 - oy).max(0.0) as usize;
            // 目标 x/y 范围 clamp 到 backing 内。
            let py0 = (cy0 as usize).min(bh);
            let py1 = (cy1 as usize).min(bh);
            let dx_start = (cx0 as usize).min(bw);
            let dx_end = (cx1 as usize).min(bw);
            if dx_end <= dx_start {
                continue;
            }
            let len = (dx_end - dx_start) * 4;
            for py in py0..py1 {
                let sy = sy0 + (py - cy0 as usize);
                if sy >= sh as usize {
                    break;
                }
                let src_off = (sy * sw as usize + sx0) * 4;
                let dst_off = (py * bw + dx_start) * 4;
                if dst_off + len <= self.backing.len() && src_off + len <= buf.len() {
                    self.backing[dst_off..dst_off + len].copy_from_slice(&buf[src_off..src_off + len]);
                }
            }
        }
    }

    /// 合成一帧：对 UI pixmap 的脏区拷贝 + 各 surface 的脏区合成。
    ///
    /// `ui_pixmap` 为已光栅化的 UI 通道像素（与窗口同尺寸），
    /// `ui_dirty` 为 UI 通道的脏区。共享表面按 z 序在后合成。
    pub fn composite(
        &mut self,
        ui_pixmap: &[u8],
        ui_dirty: &[Rect],
        surfaces: &[&SharedSurface],
        surface_origins: &[(SurfaceId, (f32, f32))],
    ) {
        if !self.frame_dirty && ui_dirty.is_empty() && surfaces.is_empty() {
            return;
        }
        // 1. 拷贝 UI pixmap 的脏区。
        for rect in ui_dirty {
            self.copy_region(ui_pixmap, *rect);
        }
        // 2. 合成各共享表面。
        for s in surfaces {
            if let Some(&(_, origin)) = surface_origins.iter().find(|(id, _)| *id == s.id) {
                self.composite_surface(s, origin);
            }
        }
        self.frame_dirty = false;
    }

    /// 把 src 中某矩形区域的像素拷贝到 backing（src 与 backing 同尺寸）。
    pub fn copy_region(&mut self, src: &[u8], rect: Rect) {
        let x0 = rect.x.max(0.0) as usize;
        let y0 = rect.y.max(0.0) as usize;
        let x1 = ((rect.x + rect.width).min(self.width as f32)) as usize;
        let y1 = ((rect.y + rect.height).min(self.height as f32)) as usize;
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let w = self.width as usize;
        for py in y0..y1 {
            let src_off = (py * w + x0) * 4;
            let dst_off = (py * w + x0) * 4;
            let len = (x1 - x0) * 4;
            self.backing[dst_off..dst_off + len].copy_from_slice(&src[src_off..src_off + len]);
        }
    }

    /// 用颜色填充一个矩形区域（backing 局部重写）。
    pub fn fill_rect(backing: &mut [u8], width: u32, _height: u32, rect: &Rect, color: [u8; 4]) {
        let w = width as usize;
        let x0 = rect.x.max(0.0) as usize;
        let y0 = rect.y.max(0.0) as usize;
        let x1 = (rect.x + rect.width).max(0.0) as usize;
        let y1 = (rect.y + rect.height).max(0.0) as usize;
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        for py in y0..y1 {
            let off = (py * w + x0) * 4;
            for px in x0..x1 {
                let o = off + (px - x0) * 4;
                backing[o..o + 4].copy_from_slice(&color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::surface::SharedSurface;

    #[test]
    fn merge_combines_overlapping_bands() {
        let mut c = Compositor::new(100, 100, [0, 0, 0, 255]);
        // 两个 y 重叠的脏区应合并成一个条带。
        c.add_dirty(Rect::new(0.0, 0.0, 10.0, 20.0));
        c.add_dirty(Rect::new(5.0, 10.0, 10.0, 20.0));
        let merged = c.merge();
        assert_eq!(merged.len(), 1);
        let m = merged[0];
        assert_eq!(m.x, 0.0);
        assert_eq!(m.y, 0.0);
        assert_eq!(m.width, 15.0);
        assert_eq!(m.height, 30.0);
    }

    #[test]
    fn merge_keeps_disjoint_bands() {
        let mut c = Compositor::new(100, 100, [0, 0, 0, 255]);
        c.add_dirty(Rect::new(0.0, 0.0, 10.0, 10.0));
        c.add_dirty(Rect::new(0.0, 50.0, 10.0, 10.0));
        let merged = c.merge();
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn composite_surface_copies_only_dirty_region() {
        let mut c = Compositor::new(20, 20, [0, 0, 0, 255]);
        let s = SharedSurface::with_id(SurfaceId(1), 10, 10);
        // 填充表面左上角一个 4x4 区域为红色。
        {
            let mut buf = s.lock_buffer();
            for py in 0..4 {
                for px in 0..4 {
                    let o = (py * 10 + px) * 4;
                    buf[o..o + 4].copy_from_slice(&[255, 0, 0, 255]);
                }
            }
        }
        // 只标记左上角 4x4 为脏。
        s.damage(Rect::new(0.0, 0.0, 4.0, 4.0));
        c.composite_surface(&s, (0.0, 0.0));
        // 该区域应为红色。
        let o = (0 * 20 + 0) * 4;
        assert_eq!(&c.backing[o..o + 4], &[255, 0, 0, 255]);
        // 脏区外（如 (5,5)）保持背景色。
        let o2 = (5 * 20 + 5) * 4;
        assert_eq!(&c.backing[o2..o2 + 4], &[0, 0, 0, 255]);
    }

    #[test]
    fn composite_surface_respects_origin() {
        let mut c = Compositor::new(20, 20, [0, 0, 0, 255]);
        let s = SharedSurface::with_id(SurfaceId(2), 10, 10);
        {
            let mut buf = s.lock_buffer();
            let o = (0 * 10 + 0) * 4;
            buf[o..o + 4].copy_from_slice(&[0, 255, 0, 255]);
        }
        s.damage(Rect::new(0.0, 0.0, 1.0, 1.0));
        // origin 平移：表面 (0,0) 落在窗口 (5,5)。
        c.composite_surface(&s, (5.0, 5.0));
        let o = (5 * 20 + 5) * 4;
        assert_eq!(&c.backing[o..o + 4], &[0, 255, 0, 255]);
        // 未平移处保持背景。
        let o2 = (0 * 20 + 0) * 4;
        assert_eq!(&c.backing[o2..o2 + 4], &[0, 0, 0, 255]);
    }
}
