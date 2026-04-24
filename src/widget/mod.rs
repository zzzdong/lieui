use std::any::Any;

use crate::core::WidgetId;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect};
use crate::layout::LayoutNode;
use crate::render::RenderNode;

pub mod tree;
pub use tree::{WidgetBox, WidgetTree};

pub trait Widget: Any {
    fn type_name(&self) -> &'static str;

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

    /// 根据已计算的布局生成渲染节点
    ///
    /// # 参数
    /// - `layout`: 已计算的布局节点（包含 computed 字段）
    /// - `ctx`: 视图上下文
    fn render(&self, layout: &LayoutNode, ctx: &crate::core::ViewContext) -> RenderNode;

    /// 命中测试
    ///
    /// 默认实现使用 layout 中的 computed 信息
    fn hit_test(&self, point: Point, layout: &LayoutNode) -> bool {
        if let Some(computed) = &layout.computed {
            computed.content_box.contains(point)
        } else {
            false
        }
    }

    /// 处理事件
    ///
    /// 返回 EventResult 控制事件传播：
    /// - Continue: 继续传播到父节点
    /// - Stop: 停止传播
    /// - PreventDefault: 阻止默认行为但继续传播
    ///
    /// 默认实现返回 Continue，表示不拦截事件
    fn handle_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Continue
    }

    fn children(&self) -> &[WidgetId] {
        &[]
    }

    fn children_mut(&mut self) -> &mut Vec<WidgetId> {
        unimplemented!()
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
