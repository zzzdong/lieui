# LieUI v6 架构设计：Widget 化 + 持久层合成

> 版本：v6.0-draft | 日期：2026-11-05 | 状态：**设计草案，待评审**
> 取代：`docs/architecture-v5.md`（其「明确不做」「ADR-v5-001」及滚动方案作废）
> 初始想法：`docs/architecture-v4.md`（持久层树 + Compositor）
> 实现基线：`lieui 0.1.0-alpha.4` 源码 + `docs/refactor-plan.md` v1.2 + `docs/operation-log.md`
>
> **一句话**：控件不再是 `Kind` 枚举，而是**一个控件一个 `Widget` 对象**（状态归自己、分派走虚调用）；
> 渲染不再是一张画布按脏区重画，而是**每层各自缓存 Pixmap + Compositor 按损伤带 blit**。
> 这两件事互相成全：Widget 化让「层内画什么」收敛为一次调用，Compositor 让「什么时候画」收敛为层的失效位。

---

## 0. 方向变更

### 0.1 v5 的判断错在哪

v5 以「Scene 遍历仅 0.061 µs/节点」为由否决 Compositor，只比较了**遍历成本**，漏掉了两点：

1. **成本大头是布局，不是 Scene**：`tests/compositor_probe.rs:98-99` 明确把 `layout%` 记为「compositor 能省下的上界」，20000 行时布局占 94.1%（`docs/operation-log.md:5549-5560`）。测度缓存修复让基线降了 4.3×（`:5665-5670`），但滚动仍然每帧重排全树（`Track::set_scroll_offset` → `mark_layout_dirty`，`src/track.rs:1496-1507`）。
2. **「变换与重绘分离」是架构清晰度，不只是性能**：有了层缓存，`origin`/`opacity` 动画与滚动天然零光栅化，脏区语义从「猜哪些像素要重画」变成「哪些层的内容变了 / 哪些层的位置变了」。v5 用矩形脏区模拟这件事，把复杂度压在 `align`/`layout`/`damage_bounds` 三处。

因此 v6 采纳 v4 的骨架，同时保留 v5 中已被验证、与方向无关的结论。

### 0.2 从 v4 采纳 / 裁剪

| v4 | v6 | 说明 |
|---|---|---|
| 持久渲染层树 + Compositor（§3 / §9 / ADR-001） | ✅ 采纳 | 核心 |
| 变换与重绘分离（ADR-003）、两级失效通道（§10） | ✅ 采纳，简化为 `content_dirty` / `geometry_dirty` 两个布尔 | 不再维护两套传播管线，统一落到层上 |
| 滚动只改 `origin`、不触发布局（§11.1） | ✅ 采纳 | v6 的主要收益来源 |
| 层是缓存边界（§2.5） | ✅ 采纳，但**层 = `Root` + 显式滚动层**，不是「每控件默认一层」 | 每控件一层会让 `Root` 数与 Pixmap 内存爆炸（v4 §6.3 的隐患） |
| Tile 网格 64/128 + 精确矩形混合（ADR-004）、256×256 `TileCache`（ADR-006） | ⚠️ 替换为**互不重叠水平带 + 滚动层条带缓存** | 直接复用现有 `damage_batches_bands`（`src/render/raster.rs:213-280`），不必引入二维 TileGrid |
| `LayerRasterizer` trait 预留 `vello_gpu`（ADR-005） | ❌ 不做 | 只有一个后端；`Rasterizer`/`Compositor`/`present` 已是三处清晰边界 |
| 6 个新 crate、引入 `taitank-rs` | ❌ 不做 | 维持 4 crate + 自研 `lieui-layout`；边界靠 `mod` |
| 动画前后自动层提升 / 合并（§12.3） | ⚠️ 只做**静态层提升**（`scroll()` 声明、动画显式 `promote`），不做自动合并优化 | 收益依赖层缓存，但自动合并是独立的复杂度源 |

### 0.3 从 v5 / 现状保留的资产（不要动）

`ViewBuf` 描述 arena 与 `DescNode` 的样式/事件/层字段、`align` 四条规则（位置即身份 / 一层 key / 类型不同即重建 / 逐字段比才置脏）、**desc/state 字段分组语义**、`Cmd` 两段式分发、`next_wakeup` 统一时钟、`RedrawRequested` 单一帧入口、`damage_batches` 族纯函数、`present_with_damage` 上屏链路、`z_ordered_roots()` z 序、`Affine`/`Op.transform` 的「祖先链已组合」语义、`#![forbid(unsafe_code)]`、像素级回归测试传统。

---

## 1. 目标与不变量

1. **控件是对象，不是枚举**：一个控件类型 = 一个 `struct` + 一份 `Widget` 实现；新增控件不改 `track.rs` / `align.rs` / `layout.rs` / `render/scene.rs`。
2. **层是缓存边界**：层持有自己的 Pixmap 与脏标记；层内重绘与层间合成是两个独立阶段。
3. **位置变化不重绘**：`origin` / `opacity` / `clip` 变化只登记旧 ∪ 新损伤，不动 `Pixmap`。
4. **绘制与命中同源**：层 origin 作为最外层仿射同时注入 `SceneBuilder` 与 `hit::descend`，层内坐标不变。
5. **一个 z 序真源**：继续用 `Layer::ALL` + `z_ordered_roots()`，层树不另建排序。
6. **一个上屏出口**：`Runner::present` + `present_with_damage(&bands)` 原样复用，合成后的损伤带与光栅批次同源。
7. **静止零开销**：无脏层、无 origin 变化、无定时器 ⇒ `ControlFlow::Wait`，三段全跳过。

**明确不做**：二维 Tile 网格、GPU 后端 trait、每控件一层、自动层合并、细粒度响应式依赖追踪、`taitank-rs` 迁移、缓存 Scene（实测不值得）、框架级 task / 遮罩概念。

---

## 2. 整体架构

```
┌──────────────────────────────────────────────────────────────┐
│ 应用：ViewModel::view(&Rc<Self>, &mut ViewBuf) + Signal<T>      │
└──────────────────────────────────────────────────────────────┘
       ↓ Dirty::VIEW            ↓ 输入 / Cmd / 定时器 / 动画
┌──────────────────────────────────────────────────────────────┐
│ ViewBuf（描述 arena）  →  align(Track)  →  Track（保留树）      │
│   DescNode{ widget: Box<dyn Widget>, 样式/事件/层字段 }          │
│   Node   { widget: Box<dyn Widget>, computed, 视图态 }           │
└──────────────────────────────────────────────────────────────┘
       ↓ Dirty::LAYOUT                       ↓ 失效登记
┌────────────────────────┐   ┌──────────────────────────────────┐
│ layout：脏边界临时       │   │ LayerTable（持久层树）             │
│ FlexNode → 写回 computed │   │  RootId → Layer{ origin, opacity,  │
│ 引擎 lieui-layout 零改动 │   │  clip, z, surface, dirty 位 }     │
└────────────────────────┘   └──────────────────────────────────┘
                                      │ content_dirty   │ geometry_dirty
                    ┌─────────────────┘                 │
                    ▼                                   ▼
        ┌───────────────────────┐            ┌──────────────────────┐
        │ ① 层重绘（按需）       │            │ 旧 ∪ 新 损伤带        │
        │ build_layer(root) →   │            │ damage_batches_bands  │
        │ Scene → 层 Pixmap     │            └──────────────────────┘
        │ 复用 Cull / clip /    │                       │
        │ flush_segment         │                       ▼
        └───────────────────────┘            ┌──────────────────────┐
                                            │ ② Compositor           │
                                            │ 逐带清底 → 按 z 序      │
                                            │ blit_premul 相交层      │
                                            └──────────────────────┘
                                                       ▼
                                            ┌──────────────────────┐
                                            │ ③ 最终 Pixmap →        │
                                            │ present_with_damage    │
                                            └──────────────────────┘
```

**三段式帧管线**（替换 `WindowCtx::frame()` 的 ③ 段，`src/window.rs:577-590`）：

```text
① align / layout  → 决定「哪些层 content_dirty、哪些层 origin 变了」
② layer_redraw    → 只重绘 content_dirty 层，渲进各自的 Pixmap
③ composite       → 按损伤带清空 + 按 z 序 blit → 最终 Pixmap → present
```

`Dirty` 位集保持 `VIEW/LAYOUT/PAINT/PRESENT`；`PAINT` 的含义从「整窗脏区重画」收窄为「有层内容脏或层几何脏」。`needs_frame()`（`src/window.rs:681`）必须把**仅 origin 变化**算作出帧条件，否则动画/滚动会停帧。

---

## 3. Widget 化：删除 `Kind` 三枚举

### 3.1 现状的代价（实测）

| 项 | 位置 | 规模 |
|---|---|---|
| 三份平行枚举 `KindTag` / `KindDesc` / `Kind` | `src/track.rs:206-220,253-293,426-481` | 11 变体 × 3 |
| `KindDesc::apply_to` 的 11×11 分支比较 | `src/track.rs:312-423` | 约 112 行，含 f32 位比较 / IME 保护 / Arc 指针判等等特例 |
| `tag()` 两份 + `from_desc` | `src/track.rs:295-310,483-530` | 3 × 11 arm |
| DSL 构造点 | `src/view.rs` | 约 25 处 `push_desc(KindDesc::…)` |
| 绘制分派 | `src/widgets/mod.rs:710-1022` | 11 arm，约 310 行 |
| 行为分派 + 测度分派 | `widgets/mod.rs:72-89`、`src/layout.rs:277-297,411-437` | 3 处 switch |
| 控件状态 API 挂在 `Track` 上 | `src/track.rs:1532-1905` | `input_*` 十余个 + slider / toggle / radio / dragging |
| 测试直接构造枚举 | `align_tests` / `track_tests` / `view_tests` / `app_tests` / `widgets_tests` / `tests/*` | 数十处 |

**加一个控件要改 ≥8 处**（`track.rs`、`view.rs`、`widgets/mod.rs`、`layout.rs`、`align.rs`、`cmd.rs`、`lib.rs`、测试）。根因是「控件的数据、身份、状态、绘制、行为被拆进不同文件的 switch 里」。

### 3.2 目标模型：一个控件 = 一个对象

```rust
// src/widget/mod.rs —— 唯一的控件契约
pub trait Widget: Any + Debug {
    fn as_any(&self) -> &dyn Any;
    fn clone_box(&self) -> Box<dyn Widget>;
    /// 声明数据覆盖到本实例，**视图态不动**；返回是否变化（对齐据此标脏）
    fn apply(&mut self, desc: &dyn Widget) -> bool;
    /// 类型身份：默认 Any::type_id，取代 KindTag
    fn widget_type(&self) -> TypeId { self.as_any().type_id() }

    fn measure(&self, cx: &MeasureCtx<'_>) -> Intrinsic { Intrinsic::Auto }  // Text / Fixed / Auto
    fn draw(&self, cx: &mut DrawCtx<'_>) {}
    fn on_event(&mut self, ev: &EventView<'_>, cmd: &mut CmdBuf) -> bool { false }
    fn paint_bounds(&self, rect: Rect) -> Rect { rect }
    fn focusable(&self) -> bool { false }
    fn on_attached(&mut self, id: NodeId) { let _ = id; }
    fn on_detached(&mut self) {}
    fn debug_name(&self) -> &'static str;   // 宏生成，取代 kind_name
}
```

- `DescNode.widget: Box<dyn Widget>`（描述态）、`Node.widget: Box<dyn Widget>`（保留态）。
- **字段分组下沉到控件内部**，但保留现有语义：每个控件把字段拆成 `props`（声明数据，`PartialEq`）与 `state`（视图态，不参与比较）。例如 `Input { props: InputProps{text, placeholder}, state: InputState{caret, anchor, preedit, scroll} }`。
- `impl_widget!(Button, Checkbox, …)` 宏生成 `as_any` / `clone_box` / `debug_name` / 默认 `apply`（downcast → 逐字段比 `props` → 变化才覆盖）；`widget_type` 用 trait 默认实现。**只有 Input（IME 保护）、Custom（`Rc::ptr_eq`）等少数控件覆写 `apply`**，特例局部化，不再污染 `align.rs`。
- `CustomNode`（`src/custom.rs:50-77`）的 5 个 hook（`intrinsic_size`/`draw`/`on_event`/`on_attached`/`on_detached`）就是这套契约的现成蓝本；`Kind::Custom` 这个「枚举里塞 trait object」的特例直接消失，`CustomCell` 变成普通 `Widget` 实现。
- 公共字段（`FlexStyle`/`PaintStyle`/`TextStyle`/`ImageStyle`/`visibility`/`clip`/`transform`/`tab_*`/handlers/bindings）**继续留在 `Node`**，与控件数据正交。

### 3.3 分派收敛

| 现在 | v6 |
|---|---|
| `widgets::draw` 11 arm | 框架先统一画公共三段（阴影 / 背景 / 边框，`widgets/mod.rs:654-706`），再 `widget.draw(cx)` 一次虚调用 |
| `widgets::handle` 的 `match kind.tag()` | `widget.on_event(ev, cmd)`；未 `handled` 时框架兜底顺序不变 |
| `layout::build` 的 `match &n.kind` | `widget.measure(cx)` → `Intrinsic::{Text(String, TextSpec), Fixed(Size), Auto}`；**`lieui-layout` 引擎零改动**（仍消费 `FlexNode.measure_text` / `intrinsic_size`） |
| `Node::paint_bounds` 的 `matches!(Kind::Text)` | `widget.paint_bounds(rect)` |
| `Track` 上十几个 `input_*` / `toggle_*` API | 下沉到 `widgets/*.rs`，统一经 `track.widget_mut::<T>(id)` 泛型助手访问 |
| `kind_name(&Kind)` 11 arm | `widget.debug_name()`，`render/scene.rs` 不再有控件 match |

`Scene` / `Op` / `Cull` / `TextCache` / `SceneBuilder::walk` / `Rasterizer` / `hit.rs` / `input.rs` / `focus.rs` / `theme.rs` / `style.rs` / `event.rs` 与控件模型正交，**零改动**（已逐一验证）。

### 3.4 对齐契约

```rust
// src/align.rs：四条规则不变，只换身份判据
if node.widget.widget_type() != desc.widget.widget_type() {
    create_subtree(...);          // 类型不同即重建，state 组随之丢弃（语义不变）
} else if node.widget.apply(&*desc.widget) {
    mark_layout_or_paint_dirty(...);  // 变化才置脏，state 保留
}
```

`apply` 的契约必须保留现有语义：f32 用 `to_bits()` 比、Input 在 `preedit` 非空时不覆盖 `text` 且不重置 `caret`、`Image` 按 `Arc::ptr_eq`、Custom 按 `Rc::ptr_eq`。这些从 `track.rs:312-423` **平移**到各自的控件文件。

### 3.5 兼容与迁移

- **公开 DSL 签名不变**：`v.button("x")` / `v.text(...)` / `v.checkbox_bound(&sig)` 等照旧，内部从 `push_desc(KindDesc::X)` 换成 `push_widget(Box::new(X::new(...)))`；`bake_button_style` / `style_input` / `style_switch` / `style_radio` 的默认样式烘焙原样复用（`src/view.rs:356-372,433-453,505-515`）。
- `Cmd::SetKindDesc` / `Cmd::SetText`（`src/cmd.rs:23,28`）→ `Cmd::SetWidget(Box<dyn Widget>)` / `Cmd::UpdateText(String)`；`CmdBuf` 两段式不变。
- `lib.rs` 根级不再导出 `Kind/KindDesc/KindTag`；`prelude` 不变（`tests/api_contract.rs` 零 Kind 引用，公共契约不破）。
- **测试迁移是主要工作量**：数十处 `Some(Kind::Text(..))` / `Kind::Slider{dragging,..}` 改为构造器助手 `Text::new(..)` / `Slider::with_state(..)`；`kind_names_are_stable_for_debugging`（`src/render/scene_tests.rs:312`）改为 `debug_name()` 断言。建议提供 `widget_of::<T>(id)` 测试助手，逐文件迁移。
- **代价**：`Box<dyn Widget>` 每帧在 `ViewBuf` 里分配一次（现有 `KindDesc` 也带 `String`/`Arc`）；后续可用槽位复用（`Option<Box<dyn Widget>>::replace`）压掉，不作为 v6 前置条件。

### 3.6 与 Compositor 的接口

`Widget::draw` 只向 `&mut Scene` 追加 `Op`，坐标是**层内逻辑坐标**；它不知道自己在哪一层、不知道 Pixmap、不知道损伤带。层 origin 由 `build_layer` 注入最外层 `parent` 仿射。这条边界让「层重绘」可以只针对某个 `Root` 构建 Scene，而无需改动任何控件代码。

---

## 4. 层树与 Compositor

### 4.1 层边界：静态、显式、数量可控

层不是每控件一个，而是：

| 来源 | 说明 |
|---|---|
| **每个 `Root`** | 现有 `Track.roots`（`src/track.rs:954`）已是天然层边界；6 类 `Layer` + `owner` 嵌套 + `Layer::ALL` z 序直接复用（`src/track.rs:590-628,1198-1289`） |
| **`scroll()` 声明的内容层** | DSL 把滚动容器子树提升为独立层；滚动条 / sticky header 作为**兄弟层**不随内容平移 |
| **动画显式 `promote`** | `Ctx::promote(id)` 在动画开始前把子树提层；动画结束由应用决定是否 `demote`（不做自动合并） |

默认层数 ≈ 根数（注释 `src/track.rs:1229` 说通常 <20）+ 滚动容器数 + 活跃动画数，可控。

### 4.2 层数据结构

```rust
// src/render/layer.rs（新模块）
pub struct Layer {
    root: RootId,
    origin: (i32, i32),          // 物理像素对齐，避免 blit 重采样
    opacity: f32,                // 层属性，区别于现有「层内组不透明度」Op::PushOpacity
    clip: Rect,                  // 轴对齐（见 §8 风险②）
    z: u32,                      // 由 z_ordered_roots() 派生，不另存真源
    bounds: Rect,                // 内容包围盒（逻辑坐标 × scale）
    storage: LayerStorage,
    content_dirty: bool,         // 内容变了 ⇒ 需要 vello 光栅化
    geometry_dirty: bool,        // origin/opacity/clip 变了 ⇒ 只需合成
    dirty_rects: Vec<Rect>,      // 层内损伤
}

pub enum LayerStorage {
    Single(Pixmap),              // 普通层：尺寸 = bounds，面积上限 = 4× 视口
    Striped(StripeCache),        // 仅滚动内容层：按视口高度切垂直条带，只留视口 ±1 屏
}
```

`Striped` 是 v4 256×256 二维 TileCache 的简化替代：单列条带、按需光栅化新条带、视口外淘汰，内存上界 = 约 3 屏。普通层超过面积上限时直接降级为「不缓存、每帧重绘」，避免长列表整层 Pixmap（v4 §15 的 1080×10000×4 ≈ 41MB 问题）。

### 4.3 两级失效

| 通道 | 触发 | 后果 |
|---|---|---|
| `content_dirty` | `align` patch、widget 事件改了 props、层尺寸变化、首次创建、层从 `Striped` 淘汰 | 层重绘（vello），并把层覆盖区并入合成损伤 |
| `geometry_dirty` | `origin` / `opacity` / `clip` / z 变化 | **不重绘**，只登记旧 ∪ 新覆盖区为合成损伤 |

传播规则沿用 v4 §10：纯视觉变化不触发布局；几何变化不触发层重绘；`Dirty::LAYOUT` 只由结构/样式/视口变化产生。`Track.damage: Vec<Rect>` 保留为**合成损伤**的累积器，`take_damage()` 仍是唯一消费点。

### 4.4 层重绘（复用现有光栅器）

```
for layer in layers.iter_mut().filter(|l| l.content_dirty) {
    let scene = SceneBuilder::build_layer(&track, layer.root, /* parent = translate(-origin) */,
                                          &layer.dirty_rects, layer.bounds);
    raster.rasterize_to(&mut layer.storage, &scene, &layer.dirty_rects);  // 渲进层画布
    layer.content_dirty = false;
}
```

可原样复用：`SceneBuilder` 的 `Cull` / `PushClip`/`PopClip` 配对 / `clips_are_balanced` / `TextCache`、`Rasterizer` 的 `reset_and_resize` + `scratch` 复用 + `shift` 变换 + `PixmapMut::new` + `TargetInit::SrcOver` + 逐行 `copy_from_slice`（`src/render/raster.rs:401-558`）。改动点仅两处：`SceneBuilder::build` 从「所有 Root 推进一个 Scene」改为「按 Root 分组」（`src/render/scene.rs:526-543`），底色 op 只属于最底层、各层 backdrop 归属对应层。

**共享一个 `Rasterizer` 实例**（`RenderContext` 构造成本高），只把目标画布作为参数传入 `rasterize_to(&mut LayerStorage, ...)`；避免给每层各建一份 `RenderContext` / `Resources` / 裁剪栈，同时让 `clips_are_balanced` 只需校验一处。层画布本身是 `LayerStorage` 里的 `Pixmap`。

### 4.5 Compositor：复用「互不重叠带」，不引入 TileGrid

`damage_batches_bands`（`src/render/raster.rs:213-280`）已经把任意脏区切成**互不重叠的水平带**，注释 `:207-211` 明确其设计目的就是「批次不重叠 + 场景剔除天然同源」。它正是合成器需要的损伤单元，无需新建 `TileGrid`：

```rust
let bands = damage_batches_bands(physical, &damage, damage_all);
for band in &bands {
    clear_region(&mut final_pixmap, *band, background);           // 或最底层不透明底色
    for layer in layers.iter_bottom_to_top() {                    // z_ordered_roots()
        let Some(r) = band.intersect(&layer.clipped_bounds()) else { continue };
        blit_premul(&mut final_pixmap, layer.surface(r), r, layer.opacity);
    }
}
present_with_damage(&bands);                                      // 上屏同源
```

- `blit_premul` 是**全库唯一的 SrcOver 像素混合实现**，同时供 `Op::Image` 的层内手动 blit（`src/render/raster.rs:565-611`）与层间合成调用，杜绝两套预乘语义（v4/本报告风险③）。
- 层 opacity 在 blit 时应用；层内 `PushOpacity` 保持原义（组不透明度），不混用。
- 最终画面仍是**一张持久 Pixmap**（现有 `Rasterizer.pixmap`），`Renderer::pixmap()` / `WindowCtx::pixmap()` / `Runner::present` 三个只读借用点不动。

### 4.6 滚动与动画：零光栅化路径

- `scroll_offset` 成为内容层的 `origin`（`-offset`，物理像素对齐）；`Track::set_scroll_offset`（`src/track.rs:1496`）**去掉 `mark_layout_dirty`**，撤销 `layout::write_back` 里把 `-offset` 烘焙进子节点 `computed` 的逻辑（`src/layout.rs:353-377`），避免双重平移。
- `ScrollChanged` 派发时点仍锁在 `WindowCtx::frame` 的 ②.6（`src/window.rs:566-575`），`virtual_list` 换窗（`src/view.rs:622-683`）语义不变。
- 命中：`hit_path`（`src/hit.rs:21-40`）把层 origin 的**逆平移**作为 `descend` 的初始 `to_parent`，与 `build_layer` 的 `parent` 仿射同源；滚动条虚拟部件分支（`:115-125`）保持层内坐标。
- 滚动条 / sticky header 作为兄弟层（`widgets/mod.rs:1033-1049` 的 overlay 提升为层），不随内容平移。
- 动画：`tick_animation` 只改 `origin`/`opacity`，登记旧 ∪ 新包围盒 ⇒ 只合成、不重绘。宽高/文本动画仍走 `content_dirty`，限制在小层内。

### 4.7 帧循环与统计

```text
WindowCtx::frame():
  ① VIEW  → view() + align()            → 标 content_dirty / LAYOUT
  ② LAYOUT→ layout()（边界重排）+ place_anchored_layers() → 层 bounds/origin
  ②.6     → ScrollChanged 派发（时点不变）
  ③ 三段式 → layer_redraw() → composite() → present_with_damage(bands)
  ④ next_clock / next_wakeup → Wait / WaitUntil
```

`FrameStats`（`src/window.rs:217-239`）新增 `layers_redrawn / layers_moved / composite_pending / blit_pixels`；`RenderStats` 加 `composite`；`FrameStats.damage` 必须是**合成后**的损伤带，否则 `Runner::tick:455` 传给上屏的是错的。`needs_frame` 纳入 `geometry_dirty`。上屏、时钟、`ControlFlow` 逻辑全部不动。

---

## 5. 模块与 crate

**不新增 crate**（维持 `lieui-geom` / `lieui-layout` / `lieui-text` / `lieui`）。主 crate 新增两个模块：

| 模块 | 职责 | 来源 |
|---|---|---|
| `widget/` | `Widget` trait、`impl_widget!` 宏、11 个内置控件各一个文件、`widget_of::<T>` 助手 | 取代 `track.rs` 三枚举 + `widgets/mod.rs` 两个 switch + Track 状态 API |
| `render/layer.rs` + `render/compositor.rs` | `Layer`/`LayerStorage`/`StripeCache`、层重绘调度、带合成、`blit_premul` | 新增；复用 `raster.rs`/`scene.rs` |
| `widgets/` | 保留公共绘制助手（`resolve_background`/`push_text`/滚动条几何）、`draw_frame`、`handle_route` | 瘦身，不再含 11 arm |

| 分类 | 清单 |
|---|---|
| **零改动** | `hit.rs`(除注入 origin)、`input.rs`、`focus.rs`、`theme.rs`、`style.rs`、`event.rs`、`post.rs`、`timer.rs`、`transform.rs`、`menu.rs`、`crates/lieui-*`、`Scene`/`Op`/`Cull`/`TextCache`、`damage_batches*`、`pack_xrgb`/`copy_damage_rects`/`Runner::present`、`z_ordered_roots()`、`Dirty`/`needs_frame`/`schedule_wakeup` |
| **必须改造** | `track.rs`（Node.widget / Root→Layer 关联 / 删状态 API）、`view.rs`（25 处构造）、`align.rs`（身份判据）、`cmd.rs`（两个命令）、`layout.rs`（measure 桥接 + 撤销 scroll 烘焙）、`render/scene.rs`（按 Root 分组）、`render/raster.rs`（目标 Pixmap 参数化 + `blit_premul`）、`window.rs`（三段式 + 统计）、`platform/mod.rs`（剪贴板 `Kind::Input` 判据）、`lib.rs` 导出 |
| **删除** | `KindTag`/`KindDesc`/`Kind`、`apply_to` 两份 `tag()`、`from_desc`、`kind_name`、`Kind::Custom` 特例、`crates/lieui-layout/src/measurable.rs` + `constraint.rs`（主 crate 零引用）、未接线的 `ImageStyle`/死事件 |

---

## 6. 落地顺序与验收

**先 Widget 化，再 Compositor**：Widget 化把「层内画什么」收敛成一次虚调用，Compositor 的 `build_layer` 才能直接复用；反过来会同时改两层，无法分批验证。

| 批次 | 内容 | 验收 |
|---|---|---|
| **W0 控件契约** | 引入 `Widget` trait + 宏；先把 `Box`/`Text`/`Custom` 三个控件迁过去，其余保留兼容层 | 现有单测全绿；`tests/api_contract.rs` 公开 DSL 签名不变 |
| **W1 控件迁移** | 其余 8 个控件逐个迁；下沉 Track 状态 API；`Cmd`/`view`/`layout`/`align` 换接口；测试逐文件迁移 | 约 550 条测试全绿；`clippy::all = deny`；像素级回归无差异 |
| **W2 删除枚举** | 删三枚举与 `kind_name`，`prelude` 收敛 | 「加一个控件只改 1 个文件 + 1 行注册」的可编译示例测试 |
| **C0 层表骨架** | `LayerTable` + `Root↔Layer` 映射、层属性、`content_dirty`/`geometry_dirty`；暂不缓存像素，仍走单画布 | 行为不变，`FrameStats` 能观测层失效 |
| **C1 层重绘** | `build_layer` 分组、每层 Pixmap、面积上限与降级 | 像素级回归（层边界、嵌套层、Modal backdrop 归属） |
| **C2 合成器** | `damage_batches_bands` + `blit_premul` + z 序合成 + 上屏同源 | 残影/双重预乘/半透明叠加专项像素测试；`present_with_damage` 矩形正确 |
| **C3 滚动与命中** | origin 平移、撤销 `-offset` 烘焙、命中同源、滚动条兄弟层、`ScrollChanged` 时点 | 滚动 / 嵌套滚动 / 虚拟列表换窗 / 拖拽像素级回归；`layout` 调用次数降为 0（`tests/compositor_probe.rs` 作基线） |
| **C4 动画与优化** | `promote`/`origin`/`opacity` 动画、`Striped` 条带缓存、层面积淘汰 | 滚动与 transform 动画期间 `vello` 光栅化像素数为 0；内存上界 ≤ 约 3 屏 + 最终画布 |

每批独立可回滚；C1–C3 必须靠**像素级断言**（`src/render/mod.rs:171-178` 的 `px()` 助手传统），逻辑断言发现不了残影。

---

## 7. 风险与缓解

| 风险 | 缓解 |
|---|---|
| **层 Pixmap 内存**：每层一张 + 最终画布 + scratch，DPI=2 再 ×4；长列表整层可达 41MB | 层 = Root / scroll / 显式 promote，禁止每控件一层；`Single` 面积上限 4× 视口，超限降级不缓存；滚动内容用 `Striped` 只留视口 ±1 屏；`add_root`/`remove_root`/`destroy`（`track.rs:1297-1350`）严格配对分配释放 |
| **clip / 非平移 transform 与 blit**：层 origin 在合成期施加后，层内 clip 要跟着位移；旋转层下轴对齐 `intersect` 不成立，`Op::screen_bounds` 只是保守包围盒 | **v6 层变换只支持平移 + 整数缩放 + opacity**，旋转/任意仿射留在层内 `Op.transform`；合成损伤一律按 `bounding_box` 外扩取整；旋转层不缓存（`content_dirty` 常驻） |
| **图片手动 blit 两套语义**：`Op::Image` 绕过 vello 裁剪栈（`raster.rs:565-611`），层间合成又要混合，易双重预乘 / z 序错乱 | 抽出唯一 `blit_premul`，层内图片与层间合成共用；`flush_segment` 分段维持 z 序；N 份 Scene 各自 `clips_are_balanced` 自检 |
| **命中与绘制不同源**：层 origin 注入若不统一会「画在一处、点在另一处」 | 把 `layer.parent_affine()` 作为唯一来源，同时传给 `build_layer` 与 `hit::descend`；滚动条分支保持层内坐标；加同源性断言测试 |
| **Widget 化改动面**：数十处测试直接构造枚举、`Box<dyn Widget>` 每帧分配、宏生成 downcast 样板 | W0 先立兼容层与构造器助手，逐文件迁移；`prelude` 签名不变；分配优化后置，不作为前置条件 |
| **Compositor 收益被高估**：测度缓存修复后 `compositor_probe` 的优先级已下降（`operation-log.md:5665-5670`） | C3 以「滚动帧 layout 次数 / vello 光栅化像素数」为硬指标，达不到就回退到 C0 骨架；先量化再决定是否继续 C4 |

---

## 8. ADR

| 编号 | 决策 | 代价 |
|---|---|---|
| ADR-v6-001 | **采纳 Compositor**，推翻 v5 的单画布路线 | 层 Pixmap 生命周期、合成正确性、帧循环三段式 |
| ADR-v6-002 | 控件模型从 `Kind` 三枚举改为 `Box<dyn Widget>` + 宏注册 | 每帧一次 Box 分配；数十处测试需迁移；downcast 样板 |
| ADR-v6-003 | 层边界 = `Root` + 显式 `scroll()` / `promote`，不做每控件一层与自动合并 | 未提层的控件动画仍走重绘 |
| ADR-v6-004 | 用现有 `damage_batches_bands` 的互不重叠带替代二维 TileGrid；滚动层用单列 `Striped` | 同行左右两块脏区会被并成整行（已知局部性损失） |
| ADR-v6-005 | 不预留 GPU 后端 trait；层变换限平移/整数缩放/opacity | 未来接 GPU 需重写 `Rasterizer` + `Compositor` |
| ADR-v6-006 | 布局引擎 `lieui-layout` 零改动，measure 桥接放在主 crate | 控件测度统一收窄为 `Intrinsic::{Text, Fixed, Auto}` |
| ADR-v6-007 | 先 Widget 化（W0–W2）再 Compositor（C0–C4） | 周期较长，但每批可独立验证与回滚 |

---

**结论**：v6 用两个「对象化」换取整体简化——控件变成 `Widget` 对象，抹掉三枚举与所有按类型分派的 switch；渲染单元变成 `Layer` 对象，抹掉「靠脏矩形猜哪些像素要重画」的整套推理。Compositor 在此不是额外一层复杂度，而是把 `align`/`layout`/`damage` 里散落的几何推理**收拢到层属性**上：内容变了才光栅化，位置变了只合成。
