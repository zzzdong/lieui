// src/core/view_context.rs

use std::any::Any;

use crate::core::WidgetId;
use crate::event::{EventManager, Key, Modifiers, MouseButton};
use crate::geometry::{Point, Size};
use crate::layout::{LayoutContext, LayoutNode};
use crate::render::RenderNode;
use crate::widget::{Widget, WidgetTree};

pub struct ViewContext {
    /// Widget 树
    widget_tree: WidgetTree,
    viewport: Size,
    needs_layout: bool,
    needs_render: bool,
    pub debug_render_tree: bool,
    /// 布局上下文
    layout_ctx: LayoutContext,
    /// 事件管理器
    event_manager: EventManager,
}

impl ViewContext {
    pub fn new(viewport: Size) -> Self {
        Self {
            widget_tree: WidgetTree::new(),
            viewport,
            needs_layout: true,
            needs_render: true,
            debug_render_tree: false,
            layout_ctx: LayoutContext::new(),
            event_manager: EventManager::new(),
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
        // 直接创建 widget，无需特殊处理
        // 组件内部回调由组件自己管理
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

    /// 获取指定类型的 Widget 可变引用
    pub fn get<W: Any>(&self, id: WidgetId) -> Option<std::cell::RefMut<'_, W>> {
        self.widget_tree.get(id)
    }

    /// 获取 widget（用于布局）
    pub fn get_widget(&self, id: WidgetId) -> Option<std::cell::RefMut<'_, Box<dyn Widget>>> {
        self.widget_tree.get_widget(id)
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
        // 扫描 Widget 脏标记，自动触发布局/渲染
        self.scan_dirty_flags();

        if self.needs_layout {
            self.perform_layout();
        }

        if self.needs_render {
            self.needs_render = false;
            let tree = self.build_render_tree();
            self.clear_dirty_flags();
            tree
        } else {
            None
        }
    }

    /// 扫描所有 Widget 的脏标记，自动设置布局/渲染需求
    fn scan_dirty_flags(&mut self) {
        let has_dirty = self
            .widget_tree
            .widgets
            .values()
            .any(|widget_ref| widget_ref.try_borrow().map_or(false, |w| w.is_dirty()));
        if has_dirty {
            self.invalidate_layout(); // dirty 总是触发重新布局（也隐含重渲染）
        }
    }

    /// 清除所有 Widget 的脏标记
    fn clear_dirty_flags(&self) {
        for (_, widget_ref) in &self.widget_tree.widgets {
            if let Ok(mut widget) = widget_ref.try_borrow_mut() {
                widget.clear_dirty();
            }
        }
    }

    pub fn invalidate_layout(&mut self) {
        self.needs_layout = true;
    }

    pub fn invalidate_render(&mut self) {
        self.needs_render = true;
    }

    /// 确保布局已计算（如果 needed）
    fn ensure_layout(&mut self) {
        if self.needs_layout {
            self.perform_layout();
        }
    }
}

impl ViewContext {
    /// 获取布局树根节点
    pub fn layout_root(&self) -> Option<&LayoutNode> {
        self.layout_ctx.root.as_ref()
    }

    /// 处理鼠标移动事件
    pub fn handle_mouse_move(&mut self, point: Point) {
        self.ensure_layout();
        if let Some(layout_root) = self.layout_ctx.root.clone() {
            self.event_manager
                .handle_mouse_move(point, &layout_root, &self.widget_tree);
        }
        self.invalidate_render();
    }

    /// 处理鼠标按下事件
    pub fn handle_mouse_down(&mut self, point: Point, button: MouseButton) {
        self.ensure_layout();
        if let Some(layout_root) = self.layout_ctx.root.clone() {
            self.event_manager
                .handle_mouse_down(button, point, &layout_root, &self.widget_tree);
        }
        self.invalidate_render();
    }

    /// 处理鼠标释放事件
    pub fn handle_mouse_up(&mut self, point: Point, button: MouseButton) {
        self.ensure_layout();
        if let Some(layout_root) = self.layout_ctx.root.clone() {
            self.event_manager
                .handle_mouse_up(button, point, &layout_root, &self.widget_tree);
        }
        self.invalidate_render();
    }

    /// 处理鼠标滚轮事件
    pub fn handle_mouse_wheel(&mut self, delta_x: f32, delta_y: f32, point: Point) {
        self.ensure_layout();
        if let Some(layout_root) = self.layout_ctx.root.clone() {
            self.event_manager.handle_mouse_wheel(
                delta_x,
                delta_y,
                point,
                &layout_root,
                &self.widget_tree,
            );
        }
        self.invalidate_render();
    }

    /// 处理键盘按下事件
    pub fn handle_key_down(&mut self, key: Key, modifiers: Modifiers) {
        self.event_manager
            .handle_key_down(key, modifiers, &self.widget_tree);
        self.invalidate_render();
    }

    /// 处理键盘释放事件
    pub fn handle_key_up(&mut self, key: Key, modifiers: Modifiers) {
        self.event_manager
            .handle_key_up(key, modifiers, &self.widget_tree);
        self.invalidate_render();
    }

    /// 处理窗口失焦（清除焦点状态）
    pub fn handle_window_unfocus(&mut self) {
        self.event_manager.handle_window_unfocus(&self.widget_tree);
        self.invalidate_render();
    }

    /// 设置焦点到指定 widget
    pub fn set_focus(&mut self, widget_id: Option<WidgetId>) {
        self.event_manager
            .handle_focus_change(widget_id, &self.widget_tree);
        self.invalidate_render();
    }

    /// 获取当前悬停的 widget
    pub fn hovered_widget(&self) -> Option<WidgetId> {
        self.event_manager.hovered()
    }

    /// 获取当前焦点的 widget
    pub fn focused_widget(&self) -> Option<WidgetId> {
        self.event_manager.focused()
    }

    /// 获取 widget 的布局边界
    pub fn widget_bounds(&self, widget_id: WidgetId) -> Option<crate::geometry::Rect> {
        self.layout_ctx.root.as_ref()?.find(widget_id)?.bounds()
    }

    /// 处理 IME 预编辑事件
    pub fn handle_ime_preedit(
        &mut self,
        text: String,
        cursor_start: Option<usize>,
        cursor_end: Option<usize>,
    ) {
        self.event_manager
            .handle_ime_preedit(text, cursor_start, cursor_end, &self.widget_tree);
        self.invalidate_render();
    }

    /// 处理 IME 提交事件
    pub fn handle_ime_commit(&mut self, text: String) {
        self.event_manager
            .handle_ime_commit(text, &self.widget_tree);
        self.invalidate_render();
    }

    /// 处理 IME 禁用事件
    pub fn handle_ime_disabled(&mut self) {
        self.event_manager.handle_ime_disabled(&self.widget_tree);
        self.invalidate_render();
    }
}
