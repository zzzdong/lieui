// src/event/context.rs
//! 事件上下文
//!
//! 提供事件处理期间对各层 Widget 树的安全访问和副作用收集能力
//!
//! 改进：
//! - 添加事件传播阶段（Capture/Target/Bubble）

use crate::core::WidgetId;
use crate::core::layers::{LayerType, Layers, WidgetRef, WidgetRefMut};
use crate::event::{Propagation, manager::EventPhase};
use crate::widget::Widget;
use std::cell::{Cell, Ref, RefMut};

/// 事件副作用
#[derive(Debug, Default, Clone, Copy)]
pub struct EventEffects {
    pub needs_render: bool,
    pub needs_layout: bool,
    pub needs_animate: bool,
}

impl EventEffects {
    pub fn request_render(&mut self) {
        self.needs_render = true;
    }

    pub fn request_layout(&mut self) {
        self.needs_layout = true;
    }

    pub fn request_animate(&mut self) {
        self.needs_animate = true;
    }

    pub fn needs_render(&self) -> bool {
        self.needs_render
    }

    pub fn needs_layout(&self) -> bool {
        self.needs_layout
    }

    pub fn needs_animate(&self) -> bool {
        self.needs_animate
    }

    pub fn clear(&mut self) {
        self.needs_render = false;
        self.needs_layout = false;
        self.needs_animate = false;
    }

    /// 合并另一个 EventEffects 的状态
    pub fn merge(&mut self, other: &EventEffects) {
        self.needs_render |= other.needs_render;
        self.needs_layout |= other.needs_layout;
        self.needs_animate |= other.needs_animate;
    }
}

/// 事件上下文，贯穿整个事件处理流程
///
/// 持有 &Layers，可以跨层访问 Widget，并直接操作各层（显示/隐藏 Modal 等）。
///
/// 提供：
/// 1. 跨层 Widget 访问（Base / Overlay / Modal）
/// 2. 层操作（show_modal / hide_modal 等）
/// 3. 副作用收集（渲染、布局、动画）
/// 4. 传播控制
/// 5. 事件阶段（Capture/Target/Bubble）
pub struct EventContext<'a> {
    /// 所有层的引用
    layers: &'a Layers,

    /// 当前正在派发事件的层（用于事件路径计算）
    current_layer: LayerType,

    /// 副作用收集器
    effects: Cell<EventEffects>,

    /// 传播控制器
    propagation: Cell<Propagation>,

    /// 当前事件传播阶段
    phase: Cell<EventPhase>,
}

impl<'a> EventContext<'a> {
    /// 创建新的事件上下文
    pub fn new(layers: &'a Layers, current_layer: LayerType) -> Self {
        Self {
            layers,
            current_layer,
            effects: Cell::new(EventEffects::default()),
            propagation: Cell::new(Propagation::new()),
            phase: Cell::new(EventPhase::Target), // 默认 Target 阶段
        }
    }

    // ========== Widget 跨层访问 ==========

    /// 获取 Widget 的不可变引用（跨层查找）
    pub fn get<W: Widget + 'static>(&self, id: WidgetId) -> Option<Ref<'_, W>> {
        self.layers.tree.get::<W>(id)
    }

    /// 获取 Widget 的可变引用（跨层查找）
    pub fn get_mut<W: Widget + 'static>(&self, id: WidgetId) -> Option<RefMut<'_, W>> {
        self.layers.tree.get_mut::<W>(id)
    }

    /// 获取 Widget 的不可变引用（类型擦除，返回 Box<dyn Widget>）
    pub fn get_widget(&self, id: WidgetId) -> Option<WidgetRef<'_>> {
        self.layers.get_widget(id)
    }

    /// 获取 Widget 的可变引用（类型擦除，返回 Box<dyn Widget>）
    pub fn get_widget_mut(&self, id: WidgetId) -> Option<WidgetRefMut<'_>> {
        self.layers.get_widget_mut(id)
    }

    /// 获取 Layers 引用（高级用法）
    pub fn layers(&self) -> &Layers {
        self.layers
    }

    /// 当前事件派发所在的层
    pub fn current_layer(&self) -> LayerType {
        self.current_layer
    }

    // ========== 层操作（直接操作 Overlay / Modal 层）==========

    /// 显示 Modal 层（传入 modal 内容的根 WidgetId）
    pub fn show_modal(&self, root_id: WidgetId) {
        self.layers.show_modal(root_id);
        let mut e = self.effects.get();
        e.request_render();
        e.request_layout();
        self.effects.set(e);
    }

    /// 隐藏 Modal 层
    pub fn hide_modal(&self) {
        self.layers.hide_modal();
        let mut e = self.effects.get();
        e.request_render();
        e.request_layout();
        self.effects.set(e);
    }

    /// 显示 Overlay 层
    pub fn show_overlay(&self, root_id: WidgetId) {
        self.layers.show_overlay(root_id);
        let mut e = self.effects.get();
        e.request_render();
        e.request_layout();
        self.effects.set(e);
    }

    /// 隐藏 Overlay 层
    pub fn hide_overlay(&self) {
        self.layers.hide_overlay();
        let mut e = self.effects.get();
        e.request_render();
        e.request_layout();
        self.effects.set(e);
    }

    // ========== 副作用 ==========

    /// 请求重新渲染
    pub fn request_render(&self) {
        let mut e = self.effects.get();
        e.request_render();
        self.effects.set(e);
    }

    /// 请求重新布局
    pub fn request_layout(&self) {
        let mut e = self.effects.get();
        e.request_layout();
        self.effects.set(e);
    }

    /// 请求动画帧
    pub fn request_animate(&self) {
        let mut e = self.effects.get();
        e.request_animate();
        self.effects.set(e);
    }

    /// 提取副作用（会清空当前副作用）
    pub fn take_effects(&self) -> EventEffects {
        let effects = self.effects.get();
        self.effects.set(EventEffects::default());
        effects
    }

    /// 获取副作用的副本（不清空）
    pub fn effects(&self) -> EventEffects {
        self.effects.get()
    }

    // ========== 传播控制 ==========

    /// 停止事件传播
    pub fn stop_propagation(&self) {
        let mut p = self.propagation.get();
        p.stop();
        self.propagation.set(p);
    }

    /// 是否已停止传播
    pub fn is_stopped(&self) -> bool {
        self.propagation.get().is_stopped()
    }

    /// 重置传播状态
    pub fn reset_propagation(&self) {
        self.propagation.set(Propagation::new());
    }

    // ========== 事件阶段 ==========

    /// 获取当前事件传播阶段
    pub fn phase(&self) -> EventPhase {
        self.phase.get()
    }

    /// 设置当前事件传播阶段
    pub fn set_phase(&self, phase: EventPhase) {
        self.phase.set(phase);
    }

    /// 是否在捕获阶段
    pub fn is_capture(&self) -> bool {
        self.phase.get() == EventPhase::Capture
    }

    /// 是否在目标阶段
    pub fn is_target(&self) -> bool {
        self.phase.get() == EventPhase::Target
    }

    /// 是否在冒泡阶段
    pub fn is_bubble(&self) -> bool {
        self.phase.get() == EventPhase::Bubble
    }
}
