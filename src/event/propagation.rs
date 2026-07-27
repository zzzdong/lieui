//! 事件传播控制

use crate::core::ElementId;

/// 命中测试结果（缓存，避免重复计算）
#[derive(Debug, Clone)]
pub struct HitTestResult {
    pub target: ElementId,
    pub path: Vec<ElementId>,
}

/// 事件传播阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EventPhase {
    #[default]
    Target,
    Capture,
    Bubble,
}

/// 事件传播控制
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Propagation {
    #[default]
    Continue,
    Stop,
}

/// 事件副作用
#[derive(Debug, Clone, Copy, Default)]
pub struct EventEffects {
    needs_rebuild: bool,
    needs_layout: bool,
    needs_render: bool,
    /// MouseDown 等事件是否被某个 listener 调用 `stop_propagation` 截断。
    /// 用于引擎层判断是否要接管后续默认交互（如 ScrollView 拖拽滚动）。
    propagation_stopped: bool,
}

impl EventEffects {
    pub fn needs_rebuild(&self) -> bool {
        self.needs_rebuild
    }
    pub fn needs_layout(&self) -> bool {
        self.needs_layout
    }
    pub fn needs_render(&self) -> bool {
        self.needs_render
    }
    pub fn propagation_stopped(&self) -> bool {
        self.propagation_stopped
    }

    pub fn request_rebuild(&mut self) {
        self.needs_rebuild = true;
    }
    pub fn request_layout(&mut self) {
        self.needs_layout = true;
    }
    pub fn request_render(&mut self) {
        self.needs_render = true;
    }
    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }

    pub fn merge(&mut self, other: &EventEffects) {
        self.needs_rebuild = self.needs_rebuild || other.needs_rebuild;
        self.needs_layout = self.needs_layout || other.needs_layout;
        self.needs_render = self.needs_render || other.needs_render;
        self.propagation_stopped = self.propagation_stopped || other.propagation_stopped;
    }
}
