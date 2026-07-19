// src/core/layers.rs
//! 三层架构：Base → Overlay → Modal
//!
//! 所有 Widget 存储在一个共享的 WidgetTree 中，
//! 每层只维护自己的 root（入口）和 LayoutContext（布局隔离）。
//! 所有层均用 RefCell 包裹，使 EventContext（持 &Layers）可直接修改各层。

use crate::core::WidgetId;
use crate::event::HitTestResult;
use crate::geometry::Point;
use crate::layout::{LayoutContext, LayoutNode};
use crate::widget::WidgetTree;
use std::cell::RefCell;

/// 类型别名：简化 Widget 跨层查找的返回类型写法
/// 这些类型来自 WidgetTree（SlotMap + RefCell）
pub type WidgetRef<'a> = std::cell::Ref<'a, Box<dyn crate::widget::Widget>>;
pub type WidgetRefMut<'a> = std::cell::RefMut<'a, Box<dyn crate::widget::Widget>>;

/// 层类型（固定 3 层，z-index 递增）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerType {
    /// z = 0，主内容层
    Base,
    /// z = 1，浮动层（float 按钮、toast 等）
    Overlay,
    /// z = 2，模态层（modal dialog）
    Modal,
}

impl LayerType {
    /// 对应的 z-index 基准值（渲染排序用）
    pub fn z_index(&self) -> i32 {
        match self {
            LayerType::Base => 0,
            LayerType::Overlay => 1000,
            LayerType::Modal => 2000,
        }
    }

    /// 事件派发顺序：从高 z 到低 z
    pub fn dispatch_order() -> [LayerType; 3] {
        [LayerType::Modal, LayerType::Overlay, LayerType::Base]
    }
}

/// 单层状态
pub struct LayerInfo {
    root: Option<WidgetId>,
    layout: LayoutContext,
}

impl LayerInfo {
    fn new() -> Self {
        Self {
            root: None,
            layout: LayoutContext::new(),
        }
    }
}

/// 三层架构容器
///
/// - 所有 Widget 存在共享的 `tree` 中（WidgetId 全局唯一）
/// - 每层有自己的 root（入口）和 LayoutContext（布局隔离）
/// - 所有层用 RefCell 包裹，允许 EventContext 通过 &Layers 修改任意层
pub struct Layers {
    /// 所有 Widget 的统一存储
    pub tree: WidgetTree,

    /// 主内容层
    pub base: RefCell<LayerInfo>,

    /// 浮动层
    pub overlay: RefCell<LayerInfo>,

    /// 模态层
    pub modal: RefCell<LayerInfo>,
}

impl Default for Layers {
    fn default() -> Self {
        Self::new()
    }
}

impl Layers {
    pub fn new() -> Self {
        Self {
            tree: WidgetTree::new(),
            base: RefCell::new(LayerInfo::new()),
            overlay: RefCell::new(LayerInfo::new()),
            modal: RefCell::new(LayerInfo::new()),
        }
    }

    /// 创建 Widget（需要 &mut self，由 ViewContext 调用）
    pub fn create<W: crate::widget::Widget>(&mut self, widget: W) -> WidgetId {
        self.tree.create(widget)
    }

    // ========== 用户 API（操作各层）==========

    /// 在 base 层创建 Widget（需要 &mut self）
    pub fn create_in_base<W: crate::widget::Widget>(&mut self, widget: W) -> WidgetId {
        self.tree.create(widget)
    }

    /// 设置 base 层根节点
    pub fn set_base_root(&mut self, root_id: WidgetId) {
        self.base.borrow_mut().root = Some(root_id);
    }

    /// 显示 Modal 层（设置 modal 层根节点）
    pub fn show_modal(&self, root_id: WidgetId) {
        self.modal.borrow_mut().root = Some(root_id);
    }

    /// 隐藏 Modal 层（清除 modal 层根节点和布局）
    pub fn hide_modal(&self) {
        let mut modal = self.modal.borrow_mut();
        modal.root = None;
        modal.layout = LayoutContext::new(); // ← 清除布局，防止 build_render_tree 使用旧布局
    }

    /// 显示 Overlay 层
    pub fn show_overlay(&self, root_id: WidgetId) {
        self.overlay.borrow_mut().root = Some(root_id);
    }

    /// 隐藏 Overlay 层（清除 overlay 层根节点和布局）
    pub fn hide_overlay(&self) {
        let mut overlay = self.overlay.borrow_mut();
        overlay.root = None;
        overlay.layout = LayoutContext::new(); // ← 同步清除布局
    }

    // ========== 布局相关 ==========

    /// 获取指定层根节点 ID
    pub fn layer_root(&self, lt: LayerType) -> Option<WidgetId> {
        match lt {
            LayerType::Base => self.base.borrow().root,
            LayerType::Overlay => self.overlay.borrow().root,
            LayerType::Modal => self.modal.borrow().root,
        }
    }

    /// 克隆指定层的布局根节点（用于事件派发，不持有 borrow）
    pub fn layer_layout_root(&self, lt: LayerType) -> Option<crate::layout::LayoutNode> {
        match lt {
            LayerType::Base => self.base.borrow().layout.root.clone(),
            LayerType::Overlay => self.overlay.borrow().layout.root.clone(),
            LayerType::Modal => self.modal.borrow().layout.root.clone(),
        }
    }

    /// 设置指定层的布局上下文（统一用 RefCell）
    pub fn set_layer_layout(&self, lt: LayerType, ctx: LayoutContext) {
        match lt {
            LayerType::Base => self.base.borrow_mut().layout = ctx,
            LayerType::Overlay => self.overlay.borrow_mut().layout = ctx,
            LayerType::Modal => self.modal.borrow_mut().layout = ctx,
        }
    }

    /// 检查某层是否有内容
    pub fn layer_has_content(&self, lt: LayerType) -> bool {
        self.layer_root(lt).is_some()
    }

    /// 在某层做命中测试
    pub fn layer_hit_test(&self, lt: LayerType, point: Point) -> Option<WidgetId> {
        self.with_layer(lt, |layer| layer.layout.root.as_ref()?.hit_test(point))
    }

    /// 在某层做命中测试，并返回目标与其从根到目标的路径
    ///
    /// 只借用布局根节点做命中测试，不会克隆整棵布局树
    pub fn layer_hit_test_with_path(&self, lt: LayerType, point: Point) -> Option<HitTestResult> {
        let target = self.with_layer(lt, |layer| layer.layout.root.as_ref()?.hit_test(point))?;
        let path = self.path_to(target);
        Some(HitTestResult { target, path })
    }

    /// 在持有布局根节点借用的前提下执行操作，避免 `LayoutNode` 整棵克隆
    pub fn with_layer_layout_root<R>(
        &self,
        lt: LayerType,
        f: impl FnOnce(&LayoutNode) -> R,
    ) -> Option<R> {
        self.with_layer(lt, |layer| layer.layout.root.as_ref().map(f))
    }

    // ========== Widget 跨层查找 ==========

    /// 获取 Widget 的不可变引用（跨层查找）
    pub fn get_widget(&self, id: WidgetId) -> Option<WidgetRef<'_>> {
        self.tree.get_widget_immut(id)
    }

    /// 获取 Widget 的可变引用（跨层查找）
    pub fn get_widget_mut(&self, id: WidgetId) -> Option<WidgetRefMut<'_>> {
        self.tree.get_widget(id)
    }

    /// 查找 Widget 到根的路径（跨层，因为所有 widget 在同一 tree 中）
    pub fn path_to(&self, target: WidgetId) -> Vec<WidgetId> {
        self.tree.path_to(target)
    }

    // ========== 辅助闭包方法（供 EventContext 使用）==========

    /// 在指定层上执行只读操作（不持有 borrow 超过闭包）
    pub fn with_layer<T>(&self, lt: LayerType, f: impl FnOnce(&LayerInfo) -> T) -> T {
        match lt {
            LayerType::Base => f(&self.base.borrow()),
            LayerType::Overlay => f(&self.overlay.borrow()),
            LayerType::Modal => f(&self.modal.borrow()),
        }
    }

    /// 在指定层上执行可变操作
    pub fn with_layer_mut<T>(&self, lt: LayerType, f: impl FnOnce(&mut LayerInfo) -> T) -> T {
        match lt {
            LayerType::Base => f(&mut self.base.borrow_mut()),
            LayerType::Overlay => f(&mut self.overlay.borrow_mut()),
            LayerType::Modal => f(&mut self.modal.borrow_mut()),
        }
    }
}
