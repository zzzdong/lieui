//! SharedSurface — 组件自持的共享像素表面 + 脏区跟踪
//!
//! 面向高频大画布组件（如 terminal）：它自己拥有、自己绘制、按需提交像素，
//! 不经过 `builder → reconcile → layout → render-tree → raster` 全链路。
//!
//! 设计要点（对应重构思路）：
//! - **widget 级脏区**：组件把整个变化区域（而非逐 cell 精细矩形）标记为 dirty。
//! - **buffer 可跨线程**：`Arc<Mutex<Vec<u8>>>`，允许外部数据线程写像素，
//!   主线程 compositor 合屏时加锁读取。
//! - **与 lieui 的 `Pixmap` 解耦**：组件自持 RGBA8 缓冲，lieui 只把它当作
//!   一个"合屏单元"合成到最终 framebuffer。

use crate::geometry::Rect;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// 表面唯一 id，用于在 compositor 中标识各共享表面。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceId(pub u64);

// 全局表面注册表：SurfaceId → Weak<SharedSurface>（thread-local，兼容单线程 Rc 模型）。
// SharedSurface 构造时自注册，析构时自动失效（Weak 指向空）。
// 供 compositor 在渲染阶段按 id 解析出 Rc<SharedSurface> 以读取像素/脏区。
type Registry = RefCell<Option<HashMap<SurfaceId, std::rc::Weak<SharedSurface>>>>;
thread_local! {
    static SURFACE_REGISTRY: Registry = const { RefCell::new(None) };
}

fn with_registry<R>(
    f: impl FnOnce(&mut HashMap<SurfaceId, std::rc::Weak<SharedSurface>>) -> R,
) -> R {
    SURFACE_REGISTRY.with(|r| {
        let mut guard = r.borrow_mut();
        let map = guard.get_or_insert_with(HashMap::new);
        f(map)
    })
}

/// 按 id 查找一个存活的共享表面。返回 `Some` 仅当该 surface 仍被持有。
///
/// 若注册表中的 `Weak` 已失效（surface 被释放），会顺手移除该条目，避免
/// 长生命周期进程里注册表只增不减。
pub fn resolve_surface(id: SurfaceId) -> Option<Rc<SharedSurface>> {
    with_registry(|map| match map.get(&id).and_then(|w| w.upgrade()) {
        Some(s) => Some(s),
        None => {
            map.remove(&id);
            None
        }
    })
}

/// 注册表面（构造时自动调用）。
fn register_surface(surface: &Rc<SharedSurface>) {
    with_registry(|map| {
        map.insert(surface.id, Rc::downgrade(surface));
    });
}

/// 注销表面（内部管理，通常无需手动调用）。
pub fn unregister_surface(id: SurfaceId) {
    with_registry(|map| {
        map.remove(&id);
    });
}

/// 一个可被 compositor 合屏的共享像素表面。
///
/// 由高频组件（如 terminal）自持。RGBA8 像素缓冲由组件自己维护，
/// 通过 [`SharedSurface::damage`] 标记变化矩形，由 compositor 消费脏区合屏。
///
/// **像素格式约定**：buffer 写入 **straight（非预乘）RGBA8**。compositor 合屏时
/// 会按 src-over 转预乘并与 UI 通道（premul）混合；alpha=255 走 memcpy 快速路径。
pub struct SharedSurface {
    pub id: SurfaceId,
    /// RGBA8 像素缓冲（行优先，`width * height * 4` 字节）。
    /// 用 `Arc<Mutex>` 允许外部数据线程写入，主线程合屏时加锁读取。
    pub buffer: Arc<Mutex<Vec<u8>>>,
    /// 表面宽（内部可变，支持 resize）。
    width: Cell<u32>,
    /// 表面高（内部可变，支持 resize）。
    height: Cell<u32>,
    /// 自上次提交以来发生变化的矩形集合（由组件自行标记）。
    dirty: Mutex<Vec<Rect>>,
    /// 本帧是否被触及（决定是否需要合屏）。
    touched: Cell<bool>,
}

impl SharedSurface {
    /// 新建一个全透明的共享表面。自动分配全局唯一 id 并自注册到注册表。
    pub fn new(width: u32, height: u32) -> Rc<Self> {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = SurfaceId(NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        let surf = Rc::new(Self::with_id(id, width, height));
        register_surface(&surf);
        surf
    }

    /// 用显式 id 构造（注册表由 `new` 自动维护，本方法供测试/内部复用）。
    pub fn with_id(id: SurfaceId, width: u32, height: u32) -> Self {
        let len = (width as usize) * (height as usize) * 4;
        Self {
            id,
            buffer: Arc::new(Mutex::new(vec![0u8; len])),
            width: Cell::new(width),
            height: Cell::new(height),
            dirty: Mutex::new(Vec::new()),
            touched: Cell::new(false),
        }
    }

    /// 表面宽度（像素）。
    pub fn width(&self) -> u32 {
        self.width.get()
    }

    /// 表面高度（像素）。
    pub fn height(&self) -> u32 {
        self.height.get()
    }

    /// 调整表面尺寸。重建像素缓冲为全透明，并标记整张为脏。
    ///
    /// 常用于窗口 resize 时终端网格自适应。`Rc` 引用保持不变，widget 无需重建。
    pub fn resize(&self, width: u32, height: u32) {
        let len = (width as usize) * (height as usize) * 4;
        *self.buffer.lock().unwrap() = vec![0u8; len];
        self.width.set(width);
        self.height.set(height);
        self.damage_all();
    }

    /// 组件标记某个区域变化（注意：只在变化时调用，勿每帧整屏标记）。
    ///
    /// 采用 **widget 级脏区**：调用方可直接传入整个组件矩形，内部会与表面尺寸
    /// 求交，避免越界。多个脏区会累积，由 compositor 消费时统一合并。
    pub fn damage(&self, rect: Rect) {
        let w = self.width.get() as f32;
        let h = self.height.get() as f32;
        // 与表面尺寸求交，避免脏区越界。
        let x0 = rect.x.max(0.0).min(w);
        let y0 = rect.y.max(0.0).min(h);
        let x1 = (rect.x + rect.width).max(0.0).min(w);
        let y1 = (rect.y + rect.height).max(0.0).min(h);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        self.dirty
            .lock()
            .unwrap()
            .push(Rect::new(x0, y0, x1 - x0, y1 - y0));
        self.touched.set(true);
    }

    /// 标记整张表面为脏（resize / 主题变化等全量更新场景）。
    pub fn damage_all(&self) {
        self.damage(Rect::new(
            0.0,
            0.0,
            self.width.get() as f32,
            self.height.get() as f32,
        ));
    }

    /// 清空脏区并返回（compositor 消费后调用）。
    pub fn take_dirty(&self) -> Vec<Rect> {
        let rects = std::mem::take(&mut *self.dirty.lock().unwrap());
        if rects.is_empty() {
            self.touched.set(false);
        }
        rects
    }

    /// 本帧是否被触及（是否需要合屏）。
    pub fn is_touched(&self) -> bool {
        self.touched.get()
    }

    /// 加锁取得 buffer 的可变写访问（供组件绘制像素）。
    pub fn lock_buffer(&self) -> std::sync::MutexGuard<'_, Vec<u8>> {
        self.buffer.lock().unwrap()
    }
}

impl Drop for SharedSurface {
    /// 释放时自动注销，避免注册表残留无效 `Weak` 条目。
    ///
    /// 这里用 `try_with` 容错：surface 可能在 thread_local 已销毁的阶段被 drop
    /// （线程退出），此时静默跳过注销。
    fn drop(&mut self) {
        let _ = SURFACE_REGISTRY.try_with(|r| {
            if let Ok(mut guard) = r.try_borrow_mut()
                && let Some(map) = guard.as_mut()
            {
                map.remove(&self.id);
            }
        });
    }
}

impl std::fmt::Debug for SharedSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedSurface")
            .field("id", &self.id)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("dirty_count", &self.dirty.lock().unwrap().len())
            .finish()
    }
}

/// 给高频组件提供的：只重绘一个矩形区域的像素。
///
/// 实现方把 `dirty` 中的矩形区域重画进 `dst`（行优先 RGBA8），
/// 只处理变化区域，跳过未变化像素。
pub trait SurfacePainter {
    /// 把指定脏区重绘进 `dst`。`dst` 尺寸为 `surface.width * surface.height * 4`。
    fn paint_into(&self, surface: &SharedSurface, dst: &mut [u8], dirty: &[Rect]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_clamps_to_bounds() {
        let s = SharedSurface::with_id(SurfaceId(1), 10, 10);
        // 越界脏区应被裁剪到表面内。
        s.damage(Rect::new(-5.0, -5.0, 20.0, 20.0));
        let dirty = s.take_dirty();
        assert_eq!(dirty.len(), 1);
        let d = dirty[0];
        assert_eq!(d.x, 0.0);
        assert_eq!(d.y, 0.0);
        assert_eq!(d.width, 10.0);
        assert_eq!(d.height, 10.0);
    }

    #[test]
    fn empty_or_outside_damage_is_ignored() {
        let s = SharedSurface::with_id(SurfaceId(2), 10, 10);
        s.damage(Rect::new(100.0, 100.0, 10.0, 10.0));
        assert!(s.take_dirty().is_empty());
        assert!(!s.is_touched());
    }

    #[test]
    fn buffer_len_matches_dims() {
        let s = SharedSurface::with_id(SurfaceId(3), 4, 3);
        assert_eq!(s.buffer.lock().unwrap().len(), 4 * 3 * 4);
    }

    #[test]
    fn new_registers_and_resolves_by_id() {
        let s = SharedSurface::new(10, 10);
        let resolved = resolve_surface(s.id);
        assert!(resolved.is_some());
        assert!(std::rc::Rc::ptr_eq(&s, &resolved.unwrap()));
    }
}
