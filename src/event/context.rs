//! EventContext — 事件处理上下文
//!
//! v1 的 EventContext 简化版，仅保留核心功能。

use crate::core::ElementId;
use crate::event::{EventEffects, EventPhase, Propagation};

/// 事件处理上下文
///
/// 在事件冒泡/捕获期间传递给 Element 的事件处理器。
/// 通过 handle 而非直接引用访问 Runtime 状态。
#[derive(Default)]
pub struct EventContext {
    /// 当前事件目标
    target_id: Option<ElementId>,
    /// 当前事件阶段
    phase: EventPhase,
    /// 副作用收集
    effects: EventEffects,
    /// 传播状态
    propagation: Propagation,
}

impl EventContext {
    pub fn new() -> Self {
        Self {
            target_id: None,
            phase: EventPhase::Target,
            effects: EventEffects::default(),
            propagation: Propagation::Continue,
        }
    }

    pub fn with_target(target: ElementId) -> Self {
        Self {
            target_id: Some(target),
            phase: EventPhase::Target,
            effects: EventEffects::default(),
            propagation: Propagation::Continue,
        }
    }

    // ---- Phase ----

    pub fn phase(&self) -> EventPhase {
        self.phase
    }

    pub fn set_phase(&mut self, phase: EventPhase) {
        self.phase = phase;
    }

    // ---- Effects ----

    pub fn request_rebuild(&mut self) {
        self.effects.request_rebuild();
    }

    pub fn request_layout(&mut self) {
        self.effects.request_layout();
    }

    pub fn request_render(&mut self) {
        self.effects.request_render();
    }

    pub fn effects(&mut self) -> &mut EventEffects {
        &mut self.effects
    }

    pub fn take_effects(&mut self) -> EventEffects {
        std::mem::take(&mut self.effects)
    }

    // ---- Propagation ----

    pub fn stop_propagation(&mut self) {
        self.propagation = Propagation::Stop;
    }

    pub fn is_stopped(&self) -> bool {
        self.propagation == Propagation::Stop
    }

    // ---- Target ----

    pub fn target(&self) -> Option<ElementId> {
        self.target_id
    }
}
