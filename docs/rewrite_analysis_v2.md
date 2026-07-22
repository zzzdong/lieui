# LieUI v2 重构：C/S 架构 + Builder 驱动设计

> 本文档从零分析 lieui v1 现状，提出 v2 的 C/S 架构设计。
> ——基于 Wayland 的 C/S 思想 + Rust 的所有权模型。

---

## 0. 现状回顾

### 0.1 v1 的"三棵树"架构

```
┌─────────────┐   ┌──────────────┐   ┌───────────────┐
│ WidgetTree  │──→│ LayoutTree   │──→│ RenderTree    │
│ (SlotMap)   │   │ (LayoutNode) │   │ (VisualElems) │
└──────┬──────┘   └──────────────┘   └───────────────┘
       │  ViewContext (God object — 待拆分)
       │  调度 layout / render / event
       ▼
┌──────────────────┐
│ EventManager     │  ← 三阶段事件传播
│   + EventContext │
└──────────────────┘
```

### 0.2 v1 的两个 API

```rust
// 模式 A：直接 API（过程式）
let root = ctx.create(Container::new());
let col = ctx.create(Column::new());
ctx.add_child(root, col);

// 模式 B：Builder 模式（声明式 + slot 匹配）
vc.set_build_fn(|bctx| {
    bctx.column(|bctx| {
        bctx.button("+", |ectx| { ... });
    });
});
```

### 0.3 v1 核心问题

| 问题 | 细节 |
|------|------|
| **Widget trait 双重职责** | 既是 UI 描述（label、style）又是运行时状态（hover、pressed） |
| **Builder 是"后天附加"** | `BuildContext` 通过 `slot_or_create` 匹配，内部仍在直接操控 WidgetTree |
| **ViewContext God Object** | 集 Builder、Runtime、Layout、Render、Event 于一身（已在 G 阶段待拆） |
| **Builder 与 Runtime 紧耦合** | Builder 函数直接持有 `&mut ViewContext`，可变借用范围过大 |
| **动态 UI 体验不够"原生"** | `for` 循环中的 widget 需手动传 explicit_key |
| **Slot 匹配脆弱** | 依赖 (parent_id, position_idx, type_tag)，增删中间节点会错位 |

---

## 1. Wayland C/S 模型如何映射到 lieui

### 1.1 Wayland 的工作方式

```
┌──────────┐   Unix Socket   ┌──────────────┐
│  Client  │ ←─── msg ────→ │  Compositor  │
│          │   (object IDs)  │   (Server)    │
└──────────┘                └──────────────┘
```

| Wayland 概念 | lieui v2 映射 |
|-------------|--------------|
| **Compositor (Server)** | **Runtime** — 拥有 ElementTree，管理布局/渲染/事件 |
| **Client** | **Builder** — 纯函数描述 UI，向 Runtime 发送"补丁" |
| **Unix Socket / 消息** | **Patch / Diff** — ViewTree 差异 → 增删改 Element |
| **Object ID (handle)** | **ElementId** — 所有操作通过 ID 进行 |
| **wl_surface** | **Element** — 运行时实体，持有状态和布局结果 |
| **wl_buffer / wl_shm** | **VisualElement** — 纯数据渲染描述 |
| **Events (compositor→client)** | **Event messages** — 输入事件转发到回调 |
| **Requests (client→compositor)** | **Patch ops** — 树操作请求 |

### 1.2 v2 的核心数据流

```
                    Builder 侧 (Client)                Runtime 侧 (Server)
                    ┌──────────────────────┐           ┌───────────────────────┐
  State<T> ───────→ │  fn(state) → View    │           │                       │
                    │         ↓            │           │     ElementTree       │
                    │     ViewTree         │──Patch──→ │   (SlotMap 存储)      │
                    │   (纯数据描述)         │           │         ↓            │
                    │         ↑            │  Events   │  Reconciler (diff)    │
  Event handlers ───│── callbacks ─────────│←───────── │  LayoutEngine         │
                    └──────────────────────┘           │  RenderEngine          │
                                                       │  EventManager          │
                                                       └───────────────────────┘
```

**关键原则**：Builder 侧是**纯数据**（`Send + Sync`），不持有任何 RefCell/Rc。所有可变状态在 Runtime 侧通过 ElementId handle 管理。

---

## 2. 核心类型设计

### 2.1 ElementId — 运行时 Handle

```rust
// 复用 slotmap 的 generational key（v1 的 WidgetId 可直接复用）
slotmap::new_key_type! {
    pub struct ElementId;
}

// ElementId 特点：
// - 数值型（u64），拷贝成本极低
// - 自带 generation（防悬垂指针）
// - 可跨线程传递（Copy + Send + Sync）
```

**对比 v1**：`WidgetId` → `ElementId`，概念上从"Widget"变为"Element"，强调它是存储在 Server 侧的运行时实体，而非用户侧的代码组件。

### 2.2 View — 用户侧纯数据描述（核心新抽象）

```rust
/// View = UI 的纯数据描述
///
/// 与 v1 Widget trait 的关键区别：
/// - View 是值类型（可 Clone、Send、Sync）
/// - View 不持有 RefCell / Rc 等运行时状态
/// - View 只描述"长什么样"，不负责"怎么工作"
///
/// 用户通过组合 View 来描述整个 UI。
/// 框架通过 `build()` 将 View 转换为 ViewTree（flat 数据结构）。
pub trait View {
    /// View 的运行时状态类型（可选，用于需要保持跨帧状态的场景）
    type State: Default;

    /// 构建 ViewTree 节点
    ///
    /// 返回当前 View 在 ViewTree 中的表示（类型 + 属性 + 子节点生成器）
    fn build(&self) -> ViewNode;
}
```

**v1 vs v2 的 Widget / View 对比**：

```rust
// v1: Widget trait — 描述 + 状态 + 逻辑 全部耦合
pub trait Widget: Any {
    fn layout(&self, id: WidgetId) -> LayoutNode;
    fn render(&mut self, layout: &LayoutNode, ctx: &ViewContext) -> Vec<LayeredElement>;
    fn handle_event(&mut self, event: &Event, ctx: &EventContext) -> EventResult;
    fn as_any(&self) -> &dyn Any;
}

// v2: View trait — 纯描述
pub trait View {
    fn build(&self) -> ViewNode;
}

// v2: Element — 运行时实体（仅框架内部，用户不可见）
// 存储在 Runtime::element_tree 中，通过 ElementId 操作
struct Element {
    type_name: &'static str,
    // 布局约束（从 ViewNode 解析而来）
    node: ViewNode,
    // 运行时脏标记
    dirty: Cell<bool>,
}
```

### 2.3 ViewNode — ViewTree 的扁平节点

```rust
/// ViewTree 的节点——纯数据，可 diff
pub struct ViewNode {
    /// 元素类型（"button", "text", "column"...）
    pub type_name: &'static str,
    /// 显式 key（用于 for 循环中的列表 diff）
    pub key: Option<String>,
    /// 具体属性值序列化
    pub props: PropMap,
    /// 子节点
    pub children: Vec<ViewNode>,
}

/// 属性 Map：可以按类型安全的方式存储任意属性
pub struct PropMap {
    entries: Vec<(&'static str, PropValue)>,
}

pub enum PropValue {
    Str(String),
    F32(f32),
    F64(f64),
    Bool(bool),
    Color(Color),
    // 事件回调以 "callback:<id>" 的形式存储，回调本身在 Builder 侧持有
    Callback(u64),
}
```

### 2.4 Patch / Reconciler — C/S 通信协议

```rust
/// Patch 描述了对 ElementTree 的原子操作
/// Runtime 接收 Patch，将其应用到 ElementTree
pub enum Patch {
    /// 创建新 Element（在 parent 下添加子节点）
    CreateElement {
        parent: ElementId,
        position: usize,
        type_name: &'static str,
        props: PropMap,
        key: Option<String>,
    },
    /// 更新已有 Element 的属性
    UpdateProps {
        id: ElementId,
        props: PropMap,
    },
    /// 移动 Element（在其 parent 的 children 中改变位置）
    MoveChild {
        id: ElementId,
        new_position: usize,
    },
    /// 删除 Element 及其所有子孙
    RemoveElement {
        id: ElementId,
    },
}

/// Reconciler：对比 ViewTree 和 ElementTree，生成 Patch 列表
pub struct Reconciler {
    // 维护 ViewTree ↔ ElementTree 的映射
    // 每次 build() 后对比，输出 Patch[]
}
```

---

## 3. 用户视角的 API 设计

### 3.1 基础示例

```rust
use lieui_v2::prelude::*;

fn main() {
    // === Runtime 侧 ===
    let mut app = Application::new();

    // === Builder 侧 ===
    let count = State::new(0);

    app.run(move |ctx| {
        // ctx: Context — 类似 v1 的 BuildContext，但不持有 &mut ViewContext
        // 它只负责构建 ViewTree

        ctx.column(|col| {
            col.text(format!("Count: {}", *count.get()))
                .font_size(48.0);

            ctx.row(|row| {
                row.button("-")
                    .on_click(|| count.update(|v| *v -= 1));

                row.button("+")
                    .on_click(|| count.update(|v| *v += 1));
            });
        });
    });
}
```

### 3.2 动态列表

```rust
let items = State::new(vec!["A", "B", "C"]);

app.run(move |ctx| {
    ctx.column(|col| {
        // for 循环中无需手动传 key——框架自动用 index + type 匹配
        for (i, item) in items.get().iter().enumerate() {
            col.text(item)
                .key(i)  // 可选：显式 key，让 diff 更精确
                .on_click(|| println!("clicked {}", item));
        }
    });
});
```

### 3.3 条件渲染

```rust
let show_detail = State::new(false);

app.run(move |ctx| {
    ctx.column(|col| {
        col.button("Toggle")
            .on_click(|| show_detail.update(|v| *v = !*v));

        // if 条件产生不同的 ViewTree，Reconciler 自动增删 Element
        if *show_detail.get() {
            col.text("Detail info here...");
        }
    });
});
```

### 3.4 自定义 View 组合

```rust
// 自定义 View = 纯函数，返回值实现 View trait
fn my_counter(count: State<i32>, label: &str) -> impl View {
    Column::new(vec![
        Text::new(format!("{}: {}", label, *count.get())),
        Row::new(vec![
            Button::new("-").on_click(move || count.update(|v| *v -= 1)),
            Button::new("+").on_click(move || count.update(|v| *v += 1)),
        ]),
    ])
}

// 使用：直接在 builder 中调用
app.run(move |ctx| {
    ctx.column(|col| {
        col.add(my_counter(count.clone(), "Counter"));
        col.add(my_counter(count.clone(), "Second"));
    });
});
```

---

## 4. Runtime 内部架构

### 4.1 Runtime 模块结构

```
src/
├── runtime/
│   ├── mod.rs           # Runtime struct — 核心编排器
│   ├── element_tree.rs  # ElementTree (SlotMap 存储)
│   ├── reconciler.rs    # ViewTree → Patch[]
│   └── scheduler.rs     # Patch 应用 + layout/render 调度
│
├── view/                # Builder 侧（纯数据）
│   ├── mod.rs           # View trait
│   ├── primitives.rs    # Text, Button, Container...
│   ├── layout.rs        # Column, Row, Flex 容器
│   └── context.rs       # Context (Builder 上下文)
│
├── state.rs             # State<T> (不变)
│
├── app.rs               # winit 集成 (精简)
│
├── layout/              # 布局引擎（从 v1 迁移）
│   └── ...
├── render/              # 渲染引擎（从 v1 迁移）
│   └── ...
├── event/               # 事件系统（从 v1 迁移）
│   └── ...
└── core/
    ├── id.rs            # ElementId
    └── ...
```

### 4.2 Runtime 主循环

```rust
pub struct Runtime {
    /// Element 树（所有运行时实体）
    element_tree: ElementTree,
    /// 布局上下文
    layout_ctx: LayoutContext,
    /// 事件管理器
    event_manager: EventManager,
    /// 三层（Base / Overlay / Modal）
    layers: Layers,
    /// 当前视口大小
    viewport: Size,
    /// 是否请求了 rebuild
    rebuild_requested: bool,
    /// Builder 侧发送来的 ViewTree（待 reconciliation）
    pending_view_tree: Option<ViewNode>,
}

impl Runtime {
    /// 接收 Builder 侧的 ViewTree，排入队列
    pub fn submit_view_tree(&mut self, view_tree: ViewNode) {
        self.pending_view_tree = Some(view_tree);
        self.rebuild_requested = true;
    }

    /// 一帧的逻辑
    pub fn frame(&mut self) -> Vec<LayeredElement> {
        // 1. Reconciliation：对比 ViewTree → ElementTree，生成 Patch
        if let Some(view_tree) = self.pending_view_tree.take() {
            let patches = self.reconciler.diff(&view_tree, &self.element_tree);
            self.apply_patches(patches);
        }

        // 2. 布局
        if self.needs_layout {
            self.perform_layout();
        }

        // 3. 收集渲染指令
        if self.needs_render {
            return self.collect_visuals();
        }

        Vec::new()
    }

    fn apply_patches(&mut self, patches: Vec<Patch>) {
        for patch in patches {
            match patch {
                Patch::CreateElement { parent, position, type_name, props, key } => {
                    let id = self.element_tree.create(type_name, props);
                    self.element_tree.insert_child(parent, position, id);
                }
                Patch::UpdateProps { id, props } => {
                    self.element_tree.update_props(id, props);
                }
                Patch::RemoveElement { id } => {
                    self.element_tree.remove(id);
                }
                Patch::MoveChild { id, new_position } => {
                    self.element_tree.move_to(id, new_position);
                }
            }
        }
    }
}
```

### 4.3 Reconciliation 算法

```
build() 产生的 ViewTree:
┌─────────────────┐
│ Column           │
│  ├ Text("A")     │  ← key=None, type="text"
│  ├ Text("B")     │  ← key=None, type="text"
│  └ Button("C")   │  ← key=None, type="button"
└─────────────────┘

当前 ElementTree:
┌─────────────────┐
│ Column           │
│  ├ Text("X")     │  ← id=0
│  └ Button("C")   │  ← id=1
└─────────────────┘

Reconciler 输出 Patches:
1. UpdateProps { id=0, props={text: "A"} }   // 类型匹配，复用 Element，更新属性
2. CreateElement { parent=Column, pos=1, type="text", props={text:"B"} }  // 新增
3. Patch 3: (Button 匹配，不产生操作)
```

**匹配规则**（比 v1 的 slot 更健壮）：

```
匹配优先级：
1. key 精确匹配（显式 key 时）：找同 key 的 Element
2. 类型匹配 + 位置匹配（无 key 时）：同类型的最近位置
3. 匹配失败 → 创建新 Element / 删除旧 Element

优势 vs v1 slot 匹配：
- v1: 依赖 (parent_id, position_idx, type_tag)，中间插入会错位
- v2: 使用类似 Flutter 的 canUpdate() 逻辑，更稳定
```

---

## 5. 事件流动：从 winit 到用户回调

```
winit event
    │
    ▼
App::window_event()
    │ 转换 Event 类型
    ▼
Runtime::handle_event(event)
    │ hit-test → 找到目标 ElementId
    │ 三阶段事件传播 (Capture → Target → Bubble)
    ▼
EventManager::dispatch(event, target_element)
    │ 在每个阶段调用 Element 的 event handler
    │ handler 是一个 callback ID（保存在 PropMap 中）
    ▼
EventContext::invoke_handler(callback_id, event_data)
    │ 回调 ID → 查找 Builder 侧注册的闭包
    ▼
User callback (e.g., on_click, on_change)
    │ 修改 State
    │ → request_rebuild()
    ▼
下一帧: submit_view_tree() → reconciliation → layout → render
```

**关键设计**：事件回调是 **Builder 侧**注册的闭包，通过 callback ID 与 Runtime 侧关联。Runtime 不持有闭包，只持有 ID。闭包存储在 Builder 侧的 Context 中。

```rust
// Builder 侧注册回调
button.on_click(|| {
    count.update(|v| *v += 1);
});

// 内部：给这个闭包分配一个 callback_id = 42
// 在 ViewNode.props 中保存: { on_click: PropValue::Callback(42) }
// Runtime 收到事件时，通过 callback_id 42 调回 Builder 侧的闭包

// Runtime 侧：不持有闭包
struct Element {
    type_name: &'static str,
    props: PropMap,      // 包含 callback IDs
    // ...
}
```

这样做的好处：
- **Runtime 是纯数据**（无闭包、无 RefCell）
- **Builder 侧是纯函数**（闭包在 Builder 闭包中被闭包捕获）
- **清晰的所有权边界**

---

## 6. v1 → v2 迁移策略

### 6.1 保留的部分

| v1 模块 | v2 中的位置 | 改动 |
|---------|------------|------|
| `geometry/` | 不变 | 完全保留 |
| `text/` | 不变 | 完全保留 |
| `state.rs` | 不变 | `State<T>` 完全保留 |
| `render/visual.rs` | 不变 | `VisualElement` 完全保留 |
| `render/engine.rs` | 不变 | `VelloRenderer` 完全保留 |
| `event/types.rs` | 不变 | `Event` enum 完全保留 |
| `layout/` | 小幅调整 | `LayoutNode`, `LayoutContext` 保留，适配 Element |
| `app.rs` | 精简 | `App` 结构体保留，内部使用 Runtime 替代 ViewContext |

### 6.2 替换的部分

| v1 模块 | v2 替换为 | 理由 |
|---------|----------|------|
| `widget/` (Widget trait) | `view/` (View trait) | Widget 双重职责 → View 纯描述 |
| `widget/tree.rs` | `runtime/element_tree.rs` | WidgetTree 耦合 Builder→ElementTree 纯 Server 侧 |
| `builder.rs` | `view/context.rs` + `runtime/reconciler.rs` | 拆分 BuildContext 为 Builder 侧 Context + Runtime Reconciler |
| `core/view_context.rs` | `runtime/` 多文件 | God Object → 分拆为 Runtime + Reconciler + Scheduler |
| `core/layers.rs` | `runtime/` 中保留 | 三层架构有价值，保留 |

### 6.3 分支策略

```bash
git branch v2-rewrite     # 新分支，基于 v2
# 保留 v2 分支上的现有代码作为参考
# 新分支从 main/v2 分叉，逐步重写模块
```

**实施阶段**：

```
Phase 1: 基础设施（1-2 天）
├── 创建 view/ 模块：View trait, ViewNode, PropMap
├── 创建 runtime/element_tree.rs
├── 创建 core/id.rs (ElementId)
├── 创建 state.rs (从 v1 复制)
└── 编写基础测试

Phase 2: Reconciler + Patch（2-3 天）
├── 实现 reconciler.rs (ViewTree → ElementTree diff)
├── 实现 Patch 枚举和 apply_patches
├── 实现 ElementTree (create/update/remove/move)
└── 编写 diff/apply 测试

Phase 3: Runtime 核心（2-3 天）
├── 实现 Runtime struct (调度 frame)
├── 移植 layout/ 适配 ElementTree
├── 移植 render/ 适配 ElementTree
├── 移植 event/ (EventManager)
└── 集成 Layers 三层架构

Phase 4: 原生 View 类型（2-3 天）
├── 实现 Text, Button, Container, Column, Row 等 View
├── 实现 Context (Builder 上下文)
├── 实现状态绑定 (State → View 自动更新)
└── 编写集成测试

Phase 5: App + winit 集成（1 天）
├── 精简 app.rs (使用 Runtime)
├── 实现 Application::run()
├── 移植 examples (builder_counter, builder_pdfkit)
└── 端到端测试

Phase 6: 文档 + 清理（1 天）
├── 更新架构文档
├── 清理 v1 旧代码
└── 用户迁移指南
```

---

## 7. 与 v1 的完整对比

| 维度 | v1 | v2 |
|------|-----|-----|
| **核心 trait** | `Widget` (描述+状态+逻辑) | `View` (纯描述) |
| **Builder 与 Runtime 关系** | 紧耦合（Builder 直接拿 `&mut ViewContext`） | 松耦合（Builder 产生 ViewTree，Runtime 消费） |
| **元素操作** | `WidgetId` + 直接方法调用 | `ElementId` + Patch 消息 |
| **树更新** | slot_or_create 内联匹配 | Reconciler diff → Patch 批量应用 |
| **Widget/Element 生命周期** | create → add_child → layout → render | build (ViewTree) → diff → patch → layout → render |
| **动态列表** | `explicit_key` 手动传 | 自动 key 匹配 + 位置感知 |
| **条件渲染** | 需手动 add/remove | if/else 产生不同 ViewTree，自动 diff |
| **事件回调位置** | Runtime 侧（EventContext 中） | Builder 侧（闭包 + callback ID 桥接） |
| **State 绑定** | `bind_text()` / `bind_progress()` | 自动（每次 build() 时读取 state） |
| **ViewContext** | God Object (640+ 行) | 拆分为 Runtime (调度) + Context (Builder 侧) |
| **增量布局** | dirty 标记 + O(dirty) | 保留（直接从 v1 迁移） |
| **VisualElement** | 保留 | 保留（不变） |
| **三层架构** | Base/Overlay/Modal | 保留（三层 + z-index） |
| **三阶段事件** | Capture/Target/Bubble | 保留（事件转发到 Builder 回调） |
| **并发** | 单线程 (RefCell/Rc) | Builder 侧纯数据，可 Send；Runtime 侧单线程 |

---

## 8. 关键设计决策记录

### 决策 1：为什么 View 不用 trait object？

```rust
// 不好的做法：
pub trait View { fn build(&self) -> ViewNode; }
let views: Vec<Box<dyn View>>;

// 好的做法（v2 采用）：
// View 是枚举 + 结构体的组合，避免 vtable 开销
pub enum ViewEnum {
    Text(TextView),
    Button(ButtonView),
    Column(ColumnView),
    Container(ContainerView),
    // 用户可通过自定义函数返回这些枚举
}
// 但用户组合是通过函数，而非 Box<dyn>：
fn my_view(count: State<i32>) -> impl View { ... }
```

实际做法：View 是结构体 + `build()` 方法，通过**函数组合**而非 trait object 组合。

### 决策 2：Event 回调如何跨 C/S 边界？

```
方案：Callback ID 方案（v2 采用）
- Builder 侧注册闭包 → 分配 u64 ID
- ViewNode.props 中存 Callback(u64)
- Runtime 侧收到事件 → 查找目标 Element → 读取 callback ID
  → 用 ID 调回 Builder 侧

这个方案比 v1 的 on_click(|ectx| ...) 更清晰：
- v1: 闭包存在 Widget 的某个字段中（Button 内部存 callback）
- v2: 闭包存在 Builder 侧的 CallbackRegistry 中，Element 只存 ID
```

### 决策 3：如何保持 v1 直接 API？

```
v2 不保留直接 API 模式。原因是：
1. 直接 API 和 Builder 模式维护两套使用路径，复杂度翻倍
2. 直接 API 通过 &mut ViewContext 操作，与 C/S 思想冲突
3. 需要直接 API 的场景可以用 Runtime::element_tree 的底层 API 替代

但提供 ElementTree 的底层访问 API（类似 v1 直接 API 但通过 handle）：
let rt: &mut Runtime = ...;
let id = rt.element_tree.create("button");
rt.element_tree.set_prop(id, "text", "Click");
// 这是底层 API，不是推荐用法
```

---

## 9. 风险与缓解措施

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| Reconciler diff 算法 Bug | 中 | 高 | 先写完整测试套件；从简单 ViewTree 开始测试 |
| 性能（每次 build 重建 ViewTree） | 低 | 中 | ViewTree 是纯数据，构建代价低；Reconciler 只输出最小 Patch |
| Builder 闭包中 State 访问复杂 | 中 | 中 | `State::get()` 已有自动解引用；builder 闭包每次重建时重新捕获 |
| 迁移成本高 | 中 | 中 | 保留 v1 分支；v2 逐步重写模块，未完成前不替换 v1 |
| Event 回调 ID 管理复杂 | 低 | 低 | CallbackRegistry 用 SlotMap 管理 ID（复用现有模式） |

---

## 10. 总结

**v2 的核心变革**：将 "Widget trait + 直接树操作" 的 v1 模式，转变为 "View 纯数据描述 + Runtime 服务" 的 C/S 模式。

```
v1: 用户代码 ──(直接操作)──→ WidgetTree
v2: 用户代码 ──(生成 ViewTree)──→ Runtime Reconciler ──(Patch)──→ ElementTree
```

**保持不变的优秀设计**：
- `State<T>` 共享状态系统
- `VisualElement` 纯数据渲染指令
- `LayoutNode` 两阶段布局 + 增量 dirty 标记
- 三层架构 (Base/Overlay/Modal)
- 三阶段事件传播 (Capture/Target/Bubble)
- SlotMap 存储 (ElementId 即 WidgetId 更名)

**新引入的关键设计**：
- `View` trait — 纯数据 UI 描述
- `Reconciler` — ViewTree ↔ ElementTree diff 引擎
- `Patch` 协议 — C/S 通信契约
- `Runtime` — 剥离出来的"服务端"核心
- `Callback ID` — 跨边界的事件回调桥接
