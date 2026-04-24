use crate::core::WidgetId;
use crate::geometry::Point;
use crate::layout::LayoutNode;

/// 事件分发器 - 负责命中测试和事件分发
pub struct EventDispatcher {
    hovered: Option<WidgetId>,
    pressed: Option<WidgetId>,
}

impl EventDispatcher {
    pub fn new() -> Self {
        Self {
            hovered: None,
            pressed: None,
        }
    }

    /// 命中测试 - 找到指定点下的最深层 widget
    pub fn hit_test(&self, point: Point, layout_root: &LayoutNode) -> Option<WidgetId> {
        Self::hit_test_recursive(point, layout_root)
    }

    fn hit_test_recursive(point: Point, layout_node: &LayoutNode) -> Option<WidgetId> {
        // 先检查子节点（从上层开始）
        for child in layout_node.children.iter().rev() {
            if let Some(id) = Self::hit_test_recursive(point, child) {
                return Some(id);
            }
        }

        // 再检查当前节点
        if let Some(computed) = &layout_node.computed {
            if computed.content_box.contains(point) {
                return Some(layout_node.id);
            }
        }

        None
    }

    /// 获取当前悬停的 widget
    pub fn hovered(&self) -> Option<WidgetId> {
        self.hovered
    }

    /// 获取当前按下的 widget
    pub fn pressed(&self) -> Option<WidgetId> {
        self.pressed
    }

    /// 设置悬停状态
    pub fn set_hovered(&mut self, id: Option<WidgetId>) {
        self.hovered = id;
    }

    /// 设置按下状态
    pub fn set_pressed(&mut self, id: Option<WidgetId>) {
        self.pressed = id;
    }
}
