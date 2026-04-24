// src/event/propagation.rs

//! 事件传播系统
//!
//! 实现 DOM 风格的事件传播：捕获 -> 目标 -> 冒泡

use crate::core::WidgetId;
use crate::layout::LayoutNode;

/// 事件传播阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPhase {
    /// 捕获阶段：从根节点向下传播到目标
    Capture,
    /// 目标阶段：事件到达目标节点
    Target,
    /// 冒泡阶段：从目标节点向上传播到根节点
    Bubble,
}

/// 事件传播路径
pub struct EventPath {
    /// 从根到目标的完整路径
    path: Vec<WidgetId>,
    /// 目标节点在路径中的索引
    target_index: usize,
}

impl EventPath {
    /// 从布局树根节点和命中点构建事件路径
    pub fn from_hit_test(
        root: &LayoutNode,
        hit_test_fn: impl Fn(WidgetId) -> bool,
    ) -> Option<Self> {
        let mut path = Vec::new();
        let mut target_index = None;

        // 递归遍历找到命中测试通过的节点
        Self::build_path(root, &hit_test_fn, &mut path, &mut target_index);

        target_index.map(|idx| Self {
            path,
            target_index: idx,
        })
    }

    fn build_path(
        node: &LayoutNode,
        hit_test: &impl Fn(WidgetId) -> bool,
        path: &mut Vec<WidgetId>,
        target_index: &mut Option<usize>,
    ) -> bool {
        path.push(node.id);

        // 先检查子节点（深度优先）
        for child in &node.children {
            if Self::build_path(child, hit_test, path, target_index) {
                return true;
            }
        }

        // 检查当前节点是否命中
        if hit_test(node.id) {
            *target_index = Some(path.len() - 1);
            return true;
        }

        // 如果没命中，从路径中移除
        path.pop();
        false
    }

    /// 获取目标节点
    pub fn target(&self) -> WidgetId {
        self.path[self.target_index]
    }

    /// 遍历捕获阶段（从根到目标之前）
    pub fn capture_phase(&self) -> impl Iterator<Item = WidgetId> + '_ {
        self.path.iter().take(self.target_index).copied()
    }

    /// 遍历冒泡阶段（从目标之后到根）
    pub fn bubble_phase(&self) -> impl Iterator<Item = WidgetId> + '_ {
        self.path.iter().skip(self.target_index + 1).rev().copied()
    }

    /// 获取完整路径
    pub fn full_path(&self) -> &[WidgetId] {
        &self.path
    }
}

/// 事件传播控制器
pub struct EventPropagation {
    /// 当前事件是否已停止传播
    stopped: bool,
    /// 当前事件是否已阻止默认行为
    default_prevented: bool,
    /// 当前阶段
    current_phase: EventPhase,
}

impl EventPropagation {
    pub fn new() -> Self {
        Self {
            stopped: false,
            default_prevented: false,
            current_phase: EventPhase::Capture,
        }
    }

    /// 停止事件传播
    pub fn stop_propagation(&mut self) {
        self.stopped = true;
    }

    /// 是否已停止传播
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// 阻止默认行为
    pub fn prevent_default(&mut self) {
        self.default_prevented = true;
    }

    /// 是否已阻止默认行为
    pub fn is_default_prevented(&self) -> bool {
        self.default_prevented
    }

    /// 设置当前阶段
    pub fn set_phase(&mut self, phase: EventPhase) {
        self.current_phase = phase;
    }

    /// 获取当前阶段
    pub fn phase(&self) -> EventPhase {
        self.current_phase
    }
}

impl Default for EventPropagation {
    fn default() -> Self {
        Self::new()
    }
}

/// 事件处理结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    /// 继续传播
    Continue,
    /// 停止传播
    Stop,
    /// 阻止默认行为但继续传播
    PreventDefault,
}

impl EventResult {
    /// 是否停止传播
    pub fn is_stopped(&self) -> bool {
        matches!(self, EventResult::Stop)
    }

    /// 是否阻止默认行为
    pub fn is_default_prevented(&self) -> bool {
        matches!(self, EventResult::PreventDefault | EventResult::Stop)
    }
}
