# LieUI v5 架构设计：单树 · 单画布

> ⚠️ **本稿已作废（2026-11-05）**：评审决定采用 Compositor 合成模式并简化 `Kind`/widget 模型，
> 现行方向见 `docs/architecture-v6.md`。本文件仅作决策记录保留；其中 §0.2、§8「明确不做」、
> ADR-v5-001 与 P1 滚动方案均不再适用。
>
> 版本：v5.0-draft | 日期：2026-11-05 | 状态：**已作废**
> 初始想法：`docs/architecture-v4.md`（持久控件树 + 持久渲染层树 + Compositor）
> 实现基线：`lieui 0.1.0-alpha.4` 当前源码（retained view tree + R1 响应式 + 纯 CPU 脏区渲染）
> 校准依据：`docs/refactor-plan.md` v1.2 实测结论、`docs/audit-0~3.md`、`docs/operation-log.md`
>
> **一句话**：接受 v4 的**目标**（静止零开销、局部更新、滚动/动画不重排、纯 Rust / 纯 CPU），
> 放弃 v4 的**手段**（三棵树、层缓存、合成器、tile 网格、后端 trait）。
> 当前实现已经用「一棵保留树 + 一张持久画布 + 一个脏区列表」达成了目标的大半，
> v5 要做的是**把这条路径走完并收窄**，而不是再加一层架构。

---

## 0. 从 v4 出发：保留什么、砍掉什么

### 0.1 保留 v4 的目标与三条原则

| v4 条目 | v5 的落法 | 当前实现状态 |
|---|---|---|
| 控件树是唯一真实源（§2.1） | `Track` 是唯一所有者；布局几何、Scene、Pixmap 全是派生 | ✅ 已成立 |
| 失效驱动，不轮询（§2.3） | `Dirty` 位集 + `next_wakeup()`；无唤醒源即 `ControlFlow::Wait` | ✅ 已成立 |
| 静止时零提交（§1.3 / §13） | 无脏区 ⇒ 不 build Scene、不光栅化、不 present | ✅ 已成立（`tests/perf_regression.rs`） |
| 变换与重绘分离（§2.2 / ADR-003） | **降级**为「几何变化不触发布局」：只登记旧 ∪ 新脏矩形 | ⚠️ 部分：滚动仍走布局 |
| 层是缓存的边界（§2.5） | **降级**为「层只是 z 序根」：`Root.layer` + `Layer::ALL` | ✅ 语义已落地，无缓存 |

### 0.2 砍掉 v4 的机制（简化的主体）

| v4 设计 | v5 决策 | 理由 |
|---|---|---|
| 6 个 crate（`gui_core/layout/text/scene/compositor/runtime`） | 维持现有 4 个 crate，边界靠 `mod` 而非 crate | 已发布的 `lieui-geom/text/layout` + 主 crate 足够；拆 crate 只增加版本与发布成本 |
| 引入 `taitank-rs` 作为布局引擎（§7 / ADR-002） | 保留自研 `lieui-layout`（Taitank 风格、零依赖） | 已有 `feature_flex/feature_position/smoke` 特征矩阵测试；换引擎是纯成本。`measurable.rs` 回调机制已被主 crate 弃用，`FlexNode.measure_text` 直连 `TextEngine` 更简单 |
| 持久布局树镜像 + 严格同步（§6.1） | 无镜像；布局期按**脏边界**临时 `build()` `FlexNode` 子树，算完即弃 | `refactor-plan.md` v1.2 实测：**FlexNode 持久化收益被高估**；当前「边界重排」已做到只重建脏子树 |
| 持久渲染层树，每控件默认一层（§5.3 / §6.3） | 无层树；`Track.roots: Vec<Root>` 多根 + z 序排序 | 层缓存是 v4 复杂度的总根源：层提升、层合并、层提升后的 z 序维护、每层 Pixmap 生命周期 |
| `LayerStorage: Single/Tiled` + `TileCache`（§11.2 / ADR-006） | 无；滚动内容不做 tile 化 | 当前是持久单 `Pixmap`，视口外像素根本不分配；tile 化解决的是「整层缓存」带来的问题，而 v5 不产生整层缓存 |
| `DamageTracker` tile 网格 + `dirty_rects` 双表示（§5.4 / ADR-004） | **一个** `Track.damage: Vec<Rect>` + `damage_all: bool` | 两套损伤表示需要同步；矩形合并（`MERGE_GAP=2`）+ 退化阈值已够用 |
| Compositor：逐 tile 按 z-order `blit_clipped`（§9.2 / ADR-001） | 无合成器；`SceneBuilder::build → Rasterizer` 一步到底 | CPU 下每个脏 tile 仍要遍历相交层并 blit，省下的是 vello 光栅化；而 v1.2 实测 Scene 遍历仅 **0.061 µs/节点**（≈帧预算 1.2%），真正瓶颈是**绘制像素量**，不是遍历。为 1.2% 引入整套层管理不划算 |
| `LayerRasterizer` trait + 关联类型预留 `vello_gpu`（§9.4 / ADR-005） | 不做后端 trait；只在 `Rasterizer` / `present` 之间留一处数据源边界 | 只有一个后端。等真要接 GPU 时，改动集中在一个 `Rasterizer`，比现在维护一套未被验证的抽象便宜 |
| 两条独立失效通道（§10.1） | 合并为 `Dirty` 4 位 + 脏矩形列表，一次 `frame()` 消费 | `VIEW/LAYOUT/PAINT/PRESENT` 已表达全部语义；再分「布局通道 / 绘制通道」是同一信息的第二份拷贝 |
| 动画前层提升、动画后合并回父层（§12.3） | 不做；动画 = `request_animation` + `damage_key` 小脏区重绘 | 层提升的收益依赖层缓存，v5 无层缓存则无收益 |

### 0.3 实测依据（为什么"更简"是安全的）

- `examples/damage_bench.rs`：整窗 / 局部 / 空闲对比，局部重绘收益 **19–94×**；
- `tests/render_cost_bench.rs`：Scene 构建每节点仅 0.061 µs ⇒ **缓存 Scene（C4）不值得做**；
- `docs/refactor-plan.md` v1.2：方向应是「**减少绘制量 / 更细 culling**」，不是再加缓存层；
- `tests/perf_regression.rs`：一次滚动产生 302 个碎片，`merge_rects` 后压到个位数 ⇒ 矩形列表能撑住。

---

## 1. 设计约束与不变量

1. **一棵树**：`Track` 唯一持有节点、视图态、布局结果、脏区。描述与 Scene 都是瞬时产物。
2. **一个帧入口**：平台层只有 `RedrawRequested → WindowCtx::frame()`；`about_to_wait` / `user_event` 只 `pump()` 并按需 `request_redraw()`，判据统一为 `needs_frame(&rt)`。
3. **一个脏区列表**：所有失效最终表现为 `Vec<Rect>`（或 `damage_all`），由 `take_damage()` 一次性消费。
4. **一张画布**：一个窗口一个持久 `Pixmap`；没有第二份像素缓存。
5. **一个控件一处定义**：新增控件不应同时改 `view.rs` DSL、`track.rs` 三枚举、`widgets/mod.rs` 分派。
6. **单线程由类型系统保证**：`Rc/RefCell`、`#![forbid(unsafe_code)]`、零 thread_local 全局单例；跨线程只经 `Poster` 投递。

**复杂度预算**：核心概念数 ≤ 树 / 描述 / 脏位 / 脏区 / Scene / Pixmap / 时钟 / 层 z 序 共 8 个；不为「将来可能」预留 trait、句柄或第二份缓存。

---

## 2. 整体架构

### 2.1 分层视图

```
┌──────────────────────────────────────────────────────────┐
│ 应用：ViewModel::view(&Rc<Self>, &mut ViewBuf) + Signal<T>  │
└──────────────────────────────────────────────────────────┘
                 ↓ 仅当 Dirty::VIEW
┌──────────────────────────────────────────────────────────┐
│ ViewBuf（描述 arena，每帧 begin() 清空复用容量）→ align()    │
│ patch 发生的那一刻登记「旧矩形」                            │
└──────────────────────────────────────────────────────────┘
                 ↓
┌──────────────────────────────────────────────────────────┐
│ Track（保留树，唯一真实源）                                │
│  Node{desc 组, state 组, computed, flags} / Root{layer,owner}│
│  damage: Vec<Rect> / damage_all / hover/focus/pressed        │
└───────┬───────────────────────────────┬──────────────────┘
        │ Dirty::LAYOUT                  │ take_damage()
        ▼                               ▼
┌──────────────────────────┐  ┌──────────────────────────────┐
│ layout：脏边界 → 临时      │  │ damage_batches（合并 + 退化）   │
│ FlexNode → 写回 computed   │  │        ↓ 同一批矩形            │
│ 登记「旧 ∪ 新」矩形        │  │ SceneBuilder::build（cull）    │
└──────────────────────────┘  │        ↓ Scene: 扁平 Op 列表    │
                               │ Rasterizer：scratch → 持久 Pixmap│
                               │        ↓ present_with_damage    │
                               │ softbuffer / 无头 PNG            │
                               └──────────────────────────────┘
                 ↓ next_wakeup()
        ControlFlow::Wait（无唤醒源）/ WaitUntil(t)
```

对比 v4 的分层图：删掉了「布局树镜像」「渲染层树」「损伤追踪器」「Compositor」四个常驻组件，以及它们之间的三条同步通道。

### 2.2 一帧的固定顺序

`WindowCtx::frame()`（`src/window.rs:503`）保持现有六步，v5 只做**收窄**，不改顺序：

1. `rt.take_dirty()` → 主题同步（变化时整窗标脏）；
2. `Dirty::VIEW` ⇒ `begin_view` RAII 守卫 → `view_buf.begin()` → `view_erased()` → `align()`；
3. `Dirty::LAYOUT || track.has_layout_dirty()` ⇒ `layout::layout()`（边界重排）；
4. `place_anchored_layers()` → `ScrollChanged` 派发；
5. `take_damage()` → `Renderer::render()`（批次与 cull **同源**）→ `present_with_damage`；
6. `next_clock = rt.next_deadline()` → 平台层决定 `Wait` / `WaitUntil`。

```rust
// 帧语义（伪代码，实际入口仍为 WindowCtx::frame）
if dirty.contains(VIEW)   { view(); align(); }         // 只改树，不画
if dirty.contains(LAYOUT) { layout(); }                // 只算几何，不画
let (rects, all) = track.take_damage();
if !rects.is_empty() || all { render(&rects, all); present(); }  // 只画脏的
wait_until(window.next_wakeup());                       // 无 ⇒ Wait
```

**硬约束（来自已踩过的坑）**：`damage_batches()` 先算，退化成整窗时 `SceneBuilder` 必须同步放弃剔除。二者用不同矩形集合会让整窗重画出一个「只剩脏区元素」的场景，导致整页消失（`src/render/mod.rs:110-114`）。

### 2.3 v4 的性能目标如何在单画布下达成的

| v4 目标 | v5 的等价实现 |
|---|---|
| 静止 0% CPU | 无脏位 + 无定时器 ⇒ `ControlFlow::Wait`，整帧跳过 |
| 局部更新只重绘脏层 | `damage_batches` 把脏矩形合并成 ≤32 块，每块渲进复用 `scratch` 再逐行拷回持久 Pixmap；光栅化开销 ∝ 脏区面积 |
| 滚动零 vello 光栅化 | **不承诺**。改为「滚动不触发布局」（§5.3），滚动帧仍按视口脏区重画，但省掉全树重排；这是 v5 唯一的语义增强 |
| 动画零重绘 | **不承诺**。动画用 RAF + `damage_key` 把重绘限制在小面积节点上 |

态度要明确：**「零光栅化」不是免费的目标**，它要求常驻层缓存与合成器；在纯 CPU、几十个控件、1080p 视口的定位下，把复杂度花在「更少像素 + 更少重排」更划算。

---

## 3. 核心状态：一棵保留树

### 3.1 三层表示（不是三棵常驻树）

| 层 | 类型 | 生命周期 | 内容 |
|---|---|---|---|
| 描述 | `ViewBuf` / `DescNode` / `DescRoot` | **瞬时**，每帧 `begin()` 清空但复用 arena 容量 | DSL 产物：`column/row/text/button/...`，`FlexStyle`/`PaintStyle`/`TextStyle`、handler、binding、层根 |
| 保留 | `Track` / `Node` / `NodeId` / `Root` | **跨帧**，唯一所有者 | `desc` 组 + `state` 组（`dragging`/`caret`/`scroll_offset`…）、`computed`、视图态、`damage` |
| 绘制 | `Scene` / `Op` | **瞬时**，每脏帧重建 | 扁平原语列表，顺序即绘制序，自带组合好的 transform |

`desc/state` 分组是必须保留的资产：`align()` 只覆盖 desc 组，因此对齐不会丢 hover、不会让输入框光标闪回（`docs/refactor-plan.md` §二·1）。

### 3.2 对齐规则（保持现状，不加机制）

位置即身份 / 一层 key（仅 `keyed_list`）/ 类型不同即重建 / 逐字段比才置脏 / 层根匹配键 `(layer, owner, 同组序号)`。
不做跨父 `Move`、不做通用 diff、不做签名哈希猜脏区——**脏区在 patch 发生的那一刻登记**。

### 3.3 失效：一个位集 + 一个矩形列表

```
Dirty: VIEW(重跑 view) / LAYOUT(重排) / PAINT(重绘) / PRESENT(上屏)
Track.damage: Vec<Rect> + damage_all
```

- 登记点只有四处：`align` patch（旧矩形）、`mark_paint_dirty`（`paint_bounds` 沿祖先累积 transform）、`layout::write_back`（旧 ∪ 新）、`place_anchored_layers`（层位移）；
- `mark_layout_dirty` 沿祖先冒泡到 `size_stable()` 边界即停（边界重排）；
- 消费点只有一处：`take_damage()`。**不设第二个损伤追踪器**。

v4 的「布局通道 / 绘制通道」在本设计里就是 `Dirty` 的两个位加上「是否产生矩形」，无需两套传播逻辑。

### 3.4 层只是 z 序根

`Layer::{Content, Overlay, Popup, Tooltip, Modal, DragPreview}` + `Layer::ALL` 是 z 序单一权威源；`Root.owner` 表达嵌套层，父层消失级联销毁。层**不持有** Pixmap、不持有 `content_dirty`、不参与提升/合并。`blocks_below` / `backdrop` 等在命中与绘制时消费。

---

## 4. 派生计算：布局与文本

### 4.1 布局：不引入镜像树

`layout::layout(track, size)` 维持现有做法：

1. `begin_layout_epoch()` → `track.layout_boundaries()` 取「自身脏、父不脏」的边界集合；
2. 每个边界 `build()` 一棵临时 `FlexNode` 子树（文本/按钮/Input 填 `measure_text`，Image/Custom 填 `intrinsic_size`）；
3. `flex.layout(avail)` → `write_back()` 写回 `Node.computed` / `desired` / `content_size` / `text_wrap`；
4. 几何新旧差异登记脏矩形（`rect_eq` 1e-3 容差）；
5. `place_anchored_layers()` 解析锚点、翻转、钳制视口。

`lieui-layout` 保持**零依赖、无 dirty/cache 字段、每轮全量算**；增量性由主 crate 的边界集合提供。
删除引擎内主 crate 零引用的 `measurable.rs`（`Measurable`/`TextMeasure`/`FixedMeasure`）与 `constraint.rs`——保留「回调式 measure」这个 v4 §7.3 的设想没有收益，直连 `TextEngine` 更短。

### 4.2 文本：统一两段缓存

`lieui-text`（parley 0.11）继续负责整形 / 测度 / glyph run：

| 阶段 | 当前 | v5 目标 |
|---|---|---|
| 布局期 measure | `MEASURE_CACHE`，key 8 元组，上限 16384，满则**不再插入** | 与绘制期共用**同一套淘汰策略**（计数上限 + 按条目淘汰） |
| 绘制期 layout | `SceneBuilder::TextCache`，FNV 两段式、上限 2048、满则**整表 clear** | 同上；并把 `color` 移出 key（颜色在绘制时应用，D26） |

理由：两套不同的溢出行为是理解成本，不是性能优化。`create_text_layout` / `ink_bounds` / 字体注册链保持不动。

---

## 5. 输出：渲染、上屏、滚动、动画

### 5.1 渲染管线（一步到底）

```
damage_batches(physical, rects, all)   // 合并 + 退化：>32 块或面积 >45% ⇒ 整窗
   ↓ 同一批矩形（缩放回逻辑坐标）
SceneBuilder::build(track, opts, cull, scene_all) // cull = 批次反推的逻辑矩形；按 z 序自下而上
   ↓ Scene: Vec<Op>
Rasterizer::rasterize                  // 每批次 reset scratch → vello_cpu 0.3 → 逐行拷回持久 Pixmap
   ↓
present_with_damage(&batches)          // softbuffer / 无头 PNG
```

`Rasterizer` 持有 `RenderContext` / `Resources` / 一张 `Pixmap` / 一块复用 `scratch`。图片走手动 blit 并受软件 `clip_stack` 约束，`fill_glyphs()` 失败只计数不 panic。
**不缓存 Scene**（C4 实测不值得），**不缓存每控件 Pixmap**。

### 5.2 时钟与空闲

平台层唯一入口 `RedrawRequested → frame()`；`pump()` 只消费定时器 / 动画帧 / 投递。`WindowCtx::next_wakeup()` 汇总光标闪烁、tooltip、定时器、RAF；无来源 ⇒ `ControlFlow::Wait`。跨线程只经 `Poster`（`Waker`）；框架**不提供** task / loading 遮罩概念。

### 5.3 滚动：唯一从 v4 吸收的机制

v4 §11 的正确内核不是 tile 化，而是「**滚动只改位置，不改布局**」。当前 `Track::set_scroll_offset` 仍 `mark_layout_dirty`（`src/track.rs:1496`），滚动导致全树重排，`virtual_list` 还要靠布局后补派 `ScrollChanged` 换窗。

v5 目标：

```
set_scroll_offset(id, offset)
  → 只更新 state 组里的 offset，并登记「旧 ∪ 新」视口脏矩形
  → 派发 ScrollChanged（顺序固定在 frame 的 ②.6，virtual_list 语义不变）
  → 不置 LAYOUT，不重建 FlexNode
```

`scroll_offset` 在 `SceneBuilder`（绘制）与 `hit::descend`（命中）里作为**同一份平移**消费，与 v4 的 `origin.y` 等价，但不需要层、不需要 tile、不需要额外 Pixmap。
风险：这是 v5 中唯一改变语义的动作，必须靠像素级回归测试护航（滚动容器、嵌套滚动、虚拟列表、滚动条拖拽）。

### 5.4 动画

用户态 `request_animation()` + `on_animation(now, dt)` + `Ctx::damage_key(...)`；框架内部只有光标闪烁与 tooltip 会话。
不做层提升、不做 transform 专用合成路径。transform / opacity 变化属于纯视觉变更：只标脏矩形，不触发布局。

---

## 6. 交互与组件（结合现有实现收敛）

### 6.1 事件链（保持两段式）

```
winit 事件 → InputEvent → input::step（hover / Tapped / 捕获 / Wheel）
  → hit::hit_path（纯函数：z 序、blocks_below、clip、逆 Affine、滚动条虚拟部件）
  → event::dispatch（用户 handler 先于内置；collect_route 只读克隆 → 释放借用 → 调用）
  → widgets::handle_route（未被 handled 时的兜底内置行为）
  → CmdBuf → apply_cmds → Dirty
```

`Cmd` 缓冲解决借用冲突；`mark_handled()` 抑制内置行为；只有 `*_bound` 控件写回 `Signal`。v5 不再争论「Cmd 是否为唯一写入通道」：内置行为直写 `&mut Track` 已是既定事实，把它与 `Cmd` 的**顺序**写进契约即可（`docs/refactor-plan.md` 分歧已裁决采纳 E1 共享 `handled`）。

### 6.2 组件：从「三处定义」收敛到「一处定义」

现有 11 个 `KindTag`（Box / Text / Image / Button / Checkbox / Slider / Progress / Input / Switch / Radio / Custom）分散定义在 `view.rs` DSL、`track.rs` 三份平行枚举、`widgets/mod.rs` 绘制分派，新增一个控件要改 ≥4 处（G5）。

v5 目标形态：

| 收敛项 | 做法 |
|---|---|
| `KindTag` / `KindDesc` / `Kind` 三枚举 | 保留 `KindDesc`（纯数据）+ `Kind`（desc 组 + state 组）两份；`KindTag` 由 `KindDesc::tag()` **派生**，不再手写第三份 |
| 组件元数据 | 一份声明（宏或表驱动）生成 DSL 入口、`tag()`、默认样式、绘制分派、内置行为注册 |
| `Track` 上的 27 个组件行为方法 | 全部迁到 `widgets/`；`Track` 只保留「树 / 视图态 / 布局结果 / 脏区 / flags」 |
| `CustomNode` 逃生舱 | 保留，作为「一处定义」做不到时的出口 |

组合构件保留：`keyed_list`、`virtual_list`（padding 撑高 + 可见窗口切片）、`MenuRef`、六种层、滚动条虚拟部件、图标字体。

### 6.3 状态收敛清单

| 现状问题 | v5 处理 |
|---|---|
| `RuntimeInner` 14+ 字段跨 5 域（时钟 / 窗口表 / 请求队列 / 主题 / 定时器） | 按域拆成小结构，`Runtime` 只做门面 |
| 焦点三重表示（`track.focused` / `focus_state` / `interaction.focused`） | 收敛为单一 `focused: Option<NodeId>`，其余派生 |
| `FocusPolicy` / `LayerOpts.focus` 零消费点 | 删除（或与焦点实现一并补齐，二选一，不留死字段） |
| `ImageStyle` / `ImageFit` 未接线（被 `tests/api_contract.rs` 显式钉住） | 接线或删除，不许长期留存 |
| `Cmd::BringIntoView`、`PreviewKeyDown` / `TextComposition*` 等无生产者事件 | 删除或补实现 |
| `lib.rs` 自述仍写「M1 进行中」、`raster.rs` 头注释仍写「行带」（实现已是矩形批次） | 同步文档与注释，避免设计基线漂移 |

---

## 7. Crate 与模块

**不新增 crate**。现有 4 个：

| crate | 职责 | v5 变化 |
|---|---|---|
| `lieui-geom` | `Point/Rect/Size/Color` | 不变 |
| `lieui-layout` | 零依赖 Taitank 风格 Flex | 删死抽象 `measurable.rs` / `constraint.rs` |
| `lieui-text` | parley 封装 + 测度缓存 | 缓存淘汰策略统一、color 移出 key |
| `lieui` | 其余全部；`winit` 为可选 feature（关掉即纯库 / 无头） | 模块边界收敛 |

主 crate 模块按**一条主路径 + 两组旁支**组织：

| 组 | 模块 |
|---|---|
| 主路径 | `view` → `align` → `track` → `layout` → `render/{scene,raster}` → `platform` |
| 驱动 | `reactive`（`Dirty`/`Signal`）、`timer`、`window`（`frame`/`next_wakeup`）、`app`（多窗口编排） |
| 旁支 | `hit` / `input` / `event` / `cmd` / `focus` / `widgets` / `menu` / `custom` / `icon` / `style` / `theme` / `transform` / `post` |

`prelude` 继续只导出用户态常用项，内部 `Track`/`NodeId` 不进 prelude。

---

## 8. 明确不做（写进契约，避免回潮）

1. 持久渲染层树、Compositor、`LayerStorage`/`TileCache`/`TileGrid`、层提升与层合并；
2. GPU 后端 trait / `LayerRasterizer` / 关联类型抽象；
3. 迁移到 `taitank-rs`、CSS Grid、百分比、Baseline / SpaceEvenly（要么补齐要么明确记为退化，不许静默）；
4. 细粒度响应式依赖追踪（保持 R1：值变 → 标脏 → 重跑 `view()`）；
5. 缓存 Scene / display list（C4 实测不值得）；
6. 框架级 task、loading 遮罩、遮罩组件（应用侧用 `Signal` + `modal_tagged` + `Poster` 自行拼装）；
7. 每控件 Pixmap、双缓冲多份最终画面；
8. 跨平台窗口抽象层（平台层是可选 feature，当前只接 winit）。

---

## 9. 落地顺序与验收

按「先删、再改语义、最后挪代码」排序，每批独立可回滚：

| 批次 | 内容 | 风险 | 验收 |
|---|---|---|---|
| P0 单真源清理 | `KindTag` 派生；删 `measurable.rs`/`constraint.rs`/死事件/未消费焦点字段；`ImageStyle` 接线或删；统一文本缓存淘汰与 key；同步过时注释 | 低 | 现有约 550 条单测 + `tests/api_contract.rs` 全绿；`clippy::all = deny` |
| P1 滚动脱离布局 | `set_scroll_offset` 不置 `LAYOUT`；绘制/命中消费同一平移；`ScrollChanged` 顺序不变 | **高** | 像素级回归：滚动、嵌套滚动、虚拟列表换窗、滚动条拖拽；`tests/perf_regression.rs` 碎片基线不劣化 |
| P2 模块边界收敛 | `Track` 组件行为迁 `widgets/`；`RuntimeInner` 拆域；焦点三表示归一 | 中 | 行为不变，仅结构变化；`tests/input_flow.rs`、`tests/spinner_modal.rs` 全绿 |
| P3 可选优化 | `Signal::set` 同值短路（`T: PartialEq`）、更细 culling、`text_wrap` 几何求值收敛 | 低 | 空闲帧零光栅化、局部脏区像素数不增 |

**观测量**：`AlignStats` / `LayoutStats` / `SceneStats` / `RasterStats` / `FrameStats` 已覆盖全链路，P1/P3 必须用它们出前后对比，避免凭感觉优化（`refactor-plan.md` v1.2：对照组要确认真的走了对照组分支）。

---

## 10. ADR 摘要

| 编号 | 决策 | 代价 |
|---|---|---|
| ADR-v5-001 | 用「单树 + 单 Pixmap + 矩形脏区」替代 v4 的「三棵树 + Compositor」 | 滚动/位移动画仍需按脏区重画视口，不承诺零光栅化 |
| ADR-v5-002 | 保留自研 `lieui-layout`，不引入 `taitank-rs` | Flex 能力边界由现有测试矩阵界定，需要新特性时自己补 |
| ADR-v5-003 | 不为 `vello_gpu` 预置 trait | 未来接 GPU 需重写 `Rasterizer`，但改动集中于一处 |
| ADR-v5-004 | 滚动只改绘制/命中平移，不触发布局 | `virtual_list` 依赖 `ScrollChanged` 的固定派发时点，需锁进契约与测试 |
| ADR-v5-005 | 组件定义收敛到一处，`Track` 不再承载组件行为 | 需要一次结构性重构（P2） |
| ADR-v5-006 | 两条失效通道合并为 `Dirty` 位集 + 矩形列表 | 无 |

---

## 11. 风险

| 风险 | 缓解 |
|---|---|
| P1 滚动脱离布局引入残影/错位 | 像素级回归是唯一有效断言；逻辑断言不足以发现（`refactor-plan.md` §二·11） |
| 单 Pixmap 在超大视口滚动时每帧重画整屏像素 | 已由「退化阈值 + 合并」限制批次；若未来基准显示瓶颈在绘制量，再评估层缓存，**先量化再引入机制** |
| 收敛组件定义时破坏 `desc/state` 分组 | 该分组列为不可动资产；宏生成只覆盖 desc 部分 |
| 不预留后端抽象导致未来迁移成本 | 接受；当前只有一个后端，且 `Renderer`/`Rasterizer`/`present` 已是三处清晰边界 |

---

**结论**：v4 想解决的「CPU 渲染下如何避免每帧重绘」，当前代码已用更少的机制解决了大半。v5 不再加树、加层、加合成器，而是把剩下的复杂度从**机制数量**转移到**实现收敛**：一处定义控件、一份脏区、一张画布、一个帧入口，并补齐「滚动不重排」这块唯一缺失的语义。
