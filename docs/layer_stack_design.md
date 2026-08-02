# LieUI Layer Stack 设计文档

> 设计阶段：RFC v1 | 创建日期：2026-08-01 | 设计目标：替换当前三层硬编码架构，建立可扩展的 z-index 分层栈

---

## 0. 设计目标

| 目标 | 说明 |
|------|------|
| **解决当前 Tooltip 缺陷** | Tooltip 不再用父容器内 `position: absolute`，彻底脱离父容器宽度约束和 `clip_content` 裁剪，且保证显示在 Modal 之上 |
| **消除单 root 限制** | 每层（Overlay/Tooltip/Modal/Popup）支持**多实例**并存，不再是 "show_overlay = 替换全局唯一 root" |
| **z-index 可扩展** | 层内自动按插入顺序分配 z，新增层类型无需改动渲染/命中测试核心逻辑 |
| **Modal 语义完备** | 支持 "Modal 叠 Modal"（顶层 Modal 阻塞下层 Modal 事件 + 背景遮罩），支持 `focus_policy` |
| **兼容现有 API** | 保留 `show_modal(view) / hide_modal() / show_overlay(view) / hide_overlay()` 语义，内部平移到 LayerStack |

---

## 1. 当前机制与已知缺陷

### 1.1 当前三层硬编码

```rust
pub enum LayerType {
    Base,    // z=0    单一 root
    Overlay, // z=1000 单一 root  ← 全局只能一个 Toast/Overlay
    Modal,   // z=2000 单一 root  ← 不能 Modal 叠 Modal
}
```

```rust
struct Layers {
    base: RefCell<LayerInfo>,    // Option<ElementId>
    overlay: RefCell<LayerInfo>, // Option<ElementId>
    modal: RefCell<LayerInfo>,   // Option<ElementId>
    tree: ElementTree,
    event_manager: RefCell<EventManager>,
}
```

### 1.2 缺陷清单

| # | 缺陷 | 现象 |
|---|------|------|
| D1 | **Tooltip 在 Base 层** | Modal 内部的 Button hover 产生的 Tooltip z=0 远低于 Modal(z=2000)，被完全遮挡 |
| D2 | **Tooltip 绝对定位在父 Div 内** | 父容器宽度（如 IconButton 28px）约束气泡可用宽度 → 文字竖排；外层 `clip_content` 则裁掉超出部分 |
| D3 | **Overlay 单 root** | 同时要 Toast + PopupMenu + Tooltip 三个浮层时冲突，后 `show_overlay` 覆盖前一个 |
| D4 | **Modal 单 root** | 不能在 "确认弹框" 之上再弹 "文件选择器"，二次 Modal 会顶掉前者 |
| D5 | **z 档位不灵活** | 只有 0/1000/2000 三档，想在 Modal 之上做 Tooltip / Debug 面板 无档位可用 |
| D6 | **Event hit_test 硬编码** | `dispatch_order()` 硬编码枚举，新增层类型时需改命中测试/派发两个核心函数 |
| D7 | **层生命周期无 owner** | Modal/Tooltip 挂入后无自动卸载机制，只能靠用户手动调 `hide_modal()`（Tooltip 理想情况：MouseLeave 时 Runtime 自动卸载对应 Tooltip 条目） |

---

## 2. 核心数据结构：LayerStack

### 2.1 LayerKind（层语义分类，保留档位空间）

```rust
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
            LayerKind::Popup   => 1000,
            LayerKind::Overlay => 2000,
            LayerKind::Modal   => 3000,
            LayerKind::Tooltip => 4000,
            LayerKind::System  => 5000,
        }
    }
    pub fn z_range(&self) -> std::ops::RangeInclusive<i32> {
        self.z_base()..=(self.z_base() + 999)
    }
    /// 事件派发 / 命中测试：高 z 优先
    pub fn dispatch_order() -> [LayerKind; 6] {
        [System, Tooltip, Modal, Overlay, Popup, Content]
    }
}
```

### 2.2 LayerEntry（栈内单个条目 = 一层视觉 + 交互单元）

```rust
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

/// 条目 ID = u64 generational，用户拿到后可 hide 指定条目
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayerHandle(u64);

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
    pub backdrop: Option<Color>,
}

impl LayerEntry {
    /// 实际渲染 z = kind.z_base() + seq
    pub fn z(&self) -> i32 {
        self.kind.z_base() + self.seq
    }
}
```

### 2.3 LayerStack（核心容器）

```rust
pub struct LayerStack {
    pub tree: ElementTree,
    pub event_manager: RefCell<EventManager>,
    entries: RefCell<Vec<LayerEntry>>,
    /// 每种 Kind 的下一个 seq 编号（单调递增，删除不回收，保证稳定）
    kind_seq: RefCell<[i32; 6]>, // 下标对应 LayerKind discriminant
    /// handle 生成器
    next_handle: Cell<u64>,
}

impl LayerStack {
    pub fn new() -> Self { /* ... */ }

    // ========== 公开 API ==========

    /// 压入一条目，返回句柄
    pub fn push(
        &self,
        kind: LayerKind,
        view: ViewNode,
        anchor: Anchor,
        focus: FocusPolicy,
        opts: LayerOptions, // { visible, dismiss_on_outside, backdrop }
    ) -> LayerHandle {
        let id = self.tree.create_from_node(&view);
        let handle = LayerHandle(self.next_handle.get());
        self.next_handle.set(handle.0 + 1);
        let seq = self.alloc_seq(kind);
        let entry = LayerEntry {
            handle, kind, root_id: id, anchor,
            visible: Cell::new(opts.visible),
            focus, seq,
            dismiss_on_outside_click: opts.dismiss_on_outside,
            backdrop: opts.backdrop,
        };
        self.entries.borrow_mut().push(entry);
        handle
    }

    /// 移除一条目（ElementTree.remove + entries 删除）
    pub fn remove(&self, handle: LayerHandle) {
        let mut es = self.entries.borrow_mut();
        if let Some(pos) = es.iter().position(|e| e.handle == handle) {
            let e = es.remove(pos);
            self.tree.remove(e.root_id);
        }
    }

    pub fn set_visible(&self, handle: LayerHandle, v: bool) { /* ... */ }
    pub fn update_view(&self, handle: LayerHandle, new_view: ViewNode) {
        // 对 root_id 做 update_node，复用 reconciler 机制
    }
    pub fn reanchor(&self, handle: LayerHandle, new_anchor: Anchor) { /* ... */ }

    // ========== 内部 ==========

    fn alloc_seq(&self, kind: LayerKind) -> i32 {
        let mut arr = self.kind_seq.borrow_mut();
        let idx = kind as usize;
        let s = arr[idx];
        arr[idx] = s + 1;
        s
    }

    /// 渲染 / 命中测试用：按 z() 升序（同 Kind 内早插入的在下）
    pub fn sorted_entries_for_render(&self) -> Vec<LayerEntry> { /* clone & sort_by_key(|e| e.z()) */ }

    /// 事件派发用：按 z() 降序（最顶条目先命中）
    pub fn sorted_entries_for_hit(&self) -> Vec<LayerEntry> { /* clone & sort_by_key(|e| Reverse(e.z())) */ }

    /// 顶层阻塞 Modal：若返回 Some(handle)，只对该 handle 及更高 z 的条目派发事件
    pub fn top_blocking_modal(&self) -> Option<LayerHandle> {
        let es = self.entries.borrow();
        es.iter()
            .filter(|e| e.visible.get() && e.focus == FocusPolicy::BlockBelow && e.kind == LayerKind::Modal)
            .max_by_key(|e| e.z())
            .map(|e| e.handle)
    }
}
```

---

## 3. 布局：Anchor 如何转成实际位置

### 3.1 布局两阶段

```
Runtime::perform_layout():
  ① 先对所有 LayerEntry 执行 FlexNode 测量（不包含位置）：
     - kind == Content  : viewport 约束下正常 Flex 布局（当前 Base 路径）
     - 其他 kind        : "无约束" 下测量（width/height = 内容本征尺寸）
  ② 再对每个 Entry 的 FlexStyle.top/left 做覆盖：
     - Anchor::Above { anchor, gap }:
         entry.top  = anchor.top - measured.h - gap
         entry.left = anchor.left + (anchor.w - measured.w) / 2
         视口边界 clamp：若 entry.left + entry.w > viewport.w 则左移至 fit
     - Anchor::Below / LeftOf / RightOf : 类似
     - Anchor::ScreenCenter:
         entry.top  = (viewport.h - measured.h) / 2
         entry.left = (viewport.w - measured.w) / 2
     - Anchor::Fixed(x, y): 直接赋值
     - Anchor::None : 保留布局结果（Content 用）
```

### 3.2 Content 层特殊处理

- Content 只有一条目（`LayerHandle(0)` 固定句柄），由 Runtime 在 `submit_view_tree` 首次创建时自动 `push(Content, view, Anchor::None, Transparent, default)`，用户不可 push/remove。
- `reconciler` 仅对 Content 的 `root_id` 做 diff/apply。

---

## 4. 渲染管线改造

### 4.1 build_render_tree 新流程

```
build_render_tree():
  let entries = layers.sorted_entries_for_render();  // 低 z → 高 z
  let mut out = Vec::new();
  for entry in entries:
    if !entry.visible.get() { continue }

    // ① Modal backdrop：在 Modal 内容之前绘制，z = entry.z() - 1
    if let Some(color) = entry.backdrop {
      out.push(LayeredElement::new(
        VisualElement::FillRect { rect: viewport_rect(), color, border_radius: 0.0 },
        entry.z() - 1,
      ));
    }

    // ② 遍历 entry.root_id 子树 → 递归 cv()，统一使用 entry.z()
    cv(entry.root_id, entry.z(), &mut out);
  done.
```

- 原 `LayerType::z_index()` 三档硬编码 → 改为每个 LayerEntry 自带 `entry.z()`
- Modal backdrop 不再需要单独 View 树，渲染管线级合成（消除 `hide_modal` 时 `request_rebuild` 依赖）

---

## 5. 事件派发改造

### 5.1 hit_test_top 新流程

```
hit_test_top(point):
  let entries = layers.sorted_entries_for_hit(); // 高 z → 低 z

  // 若存在 BlockBelow 的顶层 Modal，只派发 >= 该 Modal.z 的条目
  let cutoff_z = layers.top_blocking_modal()
    .and_then(|h| layers.by_handle(h))
    .map(|e| e.z())
    .unwrap_or(-1);

  let mut dismissed_handle: Option<LayerHandle> = None;
  for entry in entries:
    if !entry.visible.get() { continue }
    if entry.z() < cutoff_z { break }

    if let Some((hit_kind, id)) = Self::hit_test_rec(entry, point) {
      // 命中本条目
      return Some((hit_kind, id, entry.handle));
    }

    // 未命中本条目：FocusPolicy::Dismissable + dismiss_on_outside_click = true → 标记待 dismiss
    if matches!(entry.focus, Dismissable) && entry.dismiss_on_outside_click {
      dismissed_handle = Some(entry.handle);
    }
  end

  // 末命中任何条目时，对标记的条目执行 dismiss（点击外部关闭 Popup）
  if let Some(h) = dismissed_handle {
    layers.remove(h);
    emit_effects(needs_render);
  }
  None
```

### 5.2 dispatch 顺序（不变，但来自 entry 自身）

保持 Capture → Target → Bubble 三阶段；只是 `EventManager::mouse_capture` / `focused` 现在可以属于任意 LayerEntry 的 ElementId（不再跨 Base/Overlay/Modal 单独查找）。

---

## 6. Widget 层公开 API（对用户简单）

### 6.1 state.rs（thread_local 通道）

```rust
// ---- Modal ----
pub fn show_modal(view: ViewNode) -> LayerHandle;          // 阻塞式 Modal + 半透明背景
pub fn show_modal_no_backdrop(view: ViewNode) -> LayerHandle;
pub fn hide_modal(handle: LayerHandle);
pub fn hide_top_modal(); // 关闭最顶的 Modal（Esc 常用）

// ---- Tooltip ----
// Tooltip 不需要用户调用，由 Widget 内部使用：
pub(crate) fn show_tooltip(view: ViewNode, anchor: Rect) -> LayerHandle;
pub(crate) fn hide_tooltip(handle: LayerHandle);

// ---- Popup ----
pub fn show_popup(view: ViewNode, anchor: Rect) -> LayerHandle; // 可点击外部关闭
pub fn hide_popup(handle: LayerHandle);

// ---- Overlay / Toast ----
pub fn show_toast(view: ViewNode) -> LayerHandle;          // 可多个并行，自动堆叠
pub fn hide_overlay(handle: LayerHandle);

// ---- System ----
pub(crate) fn show_system_layer(view: ViewNode) -> LayerHandle;
```

### 6.2 Tooltip Widget 重写（利用 LayerStack 解决 D1 + D2）

```rust
impl Widget for Tooltip {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let handle_state = ctx.use_state::<Option<LayerHandle>>(|| None);
        let on_enter = {
            let hs = handle_state.clone();
            let tip = self.tip.clone();
            Rc::new(move |ctx: &mut EventContext| {
                // ★ 关键：用 ctx.target_rect() 拿到触发按钮的屏幕矩形
                let anchor = ctx.target_rect();
                // 构造气泡 ViewNode（与当前实现相同的黑底圆角 + 白字）
                let tip_view = make_tip_view(tip);
                // push 到 Tooltip 层（z 3000+），Anchor::Above 定位
                let h = state::show_tooltip(tip_view, Anchor::Above { anchor, gap: 4.0 });
                hs.set(Some(h));
            })
        };
        let on_leave = {
            let hs = handle_state.clone();
            Rc::new(move |_ctx: &mut EventContext| {
                if let Some(h) = hs.get().as_ref() {
                    state::hide_tooltip(*h);
                    hs.set(None);
                }
            })
        };
        // 外层只放一个 "透明包裹 Div + enter/leave listener + 子控件"
        //   → 不再内嵌气泡！气泡在 Tooltip 层，完全脱离本容器的宽度 / clip 约束 ✅
        ViewNode::Div {
            layout: FlexStyle::default(),
            paint: PaintStyle::new(),
            children: vec![ctx.child(0, &*self.child)],
            listeners: vec![
                Listener::on_mouse_enter(on_enter).builtin(),
                Listener::on_mouse_leave(on_leave).builtin(),
            ],
            key: None,
        }
    }
}
```

效果：
- D1 ✅ Tooltip 层 z=4000+ > Modal 层 z=3000+ → 显示在 Modal 之上
- D2 ✅ 气泡尺寸以无约束测量 + Anchor::Above 定位 → 不再受父容器 28px 宽限制，也不可能被 `clip_content` 祖先裁剪

---

## 7. 兼容性：旧 API 平移

| 旧 API | 新实现 |
|--------|--------|
| `show_overlay(view)` | `push(Overlay, view, Anchor::None, Transparent, default)`  + 记住一个 "默认 overlay handle"，再次调用时自动 remove 旧 handle → 保证单实例语义 |
| `hide_overlay()` | remove 默认 overlay handle |
| `show_modal(view)` （现有） | `push(Modal, view, Anchor::ScreenCenter, BlockBelow, { backdrop: Some(半透明灰), dismiss_on_outside: false })` + 记住 "默认 modal handle"，覆盖旧行为 |
| `hide_modal()` （现有） | remove 默认 modal handle |

- 现有例子 (gallery / hello) 不改一行代码也能工作
- 同时**新增**细粒度 API：`hide_modal(handle)` 用于多 Modal 场景

---

## 8. 多窗口上下文

在新的 `Application { windows: HashMap<WindowId, WindowContext> }` 中，每个 `WindowContext.runtime.layers` 都是独立的 `LayerStack`。窗口间的 Layer 互不干扰；Tooltip/Modal 只在自己的 WindowContext 里可见。

---

## 9. 性能影响评估

| 指标 | 三层旧实现 | LayerStack 新实现 | 评估 |
|------|-----------|------------------|------|
| 渲染遍历 entries 数 | 固定 3 次 | `O(n), n=条目数`（一般 < 20） | 可忽略 |
| 同 LayerKind 内排序 | 不需要 | `O(k log k), k < 20` | 可忽略 |
| 每次 Tooltip 打开/关闭 | N/A | `ElementTree.create / remove` 1 节点 | 单次 < 50μs |
| Content 的 Reconciler/Layout 热路径 | 直接取 `base.root` | 遍历找 `kind==Content` 1 次 | 可忽略 |
| Modal backdrop | 由用户手动构造 View | 渲染管线合成，少一棵子树 | 更快 |

结论：**对主内容（Content）热路径无影响**。Tooltip/Modal 的打开/关闭成本可接受。

---

## 10. 风险与缓解

| 风险 | 缓解措施 |
|------|---------|
| ElementTree 的 "layer stack 外 root" 与 "content 内节点" 被意外 diff 到一起 | 在 reconciler 入口加断言：仅对 Content.root_id 执行 diff；Tooltip/Modal/Popup 的 root 通过独立的 `create_from_node / remove` 管理 |
| `FocusPolicy::BlockBelow` 遗漏，导致穿透点击 Modal 背景到下面的 Base 页面 | 写单元测试：创建 Content + Modal(BlockBelow) + 命中 Modal 外位置 → 断言 Content 未收到事件 |
| Anchor 测量阶段报错（视图尺寸未返回前就定位） | 明确布局两阶段：先完整测量（FlexNode.layout → 写回 ComputedLayout.width/height） → 再按 Anchor 覆写 top/left；阶段分离代码清晰 |
| Tooltip 频繁 hover 导致 create/remove 开销 | 引入 `tooltip_delay_ms`（200ms）与 `hide_delay_ms`（100ms） 节流；先实现当前最简单版，必要时后续加 handle 缓存 |
