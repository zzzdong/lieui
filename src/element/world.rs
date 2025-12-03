use std::{
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    rc::Rc,
};

use quick_xml::{
    Reader,
    events::{Event, attributes::AttrError},
};
use taffy::{
    AvailableSpace, Dimension, Layout as TaffyLayout, NodeId, NodeId as TaffyId, PrintTree,
    Size as TaffySize, Style as TaffyStyle, TaffyTree,
};
use vello_cpu::{
    RenderContext,
    kurbo::{Point, Rect, Shape}, peniko::Color,
};
use winit::event::{ElementState, MouseButton, WindowEvent};

use crate::element::{
    DivElement, TextElement,
    style::{Style, StyleError},
};

use super::{ElementId, IElement};

pub struct ElementNode {
    pub content: Box<dyn IElement>,
}

impl ElementNode {
    pub fn new(content: impl IElement + 'static) -> Self {
        Self {
            content: Box::new(content),
        }
    }

    pub fn into_ref(self) -> ElementRef {
        ElementRef(Rc::new(RefCell::new(self)))
    }
}

pub struct ElementRef(Rc<RefCell<ElementNode>>);

impl ElementRef {
    pub fn get_mut(&self) -> RefMut<ElementNode> {
        self.0.borrow_mut()
    }

    pub fn get(&self) -> Ref<ElementNode> {
        self.0.borrow()
    }
}

impl Clone for ElementRef {
    fn clone(&self) -> Self {
        ElementRef(self.0.clone())
    }
}

pub struct ElementTree {
    nodes: HashMap<ElementId, ElementRef>,
    layouts: HashMap<ElementId, TaffyId>,
    styles: HashMap<ElementId, Style>,
    taffy: TaffyTree<ElementId>,
    root: Option<ElementId>,
    next_id: u64,
}

impl ElementTree {
    pub fn new() -> Self {
        let mut nodes = HashMap::new();
        let mut taffy = TaffyTree::new();

        let mut tree = Self {
            nodes,
            layouts: HashMap::new(),
            styles: HashMap::new(),
            taffy,
            root: None,
            next_id: 0,
        };

        tree
    }

    pub fn add_node(&mut self, node: impl IElement + 'static) -> ElementId {
        let node_id = ElementId(self.next_id);
        self.next_id += 1;

        let node_style = node.layout_style();

        let node = ElementNode::new(node);
        let node_ref = node.into_ref();

        // 创建布局节点
        let layout_id = self
            .taffy
            .new_leaf_with_context(TaffyStyle::default(), node_id)
            .expect("failed to create layout node");

        self.taffy
            .set_style(layout_id, node_style)
            .expect("set layout style failed");

        self.nodes.insert(node_id, node_ref);
        self.layouts.insert(node_id, layout_id);

        node_id
    }

    pub fn add_root(&mut self, root: impl IElement + 'static) -> ElementId {
        let node_id = self.add_node(root);

        self.root = Some(node_id);

        node_id
    }

    pub fn add_child(&mut self, parent: ElementId, child: impl IElement + 'static) -> ElementId {
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

    pub fn set_style(&mut self, ele_id: ElementId, style: Style) {
        self.styles.insert(ele_id, style);
    }

    pub fn style_mut(&mut self, ele_id: ElementId) -> &mut Style {
        self.styles.get_mut(&ele_id).expect("style not found")
    }

    pub fn do_layout(&mut self, width: f32, height: f32) {
        match self.root {
            Some(root_id) => {
                let layout_id = self
                    .layouts
                    .get(&root_id)
                    .expect("root layout id not found")
                    .clone();

                // compute_layout(&mut self.taffy, &mut self.nodes, layout_id, viewport);
                self.compute_layout(
                    layout_id,
                    TaffySize::<AvailableSpace> {
                        width: AvailableSpace::from(width),
                        height: AvailableSpace::from(height),
                    },
                );

                self.taffy.print_tree(layout_id);
            }
            None => {
                return;
            }
        }
    }

    pub fn do_paint(&mut self, cx: &mut RenderContext) {
        if let Some(root_id) = self.root {
            // 根节点从 (0, 0) 开始
            self.paint_node(cx, root_id, Point::new(0.0, 0.0));
        }
    }

    /// `abs_origin` 是当前节点在屏幕上的 **绝对原点**
    fn paint_node(&mut self, cx: &mut RenderContext, ele_id: ElementId, abs_origin: Point) {
        let node_ref = self.nodes.get(&ele_id).expect("element not found").clone();
        let layout_id = self
            .layouts
            .get(&ele_id)
            .copied()
            .expect("layout id not found");

        // 1. 拿到 **相对于父节点** 的布局
        let mut layout = self
            .taffy
            .layout(layout_id)
            .cloned()
            .expect("get layout failed");

        // 2. 计算 **绝对矩形**
        let abs_x = abs_origin.x + layout.location.x as f64;
        let abs_y = abs_origin.y + layout.location.y as f64;
        let abs_rect = Rect::new(
            abs_x,
            abs_y,
            abs_x + layout.size.width as f64,
            abs_y + layout.size.height as f64,
        );

        layout.location.x = abs_x as f32;
        layout.location.y = abs_y as f32;

        // 3. 画 **当前节点**（在绝对坐标系里）
        cx.push_clip_layer(&abs_rect.to_path(1.0));
        node_ref.get_mut().content.paint(cx, &layout); // 注意：layout 仍是相对值，若需要绝对值可再传 abs_origin
        cx.pop_layer();

        // 4. 递归子节点：把 **当前绝对原点** 传下去
        for child_layout_id in self.taffy.children(layout_id).expect("get children failed") {
            let child_ele_id = self
                .taffy
                .get_node_context(child_layout_id)
                .expect("get node context failed");

            // 子节点的 **绝对原点** = 父绝对原点 + 子相对偏移
            let child_origin = Point::new(
                abs_origin.x + layout.location.x as f64,
                abs_origin.y + layout.location.y as f64,
            );

            self.paint_node(cx, *child_ele_id, child_origin);
        }
    }

    /// 根据ElementId获取节点引用
    fn node(&self, ele_id: ElementId) -> ElementRef {
        self.nodes.get(&ele_id).expect("node not found").clone()
    }

    pub fn get_mut(&mut self, ele_id: ElementId) -> ElementRef {
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
        mut inspect_enter_fn: InspectEnterFn,
        mut inspect_exit_fn: InspectExitFn,
    ) -> Option<ElementId>
    where
        InspectEnterFn: FnMut(ElementId),
        InspectExitFn: FnMut(ElementId),
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
            &mut inspect_enter_fn,
            &mut inspect_exit_fn,
        )
    }

    /// 递归遍历布局树的辅助函数
    fn find_element_in_layout_recursive<InspectEnterFn, InspectExitFn>(
        tree: &TaffyTree<ElementId>,
        node_id: TaffyId,
        x: f32,
        y: f32,
        parent_absolute_pos: (f32, f32),
        inspect_enter_fn: &mut InspectEnterFn,
        inspect_exit_fn: &mut InspectExitFn,
    ) -> Option<ElementId>
    where
        InspectEnterFn: FnMut(ElementId),
        InspectExitFn: FnMut(ElementId),
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

        // 检查点是否在当前节点的边界内
        let contains_point = x >= absolute_pos.0
            && x <= absolute_pos.0 + layout.size.width
            && y >= absolute_pos.1
            && y <= absolute_pos.1 + layout.size.height;

        if !contains_point {
            return None;
        }

        // 调用进入元素钩子
        inspect_enter_fn(element_id);

        if let Ok(children) = tree.children(node_id) {
            for child_id in children.iter() {
                if let Some(child_element) = Self::find_element_in_layout_recursive(
                    tree,
                    *child_id,
                    x,
                    y,
                    absolute_pos,
                    inspect_enter_fn,
                    inspect_exit_fn,
                ) {
                    inspect_exit_fn(element_id);
                    return Some(child_element);
                }
            }
        }

        // 调用离开元素钩子
        inspect_exit_fn(element_id);

        Some(element_id)
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

    pub fn collect_element_path(&self, x: f32, y: f32) -> Vec<ElementId> {
        let mut path = vec![];
        self.find_element_in_layout(
            x,
            y,
            |element_id| {
                path.push(element_id);
            }, // 进入元素时将元素ID添加到路径中
            |_| {}, // 离开元素时不执行任何操作
        );
        path
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
        update_layout_style(&mut self.taffy, &mut self.nodes, layout_id);

        // self.taffy.compute_layout(layout_id, viewport).expect("compute layout failed");

        self.taffy
            .compute_layout_with_measure(
                layout_id,
                viewport,
                |size, available, _node_id, cx, style| match cx {
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

pub struct Builder {
    tree: ElementTree,
}

impl Builder {
    pub fn new() -> Self {
        Self {
            tree: ElementTree::new(),
        }
    }

    pub fn build(self) -> ElementTree {
        self.tree
    }


}

struct XmlElement {
    element: Box<dyn IElement + 'static>,
    style: Style,
    children: Vec<XmlElement>,
}

impl XmlElement {
    fn new(element: Box<dyn IElement>, style: Style) -> Self {
        Self {
            element,
            style,
            children: Vec::new(),
        }
    }
}

pub fn read_xml(xml: &str) -> Result<XmlElement, StyleError> {
    let mut reader = Reader::from_str(xml);

    reader.config_mut().trim_text(true);

    let mut stack: Vec<XmlElement> = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let tag = std::str::from_utf8(&e.name().as_ref())?.to_lowercase();

                let style = Style::from_attrs(
                    e.attributes()
                        .collect::<std::result::Result<Vec<_>, AttrError>>()?,
                )?;

                match tag.as_str() {
                    "view" => {
                        stack.push(XmlElement::new(Box::new(DivElement::new()), style));
                    }
                    "div" => {
                        stack.push(XmlElement::new(Box::new(DivElement::new()), style));
                    }
                    "text" => {
                        stack.push(XmlElement::new(
                            Box::new(TextElement::new("".into())),
                            style,
                        ));
                    }
                    _ => {}
                }
            }
            Event::Text(e) => {
                let text = e.decode()?;
                if let Some(XmlElement {
                    element,
                    style,
                    children,
                }) = stack.last_mut()
                {
                    if let Some(text_element) =
                        <dyn std::any::Any>::downcast_mut::<TextElement>(element.as_mut())
                    {
                        text_element.text.push_str(&text);
                    }
                }
            }
            Event::End(e) => {
                if let Some(elem) = stack.pop() {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(elem);
                    } else {
                        return Ok(elem);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Err(StyleError::Message("failed to parse xml".into()))
}


fn inherit_styles(root: &mut XmlElement) {
    fn walk(parent_style: &Style, node: &mut XmlElement) {
        // 仅继承"可继承"字段
        if node.style.color == Color::TRANSPARENT {
            node.style.color = parent_style.color;
        }
        if node.style.font_size == 0.0 {
            node.style.font_size = parent_style.font_size;
        }
        if node.style.font_family == "sans-serif" {
            node.style.font_family = parent_style.font_family.clone();
        }
        // background/border/margin 不继承，保持本地或默认值

        // 继续向下
        let next_parent = node.style.clone();
        for child in &mut node.children {
            walk(&next_parent, child);
        }
    }
    
    // 虚拟根样式：黑字 14 px sans-serif
    let root_style = Style {
        color: Color::from_rgb8(0, 0, 0),
        font_size: 14.0,
        font_family: "sans-serif".into(),
        ..Default::default()
    };
    
    walk(&root_style, root);
}
