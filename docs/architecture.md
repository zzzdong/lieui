# LieUI 架构设计文档

> 版本：v2-rewrite | 更新日期：2026-07-25

---

## 一、设计哲学

### 1.1 核心原则

| 原则 | 说明 |
|------|------|
| **Builder 驱动 UI** | 每次状态变化重新执行 Builder 闭包，生成新的 ViewNode 树，通过 Reconciliation 计算差异 |
| **三原语** | 仅 Text / Image / Div 三种 ViewNode 原语表达全部 UI，不引入自定义节点类型 |
| **内联样式** | ViewNode 直接持有 FlexStyle + PaintStyle，不实现 CSS 选择器/继承 |
| **C/S 分层** | Runtime = Server，ElementTree = Client，通过 Reconciler 同步状态 |
| **事件驱动渲染** | 非游戏循环，仅在事件或状态变化时触发重绘 |

### 1.2 架构分层

```
┌─────────────────────────────────────────────────────────────┐
│                     Widget Layer                             │
│  Button / Checkbox / Container / Row / Column / ListView    │
│  Widget trait: build() → ViewNode                          │
│  BuildContext: use_state() 状态持久化                       │
├─────────────────────────────────────────────────────────────┤
│                     ViewNode Layer                           │
│  Text { content, style, layout, listener }                  │
│  Image { data, style, layout, listener }                    │
│  Div { layout, paint, children, listener }                  │
│  不变性：每次 rebuild 生成新树，不修改已有节点               │
├─────────────────────────────────────────────────────────────┤
│                   Runtime Layer                              │
│  submit_view_tree() → Reconciler → ElementTree 突变          │
│  perform_layout() → FlexNode 计算 → 写回 ComputedLayout    │
│  build_render_tree() → cv() → VisualElement 列表            │
├─────────────────────────────────────────────────────────────┤
│                   Layout Engine                              │
│  Taitank 风格 Flexbox 引擎                                  │
│  FlexNode: flex-direction/wrap/justify/align/gap/min-max    │
│  TextMeasure: 按约束宽度测量文本                             │
├─────────────────────────────────────────────────────────────┤
│                   Event System                               │
│  Capture → Target → Bubble 三阶段分发                       │
│  EventManager: hover/pressed/focused 状态管理               │
│  ViewListener: Rc<dyn Fn()> / Rc<dyn Fn(&EventContext)>    │
├─────────────────────────────────────────────────────────────┤
│                   Render Backend                             │
│  VelloRenderer: vello_cpu 封装                              │
│  Scene 构建: fill_rect/fill_blurred_rounded_rect/text      │
│  Clip: push_clip_path/pop_clip_path                         │
│  Image blit: alpha 预乘后写入 PremulRgba8 pixmap            │
└─────────────────────────────────────────────────────────────┘
```

---

## 二、模块架构

### 2.1 模块依赖图

```
lib.rs (prelude)
├── app.rs             ← winit 事件循环 + softbuffer 表面 + VelloRenderer
├── core/              ← 基础设施
│   ├── id.rs          ← ElementId (slotmap key)
│   ├── layers.rs      ← Base/Overlay/Modal 三层 + EventManager + hit_test
│   └── state.rs       ← ElementState: { hovered, pressed, focused }
├── view/              ← UI 描述原语
│   ├── node.rs        ← ViewNode 枚举 + ViewListener + tree_eq
│   └── paint.rs       ← PaintStyle / TextStyle / ImageStyle
├── widget/            ← 组件层
│   └── mod.rs         ← Widget trait / BuildContext / Stateful
├── runtime/           ← 核心管线编排
│   ├── mod.rs         ← Runtime: frame / submit_view_tree / cv
│   ├── element.rs     ← ElementTree: SlotMap 存储
│   └── reconciler.rs  ← ViewNode diff → Patch[Create/Update/Remove/Move]
├── layout/            ← 布局引擎
│   ├── flex_node.rs   ← FlexNode (Taitank 风格)
│   ├── context.rs     ← LayoutContext: ElementTree → FlexNode → 写回
│   ├── style.rs       ← FlexStyle
│   ├── box_model.rs   ← IntrinsicSize / ComputedLayout / EdgeInsets
│   ├── measurable.rs  ← TextMeasure / FixedMeasure / EmptyMeasure
│   ├── flex_line.rs   ← FlexLine
│   └── types.rs       ← 布局类型定义
├── event/             ← 事件系统
│   ├── manager.rs     ← EventManager: 三阶段分发
│   ├── types.rs       ← Event 枚举
│   ├── context.rs     ← EventContext: 阶段/副作用/传播控制
│   └── propagation.rs ← HitTestResult / EventPhase / EventEffects
├── render/            ← 渲染层
│   ├── engine.rs      ← VelloRenderer
│   ├── visual.rs      ← VisualElement / LayeredElement
│   └── renderer.rs    ← Renderer trait
├── text/              ← 文本排版
│   └── mod.rs         ← TextEngine (parley)
├── geometry/          ← 几何类型
│   └── types.rs       ← Point / Size / Rect / Color
├── state.rs           ← 全局状态信号 (thread_local)
├── theme.rs           ← Theme 管理
└── lib.rs             ← 模块声明 + prelude 导出
```

### 2.2 模块职责

| 模块 | 职责 | 不负责 |
|------|------|--------|
| `view` | 定义不可变 UI 描述原语和样式 | 不包含状态、布局、事件处理逻辑 |
| `widget` | 提供可复用的组件，将组件配置编译为 ViewNode 树 | 不管理运行时状态（除 BuildContext 的 use_state） |
| `runtime` | 编排管线：Reconciler → Layout → Render Tree | 不直接操作窗口或 GPU |
| `layout` | 纯计算：Flexbox 布局算法 | 不存储布局结果（结果写回 ElementTree） |
| `event` | 事件分发、命中测试、交互状态管理 | 不处理 widget 特定逻辑 |
| `render` | 渲染后端封装 | 不关心 UI 语义，只消费 VisualElement |
| `text` | 文本排版引擎封装 | 不管理文本缓存（ElementTree 管理） |
| `core` | 基础设施：ID 生成、图层管理、交互状态 | 不包含业务逻辑 |
| `app` | 窗口事件循环、管线入口、像素输出 | 不包含 widget 逻辑 |

---

## 三、核心数据流

### 3.1 完整渲染帧

```
User Event (winit)
  │
  ▼
Application::window_event()
  │
  ├── CursorMoved → EventManager::handle_mouse_move()
  ├── MouseInput  → EventManager::handle_mouse_down/up()
  │     └── 更新 ElementState (hovered/pressed)
  │     └── dispatch_three_phase() → handle_lie_event() → ViewListener 回调
  │     └── 返回 EventEffects → apply_event_effects()
  │
  └── RedrawRequested
        │
        ├── take_rebuild_requested() = true → build_and_render()
        │     ├── [可选] set_viewport() (仅 viewport 变化时)
        │     ├── builder 闭包执行 → 返回 Box<dyn Widget>
        │     ├── root_widget.build(&mut ctx) → ViewNode 树
        │     ├── submit_view_tree(view_tree, rebuild_requested)
        │     │     └── rebuild_requested = true: 跳过 tree_eq()
        │     │     └── rebuild_requested = false: tree_eq() 比较
        │     │           └── 相同 → 返回 false，跳过管线
        │     ├── Runtime::frame()
        │     │     ├── take_rebuild_requested() || pending_view_tree
        │     │     ├── Reconciler::diff() → Patch[Create/Update/Remove/Move]
        │     │     ├── Reconciler::apply() → ElementTree 突变
        │     │     ├── has_dirty_node() → perform_layout()
        │     │     │     └── LayoutContext::compute() → FlexNode → 写回
        │     │     └── build_render_tree() → cv() → Vec<LayeredElement>
        │     ├── VelloRenderer::render() → Pixmap
        │     └── blit_to_window() → softbuffer::present()
        │
        └── render_visuals() (仅 hover/pressed 视觉变化)
              ├── frame_visual_update()
              │     ├── needs_layout → perform_layout()
              │     └── needs_render → build_render_tree()
              ├── VelloRenderer::render() → Pixmap
              └── blit_to_window() → softbuffer::present()
```

### 3.2 状态变化触发链路

```
State::set() / State::update()
  → request_rebuild() (thread_local flag)
  → 下次事件 → w.request_redraw()
  → RedrawRequested
  → take_rebuild_requested() = true
  → build_and_render(rebuild_requested = true)
      → builder 执行 → 新 ViewNode 树
      → submit_view_tree(view_tree, true)
          → 跳过 tree_eq()
          → 保存 pending_view_tree
      → frame()
          → Reconciler diff/apply
          → Layout
          → Render Tree
          → Vello Render
      → blit_to_window()
```

### 3.3 视觉状态变化触发链路 (hover/pressed)

```
CursorMoved
  → build_hit_result()
  → EventManager::handle_mouse_move()
      → hover 状态变化 → needs_render
  → apply_event_effects() → request_render()
  → w.request_redraw()
  → RedrawRequested
  → take_rebuild_requested() = false
  → render_visuals()
      → frame_visual_update()
          → needs_render → build_render_tree()
      → VelloRenderer::render()
      → blit_to_window()
```

---

## 四、核心组件详解

### 4.1 ViewNode — UI 描述原语

```rust
pub enum ViewNode {
    Text {
        content: String,
        style: TextStyle,
        layout: FlexStyle,
        key: Option<String>,
        listener: Option<ViewListener>,
    },
    Image {
        data: Arc<Vec<u8>>,
        style: ImageStyle,
        layout: FlexStyle,
        key: Option<String>,
        listener: Option<ViewListener>,
    },
    Div {
        layout: FlexStyle,
        paint: PaintStyle,
        key: Option<String>,
        children: Vec<ViewNode>,
        listener: Option<ViewListener>,
    },
}
```

**设计要点**：
- **不可变性**：ViewNode 一旦创建不再修改，每次 rebuild 生成新树
- **三原语原则**：Text 表示文本，Image 表示图片，Div 表示一切容器（块级、flex、定位）
- **内联样式**：`layout` 直接持有 `FlexStyle`，`paint` 直接持有 `PaintStyle`，无 CSS 继承
- **key 机制**：`key` 为 Reconciler 提供稳定标识，确保跨 rebuild 匹配同一逻辑节点

**ViewListener**：

```rust
pub enum ViewListener {
    Click(Rc<dyn Fn()>),
    ClickWithCtx(Rc<dyn Fn(&mut EventContext)>),
}
```

- 通过 `Rc::ptr_eq` 判断相等性，用于 `tree_eq()` 比较
- 生命周期由 ElementTree 管理，`update_node()` 时更新，`remove()` 时自动清理

**tree_eq() 比较策略**：

```rust
fn tree_eq(&self, other: &ViewNode) -> bool {
    match (self, other) {
        // 逐字段比较，Image 使用 Arc::ptr_eq 避免大图片逐字节比较
        // listener 使用 Rc::ptr_eq 比较
        // 递归比较子节点
    }
}
```

### 4.2 Widget — 组件抽象

```rust
pub trait Widget {
    fn key(&self) -> Option<&str> { None }
    fn build(&self, ctx: &mut BuildContext) -> ViewNode;
    fn build_node(&self) -> ViewNode { self.build(&mut BuildContext::empty()) }
}
```

**设计要点**：
- **Builder 模式**：Widget 是配置对象，`build()` 将其编译为 ViewNode 树
- **无状态组件**：`build_node()` 快捷方法，适用于纯展示型组件
- **有状态组件**：通过 `BuildContext::use_state()` 持久化状态

**BuildContext**：

```rust
pub struct BuildContext {
    path: Vec<String>,
    hook_index: usize,
    state_map: Rc<RefCell<StateMap>>,
}
```

- `use_state()` 使用 `"{path.join("/")}#{hook_index}"` 作为状态键
- 每次 rebuild 创建新的 BuildContext，但 `state_map` 跨 rebuild 共享

### 4.3 ElementTree — 运行时实体树

```rust
pub struct ElementEntry {
    pub node: ViewNode,           // 类型安全的视图配置
    pub intrinsic: Cell<IntrinsicSize>,
    pub layout: Cell<ComputedLayout>,
    pub dirty: Cell<bool>,
    pub parent: Option<ElementId>,
    pub children: Vec<ElementId>,
    pub interact: Cell<ElementState>,
    pub listener: RefCell<Option<ViewListener>>,
    pub text_layout_cache: RefCell<Option<Arc<TextLayout>>>,
}

pub struct ElementTree {
    entries: SlotMap<ElementId, ElementEntry>,
    root: Option<ElementId>,
}
```

**设计要点**：
- **SlotMap 存储**：使用 `slotmap` crate，generational key 安全
- **ViewNode 的 children 被清空**：`create_from_node()` 中容器变体的 `children` 被清空，由 ElementTree 的 `children: Vec<ElementId>` 管理
- **交互状态**：`interact: Cell<ElementState>` 存储 hovered/pressed/focused
- **文本缓存**：`text_layout_cache` 复用 Parley 布局，`update_node()` 时清除

### 4.4 Reconciler — 差异协调

```rust
pub enum Patch {
    Create { parent: ElementId, position: usize, node: ViewNode },
    Update { id: ElementId, node: ViewNode },
    Remove { id: ElementId },
    Move { id: ElementId, parent: ElementId, position: usize },
}
```

**匹配策略**（按优先级）：
1. `key()` 匹配（最优先）
2. 同位置 `type_name` 匹配（位置优化）
3. 跨序 `type_name` 回退匹配

**设计要点**：
- **Create**：`ElementTree::create_from_node()` 创建新 ElementEntry，插入到父节点的指定位置
- **Update**：`ElementTree::update_node()` 更新 ViewNode 字段，标记 dirty，清除文本缓存
- **Remove**：`ElementTree::remove()` 递归删除子树所有 entries，释放 listener
- **Move**：`ElementTree::move_child()` 仅从原父节点移除并插入新位置，保留子树、交互状态、文本缓存
- **Apply** 后标记 `needs_layout = true`，触发布局重排

### 4.5 布局引擎

```rust
// LayoutContext::compute() 流程
fn compute(tree: &ElementTree, root: ElementId, viewport: Size) {
    let flex_root = FlexNode::from_tree(tree, root);  // ElementTree → FlexNode 树
    flex_root.layout(viewport);                         // Flexbox 计算
    write_layout(flex_root, tree);                      // 结果写回 ElementEntry::layout
}
```

**设计要点**：
- **Taitank 风格 Flexbox**：完整实现 flex-direction/flex-wrap/justify-content/align-items/align-content/align-self
- **flex-grow/shrink/basis**：flex_shrink 默认 1.0（CSS 标准）
- **文本测量**：`TextMeasure` 按约束宽度重新测量文本，支持换行
- **临时树**：FlexNode 树每帧临时构建，布局结果写回 `ElementEntry::layout`（持久化）
- **dirty 检查**：`has_dirty_node()` 跳过无变化时的布局计算

### 4.6 事件系统

```
Event::Click
  → capture_phase:  root → target (仅 ClickWithCtx 响应)
  → target_phase:   target 节点 (Click 停止传播, ClickWithCtx 继续)
  → bubble_phase:   target → root (仅 Click 响应)
```

**EventManager 状态管理**：

```rust
pub struct EventManager {
    hovered: Option<ElementId>,
    hovered_listeners: Vec<ElementId>,
    pressed_node: Option<ElementId>,
    pressed_listeners: Vec<ElementId>,
    focused: Option<ElementId>,
    mouse_capture: Option<ElementId>,
}
```

- `handle_mouse_move()`：更新 hover 状态，检测 hover 变化
- `handle_mouse_down()`：更新 pressed 状态，记录 `pressed_node`
- `handle_mouse_up()`：触发 `Click` 事件，清除 `pressed_node`
- `dispatch_three_phase()`：Capture 正向遍历 → Target → Bubble 反向遍历

**EventEffects**：

```rust
pub struct EventEffects {
    needs_rebuild: bool,
    needs_layout: bool,
    needs_render: bool,
}
```

- `apply_event_effects()` 消费 effects，分别触发 rebuild/layout/render

### 4.7 渲染管线

**VisualElement**：

```rust
pub enum VisualElement {
    Group { children: Vec<VisualElement>, clip_rect: Option<Rect> },
    FillRect { rect: Rect, color: Color, border_radius: f64 },
    FillBlurredRoundedRect { rect: Rect, color: Color, radius: f64 },
    DrawText { rect: Rect, layout: Arc<TextLayout>, color: Color },
    DrawImage { rect: Rect, data: Arc<Vec<u8>>, opacity: f32 },
}
```

**VelloRenderer 渲染**：

```
render(&elements)
  ├── 填充背景色 (gray)
  ├── for element in elements:
  │     render_element(element, &scene)
  │       ├── Group → push_clip_path / 递归 / pop_clip_path
  │       ├── FillRect → fill_rect
  │       ├── FillBlurredRoundedRect → fill_blurred_rounded_rect
  │       ├── DrawText → draw_text (parley layout)
  │       └── DrawImage → blit_image (alpha 预乘)
  ├── ctx.flush()
  ├── ctx.render_to_pixmap()
  └── image blit 后处理
```

**cv() 函数**（ViewNode → VisualElement 递归转换）：

```
cv(id, tree, &mut elements)
  → 读取 ElementEntry: node, layout, interact, text_layout_cache
  → 解析交互状态 (hovered/pressed → 选择 background_color)
  → 生成 VisualElement:
      ├── FillRect (背景)
      └── 子节点:
          ├── Div → 递归 cv() 子节点
          │     └── clip_content → Group { clip_rect }
          ├── Text → DrawText (使用 text_layout_cache)
          └── Image → DrawImage
```

---

## 五、关键设计决策

### 5.1 为什么是 ViewNode 三原语而不是 Widget 树？

| 方案 | 问题 |
|------|------|
| Widget 树运行时 | 需要存储大量 widget 类型信息，Rust 的 trait 对象/枚举导致复杂度过高 |
| ViewNode 三原语 | 仅三种变体，layout/paint 统一，Reconciler 只需处理少量类型 |

### 5.2 为什么是内联样式而不是 CSS 选择器？

| 方案 | 问题 |
|------|------|
| CSS 选择器/继承 | 需要运行时计算样式级联，增加每帧开销 |
| 内联样式 | Widget 层在 build 时完成样式计算，ViewNode 层只消费确定值 |

### 5.3 为什么 Builder 每次全量重建？

| 方案 | 问题 |
|------|------|
| 增量更新 | 需要脏追踪，增加复杂度 |
| 全量重建 | 简单可靠，Reconciler 通过 diff 最小化实际变更，Rust 的 `tree_eq()` 快速跳过无变化树 |

### 5.4 为什么是 SlotMap 而不是 Arena？

| 方案 | 问题 |
|------|------|
| Arena | 删除后无法复用索引，不支持 generational key |
| SlotMap | 支持 generational key，删除安全，迭代高效 |

### 5.5 为什么 event/callback.rs 被删除？

全局回调注册表（`CALLBACKS`）在 ElementTree 管理 listener 后不再需要。`ViewListener` 通过 `Rc` 存储在 ElementEntry 中，生命周期与 Element 绑定，`remove()` 时自动释放。避免了全局状态带来的副作用。

---

## 六、性能设计

### 6.1 缓存短路策略

| 层级 | 条件 | 行为 |
|------|------|------|
| `submit_view_tree` | `rebuild_requested = false` 且 `tree_eq()` 相同 | 跳过 Reconciler/Layout/Render |
| `submit_view_tree` | `rebuild_requested = true` | 跳过 `tree_eq()`, 直接 Reconciler |
| `perform_layout` | `has_dirty_node()` 为 false | 跳过布局计算 |
| `frame_visual_update` | `needs_render` 为 false | 返回空 Vec，跳过渲染 |
| `cv()` | `text_layout_cache` 命中 | 复用 Arc<TextLayout>，不重新创建 |

### 6.2 内存管理

| 对象 | 生命周期 | 分配方式 |
|------|---------|---------|
| ViewNode | 单帧（build → submit → reconciler → drop） | 栈/堆（Widget build 产生） |
| ElementEntry | 跨帧（Reconciler create/remove 管理） | SlotMap 存储 |
| FlexNode | 单帧（layout 计算完成后 drop） | 临时分配 |
| VisualElement | 单帧（render 完成后 drop） | Vec 分配 |
| TextLayout | 跨帧（缓存命中复用，update_node 清除） | Arc 共享 |
| ViewListener | 跨帧（与 ElementEntry 生命周期绑定） | Rc 共享 |

### 6.3 Debug 模式性能

Debug 模式下 Vello CPU 软件光栅化是主要瓶颈。每次状态变化（含 hover/pressed 视觉状态）都触发全量重绘所有元素。

**当前缓解**：
- `build_and_render()` 仅在 `viewport_changed` 时调用 `set_viewport()`
- `submit_view_tree()` 已知变化时跳过 `tree_eq()` 比较

**远期方案**：切换至 `vello_hybrid` 或 `vello` GPU 渲染器，Scene API 完全兼容。

---

## 七、测试策略

### 7.1 测试金字塔

```
      /\
     /  \       集成测试 (4 个)
    /    \      event_primitive, layout_engine,
   /      \     nested_listener, widget_button
  /────────\
 /          \  单元测试 (9 个)
/ Reconciler \
\ Renderer   /
 \ Runtime   /
  \ app     /
   \───────/
```

### 7.2 测试覆盖

| 组件 | 测试类型 | 覆盖场景 |
|------|---------|---------|
| Reconciler | 单元 | Move 重排、Move+Update、无变化 |
| Renderer | 单元 | Group clip 裁剪、alpha 预乘、不透明不变 |
| Runtime | 单元 | clip Group 生成、非 clip 平铺 |
| app | 单元 | 像素字节序格式验证 |
| 事件系统 | 集成 | 事件传播、ViewListener 回调、嵌套监听器 |
| 布局引擎 | 集成 | FlexNode 布局计算 |
| 组件 | 集成 | Button 构建 |

---

## 八、未来演进方向

### 8.1 近期 (P1)

- 补充 ElementTree 单元测试（create/remove/update/move_child）
- 补充 Widget 单元测试（Button/Checkbox/ListView）
- BuildContext 状态路径改用稳定 key

### 8.2 中期 (P2)

- Widget 级 dirty 标记（增量 rebuild）
- EventManager 状态统一到 ElementTree
- `vello_hybrid` GPU 渲染器切换

### 8.3 远期 (P3)

- 动画系统支持
- 无障碍支持
- 开发者工具集成