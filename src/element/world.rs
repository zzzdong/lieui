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

use super::{ElementId, IElement};

pub struct ElementNode {
    pub content: Box<dyn IElement>,
}

impl ElementNode {
    pub fn new<T: IElement + 'static>(content: T) -> Self {
        Self {
            content: Box::new(content),
        }
    }

    pub fn into_ref(self) -> ElementRef {
        ElementRef(Rc::new(RefCell::new(self)))
    }
}

#[derive(Clone)]
pub struct ElementRef(Rc<RefCell<ElementNode>>);

impl ElementRef {
    pub fn get_mut(&self) -> RefMut<ElementNode> {
        self.0.borrow_mut()
    }

    pub fn get(&self) -> Ref<ElementNode> {
        self.0.borrow()
    }
}

struct ElementTree {
    nodes: HashMap<ElementId, ElementRef>,
    layouts: HashMap<ElementId, TaffyId>,
    taffy: TaffyTree<ElementId>,
    root: Option<ElementId>,
    next_id: u64,
}

impl ElementTree {
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

    pub fn add_node(&mut self, node: ElementNode) -> ElementId {
        let node_id = ElementId(self.next_id);
        self.next_id += 1;

        let node_ref = node.into_ref();

        // 创建布局节点
        let layout_id = self
            .taffy
            .new_leaf_with_context(TaffyStyle::default(), node_id)
            .expect("failed to create layout node");

        self.nodes.insert(node_id, node_ref.clone());
        self.layouts.insert(node_id, layout_id);

        node_id
    }

    pub fn insert_child(&mut self, parent: ElementId, child: ElementNode) -> ElementId {
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

                compute_layout(&mut self.taffy, &mut self.nodes, layout_id, viewport);

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
    fn node(&self, ele_id: ElementId) -> ElementRef {
        self.nodes.get(&ele_id).expect("node not found").clone()
    }
}

fn update_layout_style(
    tree: &mut TaffyTree<ElementId>,
    elements: &mut HashMap<ElementId, ElementRef>,
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

fn compute_layout(
    tree: &mut TaffyTree<ElementId>,
    elements: &mut HashMap<ElementId, ElementRef>,
    layout_id: TaffyId,
    viewport: TaffySize<AvailableSpace>,
) {
    tree.compute_layout_with_measure(
        layout_id,
        viewport,
        |size, available, node_id, cx, style| match cx {
            Some(ele) => elements
                .get(ele)
                .expect("element not found")
                .get_mut()
                .content
                .measure(size, available, style),
            None => TaffySize::ZERO,
        },
    )
    .expect("compute layout failed");
}

#[cfg(test)]
mod tests {
    use taffy::prelude::length;
    use vello_cpu::Pixmap;

    use crate::element::{DivElement, TextElement};

    use super::*;

    #[test]
    fn test_layout() {
        let mut tree = ElementTree::new();

        // 创建根节点
        let root_node = ElementNode::new(DivElement {
            style: TaffyStyle {
                size: TaffySize {
                    width: length(100.0),
                    height: length(100.0),
                },
                ..Default::default()
            },
            children: vec![],
        });

        let root = tree.add_node(root_node);
        tree.set_root(root);

        tree.insert_child(
            root,
            ElementNode::new(TextElement::new("Hello, 中文!".to_string())),
        );

        tree.do_layout(TaffySize::<AvailableSpace> {
            width: AvailableSpace::Definite(800.0),
            height: AvailableSpace::Definite(600.0),
        });

        let mut render_cx = RenderContext::new(800, 600);

        tree.do_paint(&mut render_cx);

        let mut pixmap = Pixmap::new(800, 600);
        render_cx.render_to_pixmap(&mut pixmap);

        let png = pixmap.into_png().expect("failed to encode png");

        std::fs::write("test.png", png).expect("failed to write png")
    }
}
