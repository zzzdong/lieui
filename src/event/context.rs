//! EventContext — 事件处理上下文
//!
//! v1 的 EventContext 简化版，仅保留核心功能。

use crate::core::ElementId;
use crate::event::{Event, EventEffects, EventPhase, Propagation};
use crate::geometry::Rect;

/// 触发 `DragStart` 的默认移动阈值（像素）。
pub const DEFAULT_DRAG_THRESHOLD: f32 = 3.0;

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
    /// 当前正在处理的事件数据（WithCtx 回调可读取）
    event: Option<Event>,
    /// 当前正在执行回调的节点（分发器每次调用 handler 前设置）
    current_id: Option<ElementId>,
    /// 当前节点的布局矩形（窗口坐标），供回调换算局部坐标
    current_rect: Option<Rect>,
    /// 鼠标捕获请求：分发结束后由 EventManager 处理
    capture_request: Option<ElementId>,
    /// 拖拽请求：(发起节点, 移动阈值)。分发结束后由 EventManager 处理。
    drag_request: Option<(ElementId, f32)>,
}

impl EventContext {
    pub fn new() -> Self {
        Self {
            target_id: None,
            phase: EventPhase::Target,
            ..Default::default()
        }
    }

    pub fn with_target(target: ElementId) -> Self {
        Self {
            target_id: Some(target),
            phase: EventPhase::Target,
            ..Default::default()
        }
    }

    pub fn with_event(target: ElementId, event: Event) -> Self {
        Self {
            target_id: Some(target),
            phase: EventPhase::Target,
            event: Some(event),
            ..Default::default()
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

    // ---- Event ----

    pub fn set_event(&mut self, event: Event) {
        self.event = Some(event);
    }

    pub fn event(&self) -> Option<&Event> {
        self.event.as_ref()
    }

    pub fn take_event(&mut self) -> Option<Event> {
        self.event.take()
    }

    // ---- Current node ----

    /// 分发器在每次调用节点回调前设置当前节点及其布局矩形。
    pub fn set_current(&mut self, id: ElementId, rect: Rect) {
        self.current_id = Some(id);
        self.current_rect = Some(rect);
    }

    /// 当前节点的布局矩形（窗口坐标），回调可用于换算局部坐标。
    pub fn current_rect(&self) -> Option<Rect> {
        self.current_rect
    }

    // ---- Mouse capture ----

    /// 请求鼠标捕获：后续 MouseMove/MouseUp 直接派发给当前节点，
    /// 即使指针移出节点范围（文本拖拽选取等场景必需）。
    /// 鼠标释放时自动解除。
    pub fn capture_mouse(&mut self) {
        self.capture_request = self.current_id;
    }

    /// 取走捕获请求（EventManager 在分发结束后调用）。
    pub fn take_capture_request(&mut self) -> Option<ElementId> {
        self.capture_request.take()
    }

    // ---- Drag ----

    /// 请求拖拽跟踪：按下后移动超过 [`DEFAULT_DRAG_THRESHOLD`] 即合成
    /// `DragStart` / `DragMove` / `DragEnd` 事件，并自动捕获鼠标
    /// （指针移出当前节点后移动/释放事件仍投递给当前节点）。
    pub fn begin_drag(&mut self) {
        if let Some(id) = self.current_id {
            self.drag_request = Some((id, DEFAULT_DRAG_THRESHOLD));
        }
    }

    /// 带自定义移动阈值的拖拽请求（px >= 0）。
    pub fn begin_drag_with_threshold(&mut self, px: f32) {
        if let Some(id) = self.current_id {
            self.drag_request = Some((id, px.max(0.0)));
        }
    }

    /// 取走拖拽请求（EventManager 在分发结束后调用）。
    pub fn take_drag_request(&mut self) -> Option<(ElementId, f32)> {
        self.drag_request.take()
    }
}
