// src/core/view_context.rs

use std::any::Any;
use std::collections::HashMap;

use crate::core::WidgetId;
use crate::event::{Event, EventHandler, EventResult, EventType, MouseButton};
use crate::geometry::{Point, Size};
use crate::layout::{LayoutContext, LayoutNode};
use crate::render::RenderNode;
use crate::widget::{Widget, WidgetTree};
use crate::widgets::Button;

/// 事件回调类型
pub type EventCallback = Box<dyn FnMut(&mut ViewContext, WidgetId)>;

pub struct ViewContext {
    /// Widget 树
    widget_tree: WidgetTree,
    viewport: Size,
    focused: Option<WidgetId>,
    needs_layout: bool,
    needs_render: bool,
    pub debug_render_tree: bool,
    /// 布局上下文
    layout_ctx: LayoutContext,
    /// 事件处理器
    event_handler: EventHandler,
    /// 事件回调存储：widget_id -> (event_type -> callbacks)
    callbacks: HashMap<WidgetId, HashMap<EventType, Vec<EventCallback>>>,
}

impl ViewContext {
    pub fn new(viewport: Size) -> Self {
        Self {
            widget_tree: WidgetTree::new(),
            viewport,
            focused: None,
            needs_layout: true,
            needs_render: true,
            debug_render_tree: false,
            layout_ctx: LayoutContext::new(),
            event_handler: EventHandler::new(),
            callbacks: HashMap::new(),
        }
    }

    pub fn set_viewport(&mut self, viewport: Size) {
        self.viewport = viewport;
        self.invalidate_layout();
    }

    /// 获取 Widget 树的引用
    pub fn widget_tree(&self) -> &WidgetTree {
        &self.widget_tree
    }

    /// 获取 Widget 树的可变引用
    pub fn widget_tree_mut(&mut self) -> &mut WidgetTree {
        &mut self.widget_tree
    }

    pub fn create<W: Widget>(&mut self, mut widget: W) -> WidgetId {
        // 特殊处理 Button：自动注册点击回调
        if let Some(btn) = widget.as_any_mut().downcast_mut::<Button>() {
            if let Some(mut callback) = btn.take_click_callback() {
                let id = WidgetId::new();
                self.register(id, EventType::Click, move |ctx, widget_id| {
                    if let Some(btn) = ctx.get_mut::<Button>(widget_id) {
                        callback(btn);
                        ctx.invalidate_render();
                    }
                });
            }
        }

        self.widget_tree.create(widget)
    }

    pub fn set_root(&mut self, root_id: WidgetId) {
        self.widget_tree.set_root(root_id);
        self.invalidate_layout();
    }

    pub fn add_child(&mut self, parent_id: WidgetId, child_id: WidgetId) {
        self.widget_tree.add_child(parent_id, child_id);
        self.invalidate_layout();
    }

    pub fn get<W: Any>(&self, id: WidgetId) -> Option<&W> {
        self.widget_tree.get(id)
    }

    pub fn get_mut<W: Any>(&mut self, id: WidgetId) -> Option<&mut W> {
        self.widget_tree.get_mut(id)
    }

    /// 获取 widget（用于布局）
    pub fn get_widget(&self, id: WidgetId) -> Option<&dyn Widget> {
        self.widget_tree.get_widget(id)
    }

    /// 获取 widget（可变）
    pub fn get_widget_mut(&mut self, id: WidgetId) -> Option<&mut dyn Widget> {
        self.widget_tree.get_widget_mut(id)
    }

    /// 执行布局（新布局系统）
    pub fn perform_layout(&mut self) {
        if !self.needs_layout {
            return;
        }

        if let Some(root_id) = self.widget_tree.root() {
            // 创建新的布局上下文来避免借用冲突
            let mut layout_ctx = LayoutContext::new();

            // Phase 1: 收集约束
            layout_ctx.collect(root_id, self);

            // Phase 2: 计算位置
            layout_ctx.compute(self.viewport);

            // 保存布局上下文
            self.layout_ctx = layout_ctx;
        }

        self.needs_layout = false;
        self.needs_render = true;
    }

    /// 从布局树构建渲染树
    pub fn build_render_tree(&self) -> Option<RenderNode> {
        self.layout_ctx
            .root
            .as_ref()
            .map(|root| self.build_render_node_recursive(root))
    }

    fn build_render_node_recursive(&self, layout_node: &LayoutNode) -> RenderNode {
        let id = layout_node.id;

        // 获取 widget 并渲染
        let mut node = if let Some(widget) = self.widget_tree.get_widget(id) {
            widget.render(layout_node, self)
        } else {
            // 默认渲染
            RenderNode::div(
                layout_node
                    .computed
                    .as_ref()
                    .map(|c| c.content_box)
                    .unwrap_or_default(),
            )
        };

        // 递归添加子节点
        for child_layout in &layout_node.children {
            let child_node = self.build_render_node_recursive(child_layout);
            node = node.add_child(child_node);
        }

        node
    }

    pub fn render(&mut self) -> Option<RenderNode> {
        if self.needs_layout {
            self.perform_layout();
        }

        if self.needs_render {
            self.needs_render = false;
            self.build_render_tree()
        } else {
            None
        }
    }

    pub fn invalidate_layout(&mut self) {
        self.needs_layout = true;
    }

    pub fn invalidate_render(&mut self) {
        self.needs_render = true;
    }

    /// 注册事件回调
    pub fn register<F>(&mut self, id: WidgetId, event: EventType, f: F)
    where
        F: FnMut(&mut ViewContext, WidgetId) + 'static,
    {
        self.callbacks
            .entry(id)
            .or_default()
            .entry(event)
            .or_default()
            .push(Box::new(f));
    }

    /// 触发事件到指定 widget（执行回调）
    fn trigger_callbacks(&mut self, id: WidgetId, event: &Event) {
        let event_type = event.to_type();

        // 取出回调（避免借用冲突）
        let callbacks = self
            .callbacks
            .get_mut(&id)
            .and_then(|m| m.get_mut(&event_type))
            .map(std::mem::take);

        if let Some(mut cbs) = callbacks {
            for cb in &mut cbs {
                cb(self, id);
            }
            // 放回去
            if let Some(entry) = self.callbacks.get_mut(&id) {
                entry.insert(event_type, cbs);
            }
        }
    }

    /// 分发事件到目标 widget，并支持冒泡
    ///
    /// 事件处理流程：
    /// 1. 找到目标 widget
    /// 2. 执行注册的回调
    /// 3. 调用 widget.handle_event()
    /// 4. 如果返回 Continue，向上冒泡到父节点
    fn dispatch_event(&mut self, target_id: WidgetId, event: Event) {
        // 获取从目标到根的路径（用于冒泡）
        let bubble_path = self.get_bubble_path(target_id);

        // 遍历冒泡路径
        for widget_id in bubble_path {
            // 先执行回调（如 on_click 注册的回调）
            self.trigger_callbacks(widget_id, &event);

            // 再调用 widget 的 handle_event 方法
            let result = if let Some(widget) = self.widget_tree.get_widget_mut(widget_id) {
                widget.handle_event(&event)
            } else {
                EventResult::Continue
            };

            // 根据结果决定是否继续传播
            match result {
                EventResult::Stop => break,
                EventResult::PreventDefault => {
                    // 阻止默认行为但继续传播
                    // TODO: 实现默认行为机制
                }
                EventResult::Continue => {
                    // 继续冒泡
                }
            }
        }
    }

    /// 获取从指定节点到根节点的冒泡路径
    fn get_bubble_path(&self, target_id: WidgetId) -> Vec<WidgetId> {
        let mut path = vec![target_id];

        // 从布局树中查找父节点关系
        if let Some(ref layout_root) = self.layout_ctx.root {
            self.find_path_to_root(layout_root, target_id, &mut path);
        }

        path
    }

    /// 递归查找从目标到根的路径
    fn find_path_to_root(
        &self,
        node: &LayoutNode,
        target_id: WidgetId,
        path: &mut Vec<WidgetId>,
    ) -> bool {
        if node.id == target_id {
            return true;
        }

        for child in &node.children {
            if self.find_path_to_root(child, target_id, path) {
                path.push(node.id);
                return true;
            }
        }

        false
    }

    /// 处理鼠标移动事件
    pub fn handle_mouse_move(&mut self, point: Point) {
        // 确保布局已更新
        if self.needs_layout {
            self.perform_layout();
        }

        if let Some(ref layout_root) = self.layout_ctx.root {
            let events = self.event_handler.handle_mouse_move(point, layout_root);
            // 处理事件
            for (id, event) in events {
                self.dispatch_event(id, event);
            }
        }
    }

    /// 处理鼠标按下事件
    pub fn handle_mouse_down(&mut self, point: Point, button: MouseButton) {
        // 确保布局已更新
        if self.needs_layout {
            self.perform_layout();
        }

        if let Some(ref layout_root) = self.layout_ctx.root {
            let events = self
                .event_handler
                .handle_mouse_down(point, button, layout_root);
            for (id, event) in events {
                self.dispatch_event(id, event);
            }
        }
    }

    /// 处理鼠标释放事件
    pub fn handle_mouse_up(&mut self, point: Point, button: MouseButton) {
        // 确保布局已更新
        if self.needs_layout {
            self.perform_layout();
        }

        if let Some(ref layout_root) = self.layout_ctx.root {
            let events = self
                .event_handler
                .handle_mouse_up(point, button, layout_root);
            for (id, event) in events {
                self.dispatch_event(id, event);
            }
        }
    }

    /// 获取当前悬停的 widget
    pub fn hovered_widget(&self) -> Option<WidgetId> {
        self.event_handler.hovered()
    }
}
