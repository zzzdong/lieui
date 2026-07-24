> ⚠️ **本文档已过时**（描述的是一次**已完成的**重构计划，且目标架构为旧 `Widget`/`WidgetTree`/`ViewContext` 模型）。当前 `v2-rewrite` 实际落地的架构为 `runtime` 协调器 + `view::ViewNode`（`View` trait）+ `ElementTree`，与之不同。
> **请以 [`../guide.md`](../guide.md) 为最新权威文档**；本文档仅留作历史参考。

# LieUI 重构改造计划

> 基于对全部源码和现有文档的深入评审，本文档给出分阶段的架构优化方案。
>
> **更新记录**：阶段 A-F 已全部完成，本文档已更新为实际完成情况。

---

## 0. 当前架构全貌

### 源码模块结构（重构后）

```
src/
├── app.rs              # winit 窗口集成、渲染循环
├── lib.rs              # prelude, 公开 API 导出
├── core/
│   ├── id.rs           # WidgetId (SlotMap key, generational)
│   ├── layers.rs       # 三层架构 (Base/Overlay/Modal)
│   ├── view_context.rs # 总控中枢
│   └── mod.rs
├── widget/
│   ├── tree.rs         # WidgetTree (SlotMap<WidgetId, WidgetEntry>)
│   └── mod.rs          # Widget trait 定义
├── widgets/
│   ├── button.rs       # Fluent UI 按钮
│   ├── column.rs       # Flex 纵排容器
│   ├── row.rs          # Flex 橫排容器
│   ├── container.rs    # 带样式的矩形容器
│   ├── text.rs         # 简单文本 (带布局缓存)
│   ├── text_input.rs   # 文本编辑器 (支持 IME)
│   └── mod.rs
├── layout/
│   ├── box_model.rs    # CSS 盒模型 (EdgeInsets, ComputedLayout, BoxStyle)
│   ├── constraint.rs   # LayoutConstraint
│   ├── context.rs      # LayoutContext (增量布局支持)
│   ├── flex.rs         # FlexStyle 定义
│   ├── measurable.rs   # Measurable trait + TextMeasure (新增 create_layout)
│   ├── node.rs         # LayoutNode (computed 必需, dirty 标记)
│   └── mod.rs
├── render/
│   ├── engine.rs       # VelloRenderer (变换栈, apply_fill_and_stroke)
│   ├── renderer.rs     # Renderer trait (简化 draw_text)
│   ├── visual.rs       # VisualElement + LayeredElement
│   └── mod.rs          # (已删除 node.rs)
├── event/
│   ├── callback.rs     # UserCallback / CallbackMap 类型
│   ├── context.rs      # EventContext (Cell 替代 RefCell, EventPhase 支持)
│   ├── manager.rs      # EventManager (单次 hit-test, 三阶段传播)
│   ├── propagation.rs  # Propagation (Copy trait)
│   ├── types.rs        # Event 枚举 + Key/MouseButton/Modifiers
│   └── mod.rs
├── geometry/
│   ├── types.rs        # Point, Size, Rect, RoundedRect, Color (唯一 Color 类型)
│   └── mod.rs
└── text/
    └── mod.rs          # TextEngine, TextStyle, TextColor=Color 别名
```

### 一帧的数据流（重构后）

```
App::render_and_present()
  → ViewContext::render()
    → has_dirty() 检查                    # O(1) 快速跳过
    → perform_layout()
      → collect_node() 递归               # 从 Widget 同步 dirty 状态
      → compute_incremental()             # 只计算 dirty 子树
        → layout_node_incremental()
          → 节点 dirty → 重新计算
          → 节点 clean + 子节点 dirty → layout_children_incremental
          → 节点 clean + 子节点 clean → 跳过
    → build_render_tree()
      → collect_visual_elements()
        → Widget::render()
          → Text::get_or_create_layout()  # 使用缓存布局
    → clear_all_dirty()
  → VelloRenderer::render()
    → draw_all_layered()
      → apply_fill_and_stroke()           # 统一 fill/stroke 模式
      → current_transform() 应用变换       # 变换栈支持
  → surface.buffer_mut() → present()
```

---

## 1. 问题汇总

### 1.1 严重问题 (P0) - 已全部解决 ✅

| # | 问题 | 状态 | 解决方案 |
|---|------|------|---------|
| 1 | **Color 类型三重复** | ✅ 已解决 | 统一为 `geometry::Color`，删除 `render::visual::Color`，`text::TextColor` 改为别名 |
| 2 | **双重渲染树** | ✅ 已解决 | 删除 `render/node.rs` (518行)，只保留 `VisualElement` |
| 3 | **Rc<RefCell<>> 存储** | ✅ 已解决 | 改为 `SlotMap<WidgetId, WidgetEntry>` + 独立 RefCell |
| 4 | **ViewContext 是 God Object** | 📋 待解决 | 阶段 G |
| 5 | **文本布局在 render 阶段重做** | ✅ 已解决 | Text widget 内部缓存 `TextLayout` |

### 1.2 中等问题 (P1) - 已全部解决 ✅

| # | 问题 | 状态 | 解决方案 |
|---|------|------|---------|
| 6 | **LayoutNode 双用途** | ✅ 已解决 | `computed` 改为必需字段，消除 `Option` 检查 |
| 7 | **layout_node / measure_node 代码重复** | ✅ 已解决 | 提取公共方法 `measure_leaf`、`compute_boxes`、`distribute_flex_space` |
| 8 | **事件系统双重 hit-test** | ✅ 已解决 | `HitTestResult` 缓存命中结果，单次 hit-test |
| 9 | **事件传播缺少 Capture 阶段** | ✅ 已解决 | 实现 `EventPhase` (Capture/Target/Bubble) 三阶段传播 |
| 10 | **draw_* 方法中 fill/stroke 模式重复** | ✅ 已解决 | 提取 `apply_fill_and_stroke` 方法 |

### 1.3 轻微问题 (P2) - 已部分解决

| # | 问题 | 状态 | 解决方案 |
|---|------|------|---------|
| 11 | `draw_text` 签名冗余 | ✅ 已解决 | 简化为 4 参数：position, color, rotation, layout |
| 12 | `push_transform` / `pop_transform` 是空壳 | ✅ 已解决 | 实现变换栈 `Vec<Affine>`，所有 draw_* 应用变换 |
| 13 | `collect_visual_elements` 的 `layer` 参数未使用 | 📋 待解决 | 阶段 G |
| 14 | `collect_node` 不必要地获取 `get_widget_mut` | ✅ 已解决 | 改为 `get_widget_immut` |
| 15 | EventContext 用 `RefCell` 包装 `Copy` 类型 | ✅ 已解决 | 改为 `Cell<EventEffects>`、`Cell<Propagation>` |
| 16 | Button.render() 内联文本渲染逻辑 | ✅ 已解决 | 使用 `Text::get_or_create_layout()` |
| 17 | `LayerType` 硬编码三层 | 📋 待解决 | 阶段 G |
| 18 | `RenderNode` 在 prelude 中导出 | ✅ 已解决 | 已删除 |

---

## 2. 阶段规划与实际完成情况

### 阶段 A：清理死代码与类型统一 ✅ 已完成

**目标**：消除冗余、统一基础类型，为后续重构铺路。

#### A1. 删除 `render/node.rs` ✅

**实际改动**：
- 删除文件：`src/render/node.rs` (518 行)
- 更新 `src/render/mod.rs`：移除 `pub mod node;` 和相关导出
- 更新 `src/lib.rs` prelude：移除 `RenderNode` 导出
- 移除 `visual.rs` 中 `From<node::BoxShadow>` 实现

**效果**：减少 ~518 行冗余代码，消除 API 混淆。

#### A2. 统一 Color 类型 ✅

**实际改动**：
- 删除 `render::visual::Color` (纯 struct `{r, g, b, a}`)
- 将 `text::TextColor` 改为 `geometry::Color` 的类型别名
- 为 `geometry::Color` 添加：
  - `from_rgb8(r, g, b)` / `from_rgba8(r, g, b, a)` 常量构造方法
  - `inner()` 返回内部 `AlphaColor<Srgb>`
  - `to_vello()` 直接返回渲染用颜色
- 更新所有 widgets 和 render/engine.rs 使用统一 `geometry::Color`

**效果**：消除每次渲染的颜色转换开销，简化 API。

#### A3. 清理事件系统中的 RefCell ✅

**实际改动**：
- 为 `EventEffects` 添加 `Copy` trait
- 为 `Propagation` 添加 `Copy + Clone` traits
- `EventContext` 中 `RefCell<EventEffects>` → `Cell<EventEffects>`
- `EventContext` 中 `RefCell<Propagation>` → `Cell<Propagation>`
- 更新所有访问点：`.borrow_mut()` → `.get()` / `.set()`

**效果**：消除运行时借用检查开销，避免 panic 风险。

---

### 阶段 B：数据存储层重构 ✅ 已完成

**目标**：用 `SlotMap + 独立 RefCell` 替代 `Rc<RefCell<>>`，消除 Rc 开销，集中父子关系。

#### B1. 引入 SlotMap + 独立 RefCell 存储 ✅

**实际改动**：
- 添加 `slotmap = "1.0"` 到 Cargo.toml
- 使用 `slotmap::new_key_type!` 定义 `WidgetId`，自带 generation + index
- `WidgetTree` 改为 `SlotMap<WidgetId, WidgetEntry>`
- `WidgetEntry` 包含：
  - `widget: RefCell<Box<dyn Widget>>` (独立 RefCell)
  - `parent: Option<WidgetId>` (反向引用)
  - `children: Vec<WidgetId>` (集中管理)

**新增公开 API**：
```rust
// WidgetTree
pub fn insert(&mut self, widget: Box<dyn Widget>) -> WidgetId;
pub fn remove(&mut self, id: WidgetId);
pub fn get_widget(&self, id: WidgetId) -> Option<WidgetRef<'_>>;
pub fn get_widget_mut(&self, id: WidgetId) -> Option<WidgetRefMut<'_>>;
pub fn get_widget_immut(&self, id: WidgetId) -> Option<Ref<'_, Box<dyn Widget>>>;
pub fn children_of(&self, id: WidgetId) -> Vec<WidgetId>;
pub fn parent_of(&self, id: WidgetId) -> Option<WidgetId>;
pub fn path_to(&self, target: WidgetId) -> Vec<WidgetId>;  // O(depth)
```

**效果**：缓存友好，O(depth) path_to，消除 Rc 开销。

#### B2. 改造 Widget trait ✅

**实际改动**：
- 移除 `children(&self) -> &[WidgetId]`、`add_child()`、`remove_child()` 从 Widget trait
- 父子关系全部由 `WidgetEntry` 集中管理
- Widget 只保留核心逻辑

**效果**：每个容器 Widget 减少约 10 行样板代码。

#### B3. 遍历和访问模式改善 ✅

**实际改动**：
- 遍历不再需要 `children().to_vec()` + `drop(widget)` 模式
- 直接从 `WidgetEntry.children` 读取
- `path_to` 从 O(n) DFS 改为 O(depth) 沿 parent 链上溯

#### B4. 与事件回调的兼容性保证 ✅

**验证**：独立 RefCell 保留，事件回调中跨 Widget 状态修改正常工作。

---

### 阶段 C：Layout 系统重构 ✅ 已完成

**目标**：分离约束描述与计算结果，支持增量布局。

#### C1. LayoutNode computed 必需化 ✅

**实际改动**：
- `computed` 字段改为必需，消除 `Option<ComputedLayout>` 检查开销
- 新建 LayoutNode 时提供默认 `ComputedLayout::default()`
- 消除所有 `if let Some(computed)` 检查

**效果**：简化代码，消除运行时分支。

#### C2. 提取公共测量逻辑 ✅

**实际改动**：
- 新增 `measure_leaf()`：叶子节点测量（被 layout_node 复用）
- 新增 `measure_content()`：测量分发器
- 新增 `compute_boxes()`：box 层级计算
- 新增 `distribute_flex_space()`：弹性空间分配
- `layout_node` 叶子节点复用 `measure_leaf()`

**效果**：减少约 98 行重复代码。

#### C3. 增量布局 ✅

**实际改动**：
- `LayoutNode` 添加 `dirty: bool` 字段
- 新增方法：
  - `mark_dirty()` / `clear_dirty()` / `is_dirty()`
  - `propagate_dirty_down()`：向下传播 dirty
  - `has_dirty_child()` / `collect_dirty_ids()`
- `LayoutContext` 新增公开 API：
  - `compute_incremental(viewport) -> bool`：只计算 dirty 子树
  - `mark_dirty(id)` / `mark_dirty_with_children(id)`
  - `clear_all_dirty()` / `has_dirty()` / `dirty_ids()`
- `collect_node` 从 Widget 同步 dirty 状态

**效果**：O(dirty) 替代 O(n)，无 dirty 时 O(1) 跳过。

---

### 阶段 D：Text 布局缓存 ✅ 已完成

**目标**：消除 render 阶段的文本布局重做。

#### D1. Text widget 内部缓存 ✅

**实际改动**：
- `Text` widget 添加：
  - `cached_layout: Option<TextLayout>`
  - `cached_max_width: Option<f32>`
- 新增 `get_or_create_layout(max_width) -> &TextLayout`：
  - 只在 dirty / 缓存不存在 / 宽度变化时重新布局
  - 否则返回缓存布局
- `render()` 使用缓存布局，不再调用 `do_layout()`
- `set_content()` / `set_font_size()` / `set_text_color()` 时清除缓存

**新增公开 API**：
```rust
// Text widget
pub fn get_or_create_layout(&mut self, max_width: Option<f32>) -> &TextLayout;
pub fn invalidate_layout(&mut self);  // 强制清除缓存
pub fn style(&self) -> &TextStyle;    // 获取样式
```

#### D2. TextMeasure 增强 ✅

**实际改动**：
- 新增 `create_layout(max_width) -> TextLayout` 方法
- `measure()` 内部调用 `create_layout()`

**效果**：消除每帧重复文本布局，只在内容变化时重新布局。

---

### 阶段 E：事件系统重构 ✅ 已完成

**目标**：消除重复 hit-test，补充 Capture 阶段，简化 EventManager。

#### E1. 单次命中测试 ✅

**实际改动**：
- 新增 `HitTestResult` 结构体缓存命中结果：
  ```rust
  pub struct HitTestResult {
      pub target: WidgetId,
      pub path: Vec<WidgetId>,
  }
  ```
- 新增 `hit_test_with_path()` 方法：一次计算 target + path
- 所有事件 handler 使用缓存的 `HitTestResult`

**效果**：消除重复命中测试，从 2-3 次 hit-test 降至 1 次。

#### E2. 补充 Capture 事件阶段 ✅

**实际改动**：
- 新增 `EventPhase` 枚举：`Capture` / `Target` / `Bubble`
- 新增 `dispatch_three_phase()` 方法实现完整三阶段传播：
  - Phase 1: Capture (Root → Parent)
  - Phase 2: Target
  - Phase 3: Bubble (Parent → Root)
- `EventContext` 新增：
  - `phase()` / `set_phase()` / `is_capture()` / `is_target()` / `is_bubble()`

**效果**：完整 DOM 标准事件模型，支持事件拦截。

#### E3. 简化 EventManager ✅

**实际改动**：
- 新增 `mouse_capture()` getter
- `dispatch_to_focused()` 简化键盘/IME 事件分发
- 所有方法使用缓存的 `HitTestResult`

---

### 阶段 F：Render 系统优化 ✅ 已完成

**目标**：消除代码重复，补全缺失功能。

#### F1. 消除 fill/stroke 重复 ✅

**实际改动**：
- 新增 `apply_fill_and_stroke(style, fill_fn, stroke_fn)` 方法
- `draw_rect` / `draw_rounded_rect` / `draw_circle` / `draw_path` 使用统一模式

**效果**：减少约 40 行重复代码。

#### F2. 简化 draw_text 签名 ✅

**实际改动**：
- 从 8 参数简化为 4 参数：`position`, `color`, `rotation`, `layout`
- 删除未使用的 `_text`、`_font_size`、`_font_family` 参数

#### F3. 实现变换栈 ✅

**实际改动**：
- `VelloRenderer` 添加 `transform_stack: Vec<Affine>`
- 新增 `current_transform()` 获取累积变换
- 新增 `apply_transform_to_path()` / `apply_transform_to_rect()` 应用变换
- 所有 `draw_*` 方法自动应用当前变换
- `push_transform()` / `pop_transform()` 实现变换栈操作

**效果**：支持嵌套变换，`VisualElement::Group` 的 transform 字段生效。

---

### 阶段 G：模块职责拆分 📋 待实施

**目标**：打破 ViewContext 的 God Object 模式。

#### G1. 拆分 ViewContext

```
ViewContext 现状 → 拆分为:

src/
├── pipeline/
│   ├── mod.rs
│   ├── layout_pipeline.rs    # 布局编排
│   └── render_pipeline.rs    # 渲染编排
├── core/
│   ├── view_context.rs       # 精简后仅暴露 API
│   └── dirty_tracker.rs      # 脏标记管理
```

---

### 阶段 H：文档更新 📋 待实施

#### H1. 需要更新的文档

| 文档 | 更新内容 |
|------|---------|
| `architecture.md` | 反映 VisualElement 替换 RenderNode，SlotMap 存储 |
| `render.md` | 重写为 VisualElement + Renderer trait + 变换栈 |
| `layout.md` | 更新为增量布局说明，dirty 标记机制 |
| `widget.md` | 更新 Widget trait 签名（移除 children 方法），SlotMap 存储 |
| `event.md` | 补充 Capture 阶段文档，HitTestResult 缓存 |
| `text.md` | 更新为 Text 布局缓存机制 |

---

## 3. 实施顺序与完成状态

```
阶段 A (P0, 清理) ✅ 已完成 ────────────────────
    │
    ├── A1: 删除 RenderNode ✅
    ├── A2: 统一 Color 类型 ✅
    └── A3: EventContext RefCell → Cell ✅
                                            │
阶段 B (P0, SlotMap + RefCell) ✅ 已完成 ──────┤
    │                                       │
    ├── B1: 引入 SlotMap + RefCell ✅        │
    ├── B2: 改造 Widget trait ✅             │
    ├── B3: 遍历和访问模式改善 ✅              │
    └── B4: 兼容事件回调 ✅                   │
                                            │
阶段 C (P1, layout) ✅ 已完成 ─────────────── ┤
    │                                       │
    ├── C1: computed 必需化 ✅               │
    ├── C2: 提取公共测量逻辑 ✅               │
    ├── C3: 增量布局 ✅                       │
    └── C4: 修正实现细节 ✅                   │
                                            │
阶段 D (P0, text cache) ✅ 已完成 ─────────── ┤
    │                                       │
    ├── D1: Text widget 内部缓存 ✅          │
    └── D2: TextMeasure 增强 ✅              │
                                            │
阶段 E (P1, event) ✅ 已完成 ──────────────── ┤
    │                                       │
    ├── E1: 单次 hit-test ✅                 │
    ├── E2: 补充 Capture 阶段 ✅              │
    └── E3: 简化 EventManager ✅              │
                                            │
阶段 F (P1, render) ✅ 已完成 ─────────────── ┤
    │                                       │
    ├── F1: 消除 fill/stroke 重复 ✅          │
    ├── F2: 简化 draw_text ✅                 │
    └── F3: 实现变换栈 ✅                     │
                                            │
阶段 G (P1, 模块拆分) 📋 待实施 ───────────── ┤
    │                                       │
    ├── G1: 拆分 ViewContext                 │
    └── G2: 职责分配                         │
                                            │
阶段 H (P2, 文档) 📋 待实施 ─────────────────┘
    └── 全部代码变更完成后更新

实际执行顺序: A → B → C → D → E → F ✅
后续阶段: G → H 📋
```

---

## 4. 风险与注意事项

### 4.1 已验证的低风险项

1. **B 阶段 (SlotMap + RefCell 迁移)**：已验证与 `Rc<RefCell<>>` 保持相同的事件回调兼容性。独立 RefCell 可同时借用，Counter 示例正常运行。
2. **C3 (增量布局)**：已添加测试覆盖 (`test_dirty_propagation`, `test_collect_dirty_ids`)，dirty 传播逻辑正确。

### 4.2 测试验证

- 所有阶段完成后运行 `cargo test`：12 个测试全部通过
- 运行现有 examples (`counter`)：正常工作

---

## 5. 衡量标准与实际达成

| 指标 | 目标 | 实际达成 |
|------|------|---------|
| `RenderNode` 删除 | ~800 行 | ✅ 删除 518 行 |
| `Color × 3` → 单一 `Color` | 消除转换 | ✅ 统一为 `geometry::Color` |
| `Rc<RefCell<>>` → `SlotMap<RefCell<>>` | 无 Rc 开销 | ✅ SlotMap + 独立 RefCell |
| `path_to` 优化 | O(depth) | ✅ 从 O(n) DFS 改为 O(depth) 上溯 |
| `Text::render()` 中的布局 | 消除 | ✅ 使用缓存，只在变化时布局 |
| `EventManager` hit-test | 1 次 | ✅ `HitTestResult` 缓存 |
| 事件传播 | 三阶段 | ✅ Capture + Target + Bubble |
| `draw_*` fill/stroke 重复 | 消除 | ✅ `apply_fill_and_stroke` |
| `draw_text` 参数 | 4 个 | ✅ 从 8 个简化为 4 个 |
| 变换支持 | 实现 | ✅ 变换栈 + 所有 draw_* 应用变换 |
| 增量布局 | O(dirty) | ✅ `compute_incremental` |
| 测试覆盖 | 通过 | ✅ 12 个测试全部通过 |

---

## 6. 新增公开 API 汇总

### WidgetTree

```rust
pub fn insert(&mut self, widget: Box<dyn Widget>) -> WidgetId;
pub fn remove(&mut self, id: WidgetId);
pub fn get_widget(&self, id: WidgetId) -> Option<WidgetRef<'_>>;
pub fn get_widget_mut(&self, id: WidgetId) -> Option<WidgetRefMut<'_>>;
pub fn get_widget_immut(&self, id: WidgetId) -> Option<Ref<'_, Box<dyn Widget>>>;
pub fn children_of(&self, id: WidgetId) -> Vec<WidgetId>;
pub fn parent_of(&self, id: WidgetId) -> Option<WidgetId>;
pub fn path_to(&self, target: WidgetId) -> Vec<WidgetId>;
pub fn root(&self) -> Option<WidgetId>;
pub fn set_root(&mut self, id: WidgetId);
```

### LayoutContext

```rust
pub fn compute_incremental(&mut self, viewport: Size) -> bool;
pub fn mark_dirty(&mut self, id: WidgetId);
pub fn mark_dirty_with_children(&mut self, id: WidgetId);
pub fn clear_all_dirty(&mut self);
pub fn has_dirty(&self) -> bool;
pub fn dirty_ids(&self) -> Vec<WidgetId>;
```

### LayoutNode

```rust
pub fn mark_dirty(&mut self);
pub fn clear_dirty(&mut self);
pub fn is_dirty(&self) -> bool;
pub fn propagate_dirty_down(&mut self);
pub fn has_dirty_child(&self) -> bool;
pub fn collect_dirty_ids(&self) -> Vec<WidgetId>;
pub fn with_dirty(mut self, dirty: bool) -> Self;
```

### EventManager

```rust
pub fn mouse_capture(&self) -> Option<WidgetId>;
```

### EventPhase (新增导出)

```rust
pub enum EventPhase { Capture, Target, Bubble }
```

### HitTestResult (新增导出)

```rust
pub struct HitTestResult {
    pub target: WidgetId,
    pub path: Vec<WidgetId>,
}
```

### EventContext

```rust
pub fn phase(&self) -> EventPhase;
pub fn set_phase(&self, phase: EventPhase);
pub fn is_capture(&self) -> bool;
pub fn is_target(&self) -> bool;
pub fn is_bubble(&self) -> bool;
```

### Text widget

```rust
pub fn get_or_create_layout(&mut self, max_width: Option<f32>) -> &TextLayout;
pub fn invalidate_layout(&mut self);
pub fn style(&self) -> &TextStyle;
```

### TextMeasure

```rust
pub fn create_layout(&self, max_width: Option<f32>) -> TextLayout;
```

---

## 7. 文件改动汇总

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| `render/node.rs` | 删除 | 删除 RenderNode 旧体系 (518 行) |
| `render/mod.rs` | 修改 | 移除 node 模块导出 |
| `lib.rs` | 修改 | 移除 RenderNode 导出，更新 prelude |
| `geometry/types.rs` | 修改 | Color 添加 from_rgb8/from_rgba8/to_vello 方法 |
| `render/visual.rs` | 修改 | 删除 Color 定义，使用 geometry::Color |
| `text/mod.rs` | 修改 | TextColor 改为 Color 别名 |
| `event/context.rs` | 修改 | RefCell → Cell，添加 EventPhase 支持 |
| `event/manager.rs` | 修改 | 单次 hit-test，三阶段传播，HitTestResult |
| `event/mod.rs` | 修改 | 导出 EventPhase, HitTestResult |
| `event/propagation.rs` | 修改 | 添加 Copy trait |
| `core/id.rs` | 修改 | 使用 SlotMap new_key_type! |
| `widget/tree.rs` | 重写 | SlotMap<WidgetId, WidgetEntry> 存储 |
| `widget/mod.rs` | 修改 | 移除 children/add_child/remove_child 方法 |
| `core/layers.rs` | 修改 | 使用新 WidgetTree API |
| `core/view_context.rs` | 修改 | 使用新 WidgetTree/Layers API |
| `layout/node.rs` | 修改 | computed 必需化，添加 dirty 标记 |
| `layout/context.rs` | 修改 | 增量布局，提取公共方法 |
| `layout/measurable.rs` | 修改 | TextMeasure 添加 create_layout |
| `widgets/text.rs` | 修改 | 添加布局缓存 |
| `widgets/button.rs` | 修改 | 使用 Text 缓存布局 |
| `widgets/row.rs` | 修改 | 移除内部 children 字段 |
| `widgets/column.rs` | 修改 | 移除内部 children 字段 |
| `widgets/container.rs` | 修改 | 移除内部 children 字段 |
| `render/engine.rs` | 修改 | 变换栈，apply_fill_and_stroke，简化 draw_text |
| `render/renderer.rs` | 修改 | 简化 draw_text 签名 |
| `Cargo.toml` | 修改 | 添加 slotmap = "1.0" |

---

**文档更新日期**：2026-06-15
**重构状态**：阶段 A-F 全部完成 ✅，阶段 G-H 待实施 📋