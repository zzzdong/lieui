//! 轻量动画 / 计时器支持。
//!
//! 通过模块级全局注册表管理周期性回调（ticker）。事件循环在空闲时调用
//! [`tick`]，推进所有到期的动画回调；当仍有活跃动画时返回最早的下一次触发
//! 时间，调用方据此用 `ControlFlow::WaitUntil` 让循环休眠到下一次触发，避免
//! 忙等。
//!
//! 典型用法（如输入框闪烁光标）：
//!
//! ```ignore
//! let _anim = Animation::new(Duration::from_millis(530), || {
//!     crate::state::request_rebuild();
//! });
//! ```
//!
//! 返回的句柄在存活期间持续触发；`Drop` 时自动取消注册。

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

struct Entry {
    interval: Duration,
    next_fire: Instant,
    callback: Option<Box<dyn FnMut()>>,
}

struct Registry {
    entries: HashMap<u64, Entry>,
    next_id: u64,
}

impl Registry {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            next_id: 1,
        }
    }
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::new());
}

/// 一个已注册的周期动画。
///
/// 句柄存活期间动画每隔 `interval` 触发一次；句柄被丢弃（`Drop`）时自动取消
/// 注册，无需手动管理。
pub struct Animation {
    id: u64,
}

impl Animation {
    /// 注册一个每隔 `interval` 触发的动画。
    ///
    /// `callback` 在每次到期时被调用，通常用于请求一次重绘（例如
    /// `crate::state::request_rebuild()` 或 `crate::state::request_redraw()`）。
    pub fn new(interval: Duration, callback: impl FnMut() + 'static) -> Self {
        let id = REGISTRY.with(|r| {
            let mut reg = r.borrow_mut();
            let id = reg.next_id;
            reg.next_id += 1;
            reg.entries.insert(
                id,
                Entry {
                    interval,
                    next_fire: Instant::now() + interval,
                    callback: Some(Box::new(callback)),
                },
            );
            id
        });
        Animation { id }
    }
}

impl Drop for Animation {
    fn drop(&mut self) {
        REGISTRY.with(|r| r.borrow_mut().entries.remove(&self.id));
    }
}

/// 推进所有到期的动画。
///
/// 返回 `(fired, next_fire)`：
/// - `fired`：本次是否有动画回调被触发（调用方据此请求一次重绘）。
/// - `next_fire`：最早的下一个触发时间；为 `None` 表示当前没有任何活跃动画。
///
/// 调用方应在事件循环的 `about_to_wait` 中调用本函数，并用 `next_fire` 设置
/// `ControlFlow::WaitUntil`，使循环休眠到下一次触发而非空转。
pub fn tick(now: Instant) -> (bool, Option<Instant>) {
    // 收集到期回调并推进 next_fire；先释放注册表借用再调用回调，
    // 避免回调内部再次注册/取消动画时造成 RefCell 重借。
    let mut to_fire: Vec<(u64, Box<dyn FnMut()>)> = Vec::new();
    let next = REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let due: Vec<u64> = reg
            .entries
            .iter()
            .filter(|(_, e)| now >= e.next_fire)
            .map(|(id, _)| *id)
            .collect();
        for id in due {
            if let Some(e) = reg.entries.get_mut(&id) {
                e.next_fire = now + e.interval;
                if let Some(cb) = e.callback.take() {
                    to_fire.push((id, cb));
                }
            }
        }
        reg.entries.values().map(|e| e.next_fire).min()
    });

    let fired = !to_fire.is_empty();
    REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        for (id, mut cb) in to_fire {
            cb();
            // 若回调执行期间未取消本动画（entry 仍在），把回调放回以便下次触发。
            if let Some(e) = reg.entries.get_mut(&id) {
                e.callback = Some(cb);
            }
        }
    });

    (fired, next)
}
