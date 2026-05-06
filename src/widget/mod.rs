use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use crate::core::WidgetId;
use crate::event::{Event, EventContext, EventResult};
use crate::geometry::{Point, Rect};
use crate::layout::LayoutNode;
use crate::render::RenderNode;

pub mod tree;
pub use tree::WidgetTree;

pub type WidgetRef = Rc<RefCell<Box<dyn Widget>>>;

pub trait Widget: Any {
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

    /// 根据已计算的布局生成渲染节点
    ///
    /// # 参数
    /// - `layout`: 已计算的布局节点（包含 computed 字段）
    /// - `ctx`: 视图上下文
    fn render(&mut self, layout: &LayoutNode, ctx: &crate::core::ViewContext) -> RenderNode;

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

    /// 获取子节点 ID 列表（只读）
    ///
    /// 默认返回空切片，表示没有子节点（叶子节点）
    fn children(&self) -> &[WidgetId] {
        &[]
    }

    /// 添加子节点
    ///
    /// 默认实现为空，叶子节点无需重写。
    /// 容器 Widget 应重写此方法，将 child_id 加入内部 children 列表。
    fn add_child(&mut self, _child_id: WidgetId) {}

    /// 移除子节点
    ///
    /// 默认实现为空，叶子节点无需重写。
    /// 容器 Widget 应重写此方法，从内部 children 列表中移除指定节点。
    fn remove_child(&mut self, _child_id: WidgetId) {}

    /// 是否为容器 Widget
    ///
    /// 用于调试和断言，容器应返回 true
    fn is_container(&self) -> bool {
        false
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
