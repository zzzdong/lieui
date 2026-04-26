// src/core/view_context.rs

use std::any::Any;
use std::collections::HashMap;

use crate::core::WidgetId;
use crate::event::{
    Event, EventCallback, EventCallbackManager, EventContext, EventHandler, EventResult, EventType,
    Key, Modifiers, MouseButton,
};
use crate::geometry::{Point, Size};
use crate::layout::{LayoutContext, LayoutNode};
use crate::render::RenderNode;
use crate::widget::{Widget, WidgetTree};
use crate::widgets::Button;

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

    pub fn create<W: Widget>(&mut self, widget: W) -> WidgetId {
        // 先创建 widget 获取真实 id
        let id = self.widget_tree.create(widget);

        // 特殊处理 Button：自动注册点击回调（使用真实 id）
        // 先取出回调，避免借用冲突
        let click_callback = self
            .get::<Button>(id)
            .and_then(|mut btn| btn.take_click_callback());
        let event_callback = self
            .get::<Button>(id)
            .and_then(|mut btn| btn.take_event_callback());

        // 处理 on_click（已包装成底层事件回调）
        if let Some(mut callback) = click_callback {
            self.register(id, EventType::Click, move |widget_id, event, ctx| {
                callback(widget_id, event, ctx);
                ctx.invalidate_render();
                EventResult::Continue
            });
        }

        // 处理 on_event（底层事件回调）
        if let Some(mut callback) = event_callback {
            self.register(id, EventType::Click, move |widget_id, event, ctx| {
                callback(widget_id, event, ctx);
                ctx.invalidate_render();
                EventResult::Continue
            });
        }

        id
    }

    pub fn set_root(&mut self, root_id: WidgetId) {
        self.widget_tree.set_root(root_id);
        self.invalidate_layout();
    }

    pub fn add_child(&mut self, parent_id: WidgetId, child_id: WidgetId) {
        self.widget_tree.add_child(parent_id, child_id);
        self.invalidate_layout();
    }

    /// 获取指定类型的 Widget 可变引用
    pub fn get<W: Any>(&self, id: WidgetId) -> Option<std::cell::RefMut<'_, W>> {
        self.widget_tree.get(id)
    }

    /// 获取 widget（用于布局）
    pub fn get_widget(&self, id: WidgetId) -> Option<std::cell::RefMut<'_, Box<dyn Widget>>> {
        self.widget_tree.get_widget(id)
    }

    /// 调用指定 widget 的事件处理方法
    ///
    /// 使用 Rc<RefCell<>> 避免借用冲突
    pub(crate) fn invoke_widget_event(&mut self, id: WidgetId, event: &Event) -> EventResult {
        // 获取 Widget 的 Rc 克隆，不持有 widget_tree 的借用
        if let Some(widget_rc) = self.widget_tree.get_widget_rc(id) {
            // 现在可以安全地借用 self
            let mut widget_ref = widget_rc.borrow_mut();
            let (result, needs_render) = widget_ref.handle_event(event, self);
            if needs_render {
                self.invalidate_render();
            }
            result
        } else {
            EventResult::Continue
        }
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
}

impl EventCallbackManager for ViewContext {
    fn register_callback(&mut self, id: WidgetId, event_type: EventType, callback: EventCallback) {
        self.callbacks
            .entry(id)
            .or_default()
            .entry(event_type)
            .or_default()
            .push(callback);
    }

    fn take_callbacks(&mut self, id: WidgetId, event_type: EventType) -> Vec<EventCallback> {
        self.callbacks
            .get_mut(&id)
            .and_then(|m| m.remove(&event_type))
            .unwrap_or_default()
    }
}

impl ViewContext {
    /// 注册事件回调（兼容旧 API）
    ///
    /// 回调签名统一为：FnMut(WidgetId, &Event, &mut ViewContext) -> EventResult
    pub fn register<F>(&mut self, id: WidgetId, event: EventType, f: F)
    where
        F: FnMut(WidgetId, &Event, &mut ViewContext) -> EventResult + 'static,
    {
        self.register_callback(id, event, Box::new(f));
    }

    /// 获取布局树根节点
    pub fn layout_root(&self) -> Option<&LayoutNode> {
        self.layout_ctx.root.as_ref()
    }

    /// 处理鼠标移动事件
    pub fn handle_mouse_move(&mut self, point: Point) {
        if self.needs_layout {
            self.perform_layout();
        }

        if let Some(layout_root) = self.layout_ctx.root.clone() {
            // 使用 EventHandler 生成 MouseEnter/MouseLeave 事件
            let events = self.event_handler.handle_mouse_move(point, &layout_root);
            for (_, event) in events {
                EventContext::dispatch(self, &event, point, &layout_root);
            }
        }
    }

    /// 处理鼠标按下事件
    pub fn handle_mouse_down(&mut self, point: Point, button: MouseButton) {
        if self.needs_layout {
            self.perform_layout();
        }

        if let Some(layout_root) = self.layout_ctx.root.clone() {
            let events = self
                .event_handler
                .handle_mouse_down(point, button, &layout_root);
            for (_, event) in events {
                EventContext::dispatch(self, &event, point, &layout_root);
            }
        }
    }

    /// 处理鼠标释放事件
    pub fn handle_mouse_up(&mut self, point: Point, button: MouseButton) {
        if self.needs_layout {
            self.perform_layout();
        }

        if let Some(layout_root) = self.layout_ctx.root.clone() {
            let events = self
                .event_handler
                .handle_mouse_up(point, button, &layout_root);
            for (_, event) in events {
                EventContext::dispatch(self, &event, point, &layout_root);
            }
        }
    }

    /// 处理鼠标滚轮事件
    pub fn handle_mouse_wheel(&mut self, delta_x: f32, delta_y: f32, point: Point) {
        if self.needs_layout {
            self.perform_layout();
        }

        if let Some(layout_root) = self.layout_ctx.root.clone() {
            let events =
                self.event_handler
                    .handle_mouse_wheel(delta_x, delta_y, point, &layout_root);
            for (_, event) in events {
                EventContext::dispatch(self, &event, point, &layout_root);
            }
        }
    }

    /// 处理键盘按下事件
    pub fn handle_key_down(&mut self, key: Key, modifiers: Modifiers) {
        if self.focused.is_some() {
            let event = Event::KeyDown { key, modifiers };
            if let Some(ref layout_root) = self.layout_ctx.root.clone() {
                EventContext::dispatch(self, &event, Point::ZERO, &layout_root);
            }
        }
    }

    /// 处理键盘释放事件
    pub fn handle_key_up(&mut self, key: Key, modifiers: Modifiers) {
        if self.focused.is_some() {
            let event = Event::KeyUp { key, modifiers };
            if let Some(ref layout_root) = self.layout_ctx.root.clone() {
                EventContext::dispatch(self, &event, Point::ZERO, &layout_root);
            }
        }
    }

    /// 设置焦点到指定 widget
    pub fn set_focus(&mut self, widget_id: Option<WidgetId>) {
        let events = self.event_handler.handle_focus_change(widget_id);
        self.focused = widget_id;

        if let Some(ref layout_root) = self.layout_ctx.root.clone() {
            for (_, event) in events {
                EventContext::dispatch(self, &event, Point::ZERO, layout_root);
            }
        }
    }

    /// 获取当前焦点的 widget
    pub fn focused_widget(&self) -> Option<WidgetId> {
        self.event_handler.focused()
    }

    /// 获取当前悬停的 widget
    pub fn hovered_widget(&self) -> Option<WidgetId> {
        self.event_handler.hovered()
    }
}
