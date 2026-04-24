// src/widget/tree.rs
//! Widget 树管理

use std::any::Any;
use std::collections::HashMap;

use crate::core::WidgetId;
use crate::widget::Widget;

/// Widget 的包装类型，用于类型擦除
pub type WidgetBox = Box<dyn Widget>;

/// Widget 树
///
/// 负责管理所有 Widget 的生命周期和树结构关系
pub struct WidgetTree {
    /// 所有 Widget 的存储
    widgets: HashMap<WidgetId, WidgetBox>,
    /// 根节点 ID
    root: Option<WidgetId>,
}

impl WidgetTree {
    /// 创建空的 Widget 树
    pub fn new() -> Self {
        Self {
            widgets: HashMap::new(),
            root: None,
        }
    }

    /// 创建并添加 Widget 到树中
    pub fn create<W: Widget>(&mut self, widget: W) -> WidgetId {
        let id = WidgetId::new();
        self.widgets.insert(id, Box::new(widget));
        id
    }

    /// 设置根节点
    pub fn set_root(&mut self, root_id: WidgetId) {
        self.root = Some(root_id);
    }

    /// 获取根节点 ID
    pub fn root(&self) -> Option<WidgetId> {
        self.root
    }

    /// 添加子节点关系
    pub fn add_child(&mut self, parent_id: WidgetId, child_id: WidgetId) {
        if let Some(parent) = self.widgets.get_mut(&parent_id) {
            parent.children_mut().push(child_id);
        }
    }

    /// 获取指定类型的 Widget 引用
    pub fn get<W: Any>(&self, id: WidgetId) -> Option<&W> {
        self.widgets.get(&id)?.as_ref().as_any().downcast_ref::<W>()
    }

    /// 获取指定类型的 Widget 可变引用
    pub fn get_mut<W: Any>(&mut self, id: WidgetId) -> Option<&mut W> {
        self.widgets
            .get_mut(&id)?
            .as_mut()
            .as_any_mut()
            .downcast_mut::<W>()
    }

    /// 获取 Widget 引用（用于 trait 方法调用）
    pub fn get_widget(&self, id: WidgetId) -> Option<&dyn Widget> {
        self.widgets.get(&id).map(|w| w.as_ref())
    }

    /// 获取 Widget 可变引用（用于 trait 方法调用）
    pub fn get_widget_mut(&mut self, id: WidgetId) -> Option<&mut dyn Widget> {
        self.widgets.get_mut(&id).map(|w| w.as_mut())
    }

    /// 检查是否存在
    pub fn contains(&self, id: WidgetId) -> bool {
        self.widgets.contains_key(&id)
    }

    /// 获取子节点列表
    pub fn children_of(&self, id: WidgetId) -> Vec<WidgetId> {
        self.get_widget(id)
            .map(|w| w.children().to_vec())
            .unwrap_or_default()
    }

    /// 获取父节点（通过遍历查找）
    pub fn parent_of(&self, child_id: WidgetId) -> Option<WidgetId> {
        for (id, widget) in &self.widgets {
            if widget.children().contains(&child_id) {
                return Some(*id);
            }
        }
        None
    }

    /// 遍历树（前序遍历）
    pub fn traverse<F>(&self, mut f: F)
    where
        F: FnMut(WidgetId, &dyn Widget),
    {
        if let Some(root_id) = self.root {
            self.traverse_recursive(root_id, &mut f);
        }
    }

    fn traverse_recursive<F>(&self, id: WidgetId, f: &mut F)
    where
        F: FnMut(WidgetId, &dyn Widget),
    {
        if let Some(widget) = self.get_widget(id) {
            f(id, widget);
            for &child_id in widget.children() {
                self.traverse_recursive(child_id, f);
            }
        }
    }

    /// 获取从根到指定节点的路径
    pub fn path_to(&self, target_id: WidgetId) -> Vec<WidgetId> {
        let mut path = Vec::new();
        if let Some(root_id) = self.root {
            self.find_path_recursive(root_id, target_id, &mut path);
        }
        path
    }

    fn find_path_recursive(
        &self,
        current_id: WidgetId,
        target_id: WidgetId,
        path: &mut Vec<WidgetId>,
    ) -> bool {
        path.push(current_id);

        if current_id == target_id {
            return true;
        }

        if let Some(widget) = self.get_widget(current_id) {
            for &child_id in widget.children() {
                if self.find_path_recursive(child_id, target_id, path) {
                    return true;
                }
            }
        }

        path.pop();
        false
    }

    /// 获取节点数量
    pub fn len(&self) -> usize {
        self.widgets.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.widgets.is_empty()
    }
}

impl Default for WidgetTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Rect, Size};
    use crate::layout::{IntrinsicSize, LayoutNode};
    use crate::render::RenderNode;

    struct TestWidget {
        children: Vec<WidgetId>,
    }

    impl TestWidget {
        fn new() -> Self {
            Self {
                children: Vec::new(),
            }
        }
    }

    impl Widget for TestWidget {
        crate::impl_widget_any!(TestWidget);

        fn type_name(&self) -> &'static str {
            "TestWidget"
        }

        fn layout(&self, id: WidgetId) -> LayoutNode {
            LayoutNode::new(id).with_intrinsic_size(IntrinsicSize::Fixed(Size::new(100.0, 100.0)))
        }

        fn render(&self, _layout: &LayoutNode, _ctx: &crate::core::ViewContext) -> RenderNode {
            RenderNode::view(Rect::zero())
        }

        fn children(&self) -> &[WidgetId] {
            &self.children
        }

        fn children_mut(&mut self) -> &mut Vec<WidgetId> {
            &mut self.children
        }
    }

    #[test]
    fn test_create_and_get() {
        let mut tree = WidgetTree::new();
        let id = tree.create(TestWidget::new());

        assert!(tree.contains(id));
        assert!(tree.get::<TestWidget>(id).is_some());
    }

    #[test]
    fn test_parent_child_relationship() {
        let mut tree = WidgetTree::new();
        let parent = tree.create(TestWidget::new());
        let child = tree.create(TestWidget::new());

        tree.add_child(parent, child);

        let children = tree.children_of(parent);
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], child);

        let parent_found = tree.parent_of(child);
        assert_eq!(parent_found, Some(parent));
    }

    #[test]
    fn test_path_to() {
        let mut tree = WidgetTree::new();
        let root = tree.create(TestWidget::new());
        let child = tree.create(TestWidget::new());
        let grandchild = tree.create(TestWidget::new());

        tree.set_root(root);
        tree.add_child(root, child);
        tree.add_child(child, grandchild);

        let path = tree.path_to(grandchild);
        assert_eq!(path, vec![root, child, grandchild]);
    }
}
