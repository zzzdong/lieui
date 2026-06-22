//! Widget trait - 核心组件接口
//!
//! Widget 是 LieUI 中所有可视化组件的基础 trait。
//! 父子关系由 WidgetTree 集中管理，Widget trait 不再包含 children 相关方法。

use std::any::Any;

use crate::core::WidgetId;
use crate::event::{Event, EventContext, EventResult};
use crate::geometry::{Point, Rect};
use crate::layout::LayoutNode;
use crate::render::visual::LayeredElement;

pub mod tree;
pub use tree::WidgetTree;

pub trait Widget: Any {
    /// 类型名称（用于调试）
    fn type_name(&self) -> &'static str;

    /// 脏标记：Widget 的状态发生了变化，需要重新布局和渲染
    fn is_dirty(&self) -> bool {
        false
    }

    fn clear_dirty(&mut self) {}

    /// 返回布局节点（仅约束，不计算位置）
    ///
    /// 这是新布局系统的核心方法：
    /// - Widget 只返回约束信息（BoxStyle, FlexStyle, IntrinsicSize 等）
    /// - 实际的位置计算由 LayoutContext::compute() 统一执行
    ///
    /// # 参数
    /// - `id`: Widget 的 ID，必须用于创建 LayoutNode
    ///
    /// # 返回
    /// - LayoutNode 包含约束信息，但 computed 字段为 None
    fn layout(&self, id: WidgetId) -> LayoutNode;

    /// 根据已计算的布局生成视觉元素列表
    ///
    /// # 参数
    /// - `layout`: 已计算的布局节点（包含 computed 字段）
    /// - `ctx`: 视图上下文
    ///
    /// # 返回
    /// - Vec<LayeredElement> 带层级信息的视觉元素列表
    fn render(
        &mut self,
        layout: &LayoutNode,
        ctx: &crate::core::ViewContext,
    ) -> Vec<LayeredElement>;

    /// 命中测试
    ///
    /// 默认实现使用 layout 中的 computed 信息
    fn hit_test(&self, point: Point, layout: &LayoutNode) -> bool {
        layout.computed.content_box.contains(point)
    }

    /// 处理事件
    ///
    /// 参数：
    /// - event: 事件数据
    /// - ctx: 事件上下文，可访问 Widget 树、请求副作用、控制传播
    ///
    /// 返回 EventResult：
    /// - Continue: 继续传播到父节点
    /// - Stop: 停止传播
    ///
    /// 默认实现返回 Continue，表示不拦截事件
    fn handle_event(&mut self, _event: &Event, _ctx: &EventContext) -> EventResult {
        EventResult::Continue
    }

    /// 是否为容器 Widget
    ///
    /// 用于调试和断言，容器应返回 true
    fn is_container(&self) -> bool {
        false
    }

    /// 层级索引，数值越大越在上层（默认 0）
    ///
    /// 用于渲染顺序和事件派发优先级。
    /// Modal 等覆盖层 widget 应返回较高的 z_index。
    fn z_index(&self) -> i32 {
        0
    }

    fn can_focus(&self) -> bool {
        false
    }

    fn bounds(&self) -> Option<Rect> {
        None
    }

    /// 转换为 Any trait object（用于 downcast）
    fn as_any(&self) -> &dyn Any;

    /// 转换为可变的 Any trait object（用于 downcast_mut）
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// 为 Widget 实现 as_any 和 as_any_mut 的宏
#[macro_export]
macro_rules! impl_widget_any {
    ($type:ty) => {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    };
}
