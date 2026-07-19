// src/core/view_context.rs

use std::cell::{Ref, RefMut};

use crate::core::WidgetId;
use crate::core::layers::{LayerType, Layers, WidgetRefMut};
use crate::event::{EventManager, Key, Modifiers, MouseButton};
use crate::geometry::{Point, Size};
use crate::layout::{LayoutContext, LayoutNode};
use crate::render::visual::LayeredElement;
use crate::state::State;
use crate::widget::Widget;
use crate::widgets::{Button, Column, Container, ProgressBar, Row, Text};

/// State -> Widget 绑定回调的类型别名
type BindingFn = Box<dyn FnMut(&Layers)>;

pub struct ViewContext {
    /// 三层架构
    layers: Layers,
    viewport: Size,
    needs_layout: bool,
    needs_render: bool,
    pub debug_render_tree: bool,
    /// 事件管理器
    event_manager: EventManager,
    /// State -> Widget 绑定回调，每帧渲染前应用
    bindings: Vec<BindingFn>,
}

impl ViewContext {
    pub fn new(viewport: Size) -> Self {
        Self {
            layers: Layers::new(),
            viewport,
            needs_layout: true,
            needs_render: true,
            debug_render_tree: false,
            event_manager: EventManager::new(),
            bindings: Vec::new(),
        }
    }

    /// 获取 Layers 引用（高级用法）
    pub fn layers(&self) -> &Layers {
        &self.layers
    }

    /// 获取 Layers 可变引用（高级用法）
    pub fn layers_mut(&mut self) -> &mut Layers {
        &mut self.layers
    }

    pub fn set_viewport(&mut self, viewport: Size) {
        self.viewport = viewport;
        self.invalidate_layout();
    }

    // ========== Builder 便捷 API ==========

    /// 创建 Widget 并添加到指定父节点（一步完成）
    ///
    /// 等价于 `ctx.create(widget)` + `ctx.add_child(parent, child_id)`。
    ///
    /// # 示例
    /// ```ignore
    /// let root = ctx.create(Container::new());
    /// let title = ctx.attach(root, Text::new("Hello").font_size(32.0));
    /// ```
    pub fn attach<W: Widget>(&mut self, parent: WidgetId, widget: W) -> WidgetId {
        let id = self.layers.create_in_base(widget);
        self.layers.tree.add_child(parent, id);
        self.invalidate_layout();
        id
    }

    /// 快捷创建 Text Widget（省去 import，不借用 self）
    pub fn text(&self, content: impl Into<String>) -> Text {
        Text::new(content)
    }

    /// 快捷创建 Button Widget（省去 import，不借用 self）
    pub fn button(&self, label: impl Into<String>) -> Button {
        Button::new(label)
    }

    /// 快捷创建 Container Widget（省去 import，不借用 self）
    pub fn container(&self) -> Container {
        Container::new()
    }

    /// 快捷创建 Column Widget（省去 import，不借用 self）
    pub fn column(&self) -> Column {
        Column::new()
    }

    /// 快捷创建 Row Widget（省去 import，不借用 self）
    pub fn row(&self) -> Row {
        Row::new()
    }

    /// 创建共享状态
    ///
    /// 可在多个闭包间克隆使用，配合 `bind_text` 自动同步到 Text widget。
    pub fn state<T>(&self, value: T) -> State<T> {
        State::new(value)
    }

    /// 将共享状态绑定到 Text widget
    ///
    /// 每帧渲染前会自动用 `f(&state.get())` 更新 Text 内容，无需手动维护 WidgetId。
    pub fn bind_text<T, F>(&mut self, state: &State<T>, text_id: WidgetId, f: F)
    where
        F: Fn(&T) -> String + 'static,
        T: 'static,
    {
        let state = state.clone();
        self.bindings.push(Box::new(move |layers| {
            if let Some(mut widget) = layers.get_widget_mut(text_id)
                && let Some(text) = widget.as_any_mut().downcast_mut::<Text>()
            {
                let value = state.get();
                text.set_content(f(&*value));
            }
        }));
    }

    /// 将共享状态绑定到 ProgressBar widget
    ///
    /// 每帧渲染前会自动用 `f(&state.get())` 更新进度值（范围 [0.0, 1.0]）。
    pub fn bind_progress<T, F>(&mut self, state: &State<T>, progress_id: WidgetId, f: F)
    where
        F: Fn(&T) -> f32 + 'static,
        T: 'static,
    {
        let state = state.clone();
        self.bindings.push(Box::new(move |layers| {
            if let Some(mut widget) = layers.get_widget_mut(progress_id)
                && let Some(pb) = widget.as_any_mut().downcast_mut::<ProgressBar>()
            {
                let value = state.get();
                pb.set_progress(f(&*value));
            }
        }));
    }

    /// 便捷运行入口
    ///
    /// 等价于：
    /// ```ignore
    /// let app = App::new(ctx);
    /// app.run(event_loop);
    /// ```
    pub fn run(self, event_loop: winit::event_loop::EventLoop<()>) {
        let app = crate::app::App::new(self);
        app.run(event_loop);
    }

    // ========== Widget 创建与树操作 ==========

    pub fn create<W: Widget>(&mut self, widget: W) -> WidgetId {
        self.layers.create_in_base(widget)
    }

    pub fn set_root(&mut self, root_id: WidgetId) {
        self.layers.set_base_root(root_id);
        self.invalidate_layout();
    }

    /// 创建 root widget 并自动设置（`create` + `set_root` 一步完成）
    ///
    /// 等价于：
    /// ```ignore
    /// let id = ctx.create(widget);
    /// ctx.set_root(id);
    /// ```
    pub fn root(&mut self, widget: impl Widget) -> WidgetId {
        let id = self.create(widget);
        self.set_root(id);
        id
    }

    pub fn add_child(&mut self, parent_id: WidgetId, child_id: WidgetId) {
        self.layers.tree.add_child(parent_id, child_id);
        self.invalidate_layout();
    }

    pub fn get<W: Widget>(&self, id: WidgetId) -> Option<Ref<'_, W>> {
        self.layers.tree.get(id)
    }

    /// 获取指定类型的 Widget 可变引用
    pub fn get_mut<W: Widget>(&self, id: WidgetId) -> Option<RefMut<'_, W>> {
        self.layers.tree.get_mut(id)
    }

    /// 获取 widget（用于布局/事件）
    pub fn get_widget(&self, id: WidgetId) -> Option<WidgetRefMut<'_>> {
        self.layers.get_widget_mut(id)
    }

    // ========== 布局与渲染 ==========

    /// 执行所有层的布局
    pub fn perform_layout(&mut self) {
        if !self.needs_layout {
            return;
        }

        // Base 层
        self.perform_layer_layout(LayerType::Base);

        // Overlay 层
        if self.layers.layer_has_content(LayerType::Overlay) {
            self.perform_layer_layout(LayerType::Overlay);
        }

        // Modal 层
        if self.layers.layer_has_content(LayerType::Modal) {
            self.perform_layer_layout(LayerType::Modal);
        }

        self.needs_layout = false;
        self.needs_render = true;
    }

    /// 执行指定层的布局
    fn perform_layer_layout(&mut self, lt: LayerType) {
        let Some(root_id) = self.layers.layer_root(lt) else {
            return;
        };

        let mut layout_ctx = LayoutContext::new();
        layout_ctx.collect(root_id, &self.layers);
        layout_ctx.compute(self.viewport);

        // 保存布局结果到对应层
        self.layers.set_layer_layout(lt, layout_ctx);
    }

    /// 从布局树构建视觉元素列表
    pub fn build_render_tree(&mut self) -> Vec<LayeredElement> {
        let mut elements = Vec::new();

        // 依次收集各层，借用布局根节点而非克隆整棵树
        for lt in [LayerType::Base, LayerType::Overlay, LayerType::Modal] {
            self.layers.with_layer_layout_root(lt, |root| {
                Self::collect_visual_elements(self, root, &mut elements);
            });
        }

        // 按 z_index 排序（稳定排序，同 z 时保持层顺序）
        elements.sort_by_key(|e| e.z_index);
        elements
    }

    fn collect_visual_elements(
        ctx: &ViewContext,
        layout_node: &LayoutNode,
        elements: &mut Vec<LayeredElement>,
    ) {
        let id = layout_node.id;

        // 获取 widget 并渲染
        if let Some(mut widget) = ctx.layers.get_widget_mut(id) {
            let mut widget_elements = widget.render(layout_node, ctx);
            elements.append(&mut widget_elements);
        }

        // 递归收集子节点
        for child_layout in &layout_node.children {
            Self::collect_visual_elements(ctx, child_layout, elements);
        }
    }

    pub fn render(&mut self) -> Vec<LayeredElement> {
        // 先应用 State -> Widget 绑定，让 Text 等 widget 有机会变脏
        self.apply_bindings();

        // 扫描 Widget 脏标记
        self.scan_dirty_flags();

        if self.needs_layout {
            self.perform_layout();
        }

        if self.needs_render {
            self.needs_render = false;
            let elements = self.build_render_tree();
            self.clear_dirty_flags();
            elements
        } else {
            Vec::new()
        }
    }

    /// 应用所有 State -> Widget 绑定
    fn apply_bindings(&mut self) {
        let layers = &self.layers;
        for binding in &mut self.bindings {
            binding(layers);
        }
    }

    /// 扫描所有 Widget 的脏标记
    fn scan_dirty_flags(&mut self) {
        // 使用 Cell 来在 traverse 闭包中收集结果
        use std::cell::Cell;
        let has_dirty = Cell::new(false);
        self.layers.tree.traverse(|_, widget| {
            if widget.is_dirty() {
                has_dirty.set(true);
            }
        });
        if has_dirty.get() {
            self.invalidate_layout();
        }
    }

    /// 清除所有 Widget 的脏标记
    fn clear_dirty_flags(&self) {
        // 使用 get_widget 遍历清除
        if let Some(root) = self.layers.tree.root() {
            self.clear_dirty_recursive(root);
        }
    }

    fn clear_dirty_recursive(&self, id: WidgetId) {
        if let Some(mut widget) = self.layers.tree.get_widget(id) {
            widget.clear_dirty();
        }
        let children = self.layers.tree.children_of(id);
        for child_id in children {
            self.clear_dirty_recursive(child_id);
        }
    }

    pub fn invalidate_layout(&mut self) {
        self.needs_layout = true;
    }

    pub fn invalidate_render(&mut self) {
        self.needs_render = true;
    }

    fn ensure_layout(&mut self) {
        if self.needs_layout {
            self.perform_layout();
        }
    }
}

// ========== 事件处理 ==========

impl ViewContext {
    /// 处理鼠标移动事件
    pub fn handle_mouse_move(&mut self, point: Point) {
        self.ensure_layout();

        // 按 z-index 从高到低遍历各层
        for lt in LayerType::dispatch_order() {
            if !self.layers.layer_has_content(lt) {
                continue;
            }
            if let Some(hit) = self.layers.layer_hit_test_with_path(lt, point) {
                let effects =
                    self.event_manager
                        .handle_mouse_move(point, Some(&hit), &self.layers, lt);
                self.apply_effects(effects);
                return; // 该层消费了事件
            }
        }
    }

    /// 处理鼠标按下事件
    pub fn handle_mouse_down(&mut self, point: Point, button: MouseButton) {
        self.ensure_layout();

        for lt in LayerType::dispatch_order() {
            if !self.layers.layer_has_content(lt) {
                continue;
            }
            if let Some(hit) = self.layers.layer_hit_test_with_path(lt, point) {
                let effects =
                    self.event_manager
                        .handle_mouse_down(point, button, &hit, &self.layers, lt);
                self.apply_effects(effects);
                return;
            }
        }

        // 没有层消费事件，清除焦点
        let effects = self.event_manager.handle_focus_change(None, &self.layers);
        self.apply_effects(effects);
    }

    /// 处理鼠标释放事件
    pub fn handle_mouse_up(&mut self, point: Point, button: MouseButton) {
        self.ensure_layout();

        for lt in LayerType::dispatch_order() {
            if !self.layers.layer_has_content(lt) {
                continue;
            }
            if let Some(hit) = self.layers.layer_hit_test_with_path(lt, point) {
                let effects =
                    self.event_manager
                        .handle_mouse_up(point, button, &hit, &self.layers, lt);
                self.apply_effects(effects);
                return;
            }
        }
    }

    /// 处理鼠标滚轮事件
    pub fn handle_mouse_wheel(&mut self, delta_x: f32, delta_y: f32, point: Point) {
        self.ensure_layout();

        for lt in LayerType::dispatch_order() {
            if !self.layers.layer_has_content(lt) {
                continue;
            }
            if let Some(hit) = self.layers.layer_hit_test_with_path(lt, point) {
                let effects = self.event_manager.handle_wheel(
                    point,
                    delta_x,
                    delta_y,
                    &hit,
                    &self.layers,
                    lt,
                );
                self.apply_effects(effects);
                return;
            }
        }
    }

    /// 处理键盘按下事件
    pub fn handle_key_down(&mut self, key: Key, modifiers: Modifiers) {
        let effects = self
            .event_manager
            .handle_key_down(key, modifiers, &self.layers);
        self.apply_effects(effects);
    }

    /// 处理键盘释放事件
    pub fn handle_key_up(&mut self, key: Key, modifiers: Modifiers) {
        let effects = self
            .event_manager
            .handle_key_up(key, modifiers, &self.layers);
        self.apply_effects(effects);
    }

    /// 处理窗口失焦
    pub fn handle_window_unfocus(&mut self) {
        let effects = self.event_manager.handle_window_unfocus(&self.layers);
        self.apply_effects(effects);
    }

    /// 应用事件副作用
    fn apply_effects(&mut self, effects: crate::event::EventEffects) {
        if effects.needs_render() {
            self.invalidate_render();
        }
        if effects.needs_layout() {
            self.invalidate_layout();
        }
    }

    // ========== 焦点与查询 ==========

    /// 设置焦点
    pub fn set_focus(&mut self, widget_id: Option<WidgetId>) {
        self.ensure_layout();
        let effects = self
            .event_manager
            .handle_focus_change(widget_id, &self.layers);
        self.apply_effects(effects);
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
        // 在各层的 layout 中查找（借用根节点，不克隆整棵树）
        for lt in LayerType::dispatch_order() {
            if let Some(bounds) = self
                .layers
                .with_layer_layout_root(lt, |root| root.find(widget_id).map(|node| node.bounds()))
            {
                return bounds;
            }
        }
        None
    }

    /// 处理 IME 预编辑事件
    pub fn handle_ime_preedit(
        &mut self,
        text: String,
        cursor_start: Option<usize>,
        cursor_end: Option<usize>,
    ) {
        let effects =
            self.event_manager
                .handle_ime_preedit(text, cursor_start, cursor_end, &self.layers);
        self.apply_effects(effects);
    }

    /// 处理 IME 提交事件
    pub fn handle_ime_commit(&mut self, text: String) {
        let effects = self.event_manager.handle_ime_commit(text, &self.layers);
        self.apply_effects(effects);
    }

    /// 处理 IME 禁用事件
    pub fn handle_ime_disabled(&mut self) {
        let effects = self.event_manager.handle_ime_disabled(&self.layers);
        self.apply_effects(effects);
    }
}
