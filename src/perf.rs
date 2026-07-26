//! 性能打点 — 通过环境变量 `LIEUI_PERF=1` 启用分阶段耗时输出
//!
//! 仅用于开发调试：启用后关键管线阶段（builder / reconcile / layout /
//! render-tree / raster / blit）会向 stderr 输出微秒级耗时。

use std::sync::OnceLock;
use std::time::Instant;

/// 是否启用性能打点（进程内只读一次环境变量）。
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("LIEUI_PERF").is_ok_and(|v| v == "1"))
}

/// 计时作用域：drop 或显式 `finish()` 时输出耗时。
pub struct Span {
    label: &'static str,
    start: Instant,
    reported: bool,
}

impl Span {
    pub fn start(label: &'static str) -> Self {
        Self {
            label,
            start: Instant::now(),
            reported: !enabled(),
        }
    }

    /// 结束计时并输出（未启用时为空操作）。
    pub fn finish(mut self) {
        self.report();
    }

    fn report(&mut self) {
        if !self.reported {
            self.reported = true;
            eprintln!(
                "[lieui-perf] {:<16} {:>8.1}us",
                self.label,
                self.start.elapsed().as_secs_f64() * 1e6
            );
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        self.report();
    }
}
