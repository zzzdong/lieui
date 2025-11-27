use std::{
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    rc::Rc,
};

use taffy::{
    AvailableSpace, Dimension, Layout as TaffyLayout, NodeId, NodeId as TaffyId, PrintTree,
    Size as TaffySize, Style as TaffyStyle, TaffyTree,
};
use vello_cpu::RenderContext;
use winit::event::{ElementState, MouseButton, WindowEvent};

use crate::event::PointerEventHandler;

use super::{ElementId, IElement};

pub struct ElementNode<State> {
    pub content: Box<dyn IElement<State>>,
}

impl<State> ElementNode<State> {
    pub fn new<T: IElement<State> + 'static>(content: T) -> Self {
        Self {
            content: Box::new(content),
        }
    }

    pub fn into_ref(self) -> ElementRef<State> {
        ElementRef(Rc::new(RefCell::new(self)))
    }
}

pub struct ElementRef<State>(Rc<RefCell<ElementNode<State>>>);

impl<State> ElementRef<State> {
    pub fn get_mut(&self) -> RefMut<ElementNode<State>> {
        self.0.borrow_mut()
    }

    pub fn get(&self) -> Ref<ElementNode<State>> {
        self.0.borrow()
    }
}

impl<State> Clone for ElementRef<State> {
    fn clone(&self) -> Self {
        ElementRef(self.0.clone())
    }
}

pub struct ElementTree<State> {
    nodes: HashMap<ElementId, ElementRef<State>>,
    layouts: HashMap<ElementId, TaffyId>,
    taffy: TaffyTree<ElementId>,
    root: Option<ElementId>,
    next_id: u64,
}

impl<State> ElementTree<State> {
    pub fn new() -> Self {
        let mut nodes = HashMap::new();
        let mut taffy = TaffyTree::new();

        Self {
            nodes,
            layouts: HashMap::new(),
            taffy,
            root: None,
            next_id: 0,
        }
    }

    pub fn set_root(&mut self, root: ElementId) {
        self.root = Some(root);
    }

    pub fn add_node(&mut self, node: ElementNode<State>) -> ElementId {
        let node_id = ElementId(self.next_id);
        self.next_id += 1;

        let node_ref = node.into_ref();

        // 创建布局节点
        let layout_id = self
            .taffy
            .new_leaf_with_context(TaffyStyle::default(), node_id)
            .expect("failed to create layout node");

        self.nodes.insert(node_id, node_ref);
        self.layouts.insert(node_id, layout_id);

        node_id
    }

    pub fn insert_child(&mut self, parent: ElementId, child: ElementNode<State>) -> ElementId {
        let child_id = self.add_node(child);

        // 获取父节点的布局ID
        let parent_layout_id = self
            .layouts
            .get(&parent)
            .expect("parent layout id not found");
        let child_layout_id = self
            .layouts
            .get(&child_id)
            .expect("child layout id not found");

        // 添加子节点到布局树
        self.taffy
            .add_child(*parent_layout_id, *child_layout_id)
            .expect("add layout child failed");

        child_id
    }

    pub fn do_layout(&mut self, viewport: TaffySize<AvailableSpace>) {
        match self.root {
            Some(root_id) => {
                let layout_id = self
                    .layouts
                    .get(&root_id)
                    .expect("root layout id not found")
                    .clone();

                // update_layout_style(&mut self.taffy, &mut self.nodes, layout_id);

                // compute_layout(&mut self.taffy, &mut self.nodes, layout_id, viewport);
                self.compute_layout(layout_id, viewport);

                self.taffy.print_tree(layout_id);
            }
            None => {
                return;
            }
        }
    }

    pub fn do_paint(&mut self, cx: &mut RenderContext) {
        match self.root {
            Some(root_id) => {
                self.paint_node(cx, root_id);
            }
            None => {
                return;
            }
        }
    }

    fn paint_node(&mut self, cx: &mut RenderContext, ele_id: ElementId) {
        let node_ref = self.nodes.get(&ele_id).expect("element not found").clone();
        let layout_id = self
            .layouts
            .get(&ele_id)
            .copied()
            .expect("layout id not found");

        let layout = self.taffy.layout(layout_id).expect("get layout failed");

        // 绘制当前节点
        node_ref.get_mut().content.paint(cx, layout);

        // 递归绘制子节点
        for child_id in self.taffy.children(layout_id).expect("get children failed") {
            let ele_id = self
                .taffy
                .get_node_context(child_id)
                .expect("get node context failed");
            let ele = self.node(*ele_id);
            let layout = self.taffy.layout(child_id).expect("get layout failed");
            ele.get_mut().content.paint(cx, layout);
        }
    }

    /// 根据ElementId获取节点引用
    fn node(&self, ele_id: ElementId) -> ElementRef<State> {
        self.nodes.get(&ele_id).expect("node not found").clone()
    }

    pub fn get_mut(&mut self, ele_id: ElementId) -> ElementRef<State> {
        self.nodes.get_mut(&ele_id).expect("node not found").clone()
    }

    /// 遍历布局树查找包含指定坐标的元素
    ///
    /// # 参数
    /// - `x`, `y`: 相对坐标
    /// - `inspect_enter_fn`: 进入元素时的回调函数
    /// - `inspect_exit_fn`: 离开元素时的回调函数  
    ///
    /// # 返回值
    /// 返回找到的元素ID，如果没有找到则返回None
    fn find_element_in_layout<InspectEnterFn, InspectExitFn>(
        &self,
        x: f32,
        y: f32,
        inspect_enter_fn: InspectEnterFn,
        inspect_exit_fn: InspectExitFn,
    ) -> Option<ElementId>
    where
        InspectEnterFn: Fn(ElementId),
        InspectExitFn: Fn(ElementId),
    {
        // 如果根节点不存在，直接返回None
        let root_id = self.root?;
        let root_layout_id = *self.layouts.get(&root_id)?;

        // 从根节点开始递归遍历
        Self::find_element_in_layout_recursive(
            &self.taffy,
            root_layout_id,
            x,
            y,
            (0.0, 0.0), // 根节点的绝对位置
            &inspect_enter_fn,
            &inspect_exit_fn,
        )
    }

    /// 递归遍历布局树的辅助函数
    fn find_element_in_layout_recursive<InspectEnterFn, InspectExitFn>(
        tree: &TaffyTree<ElementId>,
        node_id: TaffyId,
        x: f32,
        y: f32,
        parent_absolute_pos: (f32, f32),
        inspect_enter_fn: &InspectEnterFn,
        inspect_exit_fn: &InspectExitFn,
    ) -> Option<ElementId>
    where
        InspectEnterFn: Fn(ElementId),
        InspectExitFn: Fn(ElementId),
    {
        // 获取当前节点的布局信息
        let layout = match tree.layout(node_id) {
            Ok(layout) => layout,
            Err(_) => return None,
        };

        // 计算当前节点的绝对位置
        let absolute_pos = (
            parent_absolute_pos.0 + layout.location.x,
            parent_absolute_pos.1 + layout.location.y,
        );

        // 获取元素ID
        let element_id = match tree.get_node_context(node_id) {
            Some(id) => *id,
            None => return None,
        };

        // 调用进入元素钩子
        inspect_enter_fn(element_id);

        // 检查点是否在当前节点的边界内
        let contains_point = x >= absolute_pos.0
            && x <= absolute_pos.0 + layout.size.width
            && y >= absolute_pos.1
            && y <= absolute_pos.1 + layout.size.height;

        // 如果点在当前节点内，检查停止条件
        let mut found_element = None;
        if contains_point {}

        // 如果点在当前节点内且没有停止，继续检查子节点
        if contains_point && found_element.is_none() {
            // 获取子节点（从后往前遍历，因为后面的元素在视觉上层）
            if let Ok(children) = tree.children(node_id) {
                for child_id in children.iter().rev() {
                    if let Some(child_element) = Self::find_element_in_layout_recursive(
                        tree,
                        *child_id,
                        x,
                        y,
                        absolute_pos,
                        inspect_enter_fn,
                        inspect_exit_fn,
                    ) {
                        found_element = Some(child_element);
                        break;
                    }
                }
            }
        }

        // 调用离开元素钩子
        inspect_exit_fn(element_id);

        found_element
    }

    /// 查找包含指定坐标的元素
    pub fn find_element_at_point(&self, x: f32, y: f32) -> Option<ElementId> {
        self.find_element_in_layout(
            x,
            y,
            |_| {}, // 进入元素时不执行任何操作
            |_| {}, // 离开元素时不执行任何操作
        )
    }

    /// 查找包含指定坐标的元素，并提供调试信息
    pub fn find_element_at_point_with_debug(&self, x: f32, y: f32) -> Option<ElementId> {
        println!("开始查找坐标 ({}, {}) 处的元素", x, y);

        self.find_element_in_layout(
            x,
            y,
            |element_id| {
                println!("进入元素: {:?}", element_id);
            },
            |element_id| {
                println!("离开元素: {:?}", element_id);
            },
        )
    }

    fn compute_layout(&mut self, layout_id: TaffyId, viewport: TaffySize<AvailableSpace>) {
        self.taffy
            .compute_layout_with_measure(
                layout_id,
                viewport,
                |size, available, node_id, cx, style| match cx {
                    Some(ele) => self
                        .nodes
                        .get_mut(ele)
                        .expect("element not found")
                        .get_mut()
                        .content
                        .measure(size, available, style),
                    None => TaffySize::ZERO,
                },
            )
            .expect("compute layout failed");
    }
}

fn update_layout_style<State>(
    tree: &mut TaffyTree<ElementId>,
    elements: &mut HashMap<ElementId, ElementRef<State>>,
    layout_id: TaffyId,
) {
    {
        let ele = tree
            .get_node_context(layout_id)
            .expect("get node context failed");
        let ele = elements.get(ele).expect("element not found").get_mut();
        // tree.set_style(layout_id, ele.content.layout_style().clone())
        //     .expect("set style failed");
    }

    for child in tree.children(layout_id).expect("get children failed") {
        update_layout_style(tree, elements, child);
    }
}

// fn compute_layout<State>(
//     tree: &mut TaffyTree<ElementId>,
//     elements: &mut HashMap<ElementId, ElementRef<State>>,
//     layout_id: TaffyId,
//     viewport: TaffySize<AvailableSpace>,
// ) {
//     tree.compute_layout_with_measure(
//         layout_id,
//         viewport,
//         |size, available, node_id, cx, style| match cx {
//             Some(ele) => elements
//                 .get(ele)
//                 .expect("element not found")
//                 .get_mut()
//                 .content
//                 .measure(size, available, style),
//             None => TaffySize::ZERO,
//         },
//     )
//     .expect("compute layout failed");
// }

// pub(crate) struct LayoutTree<'a, State> {
//     tree: &'a mut TaffyTree<ElementId>,
//     elements: &'a mut HashMap<ElementId, ElementRef<State>>,
//     root: TaffyId,
// }

// impl<'a, State> LayoutTree<'a, State> {
//     pub fn compute_layout(&mut self, viewport: TaffySize<AvailableSpace>) {
//         self.tree
//             .compute_layout_with_measure(
//                 self.root,
//                 viewport,
//                 |size, available, node_id, cx, style| match cx {
//                     Some(ele) => self
//                         .elements
//                         .get_mut(ele)
//                         .expect("element not found")
//                         .get_mut()
//                         .content
//                         .measure(size, available, style),
//                     None => TaffySize::ZERO,
//                 },
//             )
//             .expect("compute layout failed");
//     }
// }


