// src/core/view_context.rs

use std::cell::{Ref, RefMut};

use winit::event_loop::EventLoop;

use crate::builder::{BuildContext, BuildSnapshot};
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

    // ========== Builder 模式相关 ==========

    /// 上一帧的构建快照（用于 slot 匹配）
    build_snapshot: Option<BuildSnapshot>,

    /// 用户注册的 builder 函数（每次 rebuild 时调用，接收 BuildContext）
    build_fn: Option<Box<dyn FnMut(&mut BuildContext)>>,

    /// 标记是否需要 rebuild（由事件回调通过 EventContext 设置）
    rebuild_requested: bool,
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
            build_snapshot: None,
            build_fn: None,
            rebuild_requested: false,
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

    /// 创建事件循环并直接运行（无需调用方传入 `EventLoop`）。
    pub fn run_blocking(self) {
        let event_loop = EventLoop::new().unwrap();
        self.run(event_loop);
    }

    // ========== Builder 便捷 API ==========

    /// 创建 Widget 并添加到指定父节点（一步完成）
    ///
    /// 等价于 `ctx.create(widget)` + `ctx.add_child(parent, child_id)`。
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
    pub fn state<T>(&self, value: T) -> State<T> {
        State::new(value)
    }

    /// 将共享状态绑定到 Text widget
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
    pub fn run(self, event_loop: winit::event_loop::EventLoop<()>) {
        let app = crate::app::App::new(self);
        app.run(event_loop);
    }

    // ========================================================================
    // Builder 模式 API
    // ========================================================================

    /// 注册构建函数，该函数会在 `render()` 开始时自动调用
    ///
    /// 构建函数使用 `BuildContext` 声明式地描述 UI 结构。
    /// 每次 rebuild 时，Builder 框架会自动处理 widget 的增删复用。
    /// 注册构建函数
    ///
    /// 该函数在每次 rebuild 时被调用。使用 `BuildContext` 声明式描述 UI 结构。
    ///
    /// # 示例
    /// ```ignore
    /// vc.set_build_fn(move |bctx| {
    ///     bctx.column(|bctx| {
    ///         bctx.text("Hello");
    ///         bctx.button("Click", |_| {});
    ///     });
    /// });
    /// ```
    pub fn set_build_fn<F>(&mut self, f: F)
    where
        F: FnMut(&mut BuildContext) + 'static,
    {
        self.build_fn = Some(Box::new(f));
    }

    /// 手动触发重建（内部调用存储的 build_fn）
    ///
    /// 通常在注册 build_fn 后第一次手动调用；后续由 `render()` 自动管理。
    pub fn build(&mut self) {
        self.rebuild_requested = true;
    }

    /// 执行重建（调用 build_fn + 执行 slot reconciliation）
    ///
    /// 注意：此方法不会调用 `apply_pending_ops()`，调用者需确保已在之前执行过。
    pub fn execute_build(&mut self) {
        // 用 take 避免借用冲突（build_fn 和 bctx 都借 self）
        if let Some(mut build_fn) = self.build_fn.take() {
            let snapshot = self.build_snapshot.take().unwrap_or_default();
            let mut bctx = BuildContext::new(self, snapshot);
            build_fn(&mut bctx);
            let snapshot = bctx.finalize();

            // 重要：将 builder 创建的第一个 widget 设为 Base 层 root
            // builder 只负责创建 widget 树结构（通过 add_child），
            // 不设 root 则 perform_layout() 会跳过该层。
            if let Some(first_id) = snapshot.first_widget_id() {
                self.layers.set_base_root(first_id);
            }

            self.build_snapshot = Some(snapshot);
            self.build_fn = Some(build_fn);
        }

        self.rebuild_requested = false;
        self.invalidate_layout();
    }

    /// 请求下一次 render() 时执行重建
    pub fn request_rebuild(&mut self) {
        self.rebuild_requested = true;
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

    // ========================================================================
    // 动态 Widget 树操作 API（直接模式，需要 &mut self）
    // ========================================================================

    /// 从树中删除指定 widget 及其所有子节点
    ///
    /// 会自动清理事件管理器中可能持有的对该 widget 的引用（焦点、悬停、鼠标捕获）。
    pub fn remove(&mut self, id: WidgetId) -> Option<Box<dyn Widget>> {
        let result = self.layers.tree.remove(id);
        if result.is_some() {
            self.cleanup_event_state(id);
            self.invalidate_layout();
        }
        result
    }

    /// 替换指定位置的 widget（保留 id 和父子关系不变）
    pub fn replace<W: Widget>(&mut self, id: WidgetId, widget: W) -> Option<Box<dyn Widget>> {
        let result = self.layers.tree.replace(id, widget);
        if result.is_some() {
            self.invalidate_layout();
        }
        result
    }

    /// 将 widget 从父节点分离（保留在树中但成为孤立节点）
    pub fn detach(&mut self, child_id: WidgetId) {
        self.layers.tree.detach(child_id);
        self.invalidate_layout();
    }

    /// 将子节点移动到新父节点
    pub fn reparent(&mut self, child_id: WidgetId, new_parent: WidgetId) {
        self.layers.tree.reparent(child_id, new_parent);
        self.invalidate_layout();
    }

    /// 清理事件管理器中可能存在的过期引用
    fn cleanup_event_state(&mut self, id: WidgetId) {
        if self.event_manager.focused() == Some(id) {
            self.event_manager.clear_focused();
        }
        if self.event_manager.hovered() == Some(id) {
            self.event_manager.clear_hovered();
        }
        if self.event_manager.mouse_capture() == Some(id) {
            self.event_manager.clear_mouse_capture();
        }
    }

    /// 获取 widget（用于布局/事件）
    pub fn get_widget(&self, id: WidgetId) -> Option<WidgetRefMut<'_>> {
        self.layers.get_widget_mut(id)
    }

    // ========== 布局与渲染 ==========

    /// 执行所有排队的 Widget 树操作，然后执行布局
    pub fn rebuild(&mut self) {
        if self.layers.apply_pending_ops() {
            self.invalidate_layout();
        }
    }

    /// 执行所有层的布局
    pub fn perform_layout(&mut self) {
        if !self.needs_layout {
            return;
        }

        // 先应用排队的操作
        self.rebuild();

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
        // 1. 应用排队的 Widget 树操作（来自 EventContext 的 add_child/remove 等）
        self.rebuild();

        // 2. 如果请求了 rebuild，执行 Builder 重建
        if self.rebuild_requested {
            self.execute_build();
        }

        // 3. 应用 State -> Widget 绑定，让 Text 等 widget 有机会变脏
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
        if effects.needs_rebuild() {
            self.rebuild_requested = true;
        }
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
