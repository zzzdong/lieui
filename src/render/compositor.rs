//! Compositor — 进程内脏区合屏器
//!
//! 把 UI 通道（vello 光栅化结果）与 SharedSurface 通道（组件自持像素）按脏区
//! 合成进一份与窗口同尺寸的 backing 缓冲，并把本帧脏区返回给上屏层做部分 present。
//!
//! 设计要点：
//! - **脏区驱动**：一帧只处理 `UI 脏区 ∪ surface 脏区 ∪ surface 位置变化区`，
//!   脏区外像素保持上一帧内容，不做全屏拷贝。
//! - **像素契约**：backing 与 UI pixmap 均为 **premultiplied RGBA8**；
//!   SharedSurface 写入的是 **straight（非预乘）RGBA8**，合屏时按 src-over 转换并混合。
//! - **z 序正确**：surface 与 UI 元素按同一 `z_index` 参与排序，z 更高的 UI
//!   （弹窗 / Tooltip）在 surface 之后重新拷回，不会被 surface 盖住。
//! - **残影处理**：surface 位置/尺寸变化或消失时，其旧矩形会被标脏，
//!   由 `copy_region` 用 UI 像素回填。

use crate::geometry::Rect;
use crate::render::surface::{SharedSurface, SurfaceId};
use std::collections::HashMap;
use std::rc::Rc;

// 模块内沿用短名。
use self::rect_intersect as intersect;

/// 一个待合屏的共享表面在窗口中的定位信息。
#[derive(Debug, Clone, Copy)]
pub struct SurfaceEntry {
    /// 表面 id（用于在注册表/像素源中解析）。
    pub id: SurfaceId,
    /// 表面在窗口坐标系中的矩形（由布局给出）。
    pub rect: Rect,
    /// 层序，与 UI 元素的 `z_index` 同一坐标系。
    pub z: i32,
}

/// 一个 compositor 需要合成的脏区集合。
#[derive(Debug, Default)]
pub struct Compositor {
    /// UI 通道上报的、尚未合并的脏区。
    pub dirty_rects: Vec<Rect>,
    /// 本帧是否有脏区（UI 通道侧）。
    pub frame_dirty: bool,
    /// 最终 RGBA8 帧缓冲（与窗口尺寸一致，跨帧复用）。
    pub backing: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// 上一帧各 surface 的矩形：用于位置/尺寸变化时把旧区域标脏（防残影）。
    prev_surface_rects: HashMap<SurfaceId, Rect>,
    /// 本帧合并后的上屏脏区（供部分 present 使用）。
    damage: Vec<Rect>,
}

/// 两个矩形的交；无交返回 `None`。
pub fn rect_intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
}

/// 把矩形裁剪到 backing 内并转成整像素区间 `[x0,x1) × [y0,y1)`。
fn clamp_px(r: Rect, w: u32, h: u32) -> Option<(usize, usize, usize, usize)> {
    let x0 = r.x.max(0.0).floor() as usize;
    let y0 = r.y.max(0.0).floor() as usize;
    let x1 = (r.x + r.width).ceil().min(w as f32).max(0.0) as usize;
    let y1 = (r.y + r.height).ceil().min(h as f32).max(0.0) as usize;
    if x1 <= x0 || y1 <= y0 || x0 >= w as usize || y0 >= h as usize {
        return None;
    }
    Some((x0, y0, x1.min(w as usize), y1.min(h as usize)))
}

/// 脏区合并：把 y 范围重叠（或相邻）的矩形并成尽可能少的水平条带。
///
/// 同一条带内的多个小脏区一次拷贝，兼顾合并效果与实现简单。矩形数量很小
/// （每帧几十个），O(n²) 的合并成本可忽略。
pub fn merge_rects(rects: &[Rect]) -> Vec<Rect> {
    let mut sorted: Vec<Rect> = rects.to_vec();
    sorted.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<Rect> = Vec::new();
    for r in sorted {
        let mut merged = false;
        for band in out.iter_mut() {
            let overlaps_y = r.y < band.y + band.height && r.y + r.height > band.y;
            if overlaps_y {
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
    out
}

impl Compositor {
    /// 新建一个指定窗口尺寸的 compositor，backing 缓冲以 `bg` 填充。
    pub fn new(width: u32, height: u32, bg: [u8; 4]) -> Self {
        let len = (width as usize) * (height as usize) * 4;
        let mut backing = vec![0u8; len];
        Self::fill_rect(
            &mut backing,
            width,
            height,
            &Rect::new(0.0, 0.0, width as f32, height as f32),
            bg,
        );
        Self {
            dirty_rects: Vec::new(),
            frame_dirty: false,
            backing,
            width,
            height,
            prev_surface_rects: HashMap::new(),
            damage: Vec::new(),
        }
    }

    /// 窗口尺寸变化后重建 backing。旧的 surface 位置记录作废（整屏都会重画）。
    pub fn resize(&mut self, width: u32, height: u32, bg: [u8; 4]) {
        if self.width == width && self.height == height {
            return;
        }
        *self = Self::new(width, height, bg);
    }

    /// 上报一个 UI 通道脏区。
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

    /// 标记整屏为脏（首帧 / 尺寸变化 / 全量 rebuild 时使用）。
    pub fn dirty_all(&mut self) {
        self.add_dirty(Rect::new(0.0, 0.0, self.width as f32, self.height as f32));
    }

    /// 合并已上报的 UI 脏区（消费 `dirty_rects`）。
    pub fn merge(&mut self) -> Vec<Rect> {
        let rects = std::mem::take(&mut self.dirty_rects);
        self.frame_dirty = false;
        merge_rects(&rects)
    }

    /// 判断给定定位是否与上一帧不同（有 surface 新增 / 消失 / 移动 / 缩放）。
    ///
    /// 位置变化时合成需要 UI 像素回填旧区域，调用方可据此决定是否需要重新光栅化。
    pub fn surfaces_changed_from(&self, entries: &[SurfaceEntry]) -> bool {
        if entries.len() != self.prev_surface_rects.len() {
            return true;
        }
        entries.iter().any(|e| {
            self.prev_surface_rects.get(&e.id).is_none_or(|prev| {
                (prev.x - e.rect.x).abs() > 0.5
                    || (prev.y - e.rect.y).abs() > 0.5
                    || (prev.width - e.rect.width).abs() > 0.5
                    || (prev.height - e.rect.height).abs() > 0.5
            })
        })
    }

    /// 同步本帧的 surface 定位信息。
    ///
    /// 与上一帧相比发生**移动 / 缩放 / 消失**的 surface，其**旧矩形**会被标脏，
    /// 后续合成时用 UI 像素回填，避免旧位置的像素残留在 backing 中。
    pub fn sync_surfaces(&mut self, entries: &[SurfaceEntry]) {
        let mut current: HashMap<SurfaceId, Rect> = HashMap::with_capacity(entries.len());
        for e in entries {
            current.insert(e.id, e.rect);
        }
        // 旧矩形：消失或位置/尺寸变化 → 标脏（回填 UI）。
        let stale: Vec<Rect> = self
            .prev_surface_rects
            .iter()
            .filter_map(|(id, prev)| {
                let changed = match current.get(id) {
                    Some(now) => {
                        (now.x - prev.x).abs() > 0.5
                            || (now.y - prev.y).abs() > 0.5
                            || (now.width - prev.width).abs() > 0.5
                            || (now.height - prev.height).abs() > 0.5
                    }
                    None => true,
                };
                changed.then_some(*prev)
            })
            .collect();
        for r in stale {
            self.add_dirty(r);
        }
        // 新矩形：新出现或位置/尺寸变化 → 标脏（重新合屏）。
        for e in entries {
            if self.prev_surface_rects.get(&e.id) != Some(&e.rect) {
                self.add_dirty(e.rect);
            }
        }
        self.prev_surface_rects = current;
    }

    /// 合成一帧：返回本帧的**上屏脏区**（已合并为少量条带）。
    ///
    /// - `ui`：UI 通道像素（premul RGBA8，与窗口同尺寸，来自 vello pixmap）。
    /// - `surfaces` / `entries`：本帧参与合屏的共享表面及其定位/层序。
    /// - `occluders`：可能盖在 surface 之上的 UI 元素 `(矩形, z)`；z 高于某
    ///   surface 时，在 surface 之后重新拷回，保证浮层不被遮挡。
    pub fn composite(
        &mut self,
        ui: &[u8],
        surfaces: &[Rc<SharedSurface>],
        entries: &[SurfaceEntry],
        occluders: &[(Rect, i32)],
    ) -> Vec<Rect> {
        // 1. 取出各 surface 的脏区（局部坐标），换算到窗口坐标后并入脏区集合。
        let mut surf_dirty: Vec<Vec<Rect>> = Vec::with_capacity(surfaces.len());
        let mut damage: Vec<Rect> = std::mem::take(&mut self.dirty_rects);
        self.frame_dirty = false;
        for s in surfaces {
            // 未被触及的 surface 直接跳过，省一次跨线程锁。
            let rects = if s.is_touched() {
                s.take_dirty()
            } else {
                Vec::new()
            };
            if let Some(e) = entries.iter().find(|e| e.id == s.id) {
                for r in &rects {
                    damage.push(Rect::new(r.x + e.rect.x, r.y + e.rect.y, r.width, r.height));
                }
            }
            surf_dirty.push(rects);
        }
        if damage.is_empty() {
            self.damage.clear();
            return Vec::new();
        }
        let merged = merge_rects(&damage);

        // 2. 统一按 z 排序：surface 与"可能盖住它的 UI 元素"混排，逐个处理。
        let mut items: Vec<(i32, Item)> = Vec::with_capacity(surfaces.len() + occluders.len());
        for (i, s) in surfaces.iter().enumerate() {
            let z = entries
                .iter()
                .find(|e| e.id == s.id)
                .map(|e| e.z)
                .unwrap_or(0);
            items.push((z, Item::Surface(i)));
        }
        for (i, (_rect, z)) in occluders.iter().enumerate() {
            items.push((*z, Item::Ui(i)));
        }
        items.sort_by_key(|(z, _)| *z);

        // 3. 逐条带：先铺 UI 底图，再按 z 升序覆盖 surface / 回填高层 UI。
        for region in &merged {
            self.copy_region(ui, *region);
            for (_, item) in &items {
                match item {
                    Item::Surface(i) => {
                        let Some(e) = entries.iter().find(|x| x.id == surfaces[*i].id) else {
                            continue;
                        };
                        for r in &surf_dirty[*i] {
                            let wr = Rect::new(r.x + e.rect.x, r.y + e.rect.y, r.width, r.height);
                            if let Some(ir) = intersect(wr, *region) {
                                self.blit_surface(&surfaces[*i], e.rect, ir);
                            }
                        }
                    }
                    Item::Ui(i) => {
                        if let Some(ir) = intersect(occluders[*i].0, *region) {
                            self.copy_region(ui, ir);
                        }
                    }
                }
            }
        }

        self.damage = merged.clone();
        merged
    }

    /// 取走本帧上屏脏区（供 `present_with_damage` 使用）。
    pub fn take_damage(&mut self) -> Vec<Rect> {
        std::mem::take(&mut self.damage)
    }

    /// 把 `surface` 在 `region`（窗口坐标）内的像素合成到 backing。
    ///
    /// `dest` 是 surface 在窗口中的矩形；`region` 必须已与 `dest` 求交。
    fn blit_surface(&mut self, surface: &SharedSurface, dest: Rect, region: Rect) {
        let Some((x0, y0, x1, y1)) = clamp_px(region, self.width, self.height) else {
            return;
        };
        let buf = surface.buffer.lock().unwrap();
        let sw = surface.width() as usize;
        let sh = surface.height() as usize;
        if sw == 0 || sh == 0 {
            return;
        }
        let bw = self.width as usize;
        // region 在 surface 局部坐标中的起点。
        let lx = (x0 as f32 - dest.x).max(0.0) as usize;
        let ly0 = (y0 as f32 - dest.y).max(0.0) as usize;
        for (row, py) in (y0..y1).enumerate() {
            let sy = ly0 + row;
            if sy >= sh {
                break;
            }
            let mut src_off = (sy * sw + lx) * 4;
            let mut dst_off = (py * bw + x0) * 4;
            for _ in x0..x1 {
                if src_off + 4 > buf.len() || dst_off + 4 > self.backing.len() {
                    break;
                }
                let sa = buf[src_off + 3];
                if sa == 255 {
                    // 不透明：straight == premul，直接覆盖（memcpy 快速路径）。
                    self.backing[dst_off..dst_off + 4].copy_from_slice(&buf[src_off..src_off + 4]);
                } else if sa != 0 {
                    // 半透明：src（straight）→ premul 后按 src-over 混合到 premul dst。
                    let a = sa as f32 / 255.0;
                    let inv = 1.0 - a;
                    for c in 0..3 {
                        let s = (buf[src_off + c] as f32 * a
                            + self.backing[dst_off + c] as f32 * inv)
                            .clamp(0.0, 255.0) as u8;
                        self.backing[dst_off + c] = s;
                    }
                    let da = self.backing[dst_off + 3] as f32 / 255.0;
                    self.backing[dst_off + 3] = ((a + da * inv).clamp(0.0, 1.0) * 255.0) as u8;
                }
                src_off += 4;
                dst_off += 4;
            }
        }
    }

    /// 把一个 SharedSurface 的脏区合成到 backing（旧 API，供简单场景/测试使用）。
    ///
    /// `origin` 是表面在窗口中的位置。只处理 dirty 覆盖的区域，未覆盖像素不动。
    pub fn composite_surface(&mut self, surface: &SharedSurface, origin: (f32, f32)) {
        let dirty = surface.take_dirty();
        let dest = Rect::new(
            origin.0,
            origin.1,
            surface.width() as f32,
            surface.height() as f32,
        );
        for r in dirty {
            let wr = Rect::new(r.x + origin.0, r.y + origin.1, r.width, r.height);
            if let Some(ir) = intersect(wr, dest) {
                self.blit_surface(surface, dest, ir);
            }
        }
    }

    /// 把 src 中某矩形区域的像素拷贝到 backing（src 与 backing 同尺寸）。
    pub fn copy_region(&mut self, src: &[u8], rect: Rect) {
        let Some((x0, y0, x1, y1)) = clamp_px(rect, self.width, self.height) else {
            return;
        };
        let w = self.width as usize;
        let len = (x1 - x0) * 4;
        for py in y0..y1 {
            let off = (py * w + x0) * 4;
            if off + len > self.backing.len() || off + len > src.len() {
                break;
            }
            self.backing[off..off + len].copy_from_slice(&src[off..off + len]);
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
                if o + 4 <= backing.len() {
                    backing[o..o + 4].copy_from_slice(&color);
                }
            }
        }
    }
}

/// 合屏项：按 z 排序后依次处理。
enum Item {
    /// 第 i 个共享表面。
    Surface(usize),
    /// 第 i 个可能遮挡 surface 的 UI 元素。
    Ui(usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::surface::SharedSurface;

    fn opaque_surface(id: u64, w: u32, h: u32, color: [u8; 4]) -> Rc<SharedSurface> {
        let s = Rc::new(SharedSurface::with_id(SurfaceId(id), w, h));
        {
            let mut buf = s.lock_buffer();
            for px in buf.chunks_exact_mut(4) {
                px.copy_from_slice(&color);
            }
        }
        s
    }

    #[test]
    fn merge_combines_overlapping_bands() {
        let mut c = Compositor::new(100, 100, [0, 0, 0, 255]);
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
        let s = Rc::new(SharedSurface::with_id(SurfaceId(1), 10, 10));
        {
            let mut buf = s.lock_buffer();
            for py in 0..4 {
                for px in 0..4 {
                    let o = (py * 10 + px) * 4;
                    buf[o..o + 4].copy_from_slice(&[255, 0, 0, 255]);
                }
            }
        }
        s.damage(Rect::new(0.0, 0.0, 4.0, 4.0));
        c.composite_surface(&s, (0.0, 0.0));
        let o = 0;
        assert_eq!(&c.backing[o..o + 4], &[255, 0, 0, 255]);
        // 脏区外保持背景色。
        let o2 = (5 * 20 + 5) * 4;
        assert_eq!(&c.backing[o2..o2 + 4], &[0, 0, 0, 255]);
    }

    #[test]
    fn composite_surface_respects_origin() {
        let mut c = Compositor::new(20, 20, [0, 0, 0, 255]);
        let s = Rc::new(SharedSurface::with_id(SurfaceId(2), 10, 10));
        {
            let mut buf = s.lock_buffer();
            buf[0..4].copy_from_slice(&[0, 255, 0, 255]);
        }
        s.damage(Rect::new(0.0, 0.0, 1.0, 1.0));
        c.composite_surface(&s, (5.0, 5.0));
        let o = (5 * 20 + 5) * 4;
        assert_eq!(&c.backing[o..o + 4], &[0, 255, 0, 255]);
        let o2 = 0;
        assert_eq!(&c.backing[o2..o2 + 4], &[0, 0, 0, 255]);
    }

    /// 只有脏区被触碰：surface 脏区之外的 backing 像素保持原样。
    #[test]
    fn composite_touches_only_damage_region() {
        let mut c = Compositor::new(20, 20, [0, 0, 0, 255]);
        let ui = vec![9u8; 20 * 20 * 4];
        let s = opaque_surface(1, 20, 20, [255, 0, 0, 255]);
        s.damage(Rect::new(0.0, 0.0, 2.0, 2.0));
        let entries = vec![SurfaceEntry {
            id: s.id,
            rect: Rect::new(0.0, 0.0, 20.0, 20.0),
            z: 0,
        }];
        let damage = c.composite(&ui, &[s], &entries, &[]);
        // 脏区合并后应只有 1 个 2x2 条带。
        assert_eq!(damage.len(), 1);
        assert_eq!(damage[0].width, 2.0);
        // 脏区内为 surface 红色；脏区外仍是背景色（未被 UI 覆盖）。
        let o = 0;
        assert_eq!(&c.backing[o..o + 4], &[255, 0, 0, 255]);
        let outside = (10 * 20 + 10) * 4;
        assert_eq!(&c.backing[outside..outside + 4], &[0, 0, 0, 255]);
    }

    /// surface 移动后，旧位置区域被标脏并由 UI 像素回填（无残影）。
    #[test]
    fn moved_surface_marks_old_region_dirty() {
        let mut c = Compositor::new(20, 20, [0, 0, 0, 255]);
        let s = opaque_surface(1, 5, 5, [255, 0, 0, 255]);
        let e1 = SurfaceEntry {
            id: s.id,
            rect: Rect::new(0.0, 0.0, 5.0, 5.0),
            z: 0,
        };
        c.sync_surfaces(&[e1]);
        let _ = c.merge(); // 消费掉"新出现"的脏区
        let e2 = SurfaceEntry {
            id: s.id,
            rect: Rect::new(10.0, 10.0, 5.0, 5.0),
            z: 0,
        };
        c.sync_surfaces(&[e2]);
        let dirty = c.merge();
        // 旧矩形与新矩形都应进入脏区。
        assert!(dirty.iter().any(|r| r.x == 0.0 && r.y == 0.0));
        assert!(dirty.iter().any(|r| r.x == 10.0 && r.y == 10.0));
    }

    /// z 更高的 UI 元素（弹窗）在 surface 之后重新拷回，不被 surface 盖住。
    #[test]
    fn higher_z_ui_occluder_is_restored_over_surface() {
        let mut c = Compositor::new(20, 20, [0, 0, 0, 255]);
        // UI 通道：整屏青色。
        let mut ui = vec![0u8; 20 * 20 * 4];
        for px in ui.chunks_exact_mut(4) {
            px.copy_from_slice(&[0, 255, 255, 255]);
        }
        let s = opaque_surface(1, 20, 20, [255, 0, 0, 255]);
        s.damage(Rect::new(0.0, 0.0, 20.0, 20.0));
        let entries = vec![SurfaceEntry {
            id: s.id,
            rect: Rect::new(0.0, 0.0, 20.0, 20.0),
            z: 0,
        }];
        // 弹窗：z=10，覆盖左上角 4x4。
        let occluders = vec![(Rect::new(0.0, 0.0, 4.0, 4.0), 10)];
        c.composite(&ui, &[s], &entries, &occluders);
        // 弹窗区域应是 UI 青色，而非 surface 红色。
        let o = 0;
        assert_eq!(&c.backing[o..o + 4], &[0, 255, 255, 255]);
        // 弹窗之外仍是 surface 红色。
        let outside = (10 * 20 + 10) * 4;
        assert_eq!(&c.backing[outside..outside + 4], &[255, 0, 0, 255]);
    }

    /// 半透明 surface：按 src-over 混合，且与背景/UI 混合后颜色正确。
    #[test]
    fn translucent_surface_blends_src_over() {
        let mut c = Compositor::new(4, 4, [0, 0, 0, 255]);
        let s = Rc::new(SharedSurface::with_id(SurfaceId(3), 4, 4));
        {
            let mut buf = s.lock_buffer();
            // straight 白色，alpha=50%。
            for px in buf.chunks_exact_mut(4) {
                px.copy_from_slice(&[255, 255, 255, 128]);
            }
        }
        s.damage(Rect::new(0.0, 0.0, 4.0, 4.0));
        let entries = vec![SurfaceEntry {
            id: s.id,
            rect: Rect::new(0.0, 0.0, 4.0, 4.0),
            z: 0,
        }];
        // UI 通道：不透明黑底（premul，alpha=255）。
        let mut ui = vec![0u8; 4 * 4 * 4];
        for px in ui.chunks_exact_mut(4) {
            px[3] = 255;
        }
        c.composite(&ui, &[s], &entries, &[]);
        let o = 0;
        // 黑底 + 50% 白 ≈ 128。
        let r = c.backing[o];
        assert!(
            (120..=136).contains(&r),
            "expected ~128 for 50% white over black, got {r}"
        );
        assert_eq!(c.backing[o + 3], 255);
    }
}
