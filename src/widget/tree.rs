// src/widget/tree.rs
//! Widget 树管理

use std::any::Any;
use std::cell::RefCell;
use std::cell::RefMut;
use std::collections::HashMap;
use std::rc::Rc;

use crate::core::WidgetId;
use crate::widget::Widget;

/// Widget 的包装类型，用于类型擦除
pub type WidgetRef = Rc<RefCell<Box<dyn Widget>>>;

/// Widget 树
///
/// 负责管理所有 Widget 的生命周期和树结构关系
pub struct WidgetTree {
    /// 所有 Widget 的存储（pub以便EventContext访问）
    pub widgets: HashMap<WidgetId, WidgetRef>,
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
        let widget_box: WidgetRef = Rc::new(RefCell::new(Box::new(widget)));
        self.widgets.insert(id, widget_box);
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
        if let Some(parent) = self.widgets.get(&parent_id) {
            parent.borrow_mut().children_mut().push(child_id);
        }
    }

    /// 获取指定类型的 Widget 可变引用
    ///
    /// 注意：由于使用 Rc<RefCell<>>，返回的是运行时借用守卫
    pub fn get<W: Any>(&self, id: WidgetId) -> Option<RefMut<'_, W>> {
        let widget_rc = self.widgets.get(&id)?;
        let widget_ref = widget_rc.borrow_mut();
        // 尝试 downcast
        RefMut::filter_map(widget_ref, |w| w.as_any_mut().downcast_mut::<W>()).ok()
    }

    /// 获取 Widget 的 Rc 克隆（用于事件处理等场景）
    pub fn get_widget_ref(&self, id: WidgetId) -> Option<WidgetRef> {
        self.widgets.get(&id).cloned()
    }

    /// 获取 Widget 引用（用于 trait 方法调用）
    pub fn get_widget(&self, id: WidgetId) -> Option<RefMut<'_, Box<dyn Widget>>> {
        self.widgets.get(&id).map(|w| w.borrow_mut())
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
        for (id, widget_rc) in &self.widgets {
            if widget_rc.borrow().children().contains(&child_id) {
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
        if let Some(widget_rc) = self.widgets.get(&id) {
            let widget = widget_rc.borrow();
            f(id, widget.as_ref());
            let children: Vec<WidgetId> = widget.children().to_vec();
            drop(widget); // 释放借用
            for child_id in children {
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

        if let Some(widget_rc) = self.widgets.get(&current_id) {
            let widget = widget_rc.borrow();
            let children: Vec<WidgetId> = widget.children().to_vec();
            drop(widget);
            for child_id in children {
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
