//! LayerStack — 可扩展的多层 z-index 栈
//!
//! 替换原有的三层硬编码（Base / Overlay / Modal），支持：
//! - 6 种 LayerKind：Content / Popup / Overlay / Tooltip / Modal / System
//! - 每种 Kind 内多实例并存，自动分配 z 值
//! - Anchor 定位（Above / Below / LeftOf / RightOf / Fixed / ScreenCenter）
//! - FocusPolicy（Transparent / Dismissable / BlockBelow）
//! - Modal backdrop 渲染管线级合成
//!
//! 保持向后兼容：`content_handle` / `default_overlay_handle` / `default_modal_handle`
//! 使现有 Runtime / state.rs 的旧 API 调用点零改动。

use crate::core::ElementId;
use crate::event::EventManager;
use crate::geometry::{Point, Rect};
use crate::runtime::element::ElementTree;
use crate::view::node::ViewNode;
use std::cell::{Cell, RefCell};

// ============================================================================
// 层语义分类
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LayerKind {
    /// z-range: 0..=999  主内容（原 Base）
    Content,
    /// z-range: 1000..=1999  弹出菜单 / ComboBox 下拉列表
    Popup,
    /// z-range: 2000..=2999  Toast / 通知（原 Overlay 语义）
    Overlay,
    /// z-range: 3000..=3999  模态弹框（原 Modal，支持叠加 + 阻塞下层）
    Modal,
    /// z-range: 4000..=4999  Tooltip（必须在 Modal 之上）
    Tooltip,
    /// z-range: 5000..=5999  调试面板 / 拖拽中的幽灵节点
    System,
}

impl LayerKind {
    /// 每种 Kind 保留 1000 个 z 档位（同一 Kind 内按插入顺序 ++）
    pub fn z_base(&self) -> i32 {
        match self {
            LayerKind::Content => 0,
            LayerKind::Popup => 1000,
            LayerKind::Overlay => 2000,
            LayerKind::Modal => 3000,
            LayerKind::Tooltip => 4000,
            LayerKind::System => 5000,
        }
    }

    pub fn z_range(&self) -> std::ops::RangeInclusive<i32> {
        self.z_base()..=(self.z_base() + 999)
    }

    /// 事件派发 / 命中测试：高 z 优先
    pub fn dispatch_order() -> [LayerKind; 6] {
        [
            LayerKind::System,
            LayerKind::Tooltip,
            LayerKind::Modal,
            LayerKind::Overlay,
            LayerKind::Popup,
            LayerKind::Content,
        ]
    }
}

// ============================================================================
// LayerHandle
// ============================================================================

/// 条目 ID = u64 generational，用户拿到后可 hide 指定条目
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayerHandle(pub u64);

// ============================================================================
// Anchor
// ============================================================================

/// 锚点信息：Tooltip/Popup 需要相对屏幕某个矩形出现在合理位置（上方 / 下方 / 居中对齐）
#[derive(Debug, Clone, Copy)]
pub enum Anchor {
    /// 不锚定，按 FlexStyle 正常布局（Modal / Content）
    None,
    /// 以锚矩形为基准在上方出现，水平居中对齐
    Above { anchor: Rect, gap: f32 },
    /// 以锚矩形为基准在下方出现
    Below { anchor: Rect, gap: f32 },
    /// 以锚矩形为基准在左侧出现
    LeftOf { anchor: Rect, gap: f32 },
    /// 以锚矩形为基准在右侧出现
    RightOf { anchor: Rect, gap: f32 },
    /// 固定屏幕坐标 (x, y) 左上角
    Fixed { x: f32, y: f32 },
    /// 屏幕正中央（Modal 常用）
    ScreenCenter,
}

// ============================================================================
// FocusPolicy
// ============================================================================

/// 焦点/阻塞策略：决定本条目是否吞掉落在其矩形外的事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPolicy {
    /// 不阻塞下层：落在本条目外的事件派发给下层条目（Tooltip/Popup/Overlay/Content 适用）
    Transparent,
    /// 本条目外的事件 = 先触发 "auto dismiss"（如果配置了），然后继续派发给下层
    Dismissable,
    /// 阻塞下层：所有下层事件派发跳过（顶层 Modal 适用）
    BlockBelow,
}

// ============================================================================
// LayerOptions
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct LayerOptions {
    pub visible: bool,
    pub dismiss_on_outside: bool,
    pub backdrop: Option<crate::geometry::Color>,
    /// 锚定防溢出：超出视口时自动翻转到对侧（弹出菜单等浮层用）
    pub flip: bool,
}

impl Default for LayerOptions {
    fn default() -> Self {
        Self {
            visible: true,
            dismiss_on_outside: false,
            backdrop: None,
            flip: false,
        }
    }
}

impl LayerOptions {
    /// 弹出浮层常用预设：可见、点击外部自动消失、启用防溢出翻转。
    pub fn popup() -> Self {
        Self {
            visible: true,
            dismiss_on_outside: true,
            backdrop: None,
            flip: true,
        }
    }
}

// ============================================================================
// LayerEntry
// ============================================================================

/// 栈内单个条目 = 一层视觉 + 交互单元
#[derive(Clone)]
pub struct LayerEntry {
    pub handle: LayerHandle,
    pub kind: LayerKind,
    /// 根 ElementId（ElementTree 中真实存在）
    pub root_id: ElementId,
    /// 锚定：布局阶段按 Anchor + 测量尺寸计算最终 top/left
    pub anchor: Anchor,
    /// 可见性（隐藏时：跳过命中测试 + 跳过渲染，但保留 ElementTree 节点）
    pub visible: Cell<bool>,
    /// 阻塞策略
    pub focus: FocusPolicy,
    /// 条目在其 Kind 内的插入顺序，用于同 Kind 内 z 排序（0 = 最早）
    pub seq: i32,
    /// 点击本条目外是否自动隐藏（Dismissable 专用）
    pub dismiss_on_outside_click: bool,
    /// Modal 专属：在本条目下方绘制半透明遮罩矩形，颜色可配置
    pub backdrop: Option<crate::geometry::Color>,
    /// 锚定防溢出：当按首选方向定位会超出视口时，自动翻转到对侧
    /// （如 `Below` 超出底部则翻转为 `Above`）。用于弹出菜单等浮层。
    pub flip: bool,
}

impl LayerEntry {
    /// 实际渲染 z = kind.z_base() + seq
    pub fn z(&self) -> i32 {
        self.kind.z_base() + self.seq
    }
}

impl std::fmt::Debug for LayerEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayerEntry")
            .field("handle", &self.handle)
            .field("kind", &self.kind)
            .field("root_id", &self.root_id)
            .field("anchor", &self.anchor)
            .field("visible", &self.visible)
            .field("focus", &self.focus)
            .field("seq", &self.seq)
            .field("dismiss_on_outside_click", &self.dismiss_on_outside_click)
            .field("backdrop", &self.backdrop)
            .finish()
    }
}

// ============================================================================
// LayerStack
// ============================================================================

pub struct LayerStack {
    pub tree: ElementTree,
    pub event_manager: RefCell<EventManager>,
    entries: RefCell<Vec<LayerEntry>>,
    /// 每种 Kind 的下一个 seq 编号（单调递增，删除不回收，保证稳定）
    kind_seq: RefCell<[i32; 6]>, // 下标对应 LayerKind discriminant
    /// handle 生成器
    next_handle: Cell<u64>,

    // ========== 向后兼容 ==========
    /// Content 层固定句柄（LayerHandle(0)），由 Runtime 在首次 submit_view_tree 时创建
    content_handle: Cell<Option<LayerHandle>>,
    /// 默认 Overlay 句柄（单实例语义，兼容旧 show_overlay/hide_overlay）
    default_overlay_handle: Cell<Option<LayerHandle>>,
    /// 默认 Modal 句柄（单实例语义，兼容旧 show_modal/hide_modal）
    default_modal_handle: Cell<Option<LayerHandle>>,

    // ========== Popup 生命周期 ==========
    /// Popup 父子归属：子 popup → 父 popup（菜单子菜单、右键菜单套菜单等）
    popup_parent: RefCell<std::collections::HashMap<LayerHandle, LayerHandle>>,
    /// Popup 父子归属：父 popup → 直接子 popup 列表
    popup_children: RefCell<std::collections::HashMap<LayerHandle, Vec<LayerHandle>>>,
}

impl LayerStack {
    pub fn new() -> Self {
        Self {
            tree: ElementTree::new(),
            event_manager: RefCell::new(EventManager::new()),
            entries: RefCell::new(Vec::new()),
            kind_seq: RefCell::new([0; 6]),
            next_handle: Cell::new(0),
            content_handle: Cell::new(None),
            default_overlay_handle: Cell::new(None),
            default_modal_handle: Cell::new(None),
            popup_parent: RefCell::new(std::collections::HashMap::new()),
            popup_children: RefCell::new(std::collections::HashMap::new()),
        }
    }

    // ========== 公开 API ==========

    /// 压入一条目，返回句柄。
    /// 注意：使用 `create_subtree_from_node` 递归构建整棵子树，
    /// 否则浮层只有根节点而无内容（如弹窗背景但有缺标题/按钮）。
    pub fn push(
        &mut self,
        kind: LayerKind,
        view: ViewNode,
        anchor: Anchor,
        focus: FocusPolicy,
        opts: LayerOptions,
    ) -> LayerHandle {
        let id = self.tree.create_subtree_from_node(&view);
        let handle = LayerHandle(self.next_handle.get());
        self.next_handle.set(handle.0 + 1);
        let seq = self.alloc_seq(kind);
        let entry = LayerEntry {
            handle,
            kind,
            root_id: id,
            anchor,
            visible: Cell::new(opts.visible),
            focus,
            seq,
            dismiss_on_outside_click: opts.dismiss_on_outside,
            backdrop: opts.backdrop,
            flip: opts.flip,
        };
        self.entries.borrow_mut().push(entry);
        handle
    }

    /// 与 `push` 相同，但使用调用方预先生成的 `LayerHandle`（窗口间句柄由全局命令携带）。
    /// 若传入的句柄序号不小于当前 `next_handle`，则同步推进，避免后续自增冲突。
    ///
    /// `parent` 为 `Some` 时登记 Popup 父子归属：子 popup 关闭时会级联关闭所有子孙，
    /// 父 popup 关闭时也会级联关闭其整棵子树（见 `close_popup`）。菜单子菜单、
    /// 右键菜单套菜单等均通过此参数建立归属关系。
    #[allow(clippy::too_many_arguments)]
    pub fn push_with_handle(
        &mut self,
        kind: LayerKind,
        view: ViewNode,
        anchor: Anchor,
        focus: FocusPolicy,
        opts: LayerOptions,
        handle: LayerHandle,
        parent: Option<LayerHandle>,
    ) -> LayerHandle {
        let id = self.tree.create_subtree_from_node(&view);
        if handle.0 >= self.next_handle.get() {
            self.next_handle.set(handle.0 + 1);
        }
        let seq = self.alloc_seq(kind);
        let entry = LayerEntry {
            handle,
            kind,
            root_id: id,
            anchor,
            visible: Cell::new(opts.visible),
            focus,
            seq,
            dismiss_on_outside_click: opts.dismiss_on_outside,
            backdrop: opts.backdrop,
            flip: opts.flip,
        };
        self.entries.borrow_mut().push(entry);
        if let Some(p) = parent {
            self.popup_parent.borrow_mut().insert(handle, p);
            self.popup_children
                .borrow_mut()
                .entry(p)
                .or_default()
                .push(handle);
        }
        handle
    }

    /// 移除一条目（ElementTree.remove + entries 删除）。
    /// 同时清理 popup 归属关系，但不级联子孙（级联请用 `close_popup`）。
    pub fn remove(&mut self, handle: LayerHandle) {
        let mut es = self.entries.borrow_mut();
        if let Some(pos) = es.iter().position(|e| e.handle == handle) {
            let e = es.remove(pos);
            self.tree.remove(e.root_id);
        }
        drop(es);
        self.popup_parent.borrow_mut().remove(&handle);
        if let Some(p) = self.popup_parent.borrow().get(&handle).copied()
            && let Some(children) = self.popup_children.borrow_mut().get_mut(&p)
        {
            children.retain(|c| *c != handle);
        }
        self.popup_children.borrow_mut().remove(&handle);
    }

    // ========== Popup 生命周期 ==========

    /// 返回某 popup 的所有子孙（含子、孙……），按任意顺序收集。
    pub fn popup_descendants(&self, handle: LayerHandle) -> Vec<LayerHandle> {
        let children_map = self.popup_children.borrow();
        let mut out = Vec::new();
        let mut stack: Vec<LayerHandle> = children_map
            .get(&handle)
            .cloned()
            .unwrap_or_default();
        while let Some(h) = stack.pop() {
            out.push(h);
            if let Some(ch) = children_map.get(&h) {
                stack.extend(ch.iter().copied());
            }
        }
        out
    }

    /// 关闭 popup 并**级联关闭其整棵子树**（所有子孙）。
    ///
    /// 这是 popup 生命周期的唯一标准关闭入口：菜单项点击、点击外部、hover 离开、
    /// 右键重复打开，最终都汇聚到此处，保证父子菜单不会残留为孤儿 popup。
    pub fn close_popup(&mut self, handle: LayerHandle) {
        let descendants = self.popup_descendants(handle);
        for d in descendants {
            self.remove(d);
        }
        self.remove(handle);
    }

    pub fn set_visible(&self, handle: LayerHandle, v: bool) {
        if let Some(entry) = self.by_handle(handle) {
            entry.visible.set(v);
        }
    }

    pub fn update_view(&mut self, handle: LayerHandle, new_view: ViewNode) {
        if let Some(entry) = self.by_handle(handle) {
            self.tree.update_node(entry.root_id, &new_view);
        }
    }

    pub fn reanchor(&self, handle: LayerHandle, new_anchor: Anchor) {
        let mut es = self.entries.borrow_mut();
        if let Some(pos) = es.iter().position(|e| e.handle == handle) {
            es[pos].anchor = new_anchor;
        }
    }

    // ========== 查询 ==========

    /// 按 handle 查找条目（返回克隆）
    pub fn by_handle(&self, handle: LayerHandle) -> Option<LayerEntry> {
        self.entries
            .borrow()
            .iter()
            .find(|e| e.handle == handle)
            .cloned()
    }

    /// 渲染 / 命中测试用：按 z() 升序（同 Kind 内早插入的在下）
    pub fn sorted_entries_for_render(&self) -> Vec<LayerEntry> {
        let mut es = self.entries.borrow().clone();
        es.sort_by_key(|e| e.z());
        es
    }

    /// 事件派发用：按 z() 降序（最顶条目先命中）
    pub fn sorted_entries_for_hit(&self) -> Vec<LayerEntry> {
        let mut es = self.entries.borrow().clone();
        es.sort_by_key(|e| std::cmp::Reverse(e.z()));
        es
    }

    /// 顶层阻塞 Modal：若返回 Some(handle)，只对该 handle 及更高 z 的条目派发事件
    pub fn top_blocking_modal(&self) -> Option<LayerHandle> {
        let es = self.entries.borrow();
        es.iter()
            .filter(|e| {
                e.visible.get() && e.focus == FocusPolicy::BlockBelow && e.kind == LayerKind::Modal
            })
            .max_by_key(|e| e.z())
            .map(|e| e.handle)
    }

    // ========== 内部 ==========

    fn alloc_seq(&self, kind: LayerKind) -> i32 {
        let mut arr = self.kind_seq.borrow_mut();
        let idx = kind as usize;
        let s = arr[idx];
        arr[idx] = s + 1;
        s
    }

    // ========== 向后兼容 API ==========

    /// 获取 Content 层 root ElementId
    pub fn content_root_id(&self) -> Option<ElementId> {
        self.content_handle
            .get()
            .and_then(|h| self.by_handle(h))
            .map(|e| e.root_id)
    }

    /// 获取默认 Overlay 层 root ElementId
    pub fn default_overlay_root_id(&self) -> Option<ElementId> {
        self.default_overlay_handle
            .get()
            .and_then(|h| self.by_handle(h))
            .map(|e| e.root_id)
    }

    /// 获取默认 Modal 层 root ElementId
    pub fn default_modal_root_id(&self) -> Option<ElementId> {
        self.default_modal_handle
            .get()
            .and_then(|h| self.by_handle(h))
            .map(|e| e.root_id)
    }

    /// 设置 Content 层 root（兼容旧 set_base_root）
    pub fn set_content_root(&mut self, id: ElementId) {
        let handle = LayerHandle(0);
        // 如果 content_handle 已存在，更新其 root_id；否则新建
        if self.content_handle.get().is_some() {
            if let Some(entry) = self.by_handle(handle) {
                // 如果旧 root 不同，先移除旧的
                if entry.root_id != id {
                    self.tree.remove(entry.root_id);
                    let mut es = self.entries.borrow_mut();
                    if let Some(pos) = es.iter().position(|e| e.handle == handle) {
                        es[pos].root_id = id;
                    }
                }
            }
        } else {
            // 首次创建 Content entry
            let handle = LayerHandle(self.next_handle.get());
            self.next_handle.set(handle.0 + 1);
            let seq = self.alloc_seq(LayerKind::Content);
            let entry = LayerEntry {
                handle,
                kind: LayerKind::Content,
                root_id: id,
                anchor: Anchor::None,
                visible: Cell::new(true),
                focus: FocusPolicy::Transparent,
                seq,
                dismiss_on_outside_click: false,
                backdrop: None,
                flip: false,
            };
            self.entries.borrow_mut().push(entry);
            self.content_handle.set(Some(handle));
        }
    }

    /// 设置默认 Overlay（兼容旧 show_overlay）
    pub fn push_default_overlay(&mut self, root_id: ElementId) {
        // 先移除旧的
        if let Some(h) = self.default_overlay_handle.get() {
            self.remove(h);
        }
        let handle = LayerHandle(self.next_handle.get());
        self.next_handle.set(handle.0 + 1);
        let seq = self.alloc_seq(LayerKind::Overlay);
        let entry = LayerEntry {
            handle,
            kind: LayerKind::Overlay,
            root_id,
            anchor: Anchor::None,
            visible: Cell::new(true),
            focus: FocusPolicy::Transparent,
            seq,
            dismiss_on_outside_click: false,
            backdrop: None,
            flip: false,
        };
        self.entries.borrow_mut().push(entry);
        self.default_overlay_handle.set(Some(handle));
    }

    /// 移除默认 Overlay（兼容旧 hide_overlay）
    pub fn remove_default_overlay(&mut self) {
        if let Some(h) = self.default_overlay_handle.get() {
            self.remove(h);
            self.default_overlay_handle.set(None);
        }
    }

    /// 设置默认 Modal（兼容旧 show_modal）
    pub fn push_default_modal(&mut self, root_id: ElementId) {
        // 先移除旧的
        if let Some(h) = self.default_modal_handle.get() {
            self.remove(h);
        }
        let handle = LayerHandle(self.next_handle.get());
        self.next_handle.set(handle.0 + 1);
        let seq = self.alloc_seq(LayerKind::Modal);
        let entry = LayerEntry {
            handle,
            kind: LayerKind::Modal,
            root_id,
            anchor: Anchor::ScreenCenter,
            visible: Cell::new(true),
            focus: FocusPolicy::BlockBelow,
            seq,
            dismiss_on_outside_click: false,
            backdrop: Some(crate::geometry::Color::rgba(0, 0, 0, 80)), // 半透明遮罩
            flip: false,
        };
        self.entries.borrow_mut().push(entry);
        self.default_modal_handle.set(Some(handle));
    }

    /// 移除默认 Modal（兼容旧 hide_modal）
    pub fn remove_default_modal(&mut self) {
        if let Some(h) = self.default_modal_handle.get() {
            self.remove(h);
            self.default_modal_handle.set(None);
        }
    }

    /// 检查指定 LayerKind 是否有内容（兼容旧 layer_has_content）
    pub fn layer_has_content(&self, kind: LayerKind) -> bool {
        self.entries
            .borrow()
            .iter()
            .any(|e| e.kind == kind && e.visible.get())
    }

    // ========== 命中测试 ==========

    fn hit_test_rec(tree: &ElementTree, id: ElementId, px: f32, py: f32) -> Option<ElementId> {
        let layout = tree.layout(id);
        if !layout.contains(px, py) {
            return None;
        }
        // 优先命中更内层的节点
        for cid in tree.children_ref(id).iter().rev() {
            if let Some(hit) = Self::hit_test_rec(tree, *cid, px, py) {
                return Some(hit);
            }
        }
        Some(id)
    }

    /// 跨所有层做命中测试，返回命中的层与元素（按 dispatch_order 优先高 z）
    pub fn hit_test_top(&mut self, point: Point) -> Option<(LayerKind, ElementId, LayerHandle)> {
        // 获取所有条目按 z 降序排列
        let entries = self.sorted_entries_for_hit();
        let cutoff_z = self
            .top_blocking_modal()
            .and_then(|h| self.by_handle(h))
            .map(|e| e.z())
            .unwrap_or(-1);

        let mut dismissed_handles: std::collections::HashSet<LayerHandle> =
            std::collections::HashSet::new();

        for entry in &entries {
            if !entry.visible.get() {
                continue;
            }
            if entry.z() < cutoff_z {
                break;
            }

            if let Some(id) = Self::hit_test_rec(&self.tree, entry.root_id, point.x, point.y) {
                return Some((entry.kind, id, entry.handle));
            }

            // 未命中本条目：FocusPolicy::Dismissable + dismiss_on_outside_click = true → 标记待 dismiss
            if entry.focus == FocusPolicy::Dismissable && entry.dismiss_on_outside_click {
                dismissed_handles.insert(entry.handle);
            }
        }

        // 末命中任何条目时，对标记的条目执行 dismiss（点击外部关闭 Popup）。
        // 走 `close_popup` 级联关闭整棵子树：父 popup 被点外关闭时，其所有子菜单
        //（子 popup）一并收起，避免残留孤儿 popup。用 HashSet 去重防止父子同被标记时重复关闭。
        for h in &dismissed_handles {
            self.close_popup(*h);
        }

        None
    }

    /// 跨层查找路径
    pub fn path_to(&self, target: ElementId) -> Vec<ElementId> {
        self.tree.path_to(target)
    }
}

impl Default for LayerStack {
    fn default() -> Self {
        Self::new()
    }
}
