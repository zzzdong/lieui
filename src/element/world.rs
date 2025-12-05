use std::{
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    hash::Hash,
    rc::Rc,
};

use quick_xml::{
    Reader,
    events::{
        Event,
        attributes::{AttrError, Attribute},
    },
};
use taffy::{
    AvailableSpace, Dimension, Layout as TaffyLayout, NodeId, NodeId as TaffyId, PrintTree,
    Size as TaffySize, Style as TaffyStyle, TaffyTree,
};
use vello_cpu::{
    RenderContext,
    kurbo::{Point, Rect, Shape, Size},
    peniko::Color,
};
use winit::event::{self, ElementState, MouseButton, WindowEvent};

use crate::{
    element::{
        DivElement, TextElement,
        style::{Style, StyleError},
    },
    paint::PaintContext,
};

use super::{ElementId, IElement};

pub struct ElementNode {
    pub content: Box<dyn IElement>,
}

impl ElementNode {
    pub fn new(content: Box<dyn IElement + 'static>) -> Self {
        Self { content }
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

    pub fn builder() -> Builder {
        Builder::new()
    }

    pub fn add_node(&mut self, node: Box<dyn IElement + 'static>) -> ElementId {
        let node_id = ElementId(self.next_id);
        self.next_id += 1;

        let node = ElementNode::new(node);
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

    pub fn add_root(&mut self, root: Box<dyn IElement + 'static>) -> ElementId {
        let node_id = self.add_node(root);

        self.root = Some(node_id);

        node_id
    }

    pub fn add_child(
        &mut self,
        parent: ElementId,
        child: Box<dyn IElement + 'static>,
    ) -> ElementId {
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

                // self.taffy.print_tree(layout_id);
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
        let layout = self
            .taffy
            .layout(layout_id)
            .cloned()
            .expect("get layout failed");

        let style = self.styles.get(&ele_id).cloned().unwrap_or_default();
        let old_paint = cx.paint().clone();

        // 2. 绘制背景颜色和边框

        // 计算边框矩形的尺寸关系：
        // - 边框矩形：包含边框的完整矩形
        // - 内容矩形：不包含边框的内容区域
        let border_rect_size = Size::new(layout.size.width as f64, layout.size.height as f64);
        let border_rect = border_rect_size.to_rect().with_origin(Point::new(
            abs_origin.x + layout.location.x as f64,
            abs_origin.y + layout.location.y as f64,
        ));

        // 绘制背景颜色（填充整个区域，包括内边距和内容区域）
        if style.border_radius > 0.0 {
            cx.set_paint(style.border_color);
            cx.fill_blurred_rounded_rect(&border_rect, style.border_radius as f32, 1.0);

            cx.set_paint(style.background_color);
            cx.fill_blurred_rounded_rect(
                &border_rect.inset(layout.border.left as f64),
                style.border_radius as f32,
                1.0,
            );
        } else {
            cx.set_paint(style.border_color);
            cx.fill_rect(&border_rect);
            cx.set_paint(style.background_color);
            cx.fill_rect(&border_rect.inset(layout.border.left as f64));
        }

        // 3. 计算内容矩形（考虑padding）
        let content_size = Size::new(
            layout.content_box_size().width as f64,
            layout.content_box_size().height as f64,
        );
        let content_rect = content_size.to_rect().with_origin(Point::new(
            abs_origin.x
                + layout.location.x as f64
                + layout.border.left as f64
                + layout.padding.left as f64,
            abs_origin.y
                + layout.location.y as f64
                + layout.border.top as f64
                + layout.padding.top as f64,
        ));

        // 3. 画 **当前节点**（在绝对坐标系里）
        cx.push_clip_layer(&content_rect.to_path(1.0));

        let mut paint_cx = PaintContext::new(cx, style, content_rect);

        node_ref.get_mut().content.paint(&mut paint_cx); // 注意：layout 仍是相对值，若需要绝对值可再传 abs_origin
        cx.pop_layer();
        cx.set_paint(old_paint);

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
        self.update_layout_style(layout_id);

        self.taffy
            .compute_layout_with_measure(
                layout_id,
                viewport,
                |size, available, _node_id, cx, style| match cx {
                    Some(ele) => {
                        let style = self.styles.get(&ele).cloned().unwrap_or_default();
                        self.nodes
                            .get_mut(ele)
                            .expect("element not found")
                            .get_mut()
                            .content
                            .measure(size, available, &style)
                    }
                    None => TaffySize::ZERO,
                },
            )
            .expect("compute layout failed");
    }

    fn update_layout_style(&mut self, layout_id: TaffyId) {
        let ele = self.taffy.get_node_context(layout_id).unwrap();
        if let Some(style) = self.styles.get(&ele).cloned() {
            self.taffy
                .set_style(layout_id, style.to_taffy_style())
                .expect("set style failed");
        }

        for child_id in self.taffy.children(layout_id).unwrap() {
            self.update_layout_style(child_id);
        }
    }
}

pub struct Builder {
    tree: ElementTree,
}

impl Builder {
    fn new() -> Self {
        Self {
            tree: ElementTree::new(),
        }
    }

    pub fn build(self) -> ElementTree {
        self.tree
    }

    pub fn load_xml(mut self, xml: &str) -> Result<Self, StyleError> {
        let xml_element = read_xml(xml)?;

        let XmlElement {
            element,
            style,
            children,
            ..
        } = xml_element;

        let root = self.tree.add_root(element);
        self.tree.set_style(root, style);
        self.add_children(root, children);

        Ok(self)
    }

    fn add_children(&mut self, parent: ElementId, children: Vec<XmlElement>) {
        for child in children {
            let child_id = self.tree.add_child(parent, child.element);
            self.tree.set_style(child_id, child.style);
            self.add_children(child_id, child.children);
        }
    }
}

#[derive(Debug, Default)]
struct EventHandler {
    pub on_click: Option<String>,
    pub on_mouse_move: Option<String>,
    pub on_mouse_enter: Option<String>,
    pub on_mouse_leave: Option<String>,
    pub on_mouse_down: Option<String>,
    pub on_mouse_up: Option<String>,
    pub on_key_down: Option<String>,
    pub on_key_up: Option<String>,
}

impl EventHandler {
    pub fn from_attrs(attrs: Vec<Attribute>) -> Result<Self, StyleError> {
        let mut event_handler = EventHandler::default();

        for attr in attrs {
            let key = std::str::from_utf8(attr.key.as_ref())?.to_lowercase();
            let val = std::str::from_utf8(attr.value.as_ref())?.to_string();

            if key.starts_with('@') {
                match key.as_str() {
                    "@click" => event_handler.on_click = Some(val),
                    "@mousemove" => event_handler.on_mouse_move = Some(val),
                    "@mouseenter" => event_handler.on_mouse_enter = Some(val),
                    "@mouseleave" => event_handler.on_mouse_leave = Some(val),
                    "@mousedown" => event_handler.on_mouse_down = Some(val),
                    "@mouseup" => event_handler.on_mouse_up = Some(val),
                    "@keydown" => event_handler.on_key_down = Some(val),
                    "@keyup" => event_handler.on_key_up = Some(val),
                    _ => return Err(StyleError::Message(format!("unknown event: {}", key))),
                }
            }
        }

        Ok(event_handler)
    }
}

struct XmlElement {
    element: Box<dyn IElement + 'static>,
    style: Style,
    event_handler: EventHandler,
    children: Vec<XmlElement>,
}

impl XmlElement {
    fn new(element: Box<dyn IElement>, style: Style, event_handler: EventHandler) -> Self {
        Self {
            element,
            style,
            event_handler,
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

                let event_handler = EventHandler::from_attrs(
                    e.attributes()
                        .collect::<std::result::Result<Vec<_>, AttrError>>()?,
                )?;

                match tag.as_str() {
                    "view" => {
                        stack.push(XmlElement::new(
                            Box::new(DivElement::new()),
                            style,
                            event_handler,
                        ));
                    }
                    "div" => {
                        stack.push(XmlElement::new(
                            Box::new(DivElement::new()),
                            style,
                            event_handler,
                        ));
                    }
                    "text" => {
                        let text_element = read_text_element(&mut reader)?;
                        if let Some(ele) = stack.last_mut() {
                            ele.children.push(XmlElement::new(
                                Box::new(text_element),
                                style,
                                event_handler,
                            ));
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(e) => {
                let text = e.decode()?;
                if let Some(XmlElement { element, .. }) = stack.last_mut() {
                    if let Some(text_element) =
                        (element.as_mut() as &mut dyn std::any::Any).downcast_mut::<TextElement>()
                    {
                        text_element.text.push_str(&text);
                    }
                }
            }
            Event::End(e) => {
                if let Some(mut elem) = stack.pop() {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(elem);
                    } else {
                        inherit_styles(&mut elem);
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

fn read_text_element(reader: &mut Reader<&[u8]>) -> Result<TextElement, StyleError> {
    let mut text = String::new();

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                return Err(StyleError::Message(format!(
                    "unexpected start tag: {:?}",
                    e.name().as_ref()
                )));
            }
            Event::Text(e) => {
                text.push_str(&e.decode()?);
            }
            Event::End(_) => break,
            _ => {}
        }
    }

    Ok(TextElement::new(text))
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
        if node.style.font_family.is_empty() {
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
        font_size: 16.0,
        font_family: "sans-serif".into(),
        ..Default::default()
    };

    walk(&root_style, root);
}
