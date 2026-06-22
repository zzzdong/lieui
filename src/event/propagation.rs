//! 事件传播控制
//!
//! 提供简单的事件传播控制机制

/// 事件传播控制器
///
/// 用于在事件回调中控制事件是否继续传播
#[derive(Default, Clone, Copy)]
pub struct Propagation {
    stopped: bool,
}

impl Propagation {
    /// 创建新的传播控制器
    pub fn new() -> Self {
        Self::default()
    }

    /// 停止事件传播
    pub fn stop(&mut self) {
        self.stopped = true;
    }

    /// 是否已停止传播
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }
}
