# LieUI 复杂度审计与架构简化方案

> 版本：v2.1 | 日期：2026-08-11 | 状态：**已定稿**（经 3 个原型 14 测试实证）
> 关联文档：`architecture.md`（现状基线）、`performance-refactor.md`（高性能重构）
> 本文 v2.1：**方案 B（Widget 树持久化模型）**，取代 v1.0 的 `State<T>`/`use_state` 双轨设计。
> v2.1 变更：确认"声明式为主（Vue 式组件化）"、**`State<T>` 彻底删除**、回调 `&self` 注入式 API、三个原型（14 测试）实证结论已固化。

---

## 〇、一段历史澄清：为什么会有 `State<T>` 和 `use_state`

在进入方案前，先厘清这两个概念的来源——它们**都不是 LieUI 的本意**，而是"每帧重建 Widget 树"这个设定的并发症。

- **`use_state` / `Stateful<T>`**（`src/widget/mod.rs`）：设计初衷是 **widget 自己持有 plain 数据字段，直接修改**。但当前 `Widget::build(&self, ctx)` 每次都被调用在一个**每帧重新 `new` 出来的 widget** 上——字段存不下跨帧状态，于是被迫用 `BuildContext` 里的 `StateMap = HashMap<String, Box<dyn Any>>` 按 `path#index` 兜底存取。**这是补丁，不是设计。**
- **`State<T>`**（`src/state.rs`）：设计初衷没有它。它只是"builder 闭包外要持有共享数据"时，`Rc<RefCell<T>>` + 自动 `request_rebuild()` 的一个便利包装。它自己的注释就写着"后续可以迁移到 `BuildContext::use_state`，但当前实现与现有示例兼容"（`state.rs:426-429`）——**它自己也承认是过渡物**。

> 两套机制的共同点只有一句：`set()` 时调 `request_rebuild()`。它们解决的是完全不同的两件事（跨帧持久化 vs 闭包外共享），却都被冠以"状态"之名，制造了"框架有状态管理"的错觉。

---

## 一、审计结论：为什么会变复杂

### 1.1 复杂度全景

LieUI 从 v2-rewrite 演进至今，逐步引入了多层 z-index 栈、Popup 父子级联、脏区合屏、SharedSurface 通道、多窗口 per-window 信号隔离等功能。这些功能**各自合理**，但它们的组合方式导致了结构性复杂度。

### 1.2 复杂度来源

| # | 位置 | 问题 | 影响 |
|---|------|------|------|
| **1** | `app.rs` | God Object：winit 事件循环 + 渲染调度 + 滚动拖拽状态机 + IME + 关闭守卫 + 合屏逻辑 | SRP 完全崩溃，`WindowContext` 承担 7 种职责 |
| **2** | `state.rs` | 名字欺骗 + 杂物堆：471 行中仅 45 行的 `State<T>` 跟"状态"沾边，且 `State<T>` 不是框架概念 | 所有"不知道该放哪"的代码都被塞进 `state.rs` |
| **3** | `runtime/mod.rs` | `frame()` 承担过多：Reconciliation + 层命令消费 + Popup 关闭 + 布局 + 渲染树构建，`cv()` 300 行内联 | 单方法跨 5 个 pass |
| **4** | `core/layers.rs` | LayerStack 爆炸：6 层 z-index + Popup 父子归属 + 命中测试内混 dismiss | `hit_test_top` 不再纯函数 |
| **5** | 重复渲染路径 | 3 个入口：`frame()` / `frame_render_only()` / `frame_visual_update()` | 合屏逻辑在 `present_frame()` 中重复 |
| **6** | Compositor 半成品 | `composite()` 方法是死代码，实际合屏在 `present_frame()` 里逐像素 `covered()` 判断 | 双通道合屏耦合在 `app.rs` |
| **7** | 滚动拖拽在 `app.rs` | `ScrollDragState` + 4 个方法约 200 行 | 业务逻辑污染框架入口文件 |
| **8** | 伪状态管理 | `State<T>` + `use_state` 双轨，两个都不是框架本意 | 误导性的"状态管理"概念 + 每帧重建导致的状态无处存放 |

### 1.3 根因分析

```
每帧重建 Widget 树
  → widget 字段存不下跨帧状态
    → 被迫引入 use_state（path#index 兜底）
  → builder 闭包外要共享数据
    → 被迫引入 State<T>（Rc<RefCell> + request_rebuild）
  → 两套机制都在喊 request_rebuild()
    → 全量重建 + tree_eq() 短路兜底
      → 用昂贵的比较弥补粗糙的信号粒度

隐式状态机（flags + queues）
  → frame() 需要理解全局顺序
    → 消费逻辑散落在 Runtime::frame() 的三个阶段
      → 新增功能（如 Popup 级联关闭）只能在现有缝隙中插入
```

**核心矛盾**：**"每帧重建 Widget 树"是两套伪状态机制的根源。** 只要 Widget 树不是持久的，状态就无处安放，`use_state`/`State<T>` 就永远有存在理由。要真正简化，必须**让 Widget 树跨帧存活、成为状态的唯一所有者**——这就是方案 B。

---

## 二、方案 B：Widget 树持久化模型（本方案核心）

### 2.1 核心思想

**Widget 树（组件/结构）一次构建、跨帧存活、持有 plain 可变数据；Element 树（视图/渲染）每帧从 Widget 树投影生成、纯数据、用完即弃。**

```
【现在：每帧重建 Widget 树】
builder() 每帧执行
  ├─ new 一棵 Widget 树（状态无处存放 → 逼出 use_state / State<T>）
  └─ build → ViewNode → reconciler diff 更新 ElementTree（复用 ElementId）

【方案 B：Widget 树持久 + Element 树投影】
builder() 只执行一次 → 构造持久的 Widget 树实例
  ├─ Widget 树跨帧存活，plain 数据在字段里，直接改
  └─ 变更时 rebuild：Widget 树 → 新 ViewNode → 新 Element 树（每帧，纯投影）
```

关键点：
- **Widget 树是"活的"**——它是可编程对象图。`Slider` 实例就是那个 `Slider` 实例，`value: f32` 字段直接改。
- **Element 树是"死的"**——它只是 Widget 树在某时刻的视图快照，给布局/渲染消费，每帧丢弃。

### 2.1.1 Widget 层形态决策：声明式为主（Vue 式）

> 决策（2026-08-11 与架构师确认）：**声明式为主，状态命令式藏在组件内部。**
> 否决了"命令式 widgetId（win32/GTK 式）"作为主模型，理由见下。

| 候选形态 | 状态归属 | 动态 UI | 结论 |
|---|---|---|---|
| 命令式（win32/GTK，按 widgetId `set_text`） | widget 实例 | 增删节点需手动 `add_child/remove_child`，繁琐 | 否决为**主模型**；仅作可选便利 API |
| 声明式（Vue，`build` 投影 + 实例缓存） | widget 实例 | 结构随数据自动投影，动态列表用实例缓存 | ✅ **采用为主模型** |
| 混合 | — | — | 声明式为默认，widgetId 句柄可选 |

**决策依据**：动态 UI 的增删若全走命令式，用户要手动管理控件树生命周期，丢失"UI 结构自动随数据变化"的声明式红利。而**纯声明式**配合"持久实例 + 纯投影"（方案 B 本身），在状态粒度上已比 Vue 更精确——widget 实例字段天然知道"谁变了"，无需 Vue 的响应式依赖追踪。

因此最终形态：**`build(&self)` 声明式投影为主**，状态用 `Cell`/`RefCell` 字段藏在组件内部；`widgetId`（即 `Rc<dyn Widget>` 句柄）作为可选能力暴露给偏好命令式的用户。

### 2.1.2 Vue 式组件化模型（决策确认）

> 架构师确认：目标是 **Vue 的"组件化"**（组件局部状态 + 可复用），**非性能魔法**（不要响应式自动追踪）。同时确认 **`State<T>` 彻底删除**——LieUI 不是 state-driven 框架，是 **component-driven 框架**。

Vue 的组件化三支柱映射到 LieUI：

| Vue 支柱 | LieUI 对应 | 状态 |
|---|---|---|
| 局部状态 `data`/`ref` | 组件 `Cell`/`RefCell` 字段 | ✅ 方案 B 已解决 |
| 复用：`props` 进 / `emit` 出 | 构建参数进 + `set_*_callback` 出 | 需明确（见下） |
| 模板声明式渲染 | `build(&self)` 纯投影 | ✅ 方案 B 已解决 |

**跨组件通信（Vue 的 props/emit）**——状态始终在父组件，子组件只收参数、发回调，不持有共享状态：

```rust
// 父组件：状态在父的字段里
struct FileList {
    selected: Cell<usize>,
}
// props 进：把当前选中值传进子组件
let item = FileRow::new()
    .selected(selected == i)                      // props：值进
    .on_click({ let f = this.clone(); move || f.select(i) });  // emit：回调出
// 子组件：只收参数 + 发回调，不持有状态
```

**关键**：`Cell` 字段是默认（90% 组件），跨组件靠参数+回调（9%），无全局 `State<T>`（0%，已删除）。真需要全局共享时，用户用 `Rc<Cell<T>>` 自己传——框架不管。

### 2.2 这个模型为什么能简化（且解决 1.3 的根因）

一旦 Widget 树持久、状态在 widget 字段里，**以下整套机制都可以砍掉**：

| 被砍掉的机制 | 现在的位置 | 为什么不再需要 |
|---|---|---|
| `use_state` / `Stateful<T>` | `widget/mod.rs:221` | 状态回到 `self` 字段，不再存 `StateMap` |
| `StateMap = HashMap<String, Box<dyn Any>>` | `widget/mod.rs:64` | 无跨帧值可存 |
| `State<T>` | `state.rs:430` | **彻底删除**（架构师确认）：状态归组件字段，非全局共享 |
| `Widget::key()` + reconciler 按 key 匹配 | `widget/mod.rs:72` / `reconciler.rs` | widget 实例本身稳定，child 引用不变 |
| reconciler 的 `Patch::Move` / `Create` / `Remove` | `reconciler.rs` | Element 树从 Widget 树整体重投影，无需 diff |

**简化路径**：`Widget 树 → 递归 build → Element 树`。因为结构稳定，投影是**确定性递归**，不是 diff。

**关于"重渲染"（诚实回答）**：Rust 无 JS `Proxy`，`Cell::set` 不会自动通知框架，所以**做不到 Vue 的"自动响应式"**。方案 B 用"纯投影 + `tree_eq` 短路"给出最诚实的等价物——`build` 是纯函数（读字段→产出 ViewNode），字段变化必然反映在输出里；未变化的 widget 输出相等，`tree_eq` 短路跳过重建。**状态归组件、投影纯函数、变化靠短路**，即 Rust 版的"响应式"，无需 version 号、无需依赖追踪。

### 2.3 Rust 生命周期约束（关键设计前提）

方案 B 必须在 Rust 的所有权/借用模型内成立。三个硬约束：

**约束 1：Widget 树必须是"同构"的——所有节点都是 `Rc<dyn Widget>`。**
Widget 树要持久持有、且能被父节点引用子节点，子节点不能拥有父节点（会造成环）。所以每个 widget 节点都用 `Rc` 包一层，树的组成靠 `Rc` 而非 `Box`：

```rust
pub struct Column {
    children: Vec<Rc<dyn Widget>>,   // 子节点：Rc，可被多分支引用
    spacing: f32,
}
```

**约束 2：`build` 需要一个统一入口，签名必须是 `&self`（不可变），字段用 `Cell`/`RefCell` 包裹可变态。**
如果 `build` 用 `&mut self`，那么一次投影只能遍历一次（借用冲突），且无法被多帧复用。因此可变字段用 interior mutability：

```rust
pub struct Slider {
    pub value: Cell<f32>,       // Copy 字段 → Cell（zero-cost，&self 可读写）
    pub dragging: Cell<bool>,
    on_change: RefCell<Option<Rc<dyn Fn(f32)>>>,  // 回调：RefCell + Rc，&self 可注入（见约束 3 说明）
}

impl Widget for Slider {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        // 读 self.value.get()，不需要 &mut self
    }
}
```

> 回调字段用 `RefCell<Option<Rc<dyn Fn>>>` 而非裸 `Option`：widget 被 `Rc` 包裹后不能 `mut self` 消费式注入，必须 `&self` 注入（原型 `prototype_events_and_mutation` 验证）。

**约束 3：每帧投影产出的 Element 树是独立的新对象，不持有对 Widget 树的引用。**
Element 树消费完即弃，避免两者生命周期纠缠。Element 树里的 `ViewNode` 通过**克隆配置**生成（`Slider` 把 `value` 拷进 `ViewNode`），不 borrow Widget。

> 结论：方案 B 不需要 unsafe，也不需要全局存储。只需要把 widget 树的节点类型统一为 `Rc<dyn Widget>`，把可变字段从 `Cell<T>` 持有，把回调从 `Rc<dyn Fn>` 持有。Rust 的借用检查天然保证：**Widget 树是唯一所有者，Element 树是纯投影，不可能互相引用。**

### 2.4 视图态（运行时状态）放哪

上一版担心的"Element 树每帧重建会丢 scroll/hover 状态"——方案 B 的答案是：**这些状态必须移进 Widget 实例**，Element 树只保留"布局计算缓冲"（可重建）。

| 状态 | 现在位置（`ElementEntry`） | 方案 B 归属 |
|---|---|---|
| `scroll_offset` | `element.rs:24` | Widget 实例字段（`Cell<(f32,f32)>`） |
| `content_size` | `element.rs:26` | Widget 实例字段（`Cell<(f32,f32)>`） |
| `interact`（hover/pressed） | `element.rs:21` | Widget 实例字段（`Cell<ElementState>`） |
| `text_layout_cache` | `element.rs:30` | 提升为全局文本缓存（按内容哈希），或保留在 Element 每次重建 |
| `layout` / `intrinsic` / `dirty` | `element.rs:16-18` | Element 树的布局缓冲，每帧重建，丢弃 |

> **布局结果**（`ComputedLayout`）是"计算出的"，不是"状态的"，可以随 Element 树重建。真正的**视图态**只有滚动偏移、交互 hover/pressed、焦点——这些全部移到 Widget 实例字段。

---

## 三、与 Rust 生命周期结合的 `Widget` 新签名

### 3.1 当前签名（`&self` + 每帧 new）

```rust
pub trait Widget {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode;
}
```

当前的问题是 builder 每帧执行、widget 每帧 new，`&self` 读到的永远是初始值。

### 3.2 方案 B 签名（`&self` + 持久实例 + `Cell` 字段）

```rust
// build 保持 &self（关键：允许跨帧复用、多次投影）
pub trait Widget {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode;
}

// 容器：子节点用 Rc<dyn Widget>，保证树可持久持有、可复引用
pub struct Column {
    children: Vec<Rc<dyn Widget>>,
    spacing: f32,
}

// 有状态的组件：可变字段用 Cell/RefCell，可读可改，不破坏 &self
pub struct Slider {
    pub value: Cell<f32>,
    pub dragging: Cell<bool>,
    on_change: RefCell<Option<Rc<dyn Fn(f32)>>>,  // 回调：RefCell，&self 注入
}
```

**为什么是 `Cell` 而不是 `Rc<RefCell<...>>`？**
- `Cell<T>` 是 `Copy` 类型字段的 zero-cost interior mutability，`&self` 可读写，无 borrow 运行时检查。
- 对象引用（如 `String`、`Vec`）用 `RefCell`。
- 关键在于：**状态归属 widget 实例，而不是散在全局 `StateMap` 里。** 一个 `Slider` 实例的 `value` 就是它的 `value`。

**字段类型策略（原型已验证）**：
| 组件类别 | 字段类型 | 原因 |
|---|---|---|
| 叶子组件（`Slider.value: f32`、`Button.hovered: bool`） | `Cell<T>` | `Copy` 字段 zero-cost，`&self` 可读写 |
| 对象字段（`String`、`Vec`、`InputState`） | `RefCell<T>` | 非 `Copy`，需运行期借用 |
| 回调字段（`on_click`/`on_change`） | `RefCell<Option<Rc<dyn Fn>>>` | 回调非 `Copy`，且 `&self` 可注入（见 3.3 的 API 修正） |
| 容器组件（`Column`/`VirtualList` 的"子实例缓存"） | `RefCell<RowCache>` | 需持有可变的 `HashMap<usize, Rc<dyn Widget>>` 实例映射，非 `Copy` |

> 关键发现（原型 `prototype_list_virtualization`）：容器组件需要一个**可变的子实例缓存**（`HashMap<usize, Rc<dyn Widget>>`），而 `build(&self)` 是 `&self`——必须用 `RefCell` 包这个缓存。所以**叶子用 `Cell`、容器用 `RefCell<缓存>`**，二者互补，都不破坏 `&self`。

> **回调字段用 `RefCell`**（原型 `prototype_events_and_mutation` 确认）：因为 widget 被 `Rc` 包裹后**不能再用 `mut self` 消费式 builder**（会 "cannot move out of an Rc"）。回调字段必须用 `RefCell<Option<Rc<dyn Fn>>>`，通过 `set_on_click(&self, ...)` 注入。

### 3.3 builder 形态：从"每帧执行"改为"一次构造"

```rust
// 现在：Fn(&mut BuildContext) -> Box<dyn Widget>，每帧执行
// 用 builder 消费式 `.on_click(mut self)` 注入回调
Application::new(config, move |_ctx| {
    Box::new(Column::new()
        .child(Button::new("+1").on_click({...})))
})

// 方案 B：返回一棵持久的 Widget 树（Rc 树），只执行一次
// ⚠️ 关键修正：widget 被 Rc 包裹后，回调不能用 `.on_click(mut self)` 消费式，
//    必须用 `&self` 注入式 `set_on_click(&self, ...)`。
let btn = Rc::new(Button::new("+1"));
btn.set_on_click(move || { /* 改字段或通知父 */ });  // &self 注入（回调字段是 RefCell）
let root: Rc<dyn Widget> = Rc::new(Column::new().child_rc(btn.clone()));
let app = Application::new(config, || root);
```

闭包从"每帧重建"退化为"**一次构建 Widget 树**"。之后每次 rebuild，框架直接对**这棵持久的树**递归 `build` 投影出新的 Element 树，不再重新执行 builder 闭包。

> **API 修正（原型 `prototype_events_and_mutation` 验证）**：方案 B 下 `on_click(mut self)` 消费式**不可行**（widget 被 `Rc` 包裹，`cannot move out of an Rc`）。统一改为 `&self` 注入式 `set_on_click(&self, f)` / `set_on_change(&self, f)`。这影响所有内置 widget 的回调 API 迁移（M3 里程碑）。

---

## 四、方案 B 的简化收益（对比现状）

| 维度 | 现状（每帧重建 + use_state + State<T>） | 方案 B（Widget 树持久 + Element 树投影） |
|------|------|------|
| 状态机制 | 双轨：`State<T>`（外部）+ `use_state`（内部） | **单轨**：widget 字段 |
| `StateMap` | `HashMap<String, Box<dyn Any>>` 全局兜底 | **删除** |
| reconciler | 按 key/type diff，`Patch::Move/Create/Remove` | **删除**，改为确定性递归投影 |
| `Widget::key()` | 必须给动态列表项加 key | **删除**，实例引用即身份 |
| 状态粒度 | `set()` 只能全量 `request_rebuild` | widget 字段改了，知道是哪个 widget 变了 → 局部重绘 |
| 视图态（scroll/hover） | 散在 `ElementEntry` | 收进 widget 实例字段 |
| 组件复用 | 靠 reconciler 复用 ElementId | widget 实例本身就是复用的 |

**最根本的简化**：删掉 `use_state`、`State<T>`、`StateMap`、reconciler 四个机制，换来**一个**模型——"widget 持有自己的数据，把自己的数据投影成视图"。

### 4.1 投影机制：Widget 树 → build → ViewNode → Element 树

方案 B 的核心运行时就是**递归投影**，一次"帧"的完整流程：

```
1. 用户交互（点击/拖动）
   → 组件方法改字段（Cell/RefCell，&self）
   → 组件或框架请求 rebuild

2. 投影（对持久的 Widget 树递归 build）
   Widget 树 root.build(&ctx)
     → Column.build → 遍历 children → child.build（递归）
        → Text.build / Button.build / Slider.build（读各自 Cell 字段 → 产出 ViewNode）
   → 得到一棵全新的 ViewNode 树（纯 DOM，每帧丢弃）

3. 去重（可选优化）
   → 若某 widget 的 ViewNode 与上一帧相等（tree_eq 短路），跳过该子树更新
     （build 是纯函数：同字段 → 同 ViewNode）

4. 下游消费
   → Element 树/布局/渲染消费 ViewNode（ViewNode 始终保持纯数据，不引用 widget）
```

**为什么不需要 reconciler**：
- 静态结构（`Column` 的 children 引用不变）→ 递归投影天然保留结构。
- 动态列表（`VirtualList`）→ 容器持 `RefCell<RowCache>`（`HashMap<usize, Rc<dyn Widget>>`），滚动窗口 `[first,last)` 变化时，实例命中缓存即复用（`Rc::ptr_eq` 验证身份稳定），纯投影可见窗口。
- 列表项增删 → `item_count` 变化，投影范围随之变化，已实例化的行保留（状态不丢）。

**诚实响应式**：Rust 无 Proxy，`Cell::set` 不自动通知。方案 B 用"纯投影 + tree_eq 短路"表达"谁变了"——字段变化必然反映在 `build` 输出，未变则输出相等被短路跳过。**无需 version 号、无需依赖追踪。**

---

## 五、`state.rs` 解构

`State<T>` 删除后，`state.rs` 剩下的全是信号路由和层/窗口操作——按职责拆出去：

```
state.rs (471 行)
│
├── ① 信号路由 (L13-L77)         → signal.rs（per-window 重建/重绘/关闭）
├── ② 层命令队列 (L79-L125)      → 并入 FrameCommand
├── ③ 层公开 API (L127-L242)     → layer.rs
├── ④ Popup 管理 (L244-L391)     → layer.rs + FrameCommand
├── ⑤ 窗口关闭 (L393-L424)       → signal.rs
└── ⑥ State<T> (L426-L470)       → **删除**（widget 字段取代）
```

`Stateful<T>`（`use_state`）在方案 B 中同样删除——它的职责（跨帧记忆）已被"widget 实例持久 + `Cell` 字段"取代。

---

## 六、FrameCommand 帧调度器（与方案 B 配套）

方案 B 让"谁变了"变得精确，但仍然需要一个统一入口把"改了什么 → 该重投影哪棵子树"表达出来。保留 v1.0 的 `FrameCommand` 管道，但**语义更简单**——命令不再需要 7 个 `TickResult` 变体，因为方案 B 下重建粒度是"子树"而非"整树"：

```rust
pub enum FrameCommand {
    /// 投影整棵 Widget 树 → 新 ViewNode 树（多数交互走这个）。
    Project,
    /// 仅重新投影指定 widget 的子树（widget 字段变化已知是哪个）。
    ProjectNode { widget_path: WidgetPath },
    /// 仅重绘（hover/pressed 视觉更新，不投影）。
    Redraw,
    /// 仅合屏（SharedSurface 脏区）。
    Composite,
    // ── 层操作 / 滚动 / 窗口 ──
    ShowLayer { ... }, HideLayer { kind }, ClosePopup { handle },
    ScrollBy { id, dx, dy }, ScrollTo { id, x, y }, ForceLayout,
    CloseWindow,
}
```

```rust
enum TickResult {
    Noop,
    Project,        // 投影一棵子树（方案 B 的核心粒度）
    LayoutAndRender,
    Close,
}
```

> 方案 B 之前 `TickResult` 的 `Rebuild` vs `FullRebuild` 差别（是否跑 builder）**消失了**——因为 builder 只执行一次，之后全是 `Project`（术语统一：见 §4.1 投影机制）。`Redraw`/`Composite` 不产生投影，仅重绘/合屏。

---

## 七、多线程 Compositor 评估

### 7.1 可行性分析

| 约束 | 说明 | 影响 |
|------|------|------|
| winit 单线程 | 窗口操作必须在事件循环线程 | 上屏（blit）必须回到主线程 |
| vello_cpu 非 Send | `RenderContext` / `Pixmap` 不实现 `Send` | 光栅化必须在主线程 |
| softbuffer | 表面操作需在 winit 线程 | 同 winit 约束 |
| SharedSurface buffer | `Arc<Mutex<Vec<u8>>>`，可跨线程 | **唯一可跨线程的部分** |

### 7.2 结论

**当前不适合全局多线程 Compositor**。但 SharedSurface 的 `Arc<Mutex<Vec<u8>>>` 已为"外部线程写像素"留余地——终端的数据解析和像素绘制可在子线程进行，主线程只做合屏。真正需要多线程的是 **阶段 C（Wayland C/S）**，那时 Compositor 是独立进程。

> 方案 B 的额外收益：Widget 树持久 + 纯数据投影，让"某棵子树在独立线程投影"在理论上更可行（子树是 `Rc<dyn Widget>`，可 `Send` 包装）。但本期不做，仅留口子。

---

## 八、实施计划

### 8.1 里程碑

| # | 内容 | 变更量 | 风险 |
|---|------|--------|------|
| **M0** | 拆分 `state.rs` → `signal.rs` + `layer.rs`，删除 `State<T>` | `state.rs` 删除，新增 `signal.rs` ~100 行 + `layer.rs` ~250 行 | 低：纯拆分，公开 API（show_modal 等）不变 |
| **M1** | 引入 `FrameCommand` + `FrameScheduler`，层 API 和 signal 内部推命令 | 新增 `frame.rs` ~200 行，`runtime/mod.rs` -100 行 | 低：内部重构，API 不变 |
| **M2** | 引入 `WidgetTree`：widget 节点改 `Rc<dyn Widget>`（新增 `child_rc`），builder 改为"一次构造" | 重写 `widget/mod.rs` 的容器结构，`app.rs` builder 调用点改 | **中**：容器从 `Box` 改 `Rc`，影响所有内置 widget 与 example |
| **M3** | widget 可变字段从裸 `T` 改 `Cell<T>`/`RefCell<T>`，回调改 `&self` 注入式（`set_on_click(&self)`），删除 `use_state`/`StateMap` | 25 个 widget 逐个迁移 | **高**：触及所有状态组件（Slider/Input/List 等）及其回调 API |
| **M4** | 用"确定性递归投影"替换 reconciler，删除 `Widget::key()`，容器引入 `RefCell<RowCache>` | `reconciler.rs` 删除，新增 `projection.rs` | **中**：已有原型验证"实例缓存 + 纯投影"可行，渲染管线核心替换仍需像素级回归 |
| **M5** | 视图态（scroll/hover）从 `ElementEntry` 移入 widget 实例字段 | `element.rs` 精简 | 中 |
| **M6** | 拆分 `cv()` 为 `render_tree.rs` 独立纯函数模块 | `runtime/mod.rs` -300 行 | 低：纯移动 |
| **M7** | Compositor 收窄 + 滚动拖拽独立 + `app.rs` 瘦身 | `app.rs` -350 行 | 低 |

> **建议顺序**：M0→M1 先落（低风险，立即收益）；M2→M3→M4→M5 是方案 B 的主体（需一个里程碑一个里程碑逐步替换）；M6→M7 收尾。

### 8.1.1 原型验证状态

方案 B 的最大未知数已用**三个独立原型**实证（`tests/prototype_widget_persistence.rs`、`tests/prototype_list_virtualization.rs`、`tests/prototype_events_and_mutation.rs`），**14 个测试全部通过**：

| 未知数 | 原型 | 验证结果 |
|---|---|---|
| 生命周期约束：`Rc<dyn Widget>` 树 + `Cell` 字段 `&self` 可改 + Element 树独立 | `prototype_widget_persistence`（5 测试） | ✅ 三个约束均成立 |
| 动态列表：滚动窗口变化复用实例、状态保留、增删处理、实例身份稳定 | `prototype_list_virtualization`（4 测试） | ✅ `RowCache` 缓存复用，无需 reconciler |
| 事件回调 + 字段修改 + ViewNode 纯 DOM | `prototype_events_and_mutation`（5 测试） | ✅ 回调经 `Rc` 捕获改字段、重投影反映、ViewNode 不引用 widget |

**三个关键实证结论**：
1. `M4` 风险降级：`reconciler` 可由"容器持 `RefCell<RowCache>` + 纯投影"取代。
2. **回调 API 修正**：widget 被 `Rc` 包裹后 `.on_click(mut self)` 消费式不可行，必须 `set_on_click(&self, ...)` 注入式（见 §3.3）。
3. ViewNode 始终保持纯 DOM：投影产出的 ViewNode 不引用 widget，drop widget 后仍独立可用（满足"保留 ViewNode 类 DOM"约束）。

### 8.2 不变量

- `show_modal` / `show_popup` / `hide_modal` 等公开 API 签名不变
- 渲染结果像素级不变（M4 替换 reconciler 后需全套回归）
- 布局/滚动/焦点行为不变
- `State<T>` 删除，example 改为"构造持久 Widget 树 + 直接改字段"

### 8.3 预期收益

| 指标 | 现状 | 方案 B 后 |
|------|------|-----------|
| 状态机制 | 双轨（State + use_state） | **单轨**（widget 字段） |
| `StateMap` / `use_state` / `State<T>` | 存在 | **全部删除** |
| reconciler + `key()` | 存在（按 key/type diff） | **删除**，改递归投影 |
| 重建粒度 | 全量 + tree_eq 短路 | 精确到子树（`ProjectNode`） |
| 渲染入口 | 3（frame / render_only / visual_update） | 1（FrameScheduler::tick） |
| 独立队列 | 3（PENDING_LAYER + PENDING_POPUP_HIDE + FLAGS） | 1（FrameCommand） |
| thread_local | 4 | 0（配合 Display 方案） |
| 视图态归属 | 散在 ElementEntry | widget 实例字段 |

---

## 九、与现有文档的关系

| 文档 | 关注点 | 本方案关系 |
|------|--------|-----------|
| `performance-refactor.md` | 性能：SharedSurface + 脏区 + 按需重建 | **正交 + 更优**：方案 B 提供精确子树重建，按需重建天然成立 |
| `architecture.md` | 设计哲学与模块划分 | **演进**：保持"声明式 + 纯数据投影"哲学，落到"Widget 树持久 + Element 树投影" |

**建议实施顺序**：先 M0→M1（低风险清理），再集中推进 M2→M5（方案 B 主体），完成后 M6→M7。方案 B 落地后，`performance-refactor.md` 的"按需重建"会自动变得简单，因为状态归属已收敛到 widget 实例。
