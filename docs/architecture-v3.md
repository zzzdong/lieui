# LieUI v3 架构设计：Retained MVP

> 版本：v3.4-draft | 日期：2026-09-30 | 状态：**设计待评审**
> 基线：`feature/mvp`（v2-rewrite 现状）
> 参考：`docs/architecture.md`（现状）、`refactor/dirty-surface:docs/{complexity-audit-and-simplification,performance-refactor,dirty-surface-review}.md`（两次改造尝试）、`newworld`（多 crate 布局里程碑）、WinUI `UIElement`/`FrameworkElement`（§3.13）
> 本文目标：**非立即模式 + 纯 CPU 渲染 + Rust 惯用法 + 响应式 MVVM + 大幅简化**
> **定位（重要）**：lieui 是**面向简易 GUI 的库**。响应式只做 **R1 级**——`Signal` 值变更 → 标脏 → 重跑 `view()` → 对齐。
> **明确不做**：细粒度依赖追踪、属性级绑定观察者、片段作用域、通用 diff、虚拟化超大列表（§七）。
> v3.4 变更（按评审意见）：① 新增 §3.14「多窗口」——`Runtime`/`Signal` 为 App 级共享，`Track`/渲染器/脏标志每窗口一套；
> `App` 通过 `Rc<dyn WindowView>` 擦除，**因此连 `App` 都不带泛型**；② 新增 §3.15「层」——Modal / Overlay(水印) / Popup / Tooltip / DragPreview
> 均为 `view()` 内**声明式嵌套**的根，z = (Layer, 嵌套深度, 序号)，删除父子句柄图与全部层命令队列；③ §九 补多窗口 + Modal + 水印示例。
> v3.3 变更（**推翻 v3.2 的 Msg 方案**）：① 用户态改为**响应式**——状态是 `Signal<T>`，`view(self: &Rc<Self>, v: &mut View<'_>)`，
> 事件用**闭包**直接改状态；删除 `Msg` / `update()` / 类型擦除绑定表 / 消息收集；② 框架侧新增 `reactive.rs`（`Runtime` + `Signal`，R1 只有脏标志）；
> ③ `if` 分支回归普通 Rust（删掉 `if_else`），`keyed_list` 只解决"列表身份"；④ `View` **不再泛型**；
> ⑤ 新增 §九 示例（API 契约）。对齐 / patch / 脏区 / 路由事件 / WinUI 对齐（§3.4、§3.13）沿用 v3.2 结论。
> v3.2 变更：patch 移出用户态；`view()` 不在每帧跑；`Kind` 字段分 desc/state 两组；窄逃逸接口；复用来源改为当前分支抽用。
> v3.1 变更：节点属性集与事件模型对齐 WinUI `UIElement`；路由事件 + `handled`/`handledEventsToo` + `Cmd`；视图态改为每节点 `interaction`。

---

## 〇、五条需求 → 设计约束

| 需求 | 直接推论 |
|---|---|
| **1. 不做立即模式（为纯 CPU 渲染服务）** | UI 必须**跨帧存活**，状态变化只能**增量 patch**，脏区必须**精确可知**。CPU 光栅化成本 ∝ 重绘像素面积，立即模式每帧重建 draw list + 全量光栅化，窗口越大越不可行。 |
| **2. 符合 Rust 生命周期限制与惯用法** | 树用 **arena + Copy 句柄**（`NodeId`），不用 `Rc<dyn ...>` 树、不造环、不自引用；统一 `&mut Track` 访问；**不引入 thread_local 全局单例**；泛型只留在必要边界。 |
| **3. 倾向 MVP / MVVM** | 明确 Model（`Signal<T>` 字段）/ View（保留树 + `view()`）/ ViewModel（命令方法 + 事件闭包）三层。**响应式**：改状态 → 自动重跑 `view()` → 对齐/patch；**patch 不进用户态**（§3.3/§3.4）。 |
| **4. 简化架构（最简优先）** | 只做 **R1 级响应式**（值变更 → 重跑 `view()`，**不追踪依赖**）。删掉 reconciler、`key` 匹配、`StateMap`、`State<T>`、`Msg`/`update`、三帧入口、全局信号表、层命令队列。 |
| **5. 支持多窗口 + 层（Modal / Overlay / 水印）** | `Runtime`/`Signal` 是 App 级（跨窗口共享），`Track`/渲染器/脏标志**每窗口一套**（§3.14）；Modal / Overlay(水印) / Popup / Tooltip / DragPreview 都是 `view()` 里**声明式嵌套**的层（§3.15），不用父子句柄图与命令队列。 |

---

## 一、现状诊断（基于源码实测）

### 1.1 一棵 UI 被表示 4 次

```
每次状态变化：
builder() 闭包 ──每帧重建──▶ Widget 树（Box<dyn Widget>，瞬时）
   └─ build(&self, ctx) ──▶ ViewNode 树（纯数据 IR，瞬时）        【表示 1、2】
        └─ Reconciler::diff/apply ──▶ ElementTree（SlotMap，retained）【表示 3】
             └─ cv() 递归 ──▶ Vec<LayeredElement>（每帧展开）      【表示 4】
```

证据：`src/widget/mod.rs:68-105`（`Widget::build`）、`src/view/node.rs:260-283`（`ViewNode`）、
`src/runtime/element.rs:14-36`（`ElementEntry`/`ElementTree`）、`src/runtime/mod.rs:451-731`（`cv()`，约 280 行）、
`src/runtime/reconciler.rs`（322 行）。

**代价**：每次状态变化都要跨 4 个表示搬运数据；reconciler 的存在完全是"每帧重建 Widget 树"的补偿（`key` 匹配、`Patch::Move`、`tree_eq` 全树比较、`listeners_sig_eq` 只比回调形态的 hack）。

### 1.2 状态三轨并存

| 机制 | 位置 | 问题 |
|---|---|---|
| `State<T>`（`Rc<RefCell<T>>` + 自动 `request_rebuild`） | `src/state.rs:430-470` | 外部共享状态；`set()` 只能全量重建 |
| `use_state` / `StateMap`（`HashMap<String, Box<dyn Any>>`，键 = `path#hook_index`） | `src/widget/mod.rs:62,219-234` | 字符串路径脆弱，缺失/错位直接 `expect(...)` panic（`mod.rs:248,251`） |
| `ElementEntry.interact / scroll_offset / content_size` | `src/runtime/element.rs:21-26` | 视图态散在运行时，且靠 `set_hovered_state` 批量写一条路径 |

同一个组件内部交互态被两种机制瓜分：`Slider.dragging` 用 `use_state`（`widget/slider.rs:71`），
`ScrollBar.captured` 用 build 内新建的 `Rc<Cell<bool>>`（`widget/scroll_bar.rs:103-104`）——后者每次 build 都重置，
证明"就地 Cell + 每帧重建"这条路走不通。

### 1.3 信号四件套 + 广播兜底

`state.rs` 内：全局 `static FLAGS: Mutex<HashMap<WindowId, Flags>>`（28-29）+ 4 个 thread_local
（`CURRENT_WID` 21-24、`PENDING_LAYER` 79-80、`NEXT_POPUP_HANDLE` 289-290、`PENDING_POPUP_HIDE` 379-380）；
`request_*` 在"无当前窗口"时**广播到所有窗口**（68-77）。

### 1.4 三个帧入口 + 补丁

`Runtime::frame`（`runtime/mod.rs:84-224`，约 140 行）/ `frame_render_only`（228）/ `frame_visual_update`（235），
外加"仅视觉分支在 redraw 只置标记时需补一次 `request_render()` 兜底"（`app.rs:646-651`）。

### 1.5 图层栈职责混杂

`LayerKind` 6 个变体 + 每类自增 `seq`；`LayerStack` 同时持有 `ElementTree`、`EventManager`、
`entries`、`popup_parent/children` 图（`core/layers.rs:207-229`）；
`hit_test_top` 在命中测试里**顺带做 dismiss 关闭**（`layers.rs:594-632`），已非纯函数。
死代码/半成品：`layer_has_content`(570)、`z_range`(53)、`dispatch_order`(58)、`set_visible`(375)、`update_view`(381)、`reanchor`(387)。

### 1.6 God Object 与同构性

- `app.rs` 1020 行，`WindowContext` 承担 7 种职责（`app.rs:92-116`），`window_event` 约 226 行（335-561）。
- `Widget` 层：`.on_click` / `.on_click_with_ctx` 在 **9 个文件**里逐字复制；回调字段两种存法
  （`Vec<Listener>` vs `Option<Rc<dyn Fn(..)>>`）；`.builtin()` 内置行为散在 8 个文件里以闭包形式塞进树。

---

## 二、两次改造尝试的得失

### 2.1 方案 B（`complexity-audit-and-simplification.md`，Widget 树持久化）

**对的**：诊断准确（每帧重建是两套伪状态机制的根源）；方向正确（状态归组件、删 reconciler、
删 `key`、单轨状态）；有 3 个原型 14 测试实证。

**不采纳的部分**：
1. 仍是 **"每帧递归投影 + `tree_eq` 短路"**：每次变化仍产出整棵 `ViewNode` 树，脏区只能靠元素签名哈希
   比较推断（`dirty-surface-review.md:176` 的 B5′），无法定位则**保守退化为整屏**。对 CPU 渲染不利。
2. `Rc<dyn Widget>` 树 + `Cell/RefCell` 字段 + `&self` build：为保住"每帧重建"这一前提而引入大量
   interior mutability。自身原型已证明消费式 `.on_click(mut self)` 不可行，必须改 `&self` 注入
   （"cannot move out of an Rc"），容器还要额外 `RefCell<RowCache>`——**API 变别扭，机制个数没降**。
3. 未触及图层/信号/帧入口的简化（新增的 `FrameCommand` 队列又是一套机制）。

**结论**：v3 采纳其**声明式投影**主张与"状态归组件、删 `key`/reconciler"的理念（v3.2 起更是全盘采纳声明式用户态），
但**放弃"每帧重建"这个前提**——`view()` 只在状态变更后跑，而 hover/滚动/动画/每帧都不经过它。
一旦不再每帧重建，所有为它打的补丁都不需要了：保留树由**框架 arena** 持有（不是 `Rc<dyn Widget>`）、
视图态在**保留树**（不是 widget 的 `RefCell` 字段）、描述由框架分配复用（不是每帧 `new` 一棵 `ViewNode` 树）、
对齐只需"位置 + 一层 key"（不是按 key/type 做通用 diff）。

### 2.2 dirty-surface（compositor 尝试）

**已兑现的技术资产**（应吸收进 v3 渲染层）：
- `present_with_damage(&[Rect])` 部分上屏 + `age()==0` / 脏区 ≥70% 退化全量（`app.rs:blit_to_window`）；
- 持久 `Pixmap`（免每帧 `Pixmap::new` 的分配+清零，1080p ≈ 8MB）；
- vello_cpu 0.2 局部光栅化方案已**验证可行**：`RasterizerSettings.offset` + `PixmapMut::new(w, band_h, &mut bytes[y0*w*4..])` + `ctx.set_transform(translate(0,-y0))`；
- 像素契约：backing/UI = premul，外部 surface = straight + src-over 混合 + `a==255` memcpy 快速路径；
- 残影处理：surface 移动/消失时把**旧矩形**并入脏区由 UI 回填；
- `blit_image` 需增加 y 偏移参数（配合条带光栅化）；
- 多线程光栅化（feature `parallel` → `vello_cpu/multithreading`，`LIEUI_RENDER_THREADS`）。

**不采纳的部分**：它是**架在旧架构上的旁路**——`Compositor`/`SharedSurface`/`ExternalSource` 之外，
旧的 rebuild 全链路与三帧入口原样保留，复杂度是净增；且脏区在源头上仍是"猜"（签名比较）。
`external.rs` 的全局 `OnceLock` 单例 `wake()`、thread_local surface 注册表、`SurfaceEntry` 双通道混排都应重新设计。

### 2.3 newworld（多 crate 重构，M1 布局里程碑）

**可直接搬用的基础设施**（这是 v3 的地基）：
- `crates/lieui-layout`（零依赖）：`LayoutTree` trait（`style_of/collect_children/measure/is_text/scroll_offset`，全部 `&mut self`，
  允许实现方持有 props 缓存与文本测度缓存）、`FlexNode/FlexStyle/ComputedLayout`（含 `local_x/local_y` 与 `avail_w/avail_h` 回显）；
- `crates/lieui-text`：parley 两段式测度（measure 缓存 + layout 只跑一次 align）、FNV 确定性哈希、FIFO 淘汰、命中率统计；
- `crates/lieui-core` 的 `arena.rs` / `id.rs`（`NodeId = index:32 | gen:32`，`NULL = u64::MAX`）/ `tree.rs`
  （`NodeFlags` 位标志、sibling 指针遍历、`ancestors/descendants/depth` 迭代器、`detach/set_children/destroy_subtree`）；
- `window.rs` 的**重排边界**概念（根、或宽高均 definite 的节点；`mark_layout_dirty` 冒泡到边界；`run_layout` 最多 2 轮）。

**不采纳的部分**：`props/` 的列式 `PropertyStore` + 4 层（ANIM>LOCAL>STYLE>DEFAULT）+ 继承 + 伪类。
那是为 CSS 式样式系统准备的，对 MVP 是过度设计——memory 记录的"继承 bug（只有 INHERITABLE_SLOTS 才向上走）"
正是它带来的复杂度。v3 用**内联样式 + 组件默认样式 + 主题快照**（即现状做法）。

---

## 三、v3 设计：Retained MVP

### 3.1 一句话

**一棵跨帧存活的视图树（arena + `NodeId`，含视图态）+ `Signal<T>` 状态（改值自动标脏）+ 用户态 `view(self: &Rc<Self>)` 描述 + 框架内部把描述对齐为 patch（精确脏区）+ 脏区 CPU 渲染。**

```
┌─────────────────────────────────────────────────────────────┐
│ app     窗口表（多窗口）+ winit 事件循环 + 帧调度 + softbuffer  │
│         每窗口一套：Track / Renderer / Surface / 脏标志         │
├─────────────────────────────────────────────────────────────┤
│ reactive  Runtime（per-App，单线程，App 的字段，跨窗口共享）    │
│           ├ R1：每个窗口一个脏标志；Signal::set 保守置位所有窗口│
│           └ Signal<T>：Rc 句柄（值槽 + Rc<Runtime>），无 thread_local │
├─────────────────────────────────────────────────────────────┤
│ ui      Track（每窗口一棵保留树，arena，含视图态与布局结果）    │
│         ├ 层栈：Content / Overlay / Popup / Tooltip / Modal /  │
│         │        DragPreview —— **声明式嵌套**，z 由嵌套深度决定│
│         ├ Align：描述 → 按位置/一层 key 对齐 → 内部 patch      │
│         ├ 视图态：interaction / scroll / 组件内部态（框架拥有） │
│         ├ 事件分发：Routing(Tunnel/Target/Bubble) + handled     │
│         │   处理器 = 闭包（Rc<dyn Fn(&mut Ctx)>），改状态用      │
│         └ Cmd 命令缓冲（唯一的延迟写入通道，框架内部用）        │
│         View<'_>：用户产出的**描述**（框架分配的 arena，复用）  │
├─────────────────────────────────────────────────────────────┤
│ layout  Flex 引擎（LayoutTree trait 直吃保留树，脏边界重排）    │
│ render  Scene 展开（只读）+ vello_cpu + 持久 Pixmap + 脏区上屏  │
├─────────────────────────────────────────────────────────────┤
│ text    两段式测度（复用现有 parley 封装）                     │
│ geom    Point/Size/Rect/Color（复用）                          │
├─────────────────────────────────────────────────────────────┤
│ 用户层  跨窗口共享状态（Signal）+ 每窗口一个 VM{ view() }       │
│         **不出现 NodeId、不出现 patch 调用、不需要 Msg**        │
└─────────────────────────────────────────────────────────────┘
```

- **patch 是框架内部机制**；用户态只做两件事：声明视图（`view`）+ 在事件闭包里**改状态**（`Signal`）。
- **R1 的响应式只有一条回路**：`Signal::set/update` → 置位脏标志 → 帧开始重跑该窗口的 `view()` → `align` →
  只把**真正变化的字段**落到保留树（逐字段比）→ 精确脏区。
- `Signal` **不追踪依赖**、**不用 thread_local**、**没有全局单例**：它自带 `Rc<Runtime>`，所以 `get()/set()` 在任何位置都能自己找到 runtime。
- **多窗口**：`Runtime` 与 `Signal` 是 App 级（跨窗口共享）；`Track` / 渲染器 / 脏标志是**每窗口**的。详见 §3.14。
- **层**：Modal / Overlay(水印) / Popup / Tooltip / DragPreview 都是**在 `view()` 里声明出来的根**，
  且**在其父层内部声明**——z 由嵌套深度决定，父层消失时子层自然一起消失（不需要父子句柄图）。详见 §3.15。
- **保持两棵树的角色分工**：`Track` 是唯一所有者（视图态 + 布局结果），`View<'_>` 只是"某时刻的描述"，
  由框架分配、跨次复用缓冲，**用完即弃**。`cv()` 那一步退化为"只读展开 draw list"。

### 3.2 核心类型

```rust
// ── 句柄：Copy + 'static，无生命周期参数，无 Rc，无引用 ──
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(u64);            // index:32 | gen:32；NULL 永不匹配存活节点

// ── 保留树：每窗口一棵，唯一所有者，所有访问经 &mut Track（§3.14）──
pub struct Track {
    nodes: Vec<Option<Node>>,      // slot 复用 + generation 自增
    free:  Vec<u32>,
    roots: Vec<Root>,              // 多根 = 层栈（见下与 §3.15）
    // 每窗口的视图态：hover_path / captures / focused / scroll 等（§3.5）
}

/// 一个层条目（= 一个根）。`owner` 记录"声明它的那个层条目"，
/// z 由 (layer, 嵌套深度, seq) 决定；**父层消失 → 整棵子树（含嵌套的子层）一起消失**。
pub struct Root {
    pub node: NodeId,
    pub layer: Layer,
    pub owner: Option<RootId>,
    pub opts: LayerOpts,
}

/// 层类型（z 顺序即枚举顺序）。6 个值 + 嵌套，替代旧的
/// `LayerKind`(6) + 每类 `seq` + `popup_parent/children` 图 + 2 个待处理队列。
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Content,       // 每窗口恰一个：用户 `view()` 的最外层内容
    Overlay,       // 装饰/水印/遮罩：**默认命中穿透**
    Popup,         // 锚定浮层（菜单/下拉）：默认点击外部关闭
    Tooltip,
    Modal,         // **默认 backdrop + 阻断下层**
    DragPreview,   // 拖拽预览：最高
}

pub struct LayerOpts {
    pub backdrop: Option<Color>,         // Modal 半透明遮罩
    pub blocks_below: bool,              // 阻断下层命中（Modal 默认 true）
    pub dismiss_on_outside_click: bool,  // Popup / Tooltip 默认 true
    pub hit_test_visible: bool,          // Overlay(水印) 默认 false
    pub anchor: Anchor,                  // Popup / Tooltip：按 key 在布局后解析
    pub focus: FocusPolicy,              // BlockBelow / Dismissable / Transparent
}

// ── 节点：视图描述 + 视图态 + 布局结果，三者归位一处（属性集对齐 WinUI UIElement，见 §3.13）──
pub struct Node {
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    kind: Kind,                    // 组件枚举（见下）
    layout: FlexStyle,             // 复用现有 layout/style.rs
    paint: PaintStyle,             // 复用现有 view/paint.rs（含 hover_/pressed_ 变体）
    text: TextStyle,

    // ── UIElement 对齐的可视/命中属性 ──
    visibility: Visibility,        // Visible | Hidden（占位不画）| Collapsed（不参与布局）
    hit_test_visible: bool,        // ≈ UIElement.IsHitTestVisible
    opacity: f32,                  // ≈ UIElement.Opacity（组不透明度，绘制时需 save layer）
    clip: Option<Rect>,            // ≈ UIElement.Clip
    transform: Transform,          // ≈ RenderTransform + Origin/CenterPoint/Translation/Scale/Rotation
                                   //   绘制/命中/裁剪共用同一矩阵（现状 hit test 完全不做变换，是缺口）
    cursor: Option<Cursor>,        // ≈ UIElement.ProtectedCursor
    allow_drop: bool,              // ≈ UIElement.AllowDrop
    can_drag: bool,                // ≈ UIElement.CanDrag
    layout_rounding: bool,         // ≈ UIElement.UseLayoutRounding（CPU 渲染需半像素对齐）
    tab_stop: bool,                // ≈ IsTabStop
    tab_index: i32,                // ≈ TabIndex

    // ── 视图态（≈ Control.IsPointerOver / IsPressed + IsEnabled）──
    interaction: InteractionState, // { pointer_over, pressed, focused, focus_state, enabled }
    focus_state: FocusState,       // Unfocused | Pointer | Keyboard | Programmatic

    // ── 事件处理器（`view()` 声明、对齐时整体替换；闭包只改状态，不改树）──
    handlers: SmallVec<(EventKind, Handler)>,   // Handler = Rc<dyn Fn(&mut Ctx)>

    // ── 布局结果（≈ DesiredSize / ActualOffset / ActualSize）──
    flags: Flags,                  // MEASURE_DIRTY | ARRANGE_DIRTY | PAINT_DIRTY | ATTACHED | CLIPS
    desired: Size,                 // ≈ DesiredSize（measure 结果）
    computed: ComputedLayout,      // ≈ ActualOffset + ActualSize（arrange 结果，写回）
    text_cache: Option<Arc<TextLayout>>,
}
// ── 组件：两层——框架自有（枚举） + 用户扩展（逃生舱） ──
pub enum Kind {
    // ══ 框架自有组件：框架拥有 measure/draw/behavior 的完整实现 ══
    Box,                                   // Row/Column/Container 都是 Box + FlexStyle 差异
    Text(String),
    Image(Arc<ImageData>),
    Button { label: String, hovered: bool, pressed: bool },
    Checkbox { checked: bool }, Switch { on: bool }, Radio { index: usize },
    Slider { value: f32, dragging: bool },
    Input(InputState),                     // 文本/选区/IME/光标动画
    Progress { value: f32 }, Scroll { offset: (f32, f32), axis: Axis },
    VirtualList { first: usize, count: usize, item_h: f32 },
    Surface(Rc<SharedSurface>),            // 外部高频画布（终端）

    // ══ 用户扩展逃生舱（见 §3.12）══
    Custom(Box<dyn CustomNode>),
}
```

事件处理器是**闭包**（`Handler = Rc<dyn Fn(&mut Ctx)>`），非泛型，直接存在 `Node.handlers` 里；
所以 `Track`/`Node` 都保持非泛型；连 `App` 也不带泛型（窗口视图用 `Rc<dyn WindowView>` 擦除，§3.14）。

**为什么框架自有组件用枚举而不是 `Box<dyn WidgetNode>`（关键决策）**：
若节点持有 trait 对象，则"可变借用节点自身"与"把 `&mut Track` 传给组件行为"会同时发生，必然冲突；
用枚举 + `match` 分派，组件行为写成
`fn input_on_event(track: &mut Track, id: NodeId, ev: &Event, cmd: &mut Cmd)`，
在函数内部按需 `track.get_mut(id)`，借用天然可分。

**用户扩展不适用这个限制**（`CustomNode` 的 hook 不拿 `&mut Track`，只拿 `Cmd` 缓冲），
因此 `Kind::Custom` 可以安全地持有 trait 对象。三档扩展能力与判据见 §3.12。

### 3.3 用户态①：`view(self: &Rc<Self>)`（读状态，产出描述）

```rust
// 纯函数：状态 → 描述。容器闭包 + 当前父栈，语法像声明式 DSL，实现上是往 arena 追加描述
fn view(self: &Rc<Self>, v: &mut View<'_>) {
    v.column(|c| {
        c.center().gap(12.0);
        c.text("Counter").font_size(48.0);
        c.text(self.count.get().to_string()).font_size(72.0).color(RED);   // 读 Signal
        c.row(|r| {
            r.gap(12.0);
            r.button("-1").on_tap(act(self, Self::dec));   // 闭包：直接改状态
            r.button("+1").on_tap(act(self, Self::inc));
        });
    });
}

// ── 状态：Signal 字段（≈ Vue ref / WinUI 可观察属性）──
struct Counter { count: Signal<i32> }

impl Counter {
    fn inc(&self) { self.count.update(|v| *v += 1) }   // ViewModel 的"命令"= 普通 &self 方法
    fn dec(&self) { self.count.update(|v| *v -= 1) }
}

// ── 事件辅助：把 `Rc<Self>` + 方法变成 `Fn()` 闭包（零噪音）──
fn act<T: 'static>(vm: &Rc<T>, f: fn(&T)) -> impl Fn() + 'static {
    let me = Rc::clone(vm);
    move || f(&me)
}

// ── 框架侧：描述 arena 的写入接口（无 NodeId、无 patch、无状态存储）──
impl View<'_> {
    pub fn column(&mut self, f: impl FnOnce(&mut Self));   // 容器：推栈 → f → 弹栈
    pub fn text(&mut self, s: impl Into<String>) -> TextRef<'_>;   // 叶子：返回句柄以便链式设样式
    pub fn button(&mut self, label: &str) -> ButtonRef<'_>;
    // ── 层（§3.15）：在内容内声明，可嵌套；z 由 (Layer, 嵌套深度, 序号) 决定 ──
    pub fn modal(&mut self, f: impl FnOnce(&mut Self));                     // backdrop + 阻断下层
    pub fn overlay(&mut self, f: impl FnOnce(&mut Self));                   // 装饰 / 水印（命中穿透）
    pub fn popup_at(&mut self, key: &str, p: Placement, f: impl FnOnce(&mut Self));
    pub fn popup_at_point(&mut self, pos: Point, p: Placement, f: impl FnOnce(&mut Self));
    pub fn tooltip_at(&mut self, key: &str, p: Placement, f: impl FnOnce(&mut Self));
    // > M5 实现注记：锚定层的落位是布局**之后**的独立一步 `layout::place_anchored_layers`——
    // > 锚点 rect 与层自身尺寸都要等布局才知道。锚定层根用未定义可用空间布局（收缩到内容，
    // > 否则会被拉伸成整窗）；默认侧放不下翻转到另一侧、再不行钳到视口内；挪层平移整棵
    // > 子树并把旧 ∪ 新矩形登记脏区。新建层根时 `align` 会带上描述的 `LayerOpts`。
    // > 锚点有两种（`track::AnchorTarget`）：**节点 key**（`popup_at`）与**逻辑坐标点**
    // > （`popup_at_point`，右键菜单贴鼠标）。点被当作零尺寸矩形参与同一套翻转/钳制 ——
    // > 所以点锚点会自动贴边回视口，而 `Placement::Fixed` 是字面量、不翻不钳。
    pub fn drag_preview(&mut self, f: impl FnOnce(&mut Self));              // 最高层
    pub fn keyed_list<I, K>(&mut self, items: I, key: impl Fn(&I::Item) -> K,
                            item: impl FnMut(&mut Self, &I::Item));   // 唯一的 key 匹配点
    pub fn input_bind(&mut self, s: Signal<String>) -> InputRef<'_>;  // 双向绑定（≈ v-model）
    pub fn slider_bind(&mut self, v: Signal<f32>, min: f32, max: f32) -> SliderRef<'_>;
    pub fn custom<T: CustomNode + Default>(&mut self, desc: impl IntoDesc<T>);   // 见 §3.12
}
```

> 命名对照：本文写作 `View<'_>`，代码里叫 **`ViewBuf`**（强调"可复用的描述缓冲"，
> 也避开将来可能出现的 `View` trait 命名冲突）。`view()` 期间禁止 `Signal::set` 的断言也已生效。
> 另外：容器/层入口的闭包签名是 `FnOnce(&mut Self)` —— **闭包不能返回 `DescRef`**
> （返回借用会触发 `'1 must outlive '2`，这是类型系统硬约束），所以链式样式句柄只能当**语句**用。

**三条硬约束**（写进 crate 文档，违反会出 bug）：

1. **`view()` 不在每帧跑**。它在「挂载时」与「每次状态变更后」各跑一次；
   hover / pressed / 滚动 / 光标闪烁 / 动画 / 窗口 resize **都不经过 `view()`**。
2. **`view()` 必须无副作用**：只读状态、产出描述；**内部禁止 `Signal::set`**（否则会自我触发循环，debug 下断言）。
   要改状态只能在事件闭包里改。
3. **视图态不归 Model**：`interaction`、`scroll offset`、`Input` 编辑态、`Slider.dragging`、列表实例缓存
   都留在保留树里（框架拥有），对齐时不会被描述覆盖（见 §3.4.1 的 desc/state 字段分组）。

> 这三条是"非立即模式"在 API 层的体现：**每帧发生的只有"重排 + 重绘 + 上屏"；`view()` 只在状态变化时跑。**

### 3.4 框架内部：对齐（Align）→ patch → 精确脏区

用户态不出现 patch。框架内部一次性完成 `描述 → 保留树` 的落地：

```rust
// 框架内部（pub(crate)），唯一的落树通道
fn align(&mut self, view: &View<'_>) {
    // ① 位置对齐：沿保留树与新描述**并行下降**，下标 + 一层 key 即身份（无实例表、无 type_name 回退匹配）
    // ② 差异落树：只对"不同"的字段/节点调用 patch（下列 API 全部 pub(crate)）
    // ③ 标脏：patch 内部同步登记脏矩形（见下表），累积到 needs + dirty_rects
}

// pub(crate) patch API（内部）
fn set_text(&mut self, id: NodeId, s: &str);
fn set_style(&mut self, id: NodeId, f: impl FnOnce(&mut Style));
fn set_kind_desc(&mut self, id: NodeId, k: KindDesc);   // 只覆盖 desc 字段，保留 state 字段（见 §3.4.1）
fn insert(&mut self, parent: NodeId, idx: usize, child: NodeId);
fn detach(&mut self, id: NodeId);                        // 移入回收池，不销毁
fn remove(&mut self, id: NodeId);
fn set_children(&mut self, parent: NodeId, kids: &[NodeId]);
fn mount(&mut self, layer: Layer, id: NodeId);
fn unmount(&mut self, id: NodeId);
```

对齐规则（**这是全框架唯一需要"匹配"的地方，刻意做得极小**）：

| 规则 | 说明 |
|---|---|
| **位置即身份** | 同一 `view()` 每次以相同顺序产出相同结构 → 按下标对齐天然稳定 |
| **一层 key** | 仅 `keyed_list` 内按下标 → key 匹配（复用已有实例）；不做跨父 `Move`、不做 `type_name` 回退匹配 |
| **结构变化必须走对入口** | 条件分支是普通 `if`（位置对齐天然处理）；**列表必须走 `keyed_list(..)`**，否则位置错配（现状靠 `key` 解决同一问题） |
| **类型不同即重建** | 下标处 `Kind` 判别式变了 → 销毁旧节点、建新节点（子树随之重建，视图态丢失；这是有意的边界） |
| **未变化即零操作** | 描述逐字段比（`Text` 比字符串、样式比值），相同则完全不动该节点（**不需要全树 `tree_eq`**） |

每个 patch 的副作用（对齐阶段与事件阶段共用同一批内部 API）：

| patch | 标记 | 脏区来源 |
|---|---|---|
| `set_text` | 自身 `MEASURE_DIRTY`（文本固有尺寸可能变）+ 祖先冒泡到重排边界 | 该节点**旧 bounds ∪ 新 bounds**（布局后取新值） |
| `set_style` | 同上（若影响布局）或仅 `PAINT_DIRTY` | 该节点 bounds |
| `set_kind_desc` | 视字段决定 `MEASURE_DIRTY`/`PAINT_DIRTY` | 该节点 bounds |
| `insert` / `remove` / `detach` / `mount` / `unmount` | `MEASURE_DIRTY` | 受影响子树 bounds（旧 ∪ 新） |
| hover/pressed/focus 变化 | 旧路径 ∪ 新路径的节点 `PAINT_DIRTY` | 这些节点的 bounds 并集 |

**脏区在 patch 发生的那一刻就被明确登记**——不需要"元素签名哈希比较"兜底，也不需要"猜不到就整屏"。

#### 3.4.1 每个 `Kind` 变体的字段分两类（声明式模型的关键约束）

描述只携带"数据"，不能覆盖"视图态"，因此有状态组件的字段必须显式分组：

```rust
pub enum Kind {
    Box, Text(String), Image(Arc<ImageData>),
    Slider { value: f32, /* desc */ dragging: bool /* state，对齐时保留 */ },
    Input  { text: String /* desc */, selection: Range, ime: Option<ImeState>, caret_blink: u64 /* state */ },
    Scroll { axis: Axis /* desc */, offset: (f32,f32), content: Size /* state */ },
    // …所有有状态变体同此约定
}
```
`set_kind_desc` 只写 `desc` 组；`state` 组只由事件/框架自身改写。**这条约定是"hover 不丢、滚动不跳、输入框不闪回"的根本保证。**

#### 3.4.2 窄逃逸接口（受控，不进常规文档）

对"外部数据直达"场景（终端像素流、`Kind::Custom`、逐帧动画），允许绕过 `view()` 做局部刷新，
但**只有 `Ctx` 上的少量显式方法**（不额外引入一套 `raw` 类型）：
`cx.damage(key)`（只把该节点矩形并入脏区）、`cx.invalidate()`（下次帧重跑 `view()`）、
`cx.set_text_raw(key, s)`（直接改一个文本节点，不重跑 `view()`）。
`key` 是**由 `view()` 显式声明的稳定名字**（`v.text(..).key("title")`），不是 `NodeId`，
因此逃逸接口也不会把 arena 身份泄漏进用户态。定位方式：key → 保留树查表（框架维护，随对齐更新）。

> 结论：**patch 只有一份实现（框架内部），暴露面受控**。用户态 99% 的场景用不到它。

### 3.5 事件与 ViewModel

#### ViewModel trait（没有 Msg、没有 update）

```rust
pub trait ViewModel: 'static {
    /// 声明式：状态 → 描述。**&self、不碰视图树、不出现 NodeId、内部禁止 set**。
    /// 作用域 = "我所属的那个窗口"：最外层调用产生的根就是该窗口的 Content 根；
    /// 层用 `v.modal(..)` / `v.overlay(..)` / `v.popup_at(..)` 在内容内声明（§3.15）。
    /// 挂载时跑一次；此后每次 `Signal` 变更后再跑一次（不是每帧）。
    fn view(self: &Rc<Self>, v: &mut View<'_>);

    // ── 可选钩子（都有默认实现；都只拿到 `&mut Ctx`，改状态仍靠自己持有的 Signal）──
    fn on_external(self: &Rc<Self>, _cx: &mut Ctx, _data: ExternalData) {}
    fn on_close_request(self: &Rc<Self>, _cx: &mut Ctx) -> CloseAction { CloseAction::Close }
    fn on_tick(self: &Rc<Self>, _cx: &mut Ctx, _now: Instant) {}
}

/// 对象安全的窗口视图。`ViewModel::view` 的 receiver 是 `&Rc<Self>`，不能直接 `dyn`，
/// 所以用一层薄擦除——**让不同窗口可以是不同的 ViewModel 类型**（§3.14）。
pub trait WindowView {
    fn view_erased(&self, v: &mut View<'_>);
    fn on_tick(&self, cx: &mut Ctx, now: Instant) {}
    fn on_close_request(&self, cx: &mut Ctx) -> CloseAction { CloseAction::Close }
}
impl<V: ViewModel> WindowView for Rc<V> {
    fn view_erased(&self, v: &mut View<'_>) { V::view(self, v) }   // `self: &Rc<V>` 正好是 view 的 receiver
    fn on_tick(&self, cx: &mut Ctx, now: Instant) { V::on_tick(self, cx, now) }
    fn on_close_request(&self, cx: &mut Ctx) -> CloseAction { V::on_close_request(self, cx) }
}

pub struct App {
    rt: Runtime,                  // App 级：所有窗口共享（Signal 的宿主）
    windows: Vec<WindowCtx>,      // 每窗口：Track + Renderer + Surface + dirty + view
}
struct WindowCtx {
    id: WindowId,
    cfg: WindowConfig,
    track: Track,                 // 每窗口一棵保留树（含层栈与视图态）
    renderer: Renderer,
    surface: Surface,
    view: Rc<dyn WindowView>,     // 该窗口的 VM（已擦除，允许不同窗口类型不同）
    dirty: Dirty,                 // 每窗口脏标志
    view_buf: ViewBuf,            // 描述 arena（每窗口复用）
}
```

> `view` 的 receiver 是 `self: &Rc<Self>`：这样闭包能通过 `act(self, Self::inc)` 捕获 `Rc<Self>` 去调自己的命令方法。
> **状态变更的入口只有两个**：事件闭包、可选钩子（`on_tick` / `on_external`）。两者都只能改 `Signal`（或 `&self` 可达的内部可变槽 + `cx.invalidate()`）。

#### 三条职责线（MVVM 的落法）

| 角色 | 谁 | 职责 |
|---|---|---|
| **Model** | 用户 `struct` 的 `Signal<T>` 字段 | 状态 + 领域逻辑（`&self` 命令方法）；**不含任何框架类型**（除 `Signal`） |
| **View** | 保留树 `Track` + `widgets/`（框架） / `view()` 描述（用户） | 被动：只描述长相（Kind/Style/children），不含业务判断；**视图态由框架持有** |
| **ViewModel** | 实现 `ViewModel` 的 `Rc<Self>` | `view()` 声明视图；命令方法改状态。**既不持有句柄，也不调用 patch** |
| **框架** | `Runtime` + `align` + `Cmd` + 渲染 | 把描述落成 patch、把 patch 落成脏区、上屏 |

#### R1 的响应式回路（唯一回路，作为实现验收基线）

```
用户交互（点按钮 / 拖滑块 / 敲键盘）
  → 路由分发（Tunnel/Target/Bubble，框架内置行为先跑）
  → 命中节点上的闭包执行 → 改 Signal
        └ Signal::set/update ⇒ Runtime.dirty |= VIEW      （只置位，不立即干活）
  → 同一批事件里再改别的 Signal 也只置位（天然批处理，无重入问题）

App::frame()
  ① if dirty.VIEW:  view(&mut view_buf) → align(描述 ↔ 保留树) → patch → 登记脏区
  ② if dirty.LAYOUT: 脏边界重排（写回 Node.computed）
  ③ if dirty.PAINT:  Scene 展开 + 局部光栅化到持久 Pixmap
  ④ if dirty.PRESENT: present_with_damage(合并后的脏矩形)
```

**注意（三条"不经过 `view()`"的路径）**：
- **hover / pressed / focus** 变化 → 框架改 `Node.interaction` + 标脏，**不跑 `view()`**；
- **滚动 / 光标闪烁 / 逐帧动画** → 框架改保留树或 `cx.damage(key)` + `request_repaint_after`，**不跑 `view()`**；
- **外部数据（PTY 等）** → `on_external` 里改 `Signal`（自动）或直接 `cx.damage(key)`（只重绘）。

#### 事件模型：路由事件 + 两段式分发（对齐 WinUI `UIElement`，见 §3.13）

```rust
/// 路由策略（对齐 WinUI RoutedEvent 的三态）
pub enum Routing { Tunnel, Bubble, Direct }

/// 事件种类（裁剪自 UIElement 的 32 个 `*Event` + FrameworkElement 的 4 个）
pub enum EventKind {
    PointerEntered, PointerExited, PointerMoved, PointerPressed, PointerReleased,
    PointerWheelChanged, PointerCanceled, PointerCaptureLost,
    Tapped, DoubleTapped, RightTapped, Holding, ContextRequested, ContextCanceled,
    KeyDown, KeyUp, PreviewKeyDown, PreviewKeyUp, CharacterReceived,
    GettingFocus, GotFocus, LosingFocus, LostFocus, NoFocusCandidateFound,
    TextCompositionStarted, TextCompositionChanged, TextCompositionEnded,
    Loaded, Unloaded, SizeChanged, EffectiveViewportChanged, ScrollChanged, BringIntoViewRequested,
    DragStarting, DragEnter, DragOver, DragLeave, Drop, DropCompleted,
}

/// 事件分发（两段式：借用安全的唯一可行路径）
fn dispatch(&mut self, ev: &Event) {
    // ① 只读遍历：命中测试 + 沿祖先链按 Routing 收集「要跑的处理器」（clone Rc）
    let path: SmallVec<NodeId> = self.track.hit_path(ev.pos());     // &Track
    let plan = self.collect_route(&path, ev);                       // 只读：(NodeId → Handler) 等
    //   ↑ 至此对 Track 的不可变借用结束（clone 出来的 Rc 独立存活）

    // ② 可写：按 Tunnel（外→内）→ Target → Bubble（内→外）执行
    //    用户闭包拿到 &mut Ctx（可从 Ctx 里改状态、invalidate/damage），不拿 &mut Track
    let mut cmd = Cmd::new();
    for item in plan.handlers {
        let handled = item.invoke(&mut self.cx, &mut cmd);
        self.args.handled |= handled;
        // handledEventsToo = true 的处理器（框架内置行为）即便 handled 也会执行
    }
    self.apply_cmd(cmd);                    // 借用结束后统一落树（内部 patch，只改视图态）

    // ③ 闭环：闭包里改了 Signal ⇒ dirty.VIEW 已被置位；view()/align 留给 frame()（天然批处理）
    //    注意：这里**不**立即跑 view()，所以同一批事件里改 10 个 Signal 只重跑一次 view()
}
```

**处理器形态（R1 只有一种，闭包不再分主辅路）**：

```rust
pub type Handler = Rc<dyn Fn(&mut Ctx)>;         // 用户态：.on_tap(f) 内部包一层
pub(crate) struct HandlerSlot { ev: EventKind, h: Handler, handled_events_too: bool }

// 用户态 API（两个即可覆盖全部场景）
impl ButtonRef<'_> {
    pub fn on_tap(self, f: impl Fn() + 'static) -> Self;              // 最常见：无参闭包
    pub fn on_tap_with(self, f: impl Fn(&mut Ctx) + 'static) -> Self; // 需要 invalidate/damage 时
}
```
- **框架内置行为** = `handled_events_too = true` 的同一种槽位（这套机制取代现状 `ListenerKind::BuiltIn`
  先于 `User` 执行的排序 hack——见下方第 1 点）。
- 用户闭包**不拿 `&mut Track`**，因此不可能在分发期间破坏树结构；要改树只能改状态 → 下一帧 `view()` 重跑。

**三点关键**（这也是对现状两处 hack 的正面替换）：

1. **`handled` + `handledEventsToo` 取代 `.builtin()` 排序 hack**。
   现状靠 `ListenerKind::BuiltIn` 先于 `User` 执行来让框架行为"不被用户 `stop_propagation` 打断"
   （`guide.md` §2.4 明确写了这个限制）。WinUI 的做法更精确：框架行为以
   `add_handler(routed, handler, handled_events_too = true)` 注册，语义是"无论是否已被处理都要调用"。
2. **`Preview*` 对取代隐式捕获规则**。现状捕获阶段只对 `ClickWithCtx` 响应，是个特例；
   WinUI 用 `KeyDown`(bubble) / `PreviewKeyDown`(tunnel) 显式成对，`Routing::Tunnel` 统一处理。
3. **`Cmd` 命令缓冲是唯一的延迟写入通道**。它同时取代：`EventContext` 的
   `effects` 标志 + `capture_mouse` 请求 + `begin_drag` 请求 + `PENDING_LAYER` + `PENDING_POPUP_HIDE`
   （4 个机制 → 1 个）。`Cmd` 是纯数据枚举（`SetText/SetStyle/SetKind/Focus/CapturePointer/
   ReleasePointer/BringIntoView/ScrollTo/ShowLayer/HidePopup/StartAnimation/...`），
   分发结束、借用释放后由 `apply_cmd` 落树 —— **这是让 `Kind::Custom` 能安全持有 trait 对象的前提**。

**只有一种处理器（闭包），没有 Msg**：
- 用户态：`view()` 里 `.on_tap(act(self, Self::inc))`，闭包签名是 `Fn()` 或 `Fn(&mut Ctx)`。
- 框架内部：同一种槽位 + `handled_events_too = true`（用于内置行为，如释放指针捕获、收尾拖拽）。
  > **M5 实现修正**：内置行为最终**没有**走闭包槽位。`Handler = Rc<dyn Fn(&mut Ctx)>` **拿不到保留树**，
  > 而滑块拖拽 / 文本编辑必须算几何、改视图态 ⇒ 绕道加 `Cmd` 变体很别扭。
  > 于是按本节开头"框架内置行为先跑"的原意实现成**独立的一遍**：
  > `WindowCtx::dispatch` = **① `widgets::handle_route(&mut Track, path, ev, &mut CmdBuf)` →
  > ② 用户处理器（只读收集 → `&mut Ctx`）→ ③ `apply_cmds`**。
  > 组件行为因此就是普通函数 `fn slider_handle(track: &mut Track, id: NodeId, ev: &EventView, cmd: &mut CmdBuf)`。
  > `handled_events_too` 保留给用户态的"即便已处理也调用"。
- **双向绑定**（`Node.bindings`，≈ `v-model`）：`slider_bound(&sig)` / `checkbox_bound(&sig)` / `input_bound(&sig)`。
  规则：**只有存在绑定，内置行为才改模型**；`slider(0.5)` 只是"显示这个值"。
  理由：允许拖未绑定的滑块会得到"下次 `view()` 才回弹"的不可预测中间态。
  绑定写入用 `desc` 立即反馈（`slider_drag_to` 直接改 `Kind::Slider.value`）+ 同时 `Signal::set`，
  两者值一致 ⇒ 下一帧 `align` 零 patch（有测试守这条）。

  > **M5-B：Input 的四个实现决定**
  > 1. **desc/state 划分**：`Kind::Input { text, placeholder | caret, anchor, preedit }`——
  >    `text` 属于 desc（模型是唯一真相），编辑时先改 `text`（立即重绘）再写回 signal；
  >    光标/选区/组合串是纯视图态，`apply_to` 不碰。
  > 2. **光标不回弹**：编辑回写的值与模型一致 ⇒ 下帧 `apply_to` 判相等 ⇒ 零补丁；
  >    只有模型**真的**换值（外部同步）才覆盖编辑缓冲，并把光标放到末尾（IME 组合中不覆盖）。
  > 3. **编辑只在有绑定时生效**（与滑块/复选框同一条规则）：`input("固定文本")` 是只显示形态；
  >    字符事件还要求 `track.focused == Some(id)`，杜绝多框串字。
  > 4. **剪贴板归平台层**（Ctrl+C/X/V 在 `platform` 拦截，`arboard` 是 winit feature 的可选依赖）：
  >    核心只暴露 `Track::input_selected_text / input_insert` 等纯数据 API，不依赖 OS 服务。
  >    另两处已知欠账：光标不闪烁（闪烁要"聚焦期间逐帧重绘"，与空闲帧零功耗冲突，留主题/动画一起做）、
  >    文本超宽只裁剪不滚动（需 Input 内的水平 scroll offset，随 Scroll 组件补）。
- **不需要"消息分发 + update 汇总"这一层**：闭包直接改 `Signal`，状态是唯一真相。
- 代价（诚实交代）：逻辑分散在各命令方法里，而不是集中在一个 `update` 分支；
  换来的是零样板与"改状态即自动更新"（含异步/外部数据，无需记得发消息）。
  命令方法本身**可单测**（`vm.inc(); assert_eq!(vm.count.get(), 1)`），不依赖窗口。

#### 视图态归属（单一真相，对齐 WinUI 的每元素状态属性）

| 状态 | 归属 | WinUI 对应 |
|---|---|---|
| `pointer_over` / `pressed` / `focused` / `enabled` | **每节点 `Node.interaction`** | `Control.IsPointerOver` / `IsPressed` / `IsEnabled` |
| `focus_state`（Pointer/Keyboard/Programmatic） | 每节点 `Node.focus_state` | `UIElement.FocusState` |
| 指针捕获（多指针） | `Track.captures: SmallMap<PointerId, NodeId>` | `PointerCaptures` |
| 键盘焦点 | `Track.focused: Option<NodeId>` | `FocusManager` |
| 当前 hover 命中链（用于**传播**与标脏） | `Track.hover_path: SmallVec<NodeId>`（瞬态，不参与查询） | — |
| scroll offset / content size | 对应 `Node`（`Kind::Scroll` / `Node.computed`） | `ScrollViewer` |
| 组件内部态（Input 编辑、Slider 拖拽、菜单开合） | 对应 `Node.kind` 的 `state` 组字段（§3.4.1） | Control 内部状态 |
| 应用态 | 用户 `Model`（在 ViewModel 里） | `DataContext` |

> 注：本文档 v3.0 初稿曾提出把 hover/pressed 放在一个单值 + 查询 `hover_path`；**现改为每节点字段**
> （对齐 `UIElement`/`Control` 的做法）。理由：绘制与命中都是 O(1) 直接读取；`hover_path` 退化为
> "传播用的瞬态命中链"，只在 hover 变化时把**旧路径 ∪ 新路径**的节点标 `PAINT_DIRTY` 并更新其字段。
> 相比现状仍删掉了 `hovered_listeners` 路径集合与 `set_hovered_state` 的隐式批量写。

### 3.6 布局（Measure / Arrange 两段，对齐 UIElement）

- **复用当前分支已移植好的 Taitank 引擎**（`layout/flex_node.rs` 956 行 + `flex_line.rs` + `types.rs` + `box_model.rs`，
  见 §六复用清单）。适配器就是"保留树 → `FlexNode` 树 → 求解 → 写回"三步：
  `build()`（叶子带 `measure_text` / `intrinsic_size`，滚动容器的直接子节点禁 `grow/shrink`）、
  `flex.layout(avail_w, avail_h, Ltr)`、`write_back()`（写 `Node.computed`/`desired`，旧 bounds ∪ 新 bounds 入脏区，
  滚动容器算 `content_size` + 钳制偏移 + 子原点平移）。
  > **实现修正（M2）**：原稿要让适配器实现 `LayoutTree` trait，**实现时放弃了**——M0 抽出的 `FlexNode`
  > 自带 `measure_text: Option<(String, TextSpec)>` 与 `intrinsic_size`，而 `build()` 本来就自底向上持有整棵
  > flex 树（`children: Vec<FlexNode>`），再加一层 trait 回调是纯开销。
- **失效 API 对齐 WinUI**：结果字段 `Node.desired`（≈ `DesiredSize`）与 `Node.computed`（≈ `ActualOffset` + `ActualSize`）。
  `MEASURE_DIRTY` / `ARRANGE_DIRTY` 在 v1 **同置同清**（arrange-only 优化留待基准验证后做）。
- **脏边界重排**，且**脏分两类**（M2 实现时发现的分类，见 `docs/operation-log.md`）：
  | 脏类型 | 含义 | 影响范围 | API |
  |---|---|---|---|
  | 自身尺寸脏 | 内容/尺寸可能变了（文本、`Kind::desc`） | 只有"尺寸未确定"的节点会把影响传给祖先 | `mark_layout_dirty` |
  | **流脏** | 在父的**流**里变了（`Collapsed`、增删、`FlexStyle`/margin 变了） | **必然**让兄弟重排 ⇒ 连父一起标脏 | `mark_flow_dirty` |

  冒泡到"宽高均 definite"的节点为止 ⇒ `App::frame` 只对**最上层脏节点**（`layout_boundaries()`）的子树跑引擎。
- **层根各自是一次布局的根**：`Content` 根用窗口视口；浮层先按自身内容自然尺寸布局，再按 `anchor`（节点 key 解析出的 rect）定位，
  视口不足时翻转；定位若影响尺寸则再跑一轮（复用"最多 2 轮"机制）。
- 相比现状：删掉每帧从零 clone 整棵 `FlexNode` 树（`layout/context.rs`）与全表 `has_dirty_node()` 扫描。

### 3.7 渲染与脏区（纯 CPU 的核心）

```
App::frame：
  ⓪ dirty.VIEW    → vm.view(&mut view_buf) → align(描述 ↔ 保留树) → 内部 patch → 登记脏区
  ① dirty.LAYOUT  → 脏边界重排（写回 Node.computed）
  ② dirty.PAINT   → scene::expand(脏子树)  ──只读遍历──▶ Vec<DrawOp>
                    raster::rasterize(dirty_rects)      ──▶ 持久 Pixmap（vello_cpu）
  ③ dirty.PRESENT → present_with_damage(merged_rects)  ──▶ softbuffer
```

- **draw list 展开**：v1 先做全量展开（成本是 Vec 分配 + memcpy，远小于光栅化），
  只把**光栅化 + 上屏**做成脏区；后续再按脏子树增量展开。
  M3 落地形态：`SceneBuilder::build` 产出**扁平 op 列表**（`Op` 自带已组合好的 `transform`，
  `PushClip`/`PopClip`/`PushOpacity`/`PopOpacity` 显式成对），并按 `Cull`（脏区）**逐原语剔除**；
  节点带裁剪（`clip_content`/滚动/显式 `Clip`）且整体在脏区外时**整棵子树跳过**。
- **局部光栅化**（M3 实测修正了原稿的两处）：
  - 原稿写"`RasterizerSettings.offset` + `PixmapMut` 条带"。**实测：0.2 的 `offset` 是 `(u16, u16)`（不能为负）**，
    所以批次原点的位移改由**场景变换**完成（`shift = translate(-x0, -y0)` 与每个 op 的变换组合）；
  - **不要用"全宽行带"**：行带像素数 = 全宽 × 行数，会把 38×77 的文本脏区放大成 400×77（收益减半），
    且多个小脏区面积一累加就触发退化。改为**任意脏矩形批次**：每个脏区单独渲染进复用的 `scratch`
    `Pixmap`，再逐行拷回持久 pixmap ⇒ 开销 ∝ **脏区面积**（`damage_batches` 负责取整/裁剪/去重，
    面积 > 45% 或碎片 > 8 块时退化整窗）；
  - 无 unsafe 路径：`Pixmap::data_as_u8_slice_mut()` + `PixmapMut::new(..)`；
  - **脏区必须按"绘制影响范围"而不是节点矩形**：文本节点用内容范围（`Node::paint_bounds`，
    否则被 stretch 的文本"改一个字"就重画整行宽度），并沿祖先链组合变换（`Track::damage_bounds`，防变换残影）。
- **上屏**：`present_with_damage`；`age()==0`（缓冲内容未定义）或脏区 ≥70% 时退化全量 `present()`。
- **像素契约**（直接沿用 dirty-surface 已踩过的坑）：backing/UI = premul；`Kind::Surface` 写入 straight RGBA，
  合成时 src-over + `a==255` memcpy 快速路径；surface 移动/消失时**旧矩形必须由 UI 回填**（防残影）。
- **图层**：`roots: Vec<Root>`，按 `z = (Layer, 嵌套深度, 序号)` **升序**遍历（后画覆盖先画）；
  Modal 的 backdrop 在该层内容**之前**、以更低 z 画整屏遮罩；`Overlay`(水印) 不参与命中但参与绘制。
  不再有 `SurfaceEntry` 双通道混排、不再有每类 `seq`。
- **命中测试是纯函数**：`fn hit_path(&self, p) -> Vec<NodeId>`（`path[0]` = 层根，`last()` = 目标），逐级套用
  `transform` 求逆 + `clip` 求交 + `visibility`/`hit_test_visible` 过滤
  （现状 `hit_test_rec` 只做矩形包含，完全不处理变换与裁剪，是缺口）。
  popup 的 Dismissable 关闭**移到 `App` 里命中测试之外**（现状它藏在 `hit_test_top` 内做副作用）。
  M2 落地时的三条语义决定（测试已钉住）：
  1. **`Hidden` 占位但不绘制 ⇒ 也不可命中**（命中跟随渲染，与 WPF 一致）；`Collapsed` 连布局都不参与；
  2. **`blocks_below`（Modal）无条件吸收**点击（不看层根矩形），否则"点在小对话框旁边"会漏给背后内容；
     水印类 `Overlay` 则整层穿透（`LayerOpts.hit_test_visible = false`）；
  3. **变换只作用于绘制/命中，不作用于布局**（对齐 WinUI `RenderTransform`）：被放大的元素的兄弟不挪位，
     它的子树随它一起缩放（命中按逆矩阵递归；`scale = 0` 这类退化变换视为不可命中）。
- **外部画布**：`Kind::Surface(Rc<SharedSurface>)` 是树里的普通节点 → surface 注册表就是树本身，
  删掉 thread_local `Weak` 注册表（dirty-surface P1#10 的泄漏源）。
- **`BringIntoViewRequested` / `StartBringIntoView`**：需要"让我可见"的一方发
  `cmd.bring_into_view(id, opts)`，事件沿祖先冒泡到最近的滚动容器，由它按自己的视口/内容尺寸处理。
  这取代现状散落在各处的 `scroll_to` / `apply_anchor` 手动算滚动。
- **`EffectiveViewportChanged` + `ScrollChanged`**：滚动容器在自身视口或偏移变化时向子树广播
  生效视口（被滚动祖先裁剪后的可见矩形），`VirtualList` 据此决定实例化范围——
  取代现状 `virtual_list.rs` 自建的滚动绑定管道（`bind_scroll_state`）。

### 3.8 帧调度与信号（收敛到一条路径）

```rust
bitflags! { pub struct Dirty: u8 { const VIEW = 1; const LAYOUT = 2; const PAINT = 4; const PRESENT = 8; } }

impl App {
    fn frame(&mut self) {
        self.drain_pending_windows();                        // cx.open_window / cx.close_self 的队列
        for w in 0..self.windows.len() { self.frame_window(w); }   // 逐窗口，各自独立脏标志
    }

    fn frame_window(&mut self, w: usize) {
        // ⚠ 实现注意：下面每步都用**字段级 disjoint borrow**
        //   （`&mut windows[w]` 与 `&rt` 分开取），不要写成 `self.xxx()` 链式调用，否则整棵 App 被借走。
        if windows[w].dirty.contains(Dirty::VIEW) {
            windows[w].dirty.remove(Dirty::VIEW);
            rt.enter_window(windows[w].id);                  // Runtime 上的 Cell<Option<WindowId>>
            let mut v = windows[w].view_buf.begin(&rt, windows[w].id);   // 描述 arena 复用（只重置游标）
            windows[w].view.view_erased(&mut v);
            align(&mut windows[w].track, &windows[w].view_buf, &mut rt); // 描述 ↔ 保留树 → patch → 脏区
            rt.leave_window();
        }
        if windows[w].dirty.contains(Dirty::LAYOUT)  { layout(&mut windows[w]); }   // 脏边界重排
        if windows[w].dirty.contains(Dirty::PAINT)   { paint(&mut windows[w]); }    // 脏区光栅化
        if windows[w].dirty.contains(Dirty::PRESENT) { present(&mut windows[w]); }  // damage 上屏
        windows[w].dirty = Dirty::empty();
    }
}
```

- **删掉**三个 frame 入口与"补一次 request_render 兜底"；也**删掉**旧的多窗口分支逻辑（`RedrawRequested` 里
  `size_mismatch / rendered_once / rebuild_requested` 那套判断，现在只剩"脏标志 → 对应阶段"）。
- **删掉**全局 `FLAGS: Mutex<HashMap<WindowId>>` 与 4 个 thread_local：脏标志变成 `WindowCtx.dirty` 这个**普通字段**，
  由 `Runtime` 里的窗口表统一持有（`Signal::set` 需要能遍历到所有窗口，见 §3.14）。
- **删掉**"无当前窗口时广播"的隐式兜底：`Ctx` 自带 `WindowId`，每条消息的去向都是显式的。
- `Dirty` 只由三条路置位：① `Signal::set/update` → 所有窗口的 `VIEW`；② 内部 patch（含 `apply_cmd`）→ `LAYOUT`/`PAINT`；
  ③ `Ctx` 上的 `cx.request_repaint()` / `cx.damage(key)` / `cx.invalidate()`。
  窗口外线程用显式 `RepaintHandle`（`EventLoopProxy` 的包装，`App` 持有；`wake()` 返回 `io::Result<()>`，不是全局单例）。
- `view()` 不在每帧跑 ⇒ 帧循环里没有"重建"概念；`VIEW` 只在真的有状态变化时才置位。
- **批处理免费**：`Signal::set` 只置位、不在 set 当下跑 `view()`，所以一次事件里改 N 个 Signal 只重跑一次。

### 3.9 动态列表（唯一的"key 匹配"点）

不做通用 reconciler。列表是**唯一**需要 key 匹配的地方，封装成一个辅助：

```rust
fn view(self: &Rc<Self>, v: &mut View<'_>) {
    let me = Rc::clone(self);
    v.keyed_list(self.items.get(), |i| i.id, move |v, i: &Item| {   // items: Signal<Vec<Item>>
        let me = Rc::clone(&me);                                    // 每个 item 一份句柄
        let id = i.id;
        v.row(|r| {
            r.gap(8.0);
            r.text(&i.title);
            r.icon_button(IconName::Close)
                .on_tap(move || me.remove(id));                     // 闭包改状态，不改树
        });
    });
}
```

- 语义：按下标 → key 匹配（复用已有节点与视图态）；新增 key → 新建；消失 key → `detach` 进回收池；
  重排 → `set_children`。**仅此一层**，不做跨父 `Move`、不做 `type_name` 回退匹配。
- **条件分支就是普通 Rust `if`**（`view()` 重跑时自然求值），不需要 `if_else` 之类的构造。
  若两个分支在同一位置产出**不同 `Kind`**，对齐会重建该位置子树（丢视图态，但**不会错配**）。
- `self.items.get()` 每次 `view()` 克隆一次 `Vec<Item>`——只在状态变化时发生，可接受；
  数据量大时用 `self.items.with(|items| ...)` 免克隆（**回调内不要 `set`**，见 §4 借用纪律）。

### 3.10 主题

> **M5 实现注记**：按本节方案落地（`Theme` = `Runtime` 的普通字段、构造时烘焙、
> 切换 = view 重跑 + 整窗重绘、0 thread_local）。补充一点：绘制期颜色（光标/选区/滚动条）
> 不经过 `view()`，走 `SceneOptions.theme` 快照由渲染管线每帧携带；窗口底色跟随
> `theme.window_background`，显式 `WindowConfig.background` 优先。

`Theme` 是 `App`/`Ui` 的普通字段，widget 构造时把 token **烘焙**进节点样式（现状做法）。
切换主题 = 令 `view()` 再跑一次（描述里的颜色值随主题变）+ 全屏 `PAINT_DIRTY`。
**不引入** `theme::current()` thread_local，也不引入属性表/继承（拒绝 newworld 的 `PropertyStore` 路线）。

### 3.11 模块规划

```
crates/
  lieui-layout/   零依赖 Flex 引擎（Taitank 移植，从当前分支抽出）+ LayoutTree trait
  lieui-text/     parley 两段式测度 + 缓存（从当前分支 text/ 抽出）
  lieui/          其余全部（单 crate，严格模块边界）
    geom.rs        Point/Size/Rect/Color                  ← 现状直接抽用
    style.rs       FlexStyle + PaintStyle + TextStyle      ← 现状直接抽用
    reactive.rs    Runtime（R1：脏标志）+ Signal<T>（Rc 句柄）+ act() 辅助     ~250 行
    track.rs       Arena / Track / Node / NodeId / Flags   ← 自研（保留树，含视图态 + handlers）
    view.rs        View<'_>：用户态**描述** arena + 构造 DSL（column/text/button/keyed_list/*_bind/custom）
    align.rs       描述 ↔ 保留树的"位置 + 一层 key"对齐 + 内部 patch（**patch 只在 pub(crate) 出现**）
    cmd.rs         Cmd 命令缓冲（唯一的延迟写入通道，框架内部）
    event/         EventKind / Routing / Handler / Ctx / 路由分发 / 内置行为分派
    widgets/       内置组件的 desc/state 定义 + 绘制 + 行为（按 Kind 枚举分派）
    custom.rs      CustomNode trait + Kind::Custom（用户扩展点，见 §3.12）
    layout.rs      LayoutTree 适配器 + Measure/Arrange 失效 + 重排边界
    render/        scene.rs（draw list）+ raster.rs（vello + 脏区）+ surface.rs
    app/           mod.rs（App + 多窗口 WindowCtx 表 + winit + 帧调度）+ ime/drag/close/clipboard
    window.rs      WindowConfig / WindowId / 动态开关窗
```

**框架里几乎没有泛型**：`App` 用 `Rc<dyn WindowView>` 擦除（§3.14），`View` / `Track` / `Node` / `Runtime` / `Ctx` / `Cmd` 全部非泛型；
唯一带类型参数的是 `Signal<T>`（必须），以及用户自己的 VM 类型。

规模预估：`lieui` 约 6k 行（其中 ui 700、tree 450、widgets 2000、render 1100、app 700、style 600），
加上复用的 layout+text ≈ 3k，合计约 9k 行 —— 对比现状 `src/` 约 22k 行。

### 3.12 自定义组件与扩展点（回答"Kind 能否支持自定义 Widget"）

**结论：`Kind` 对框架自有组件封闭、对用户扩展开放；"自定义 Widget"要拆成三种诉求，各给一条路。**

| 档 | 诉求 | 做法 | 需要框架机制吗 |
|---|---|---|---|
| **A. 组合**（覆盖 ~90%） | 把现有零件组成可复用 UI 单元 | **普通函数**：`fn my_card(vm: &Rc<MyVm>, v: &mut View<'_>)`，在 `view()` 里直接调用 | **不需要**。声明式下"组件"就是函数，比现状 `impl Widget` 更简单（无实例、无状态、无 key） |
| **B. 自绘** | 画框架画不出的东西（图表、自定义形状、像素画布） | `v.custom::<MyChart>(desc)` → `Kind::Custom`，hook 为 `draw` | 是（`custom.rs`，约 150 行） |
| **C. 自定义行为** | 自己的交互状态机（富文本编辑器、自定义控件） | 同一个 `Kind::Custom`，hook 为 `on_event(&mut self, ev, cmd) -> bool` | 是（同上） |

```rust
/// ══ Kind::Custom 的 payload：对应 UIElement 的「受保护虚方法集」══
pub trait CustomNode: std::any::Any {
    fn layout_style(&self) -> &FlexStyle;

    // ── 布局（≈ MeasureOverride / ArrangeOverride）──
    fn measure(&mut self, text: &mut TextService, avail: Constraint) -> Size { Size::ZERO }
    fn arrange(&mut self, rect: Rect, text: &mut TextService) {}

    // ── 绘制（≈ OnRender）：只读 → 无借用冲突 ──
    fn draw(&self, scene: &mut Scene);

    // ── 事件（≈ OnPointerPressed / OnKeyDown …）：只拿 Cmd，不拿 &mut Track ──
    fn on_event(&mut self, ev: &Event, cmd: &mut Cmd) -> bool { false }

    // ── 生命周期（≈ Loaded / Unloaded）──
    fn on_attached(&mut self, cmd: &mut Cmd) {}
    fn on_detached(&mut self, cmd: &mut Cmd) {}

    // ── 焦点（≈ OnGotFocus / OnLostFocus）──
    fn on_focus_changed(&mut self, state: FocusState, cmd: &mut Cmd) {}

    /// 对齐：用最新描述更新自身状态（描述类型由实现决定，内部 downcast）。
    /// 框架在每次 `align` 覆盖到该位置时调用；**实例本身跨帧保留**（视图态不丢）。
    fn update_desc(&mut self, desc: &dyn Any);

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// 用户态（在 view() 里声明）：类型参数给出"工厂"，desc 给出数据
impl View<'_> {
    /// 该位置若已有 `T` 实例 → 调 `T::update_desc(desc)`；否则 `T::default()` 新建。
    /// 实例生命周期由对齐（位置/一层 key）决定，与框架自有组件一致。
    pub fn custom<T: CustomNode + Default>(&mut self, desc: impl IntoDesc<T>);
}

// 逃逸接口（§3.4.2）：`update`/`on_tick` 侧经 `Ctx` 按 key 驱动，不持有 NodeId
impl Ctx {
    pub fn custom_mut<T: CustomNode>(&mut self, key: NodeKey) -> Option<&mut T>;
}
```

> **M6 实现决定：`custom_mut` 不需要了。** `v.custom(&cell)` 要求 ViewModel 持有
> `CustomCell`（`Rc<RefCell<T>>`）——实例本来就在用户手里，直接改自己的 `RefCell`
> 再 `cx.request_repaint()` 即可（`on_event` 里则用 `cmd.damage(id)`）。
> Ctx 不持树的设计反而让这条逃生舱变得多余，据此关闭。

> **M5 实现注记（与上文的差异）**：
> ① 放弃 `update_desc(&dyn Any)` downcast 协议 —— 用户直接持有 `CustomCell`（`Rc<RefCell<dyn CustomNode>>`）
> 并跨帧复用，同一 cell 指针相等 ⇒ 对齐器视为没变，实例留在保留树里；换 cell = 数据变了。
> ② `on_event` 带 `NodeId`：自定义节点改状态后用 `cmd.damage(id)` **自标脏**
> （`Signal::set` 只触发 `view()`，不碰自绘区域）。
> ③ `measure/arrange` 钩子未做：`intrinsic_size() -> Size`（静态内容尺寸，未显式定宽高时生效）
> 已覆盖自绘组件的需求；约束驱动的自适应测度等真实需求出现再加。
> ④ B 档读 `Signal` 可行但不会自动重绘（数据变化只触发 `view()`）——见操作日志的诚实说明。

**`Kind::Custom` 为什么能安全持有 trait 对象，而框架自有组件用枚举？**
区别只在 hook 的签名：`CustomNode` 的所有 hook **都不拿 `&mut Track`**（`draw` 只读、`on_event`/生命周期只拿 `Cmd`），
所以"可变借用节点自身（`&mut track.nodes[i]`）"与"把可变引用传给外部"不会同时出现；
需要的树操作一律写成 `Cmd` 记录，分发结束后统一 `apply_cmd`。
框架自有组件反而要直接操作保留树（做更复杂的事），所以用枚举 + `match` 分派（见 §3.2）。

**声明式模型带来的额外好处**：`CustomNode` 实例**跨帧保留在保留树里**，
用户每次 `view()` 只需给出新的 `desc`（`v.custom::<MyChart>(desc)`）——
"自绘组件"因此天然获得"视图态不丢"（动画相位、缓存、滚动位置不用自己管）。

**诚实说明这个设计的代价**：
1. 第三方 widget 库无法提供"一等公民"的优化 `Kind`（例如高性能表格）。缓解：A/B/C 三档已覆盖绝大多数需求；
   若将来确有需求，可加一层 `WidgetKindRegistry`（`KindId` + 行为函数表）而不动核心。
2. `CustomNode::measure` 需要 `&mut TextService`，实现时必须用**字段级 disjoint borrow**
   （`&mut track.nodes` 与 `&mut text` 分开取），不能通过 `track.xxx()` 方法间接访问——这条要写进实现规范。
3. `CustomNode::update_desc(&dyn Any)` 的 downcast 失败要在 debug 下断言（描述类型与实例类型必须一致；
   位置对齐后类型变了会触发"类型不同即重建"，见 §3.4）。
4. WinUI 自身对"扩展"的官方姿态也是三层：`UserControl`（组合）/ `Control` 子类（模板化）/ 自绘虚方法。
   我们砍掉中间那层（不做控件模板系统），保留 A + B/C，与其建议一致。

### 3.13 与 WinUI `UIElement` / `FrameworkElement` 的对齐

> 依据：[`Microsoft.UI.Xaml.UIElement`](https://learn.microsoft.com/zh-cn/windows/windows-app-sdk/api/winrt/microsoft.ui.xaml.uielement)
> （32 个事件、约 90 个属性、约 30 个方法）与 `FrameworkElement`。
> 注意：`Loaded`/`Unloaded`/`EffectiveViewportChanged`/`InvalidateViewport`/`SizeChanged`/`Tag`
> 属于 **FrameworkElement**，不在 `UIElement` 页面。

**响应式部分的对应关系**（WinUI MVVM ↔ v3.3）：

| WinUI | v3.3 | 差异 |
|---|---|---|
| `INotifyPropertyChanged` + 手写 `RaisePropertyChanged` | `Signal<T>` | **我们自动**：`set()` 即置脏，不用手写通知 |
| `{x:Bind ViewModel.Count}`（编译期绑定，单向） | `c.text(self.count.get().to_string())` | 粒度更粗：R1 是"重跑 `view()`"而不是"只更新那个绑定" |
| `{x:Bind ..., Mode=TwoWay}` | `c.input_bind(sig)` / `c.slider_bind(sig, ..)` | 等价 |
| `ICommand` + `Command="{x:Bind IncCommand}"` | `impl Vm { fn inc(&self) }` + `act(self, Self::inc)` | 等价，但不用先写一个 Command 类型 |
| 依赖属性系统（`SetValue/GetValue`） | **不采纳**（见下方"明确不采纳"） | 我们用内联样式 + `Signal` |

#### 借鉴（采纳，理由：机制净减或补上真实缺口）

| UIElement 成员 | v3 落地 | 收益 |
|---|---|---|
| `AddHandler(routedEvent, handler, handledEventsToo)` + `handled` | 路由分发 + `handledEventsToo` | **取代 `.builtin()` 排序 hack**（现状 `guide.md` §2.4 明确承认的限制） |
| `PreviewKeyDown/Up`（tunnel）| `Routing::Tunnel` 显式化 | 取代"捕获阶段只对 `ClickWithCtx` 响应"的特例 |
| `PointerCaptureLost` | 捕获被夺/节点移除/失焦时**必发** | 修现状缺口：widgets 无法可靠清理拖拽态（`ScrollBar` 的 `Rc<Cell>` 重置即证据） |
| `CapturePointer/ReleasePointerCapture/PointerCaptures` | `Ui.captures: SmallMap<PointerId, NodeId>` + `cmd` | 多指针（为触控留口），取代单值 `mouse_capture` |
| `CharacterReceived` | 独立事件 | 文本输入的正确入口（现状只有 KeyDown 拼装） |
| `GettingFocus/LosingFocus`（同步、可取消）+ `GotFocus/LostFocus`（异步） | 四事件 + **返回 `Allow/Deny`**（不用闭包） | 可取消的焦点迁移；`FocusState` 让"键盘聚焦才画 ring"成立 |
| `IsHitTestVisible` | `Node.hit_test_visible` | 显式命中过滤（现状靠隐式视觉状态） |
| `Clip` / `Visibility` | `Node.clip` / `Node.visibility`（三态） | `Collapsed` 不参与布局（现状无此区分） |
| `RenderTransform`+`Origin`/`CenterPoint`/`Translation`/`Scale`/`Rotation` | `Node.transform` 单一变换 | **补上现状缺口**：hit test / clip / 绘制共用同一矩阵 |
| `DesiredSize` / `ActualOffset` / `ActualSize` | `Node.desired` / `computed` | 布局结果语义明确（`RenderSize` 官方已标"不建议使用"，不采纳） |
| `InvalidateMeasure/InvalidateArrange` | `ui.invalidate_measure/arrange(id)` | 两段失效，脏剪枝更准（见 §3.6） |
| `BringIntoViewRequested` / `StartBringIntoView` | `cmd.bring_into_view(id, opts)` + 冒泡 | 取代散落的 `scroll_to`/`apply_anchor` 手动滚动 |
| `EffectiveViewportChanged`（FrameworkElement） | 滚动容器向子树广播 | `VirtualList` 取代自建 `bind_scroll_state` 管道 |
| `Loaded` / `Unloaded` | `NodeFlags::ATTACHED` + `on_attached/on_detached`（经 `cmd`） | 动画/订阅的启停时机，借用安全 |
| `ContextRequested` / `ContextCanceled` | 语义化上下文菜单请求 | 取代"监听 `MouseButton::Right`"（并天然覆盖长按/键盘菜单键） |
| `KeyboardAccelerators` | 元素作用域快捷键（沿祖先链查找） | 取代全局表，作用域随树 |
| `UseLayoutRounding` | `Node.layout_rounding` | CPU 渲染下半像素对齐（清晰度） |
| `ProtectedCursor` | `Node.cursor` | 悬停光标 |
| `DragEnter/Over/Leave/Drop` + `AllowDrop/CanDrag` | 拖放目标侧（现状只有 `Draggable` 源侧） | 补上放置目标 |
| `TabIndex` / `IsTabStop` | `Node.tab_index` / `tab_stop` | Tab 链顺序（现状无 Tab 焦点） |

#### 裁剪采纳（先留位，按里程碑再做）

- `Tapped`/`DoubleTapped`/`RightTapped`/`Holding`：v1 只做 `Tapped`（≈ 现状 `Click`）+ `ContextRequested`，
  其余变体保留在 `EventKind` 里但不实现合成。
- `Manipulation*`（5 事件 + `ManipulationMode` + 惯性）：先用 `Drag*` 合成覆盖平移，惯性自算；
  做触控时再上 `ManipulationMode` 的最小集。
- `XYFocus*`（8 属性）：不做，只留 `NoFocusCandidateFound` 事件位。
- `AccessKey*` + `KeyTip*`（7 属性 + 3 事件）：不做。
- `CacheMode`（把子树缓存为复合位图）：**概念保留**（CPU 渲染下"静态子树栅格缓存"是重要优化），
  先作为 `NodeFlags::CACHE_AS_BITMAP` 留位。
- `AutomationPeer`：留 `CustomNode` 的一个可选方法位，不做实现。

#### 明确不采纳

| 成员 | 理由 |
|---|---|
| `DependencyObject` 的 `SetValue/GetValue/RegisterPropertyChangedCallback` 依赖属性系统 | **正是 newworld `PropertyStore` 那条过度设计路**，与"内联样式、无选择器/继承"的既定决策冲突 |
| `OpacityTransition/RotationTransition/ScaleTransition/TranslationTransition` + `Transitions` | 用统一的动画系统按属性名驱动，不铺 4 套 Transition 类型 |
| `Projection` / `Transform3D` / `RotationAxis` / `Lights` / `CompositeMode` | 纯 CPU 2D 渲染，3D/光照不做 |
| `Shadow`（作为节点属性） | 我们已有 `ShadowRoundedRect` 绘制原语，保留为**绘制样式**而非节点属性 |
| `RasterizationScale` | 缩放归 App 层的 DPI 处理，不进节点 |
| `FindSubElementsForTouchTargeting` / `RegisterAsScrollPort` | 触控目标解析不做；滚动端口由 `Kind::Scroll` 自身表达 |

#### 关于"事件种类变多"与"简化"的关系

上表把 `EventKind` 从现状 17 个扩到约 40 个。这不矛盾：
**简化的度量是"机制个数"，不是"枚举成员个数"**。v3 的事件机制只有一套
（`EventKind` + `Routing` + `handled` + `handledEventsToo` + `Cmd`），
而现状是 17 个类型 + 8 个文件各自实现 `.builtin()` + 三阶段里的特例 + 4 个散落的延迟请求通道。
按里程碑分批实现（核心 `Pointer*`/`Tapped`/`Key*`/`Focus*`/`Loaded` 先做，拖放/加速器/访问键后做）即可。

---

### 3.14 多窗口

**分层**：`Runtime` / `Signal` 是 **App 级**（跨窗口共享）；`Track` / 渲染器 / `Surface` / 脏标志 / 描述缓冲是 **每窗口一套**。

```rust
let rt = Runtime::new();
let shared = Shared::new(&rt);        // 跨窗口共享状态（就是一个装 Signal 的普通 struct）

App::new(rt)
    .window(WindowConfig::new().title("主窗口").size(960.0, 640.0), MainVm::new(&shared))
    .window(WindowConfig::new().title("面板").size(360.0, 480.0),  PanelVm::new(&shared))
    .run()
```

**M4 落地补充（三条实现约定）**：

1. **动态开关窗走"类型擦除请求槽"**：`Ctx` 只拿得到 `Runtime`，但载荷类型
   （`(WindowConfig, Rc<dyn WindowView>)`）属于 `app` 层 —— 让 `event`/`reactive` 反向依赖 `app` 会破坏分层。
   于是 `Runtime::requests()` 提供一个 `push<T>/take<T>` 的槽：`Ctx::request(payload)` 写入，
   `App::drain_requests()` 取出并解释（用户态入口是 `lieui::app::open_window(cx, ..)` / `close_self(cx)`）。
   好处：**无窗口环境（单测）也能验证动态开关窗**。
2. **逻辑 / 物理坐标分工**：布局与命中吃**逻辑**像素，pixmap 与 softbuffer surface 吃**物理**像素
   （= 逻辑 × `scale`）。`Rasterizer` 把 `scale` 组合进批次场景变换；脏区先乘 `scale` 再整数对齐；
   winit 侧的光标位置要**除以 `scale`** 才是 `InputEvent` 的坐标。`ScaleFactorChanged` 只重建 pixmap、
   **不重排**。
3. **`RepaintHandle` 不是全局单例**：它是 `EventLoopProxy<AppEvent>` 的包装（`Clone + Send`），
   由 `App::run_with_handle(|h| ..)` 在进入循环前交出 —— 后台线程用它
   `post_external(window, data)` + 唤醒；`Signal` 是 `!Send`，所以跨线程只能"投递数据 + 唤醒"，
   由 UI 线程在 `ViewModel::on_external` 里落到 `Signal`（`ExternalData` 因此必须是 `Send`）。
   `winit` 是可选特性（`--no-default-features` 可完全无头编译）。

- **不同窗口可以是不同的 VM 类型**：`.window(cfg, vm)` 接受任何 `impl ViewModel + 'static`，
  内部包成 `Rc<dyn WindowView>`（§3.5 的薄擦除层）。因此 `App` **没有任何泛型参数**。
  > 实现细节：`Rc<V> → Rc<dyn WindowView>` **不能直接强转**（Rust 的 unsizing 要求 `V: WindowView + Sized`，
  > 而 `view` 的 receiver 是 `&Rc<Self>`，拿不到 `Rc` 就调不了），所以擦除由一个持有 `Rc<V>` 的适配器完成：
  > `struct VmAdapter<V>(Rc<V>)` + `fn erased<V>(Rc<V>) -> Rc<dyn WindowView>`（代价：每次调用多一跳指针）。
- **跨窗口共享**：Signal 由 `Runtime` 创建，`clone()` 给各窗口的 VM 即可；不想共享就各建各的。
  （现状是"每窗口一份 `StateMap`，状态不串味但也无法共享"——现在是显式选择。）
- **`Signal::set` 的传播**：R1 不做依赖追踪 → **保守地把所有窗口标 `VIEW`**。
  代价是其他窗口也重跑一次 `view()` + `align`；因为 align 是逐字段比，未用该 signal 的窗口几乎全是 no-op。
  2~3 个窗口完全可接受。**将来若要精确**：给 `Signal` 加 `seen_by: SmallVec<WindowId>`（`get()` 时登记当前窗口、
  窗口关闭时批量清理）——即"窗口粒度的粗追踪"，**不引入观察者图**。现在不做。
- **动态开/关窗**：`cx.open_window(cfg, vm)` / `cx.close_self()`。
  命令进 `Runtime` 上的 `pending_windows: RefCell<Vec<PendingWindow>>`，`App::frame` 开头 drain。
  创建 VM 需要 Runtime：`cx.runtime() -> Runtime`（`Runtime` 是 `Clone` 的 `Rc` 句柄）。
- **窗口事件路由**：winit 的 `WindowEvent` 自带 `WindowId` → 直接定位 `windows[w]`；
  `Ctx` 也自带 `WindowId`，所以 `cx.damage/close_self/...` 的去向都是**显式**的，不需要"当前窗口"thread_local。
- **每窗口配置**：`WindowConfig`（title / size / min_size / max_size / resizable / decorations / icon / position /
  always_on_top）+ 每窗口的 `.close_guard(..)` / `.theme(..)` / `.font(..)` / `.external_source(..)`。

| 现状 | v3.3 |
|---|---|
| `Application { windows: HashMap<WindowId, WindowContext> }`，每窗口一套 runtime/renderer/state/标志 | `App { windows: Vec<WindowCtx> }`，每窗口一套 Track/Renderer/Surface/dirty/view_buf；**共享** Runtime 与 Signal |
| per-window 标志路由（`CURRENT_WID` thread_local + 全局 `FLAGS: Mutex<HashMap>` + 无当前窗口时广播） | 脏标志是 `WindowCtx.dirty` 普通字段；`Ctx` 自带 `WindowId`；**0 个 thread_local** |
| `RedrawRequested` 里 `rendered_once / size_mismatch / rebuild_requested` 三分支 | 只剩"看 `dirty` → 跑对应阶段" |
| 层命令走全局 `PENDING_LAYER` → 路由到"当前窗口" | 层在 `view()` 里声明 → 天然属于该窗口（§3.15） |

**已知限制（明确不做）**：不做跨窗口拖拽 / 共享 GPU 表面 / 窗口间 z 序控制；不做子窗口（child window）。

### 3.15 层：Modal / Overlay（水印）/ Popup / Tooltip / DragPreview

**核心规则：层是"在 `view()` 里声明、且在其父层内部声明"的根。** 声明式 + 嵌套，z 由嵌套深度决定。

```rust
fn view(self: &Rc<Self>, v: &mut View<'_>) {
    // ① 内容：最外层调用自动成为该窗口的 Content 根（每窗口恰一个）
    v.column(|c| { c.padding(16.0); c.text("主界面"); });

    // ② 水印 / 装饰层：默认命中穿透（盖住内容但不吃点击）
    v.overlay(|o| { o.watermark("CONFIDENTIAL"); });

    // ③ 锚定浮层：锚点是**节点 key**（布局后才解析）；点外部关闭 → 发 Dismissed
    if self.menu_open.get() {
        v.popup_at("menu-btn", Placement::Below, |p| {
            p.dismiss({ let me = Rc::clone(self); move || me.close_menu() });
            p.menu(|m| { m.item("全部").on_tap(act(self, Self::filter_all)); });
        });
    }

    // ④ Modal：默认 backdrop + 阻断下层
    if self.confirm.get() {
        v.modal(|m| {
            m.card(|c| { c.text("确认退出？"); c.button("确定").on_tap(act(self, Self::confirm_exit)); });
            // 在 modal **内部**声明的浮层自动高于该 modal
            if self.sub_open.get() { m.popup_at("more", Placement::RightOf, |p| { /* … */ }); }
        });
    }

    // ⑤ 拖拽预览：最高层
    if let Some(p) = self.dragging.get() { v.drag_preview(|d| { d.text(&p.label).at(p.pos); }); }
}
```

**z 规则**：`z = (Layer 枚举序, 嵌套深度, 声明序号)`。所以在 modal 内声明的 popup 天然高于该 modal。

**声明式带来的四个删除**（全部是现状里真实存在的机制）：

| 现状机制 | v3.3 怎么取代 |
|---|---|
| `LayerStack.popup_parent / popup_children` + `popup_descendants` + `close_popup` + `PENDING_POPUP_HIDE` | 父层消失 = 声明它的 `if` 为假 → **整棵子树（含嵌套子层）一起 `unmount`** |
| `PENDING_LAYER` thread_local + `LayerCmd::Show/Hide` + "下一帧消费" | 层在 `view()` 里声明，**同一帧生效**，无队列 |
| "锚点 rect 还没算出来"（必须先拿到 `ctx.current_rect()` 才能 `show_popup_with`） | `popup_at(key, placement, ..)` 用**节点 key** 作锚点，框架在布局后解析 |
| `hit_test_top` 内顺带做 dismiss 副作用（非纯函数） | 命中测试仍是**纯函数**；dismiss 判定在 `App` 里做，把 `Dismissed` 事件发给该层根 → 用户闭包改状态 |

**每层可配项**（默认值由 `Layer` 给出）：

| 选项 | Modal | Popup / Tooltip | Overlay(水印) | DragPreview |
|---|---|---|---|---|
| `backdrop` | 半透明黑（默认开） | — | 可选 | — |
| `blocks_below` | **true** | false | false | false |
| `dismiss_on_outside_click` | false | **true** | false | false |
| `hit_test_visible` | true | true | **false**（穿透） | false |
| `anchor` | —（居中由层语义给出） | 由 `popup_at(key, placement)` 给 | — | — |

**层根也要参与对齐**：对齐键 = `(owner, Layer, 层内声明序号)`；
消失的层根 → `unmount`（整棵子树）；新增 → 新建；同一个层根内则按普通节点规则对齐。
**Modal/backdrop 出现或消失时把窗口矩形（而非小矩形）并入脏区。**

**水印的边界（诚实）**：`v.overlay(..)` 永远是**窗口级**的根，不能"只盖住某个子区域"。
需要局部水印/着色的场景，用普通节点 + `.hit_test_visible(false)` 在内容里声明（跟着内容滚动与裁剪）。
`o.watermark("CONFIDENTIAL")` 只是 `widgets/` 里的一个组合函数（平铺 + `rotate(30°)` + 低不透明度 + 不吃事件）。

**与 WinUI 对照**：`ContentDialog` ≈ `v.modal`；`Flyout / Popup` ≈ `v.popup_at`；`ToolTip` ≈ `v.tooltip_at`；
SystemOverlay / 水印 ≈ `v.overlay`；拖拽预览 ≈ `v.drag_preview`（WinUI 由系统 `DragUI` 绘制，我们自绘）。

---

## 四、Rust 生命周期合规检查表（对应需求 2）

| 潜在违规 | v3 的做法 |
|---|---|
| 自引用树 / 父子互指 | arena + `NodeId`（整数），父子关系都是 id；统一 `&mut Track` |
| `Rc<dyn Widget>` 环 / `RefCell` 字段 | 不存在 widget 实例树；用户态无实例、无句柄（`view()` 产出描述，框架持保留树） |
| 遍历中改树（借用冲突） | 显式两段式：只读收集 → 可写应用；内部 API 接 `NodeId` 而非 `&Node` |
| 用户态要"同时拿到树与模型" | **不需要**：`view()` 只读状态出描述；事件闭包只改状态。两者都不碰保留树 |
| **闭包捕获 `&mut self`** | 闭包只捕获 `Signal`（`Rc`）/ `Rc<Self>` / `Copy` 数据；`Handler = Rc<dyn Fn(&mut Ctx)>`，**不拿 `&mut Track`** |
| **`Rc` 循环 / 内存泄漏** | 闭包存在**保留树**里、指向 VM（`Track → 闭包 → Rc<VM>`，单向）；**VM 不持有闭包** → 无环。这是唯一要守的规矩 |
| **`view()` 内部改状态** | 禁止：会自我触发循环；Runtime 在 `view()` 期间被置 `VIEW` 时 **debug 断言**拦下 |
| thread_local 全局句柄 | **0 个**；`Signal` 自带 `Rc<Runtime>`，信号走 `Ctx::request_repaint()` + `RepaintHandle` |
| dyn trait + `&mut Track` 冲突 | 框架自有组件用 `Kind` 枚举 + `match`；`CustomNode` 的所有 hook 都不拿 `&mut Track`（只拿 `Cmd`） |
| 泛型传染 | 框架里只有 `Signal<T>` 带类型参数；`App`（`Rc<dyn WindowView>` 擦除）/`View`/`Track`/`Node`/`Runtime`/`Ctx` 全部非泛型 |
| 每帧 clone 大对象 | 保留树持久；描述 arena 跨次复用；draw list 全量遍历但**逐原语按脏区剔除**（带裁剪的子树整体跳过）；文本排版 `Arc` 缓存（`(内容,规格,颜色)` 哈希） |
| `Signal: !Send`（跨线程） | 后台线程经 `RepaintHandle`/channel 回主线程写 signal；不引入 `Send` 运行时 |
| 不安全代码 | `#![forbid(unsafe_code)]`（现状也无 unsafe） |

#### 借用纪律（写进实现规范，违反会 panic 或死循环）

1. **`Signal::with(|v| ...)` 回调内不要 `set()`** → 同槽 `RefCell` 借用冲突 panic。`get()` 返回克隆，没这个问题。
2. **`view()` 内禁止 `set()`** → 自我触发循环；debug 下断言 `VIEW` 未被置位（§3.3 第 2 条约束）。
3. **事件分发必须两段式**：先只读收集 `Handler`（`Rc::clone`）→ 释放 `&Track` 借用 → 再逐个调用。
4. **`CustomNode::measure` 需要 `&mut TextService`**：用字段级 disjoint borrow（`&mut track` 与 `&mut text` 分开取），
   不要经 `track.xxx()` 方法间接访问（那样会把整棵 track 借走）。

---

## 五、简化收益对照

| 维度 | 现状（feature/mvp） | v3.3 |
|---|---|---|
| UI 表示 | 4 重（Widget / ViewNode / ElementTree / VisualElement）且每帧全量重建 | **2 个角色**：保留树 `Track`（含视图态）+ 描述 `View<'_>`（仅状态变化时产出，缓冲复用）+ 只读 draw list |
| 用户态 API | 持有 `State<T>` / `use_state` / 每帧写 builder | `Signal` 字段 + `view(self: &Rc<Self>)` + 事件闭包；不出现 `NodeId`、不调用 patch、不需要 `Msg` |
| 触发方式 | `State::set` → 全局 `request_rebuild` → 全量重建 | `Signal::set` → `dirty.VIEW` → 重跑 `view()`（**批处理免费**：一次事件改 N 个 Signal 只重跑一次） |
| 状态机制 | 3 轨（`State<T>` / `use_state` / `ElementEntry`） | **2 处且边界清晰**（业务状态在 `Signal`；视图态在保留树） |
| reconciler / `key` / `Patch` / `tree_eq` | 有（322 行 + 全树 eq + `listeners_sig_eq` hack + 跨父 `Move` + `type_name` 回退匹配） | **只剩"位置 + 一层 key"对齐**（见 §3.4），无实例表、无全树比较 |
| 帧入口 | 3 + 补丁兜底 + 多窗口三分支 | **1**（`Dirty` 位标志；每窗口 4 步） |
| 全局信号 | 全局 `Mutex<HashMap>` + 4 个 thread_local + 广播兜底 | **0**（`Signal` 自带 `Rc<Runtime>`；脏标志是 `WindowCtx.dirty` 字段） |
| 多窗口 | `HashMap<WindowId, WindowContext>` + per-window 标志路由（thread_local + Mutex + 广播） | `Vec<WindowCtx>`；`Runtime`/`Signal` 共享，其余每窗口一套；`Ctx` 自带 `WindowId`（§3.14） |
| 图层 | 6 kind + 每类 seq + `LayerStack` 混杂 tree/event/popup 图 + 2 个待处理队列 + `hit_test_top` 内做 dismiss | `roots: Vec<Root>`，层在 `view()` 里**声明式嵌套**；z = (Layer, 嵌套深度, 序号)；父层消失即级联消失（§3.15） |
| 层命令 | `PENDING_LAYER` + `LayerCmd` + `PENDING_POPUP_HIDE` + "下一帧消费" | **无队列**：层同帧声明同帧生效 |
| 事件状态 | 每节点 `ElementState` + `hovered_listeners` 路径集 | 每节点 `interaction`（≈ `IsPointerOver/IsPressed`）+ `Track.captures` |
| 回调 API | 9 份 `on_click` 拷贝 + 2 种回调存法 | 统一 `Handler = Rc<dyn Fn(&mut Ctx)>`；用户态只需 `on_tap` / `on_tap_with` 两个 |
| 内置行为 | 8 个文件把闭包塞进树（`.builtin()`） | 同一种槽位 + `handled_events_too = true`，按 `Kind` 分派 |
| 延迟写入通道 | `EventContext` effects + `capture_mouse` + `begin_drag` + `PENDING_LAYER` + `PENDING_POPUP_HIDE`（5 个） | **1 个** `Cmd` 命令缓冲 |
| 事件种类 | 17 个 `EventType` | 约 40 个 `EventKind`（补 `PointerCaptureLost`/`CharacterReceived`/`Loaded`/`BringIntoViewRequested`/`ContextRequested` 等缺口），但机制只有 1 套 |
| 用户扩展 | `impl Widget for MyThing`（每帧重建、状态靠 `use_state`） | A 组合 = 普通函数（零机制） / B/C `Kind::Custom`（见 §3.12） |
| 脏区来源 | （dirty-surface）元素签名哈希比较，无法定位则整屏 | patch 发生时**精确登记** |
| 依赖 | winit/softbuffer/vello_cpu/parley/kurbo/slotmap/arboard/image | 去掉 `slotmap`（自研 arena） |
| 代码量 | `src/` 约 22k 行 | 约 8~9k 行（响应式 +250 行、对齐层 +400 行，换掉 reconciler/StateMap/Msg 三层机制） |

> **代价的诚实交代**（R1 的取舍，已知且接受）：
> 1. **任何 `Signal` 变更都会重跑整个 `view()`**（不追踪依赖）。在"简易 GUI"的规模下这是可接受的：
>    重跑只做"读状态 + 写描述 arena + 逐字段比"，不涉及光栅化；且 hover/滚动/动画不触发它。
> 2. 事件处理必须用**闭包**（Rust 里没有 Vue 那种模板编译）。
> 3. `Signal` 是 `Clone`（`Rc`）不是 `Copy`；`!Send`（跨线程走 `RepaintHandle`）。
> 缓解手段：① 描述 arena 跨次复用（只重置游标）；② 未变化节点零操作（逐字段比，不做全树 `tree_eq`）；
> ③ 逃生口：需要"只重绘不重跑 `view()`"时用 `cx.damage(key)` / `cx.invalidate()`。
> **若将来真的需要细粒度**（属性级绑定观察者），是纯增量升级——用户代码不用改（§3.4、§七）。

---

## 六、迁移路线

| # | 内容 | 产出 | 风险 |
|---|---|---|---|
| **M0** | 抽 crate 骨架：**从当前分支抽出** `layout/{flex_node,flex_line,style,types,box_model,measurable,constraint}` + `text/mod.rs` + `geometry/`（见下方复用清单） | 可编译的空壳 + 复用资产（~2.3k 行直接可用） | 低（这些文件是纯计算，已被 19 个测试套件覆盖） |
| **M1** | `reactive.rs` + `track.rs` + `view.rs` + `align.rs` + `cmd.rs` + `event/`：`Runtime`/`Signal`/`act` + arena/Node/Flags + **层栈（`roots`/`owner`/`LayerOpts`/z 规则）** + 描述 arena + 对齐（位置/一层 key/层根）+ 视图态 + 路由分发 + `Handler`/`Ctx` + `Cmd` + 焦点/捕获；**纯单测，不依赖窗口**（`Ctx` 用占位 `WindowId`） | v3 地基（可测试） | 中：`Signal`/`view`/对齐三处 API 定稿后返工成本高 |
| **M2** | `layout.rs`：`LayoutTree` 适配器 + `Measure/Arrange` 失效 + 重排边界；命中测试（含 transform/clip + 层 z 降序 + `blocks_below`/穿透） | 布局/命中可单测 | 中：重排边界与变换命中用例 |
| **M3** | `render/`：draw list 展开（含多层） + **每窗口**持久 Pixmap + 脏区 + `present_with_damage`；backdrop/半透明层合成；移植 dirty-surface 的契约与单测 | 窗口内可见 | 中：残影/z 序/alpha 契约 |
| **M4** | `app/`：`App`（`Rc<dyn WindowView>` 擦除）+ **多窗口 `WindowCtx` 表** + `Runtime` 接线 + 动态开关窗 + winit + IME/拖拽/关闭守卫/clipboard + `RepaintHandle` | 端到端可跑（含多窗口 + modal） | 中（逻辑从现状搬迁 + 多窗口重构） |
| **M5** | `widgets/` + `custom.rs`：按复杂度递增迁移（Text/Box/Button → Checkbox/Switch/Radio/Progress → Scroll/VirtualList → Menu/Popup → Input/Slider）；`CustomNode` 与 `Kind::Custom` 在 M1 之后即可先行开放 | 功能对齐 + 扩展点可用 | **高**：Input（IME/选区/动画）、VirtualList、菜单翻转最重 |

原则：**M1~M4 期间不改动现有分支**，新开 `examples/v3_*.rs` 验证；M5 尾声再统一切换 `prelude`。每步 `cargo test` + `clippy` 全绿。

### 复用清单（按"复用现有移植好的实现"口径）

**直接抽用（不重写）**——全部来自当前分支，是已经验证过的纯计算代码：

| 来源（当前分支） | 行数 | 处理方式 |
|---|---|---|
| `src/layout/flex_node.rs` | 956 | 原样搬（Taitank 式 Flex 求解核心） |
| `src/layout/flex_line.rs` | 298 | 原样搬 |
| `src/layout/style.rs` | 556 | 原样搬（`FlexStyle` 即节点 `layout` 字段类型） |
| `src/layout/types.rs` | 201 | 原样搬 |
| `src/layout/box_model.rs` | 146 | 原样搬（`ComputedLayout`/`IntrinsicSize`/`LayoutConstraint`） |
| `src/layout/measurable.rs` + `constraint.rs` | 124 | 原样搬（`TextMeasure` 等） |
| `src/text/mod.rs` | 196 | 搬（parley 封装 + 图标字体 + `TextLayout` 缓存）；`TextService`/`FontContext`/`LayoutContext` 所有权重构为独立服务对象 |
| `src/geometry/types.rs` | 120 | 原样搬（`Point/Size/Rect/Color`） |
| `src/view/paint.rs` | 243 | 搬（`PaintStyle`/`TextStyle`/`ImageStyle`，去掉与 `ViewNode` 的耦合） |
| `src/render/visual.rs` | 331 | 搬（`VisualElement` = 我们的 draw list 元素类型） |
| `src/render/engine.rs` | 531 | 搬绘制原语（shadow/shape/clip/blit_image），**替换**全量 Pixmap 为持久 + 脏区（M3） |
| `src/theme.rs` | 296 | 搬 token 结构，改为 `Track` 的字段（去掉 thread_local `current()/set()`） |
| `assets/`（Material Icons 字体等） | — | 原样搬 |

**不搬（要新写或改造）**：
- `src/layout/context.rs`（146 行）——它把 `ElementTree` 编译成 `FlexNode`，与旧树耦合；
  新写 `layout.rs` 适配器（~200 行）实现 `LayoutTree`（`style_of/collect_children/measure/is_text/scroll_offset`，全部 `&mut self`，
  让实现方持有布局缓存与文本测度缓存）。trait 形状照抄 `newworld` 已验证的版本，**但不搬它的 `props/` 属性表**。
- `src/runtime/*`、`src/view/node.rs`、`src/widget/*`、`src/event/*`、`src/core/layers.rs`、`src/app.rs`、`src/state.rs` —— 全部重写（见 §一、§三）。

**从其他分支取**：
- `dirty-surface`：持久 Pixmap、`present_with_damage` 分支策略、条带局部光栅化的 vello_cpu 用法、像素契约（premul/straight + src-over）、残影回填、MT 光栅化 feature（**只取这些**）。
- `newworld`：`LayoutTree` trait 的形状、重排边界（`mark_layout_dirty` 冒泡 + `normalize_boundaries` + 最多 2 轮）的思路、`lieui-text` 两段式测度与 FNV 缓存键的结论（**不搬代码与 `props/`**）。
- 方案 B：**理念**（状态归组件、单轨状态、删 `key` 与 key 匹配）。

**不取**：
- `Rc<dyn Widget>` 持久树 + `RefCell/Cell` 字段 + `&self build` + 每帧投影（方案 B 主体）；
- `Compositor`/`ExternalSource` 的双通道与全局单例、thread_local surface 注册表（dirty-surface）；
- `props/` 列式属性表 + CSS 继承 + 伪类（newworld）——过度设计；
- 6 层 `LayerStack` + `PENDING_LAYER`/`PENDING_POPUP_HIDE` 队列（改为 `Cmd` + `Track.roots`）。


---

## 七、风险与明确不做的事

| 风险 | 说明 | 缓解 |
|---|---|---|
| 组件能力回退 | Input（IME/选区/光标动画）、VirtualList、菜单防溢出翻转逻辑最重 | M5 逐组件迁移 + 保留现状测试场景；M1~M4 不动旧代码 |
| 脏区正确性 | 残影、z 序、alpha 混合、scale 后旧矩形未清 | 契约写进 render 单测（dirty-surface 已提供 8 例旁证） |
| **对齐错配** | 结构随数据变化而未走 `keyed_list` → 节点错位、视图态串台 | 对齐时做**类型判别式检查**：类型不同即重建该位置子树（宁可丢视图态，不可错配）；debug 下加"位置对齐异常"断言 |
| **`view()` 每次变更全量重跑** | R1 不追踪依赖，改一个 Signal 也重跑整棵描述 | ① 描述 arena 跨次复用（只重置游标）；② 逐字段比后未变化节点零操作；③ `cx.damage(key)`/`cx.invalidate()` 逃生口（只重绘、不重跑）；④ 若将来确实需要细粒度，加属性级绑定观察者即可（向后兼容） |
| **`view()` 内误改状态** | 自我触发循环 | debug 断言：`view()` 期间 `VIEW` 被置位即报错（§4 借用纪律 2） |
| **闭包数量与内存** | `view()` 每次都新建闭包 → 节点 handler 槽整体替换、旧闭包释放 | 节点数级别的 `Rc` 分配，与现状"每帧重建 listener 闭包"同量级但频率低得多（仅状态变化时） |
| 布局直吃树的性能 | 引擎按边界重建小树的开销 | 现有 Taitank 移植已在 19 个测试套件上验证；配合脏边界只做局部（改造后需重跑基准） |
| 基准未兑现 | dirty-surface 实测小脏区 17.1ms/帧（debug/opt-level=1，结论不可信） | M3 后建立 `examples/perf_v3.rs`，以 release 基准验收（小脏区 ≤ 全量的 20%） |

**明确不做（定位：简易 GUI 库）**
1. 不做立即模式（需求 1 已排除）；
2. 不做通用 diff/reconciler（对齐只需"位置 + 一层 key"，无实例表、无跨父 `Move`、无 `type_name` 回退匹配）；
3. **不做细粒度响应式**：不实现依赖追踪、属性级绑定观察者、片段作用域（**R1 就够**）；
4. 不做 `Computed`/`Derived`（`view()` 重跑时重算即可；真需要再谈）；
5. 不做**框架自有**组件的 trait 对象化（框架自有用 `Kind` 枚举 + `match`；用户扩展走 `Kind::Custom`，见 §3.12）；
6. 不做 thread_local 全局状态（0 个，含主题）；
7. 不做 CSS 选择器/继承/属性表（含 WinUI 的依赖属性系统）；
8. 不做"每帧重建视图表达"——`view()` 只在状态变更后跑；hover/滚动/动画/每帧都不经过它；
9. 不做控件模板系统（WinUI 的 `Control.Template` / `VisualStateManager`）——组合 + 自绘已够；
10. 不做虚拟化超大列表（`keyed_list` 只做 key 匹配；几十~几百行规模够用）；
11. 不做跨窗口拖拽 / 共享 GPU 表面 / 窗口间 z 序控制；不做子窗口（child window）（§3.14 已知限制）；
12. 不做"局部的窗口级浮层"——`v.overlay(..)` 永远是整窗口的根；局部水印用普通节点 + `hit_test_visible(false)`（§3.15）。

---

## 八、与既有文档的关系

| 文档 | 关系 |
|---|---|
| `docs/architecture.md` | 现状基线，v3 落地后整体替换 |
| `complexity-audit-and-simplification.md`（方案 B） | **关系最近**：v3.3 采纳了它的"声明式投影 + 状态归组件 + 删 `key`/reconciler"主张，并进一步用 `Signal` 把"状态变更 → 重投影"变成**自动触发**。差异：保留树在框架 arena（不是 `Rc<dyn Widget>`）、描述由框架分配复用、`view()` 不在每帧跑、视图态在保留树（不是 widget 的 `RefCell` 字段）；对齐只需"位置 + 一层 key"（方案 B 仍需按 key/type diff）。理由见 §2.1 |
| `performance-refactor.md`（阶段 A/B/C） | 阶段 A 的 SharedSurface/Compositor 收敛为 `Kind::Surface` + 渲染层脏区（不再并行通道）；阶段 B 的"按需重建/状态订阅"由"`view()` 只在 `Signal` 变更后跑"天然满足，不需要 `RebuildScope` |
| `dirty-surface-review.md` | 其阶段 1/2 的技术结论作为 v3 渲染层（M3）的实现依据 |
| WinUI `UIElement` / `FrameworkElement` | 属性集、路由事件、指针捕获、焦点、`Measure/Arrange`、`BringIntoView` 的对齐来源（§3.13 逐项列出借鉴/裁剪/不采纳）；`Signal` ≈ `INotifyPropertyChanged`（但自动） |

---

## 九、示例（API 契约，也是 M1 的验收目标）

> 这些代码**目前不可编译**——它们是 API 契约。M1 完成后应能原样编译并通过。

### 9.1 最小例子：counter

```rust
// examples/counter.rs
use lieui::prelude::*;
use std::rc::Rc;

#[derive(Default)]
struct Counter { count: Signal<i32> }

impl Counter {
    fn inc(&self) { self.count.update(|v| *v += 1) }   // ViewModel 的命令
    fn dec(&self) { self.count.update(|v| *v -= 1) }
}

impl ViewModel for Counter {
    fn view(self: &Rc<Self>, v: &mut View<'_>) {
        v.column(|c| {
            c.center().gap(12.0);
            c.text("Counter").font_size(48.0);
            c.text(self.count.get().to_string()).font_size(72.0).color(Color::RED);
            c.row(|r| {
                r.gap(12.0);
                r.button("-1").on_tap(act(self, Self::dec));
                r.button("+1").on_tap(act(self, Self::inc));
            });
        });
    }
}

fn main() -> lieui::Result<()> {
    let rt = Runtime::new();
    let vm = Counter { count: Signal::new(&rt, 0) };
    App::new(rt)
        .window(WindowConfig::new().title("Counter").size(400.0, 300.0), vm)
        .run()
}
```

### 9.2 常用写法速查

> 以下为片段：`view` / `on_tick` / `on_external` 都是 `ViewModel` 的方法；闭包体省略。

```rust
// 双向绑定（≈ v-model / WinUI TwoWay）
c.input_bind(self.name.clone()).placeholder("姓名");
c.slider_bind(self.volume.clone(), 0.0, 100.0);
c.checkbox_bind(self.done.clone());

// 条件：普通 if（view() 重跑时求值）
if self.loading.get() { c.text("加载中…"); } else { c.text("就绪"); }

// 列表：唯一需要 key 的地方
v.keyed_list(self.items.get(), |i| i.id, {
    let me = Rc::clone(self);
    move |v, i: &Item| {
        let me = Rc::clone(&me);
        let id = i.id;
        v.row(|r| {
            r.text(&i.title);
            r.icon_button(IconName::Close).on_tap(move || me.remove(id));
        });
    }
});

// 浮层：声明式（锚点按 key 在布局后解析）；开关由状态驱动
if self.menu_open.get() {
    v.popup_at("menu-btn", Placement::Below, |p| {
        p.dismiss({ let me = Rc::clone(self); move || me.close_menu() });
        p.menu(|m| { m.item("全部").on_tap(act(self, Self::filter_all)); });
    });
}

// Modal：backdrop + 阻断下层（默认）；层内声明的浮层自动更高
if self.confirm_exit.get() {
    v.modal(|m| {
        m.card(|c| {
            c.text("确认退出？");
            c.row(|r| {
                r.button("取消").on_tap(act(self, Self::cancel_exit));
                r.button("确定").danger().on_tap(act(self, Self::do_exit));
            });
        });
    });
}

// 水印 / 装饰层：覆盖内容但**命中穿透**（点击落到下面的内容上）
v.overlay(|o| { o.watermark("CONFIDENTIAL"); });

// 局部水印（只盖住一块区域）：用普通节点 + 不吃命中，跟着内容走
c.stack(|s| {
    s.text("正文…");
    s.text("DRAFT").opacity(0.08).rotate(-30.0).hit_test_visible(false);
});

// 只重绘、不重跑 view()（动画 / 高频视觉）
fn on_tick(self: &Rc<Self>, cx: &mut Ctx, _now: Instant) {
    self.phase.update(|p| *p += 0.02);
    cx.damage(key!("chart"));                                   // 只把该节点矩形并入脏区
    cx.request_repaint_after(Duration::from_millis(16));
}

// 外部数据（PTY / 后台线程）
fn on_external(self: &Rc<Self>, cx: &mut Ctx, data: ExternalData) {
    if let Some(chunk) = data.downcast::<PtyChunk>() {
        self.term.feed(&chunk.0);
        cx.damage(key!("term"));       // surface 内部已有自己的 damage，这里只补充节点矩形
    }
}
```

### 9.3 声明式的可测试性

```rust
#[test]
fn counter_inc() {
    let rt = Runtime::new();
    let vm = Counter { count: Signal::new(&rt, 0) };
    vm.inc();
    assert_eq!(vm.count.get(), 1);       // 命令方法可脱离窗口单测
}
```

### 9.4 多窗口 + 共享状态 + Modal + 水印

```rust
// 跨窗口共享状态：就是一个装 Signal 的普通 struct（Clone 便宜）
#[derive(Clone)]
struct Shared {
    docs: Signal<Vec<Doc>>,
    confirm: Signal<Option<usize>>,      // 待确认删除的下标（None = 不显示 Modal）
}
impl Shared { fn new(rt: &Runtime) -> Self { /* Signal::new(rt, ..) ×2 */ } }

// ── 主窗口 ──
struct MainVm { sh: Shared }

impl ViewModel for MainVm {
    fn view(self: &Rc<Self>, v: &mut View<'_>) {
        v.column(|c| {
            c.row(|r| {
                r.spacer();
                r.button("打开面板").on_tap_with({
                    let sh = self.sh.clone();
                    move |cx: &mut Ctx| {                        // 需要 cx 时用 on_tap_with
                        cx.open_window(
                            WindowConfig::new().title("面板").size(320.0, 480.0),
                            PanelVm { sh: sh.clone() },
                        );
                    }
                });
            });
            v.keyed_list(self.sh.docs.get(), |d| d.id, { /* 见 §9.2 */ });
        });

        v.overlay(|o| { o.watermark("INTERNAL"); });             // 水印：命中穿透

        if let Some(i) = self.sh.confirm.get() {                 // Modal：backdrop + 阻断下层
            v.modal(|m| {
                m.card(|c| {
                    c.text("删除这篇文档？");
                    c.row(|r| {
                        let sh = self.sh.clone();
                        r.button("取消").on_tap(move || sh.confirm.set(None));
                        let sh = self.sh.clone();
                        r.button("删除").danger().on_tap(move || {
                            sh.docs.update(|d| { d.remove(i); });
                            sh.confirm.set(None);
                        });
                    });
                });
            });
        }
    }
}

// ── 面板窗口：**另一个 VM 类型**，与主窗口共享同一份 Shared ──
struct PanelVm { sh: Shared }

impl ViewModel for PanelVm {
    fn view(self: &Rc<Self>, v: &mut View<'_>) {
        v.column(|c| {
            c.text("面板");
            c.text(format!("共 {} 篇", self.sh.docs.get().len()));
        });
    }
}

fn main() -> lieui::Result<()> {
    let rt = Runtime::new();
    let sh = Shared::new(&rt);
    App::new(rt)
        .window(WindowConfig::new().title("文档").size(960.0, 640.0), MainVm { sh: sh.clone() })
        .window(WindowConfig::new().title("面板").size(320.0, 480.0), PanelVm { sh })
        .run()
}
```

要点：
- `Shared` 是**普通 struct + Signal 字段**；跨窗口共享 = `clone()` 给两边（不想共享就各建各的）。
- 两个窗口是**不同的 VM 类型** → 靠 `Rc<dyn WindowView>` 擦除（§3.14），`App` 本身不带泛型。
- 任一窗口改 `sh.docs` → 两个窗口都会重跑各自 `view()`（R1 保守标脏）；`align` 逐字段比让没变的那个窗口几乎零成本。
- Modal 是**主窗口的层**，不影响面板窗口（层属于它所在的那棵 `Track`）。
- `cx.open_window(cfg, vm)` 里建 VM 需要 Runtime 句柄：`cx.runtime()`。

