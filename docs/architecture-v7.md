# LieUI v7：一棵树 · 缓存装饰 · 一个 Op

> 版本：v7.0-draft | 日期：2026-11-05 | 状态：**提案，待决策**
> 起点：`docs/architecture-v4.md`（保留其判据，重做其手段）
> 若采纳：取代 `docs/architecture-v6.md`；v6 的 Widget 化与 Track 拆分保留并修正，**其层树 / `LayerTable` / 三段式帧管线 / `Striped` 条带缓存作废**
> 实现基线：`lieui 0.1.0-alpha.4` 源码（行号均对应当前工作区）+ `docs/operation-log.md`

**一句话**：v4 的目标（静止零提交、位置变化零光栅化）成立，但它把"缓存"做成了**树和管线阶段** ——
于是需要同步、需要 z 键、需要裁剪栈对齐、需要 tile 淘汰。v7 把它降维成**装饰**：
树里一个 `bool`，渲染器里一个像素槽，Scene 里一个 `Op::Layer`。
层不再有对象、不再有表、不再是阶段；`render()` 仍是今天这一个函数。

---

## 0. 与 v4 / v6 的关系

### 0.1 从 v4 保留的判据

| v4 | v7 | 说明 |
|---|---|---|
| §2.1 控件树是唯一真实源 | ✅ | 但**禁反向句柄**（见 §0.3） |
| §2.2 变换与重绘分离（ADR-003） | ✅ | 唯一真正值钱的性能判据 |
| §2.3 失效驱动 / §13 静止零开销 | ✅ | 已落地（`Dirty` / `next_wakeup` / `ControlFlow::Wait`） |
| §2.5 层是缓存边界 | ✅ 判据，❌ 边界 | 层 = "稳定内容 + 独立平移/淡出"，不是"每控件"也不是"每 Root" |
| §9.3 持久最终 Pixmap + 损伤呈现 | ✅ | 已落地（`present_with_damage`） |
| §11.1 滚动只改 origin、损伤 = 视口 | ✅ | **全部收益来自这一条** |

### 0.2 从 v4 明确放弃的手段

| v4 | 否决理由（现状证据） |
|---|---|
| §5.2 / §6.1 持久布局镜像 + 严格同步 | 现状"边界临时 `build` FlexNode"已实测更好（`layout.rs:270-313`，测度缓存修复 4.3×） |
| §5.3 每控件默认一层、`blend_mode`、`z_index` | 每控件一层 ⇒ Pixmap 爆炸（v4 §15 自承 41MB）；`blend_mode`/`z_index` 无需求且翻倍测试面 |
| §5.4 损伤用 Tile 网格 + 精确矩形**双表示** | 双表示须同步；`damage_batches_bands`（`raster.rs:203-280`）已是"天然不重叠"的更好单元 |
| §7 `taitank-rs` | 纯成本 |
| §9.4 `LayerRasterizer` trait / §4 六个 crate | 只有一个后端；4 crate 够 |
| §11.2 滚动内容 Tile 化（ADR-006） | 它在替"整层缓存"擦屁股。层只需视口大小 ⇒ 不需要二维网格（§4） |
| §12.3 动画后自动层合并 | 不做；显式 promote/demote 是一行 `bool` |

### 0.3 v4 最深的错误：反向句柄

v4 §5.1 把 `layout_id: TaitankNodeId` / `layer_id: LayerId` 存进控件节点。"派生结构持有反向指针"这一个决定，直接生成了"三棵树必须严格同步"这个复杂度总源头 —— 一旦派生结构被真实源反向引用，"唯一真实源"这句话就失效了。

**同一个病今天还在 `Track` 里**：`Node`（`track.rs:772-835`，40 字段）同时装着
滚动子系统状态（`scroll_offset`/`content_size`/`scroll_drag`/`needs_scroll_event`）、
输入子系统状态（`interaction`/`focus_state`）、
布局子系统状态（`flags`/`layout_epoch`/`desired`/`computed`），
以及 19 个只服务**单个控件类型**的 `Track` 方法（`track.rs:1529-1925`）。
`impl Track` 因此有 70 个方法、跨 1080 行。

**v7 第一不变量：派生结构只允许单向引用树**（层表存 `NodeId`，树上不存 `LayerId`）；子系统状态各自归位。

### 0.4 与 v6 的差异（一张表看清）

| 主题 | v6 | v7 |
|---|---|---|
| 层是什么 | `Layer` 对象 + `LayerTable`（持久层树、`Root↔Layer` 映射） | **`Node.is_layer: bool` + `Renderer` 里的一个像素槽** |
| 层有哪些属性 | `origin/opacity/clip/z/bounds/dirty_rects` 七个字段 | `origin`=`rect().x/y`，`bounds`=`rect()`，`z`=树序，`clip`=`PushClip`；**只剩 `content_dirty` 与像素** |
| 合成在哪发生 | 新增第三阶段 `Compositor` + `damage_batches_bands` 驱动 | `rasterize_batches` 批次循环里的一个 `Op` 分支（与 `Op::Image` 同构） |
| 帧管线 | 三段式（层重绘 / 合成 / 上屏） | **两段 + 一个 `render()`**，内部三个循环 |
| 滚动内容存储 | `Striped(StripeCache)` 条带 + 淘汰 | 视口大小的单个 Pixmap + **shift blit** |
| 滚动层 z 序 | 未解决（`Layer::ALL` 表达不了树内子层） | 不存在该问题（层在树里） |
| 命中与绘制同源 | 需把层 origin 同时注入 `scene` 与 `hit` | 层不参与命中，`hit::descend` 一行不改 |
| 控件实体 | `Box<dyn Widget>` + `track.widget_mut::<T>` | `Box<dyn Widget>` + **双 arena + `WidgetCtx`**（解借用冲突） |
| `apply` 返回值 | `bool` | `Changed::{None, Paint, Layout}` |
| `on_event` 事件类型 | `&EventView`（**装不下 IME payload**） | `&Event` |
| `Track` | 保留单体 + `LayerTable` | 拆 5 张表（树/样式/控件/布局/交互+脏） |

---

## 1. 不变量

1. **一棵树**：`Track` 是唯一所有者。几何、像素、命中都是它的**按需派生**，不反向引用树。
2. **层是装饰，不是对象**：`is_layer` 一个 bool；像素槽归渲染器；层在 Scene 里是一个 `Op`。
3. **位置变化不重绘**：`origin` / `opacity` 变化只登记损伤带，不动任何 `Pixmap`；滚动另走 shift blit（§4）。
4. **绘制只读**：`Widget::draw(&self)`、`Widget::measure(&self)`、`SceneBuilder::build(&Track)`、`hit_path(&Track)` 全部是 `&`。唯一需要 `&mut` 的是 `on_event`。
5. **一个出帧判据**：`Track::needs_frame()`。禁止任何子系统"自己偷偷置帧"或"偷偷不置帧"。
6. **静止零开销**：无脏、无 origin 变化、无定时器 ⇒ `ControlFlow::Wait`，`render()` 不被调用。

**明确不做**：Tile 网格、`LayerTable`、层嵌套、每控件一层、自动层合并、`blend_mode`、GPU 后端 trait、缓存 Scene、布局镜像、`taitank-rs`、6 crate 拆分、细粒度响应式依赖追踪。

---

## 2. 架构

### 2.1 一张图

```
┌──────────────────────────────────────────────────────────────┐
│ 应用：ViewModel::view(&Rc<Self>, &mut ViewBuf) + Signal<T>      │
└──────────────────────────────────────────────────────────────┘
      ↓ Dirty::VIEW              ↓ 输入 / Cmd / 定时器
┌──────────────────────────────────────────────────────────────┐
│ Track（唯一真实源）                                            │
│   tree      parent / children / key / roots                   │
│   props     样式 + 语义属性 + handlers + bindings              │
│   widgets   Box<dyn Widget>（同 index）                        │
│   layout    computed / desired / text_wrap / flags / epoch     │
│   inter     hover / pressed / focus / captures / scroll        │
│   damage    Vec<Rect> + damage_all                             │
└──────────────────────────────────────────────────────────────┘
       ↓ ① align            ↓ ② layout
       └──────────┬─────────┘
                  ↓ ③ render(&Track, damage) —— 仍是当前这一个函数
   ┌───────────────────────────────────────────────────────┐
   │ ③.1 层重绘（仅 content_dirty 的 is_layer 子树）          │
   │      build_subtree → Scene → 层 Pixmap（复用 rasterize）│
   │ ③.2 行带批次循环（damage_batches_bands）                 │
   │      每带：底色 → 逐 Op（含 Op::Layer 的一次 blit）      │
   │ ③.3 拷回最终 Pixmap → present_with_damage(bands)        │
   └───────────────────────────────────────────────────────┘
```

### 2.2 三段在同一个函数里

v6 的"三段式"是**三个管线阶段**（中间产物是 `LayerTable`）。v7 的三个循环都在 `Renderer::render` 内部，**没有跨函数的中间产物**：

```rust
pub fn render(&mut self, track: &Track, damage: &[Rect], damage_all: bool) -> RenderStats
//   ↑ 签名一个字符都不改（render/mod.rs:115）
```

`hit` / `scene` / `layout` 仍然是 `&Track` 的纯函数，`present` 仍然吃 `bands`。

---

## 3. 核心改动一：层 = 缓存装饰

### 3.1 三个事实源，没有第四个

| 事实 | 存在哪 | 谁写 |
|---|---|---|
| 哪个子树是层 | `Node.is_layer: bool`（`inter` 表） | DSL（`v.scroll(..)`）/ `Ctx::promote(id)` |
| 层的像素 | `Renderer.caches: Vec<LayerCache>`（`NodeId` 索引） | 渲染器自己 |
| 层的位置 | `Node.rect()`（布局结果，已存在） | 布局（已存在） |

没有 `Layer` 结构体、没有 `LayerTable`、没有 `Root↔Layer` 映射、没有 `origin`/`bounds`/`z`/`clip` 字段 —— 它们全是上表三项的派生。

```rust
// src/render/cache.rs（新，约 80 行）
pub struct LayerCache {
    /// 内容画布；尺寸 = min(内容 bounds, 视口) × scale（见 §3.4）
    pixmap: Pixmap,
    /// 内容画布覆盖的逻辑矩形（左上角 = 内容原点，滚动时为容器原点）
    bounds: Rect,
    content_dirty: bool,
    /// 层内损伤（逻辑坐标，相对 bounds 原点）
    dirty: Vec<Rect>,
}
```

### 3.2 `Op::Layer`：合成就是一次 blit，和 `Op::Image` 同构

现状 `Op::Image`（`scene.rs:57-62` + `raster.rs:445-463`）已经在做这件事：绕开 vello、`flush_segment` 保住 z 序、手动 blit、受同一套软件裁剪栈约束（`raster.rs:434-443`，为修"图片溢出裁剪区"而加）。层合成与它**完全同构**。

```rust
// scene.rs：唯一新增的原语
Op::Layer {
    slot: u32,          // = Renderer.caches 的下标（由 NodeId 映射）
    rect: Rect,         // 层在场景坐标系里应占的逻辑矩形（含当前滚动偏移）
    transform: Affine,
    opacity: f32,       // 1.0 时走 memcpy 快路径
}
```

```rust
// raster.rs：加在 Op::Image 分支旁边
Op::Layer { slot, rect, transform, opacity } => {
    self.flush_segment(&mut pending, bw, bh, &clip_stack);   // z 序（已有）
    let tb = transform.bounding_box(*rect);
    let dst = to_batch_phys(tb, scale, batch);
    let clip = clip_stack.last().copied().unwrap_or(batch_bounds);
    if let Some(c) = clip.intersect(&dst) {
        blit_premul(&mut self.scratch, &caches[*slot].pixmap, c, *opacity); // 全库唯一混合实现
    }
}
```

**这一处替换掉了 v6 的整层结构**：

| v6 需要 | v7 为什么不需要 |
|---|---|
| `LayerTable` + `Root↔Layer` 映射 + 生命周期配对 | 层在 Scene 里是 op，没有表要维护 |
| `Layer.z` + `z_ordered_roots()` 驱动层合成 | `scene::walk` 的输出顺序就是 z 序（`scene.rs:529-543`） |
| `Layer.clip` + "层内 clip 要跟着位移" | 祖先的 `PushClip` 还在 op 流里，软件裁剪栈接管（已为图片实现） |
| 清底 / 残影问题 | 每批次从底色 op 重建（`scene.rs:519-524`），最终画布没有"半成品"状态 |
| 命中链跨层拼接 | 层不参与命中，`hit::descend` 一行不改 |
| 合成损伤单独一套 | `damage_batches_bands` 一个函数，场景剔除与批次同源 |

### 3.3 层重绘

```text
for id in track.dirty_layers():            // 只有 is_layer 且 content_dirty 的节点
    c = caches.entry(id)
    measure_bounds(c, layout_rect)          // 尺寸变了 ⇒ 重建 pixmap、整层脏
    scene = builder.build_subtree(track, id, cull = c.bands(), stop_at_layers = true)
    raster.rasterize_into(&mut c.pixmap, &scene, c.bands())   // 目标 Pixmap 参数化
    c.content_dirty = false
    track.damage_rect(layer_screen_rect)    // 层覆盖区并入合成损伤
```

要点三条：

1. **`rasterize_into` 复用 `rasterize_batches`**（`raster.rs:407-517`）：只把目标从 `self.pixmap` 变成参数。批次画布 `scratch` 每次 `reset_and_resize`（`:424`）后从全透明开始，**层画布的透明度天然正确，不需要显式 `clear_region`**。
2. **层内剔除必须由批次反推**（`render/mod.rs:110-114` 记过的坑："一滚整页消失"）：`c.bands()` = `damage_batches_bands(c.bounds, c.dirty)`，同一个列表喂给 `build_subtree` 和 `rasterize_into`。
3. **`build_subtree` 遇到 `is_layer` 就停**（不再下降），并把它压成一个 `Op::Layer`。这样父子两层不会互相嵌套，光栅化时"读别的层 + 写自己"永不产生别名。

### 3.4 谁能成为层

| 来源 | 说明 | 存储 |
|---|---|---|
| `v.scroll(..)` 的内容层 | DSL 生成 ⇒ `is_layer = true` | 视口大小（§4） |
| `Ctx::promote(id)` | 动画前显式提升；`demote` 收回 | `min(bounds, 4× 视口)`，超限**不缓存**（等价 `is_layer=false`） |
| `Layer::Popup/Tooltip/Modal/DragPreview` 根 | 已天然是独立子树 | 默认**不缓存**（它们通常每帧都变），需要时 promote |

**层不嵌套**：`is_layer` 子树内部的 `is_layer` 被忽略（要么提升为同级，要么不做）。这是 v7 与 v6 的一个关键分歧 —— 换来"光栅化无别名、z 序无第二套键"。

**默认没有层**：一个只有 `Content` 根的应用，`caches` 为空，`render()` 走的是**今天一行未改的路径**（`Op::Layer` 分支不命中）。v6 在这里要额外分配一张全窗 Pixmap 并每次重绘多拷一遍 —— 这是纯度更差的回归。

### 3.5 `opacity`

层 `opacity` 只是 `blit_premul` 的一个乘数（预乘 RGBA 乘全体 4 通道即正确）。层内 `Op::PushOpacity` 语义不变（vello 的 `push_opacity_layer`，`raster.rs:719`），两者不混用：**层的 opacity 作用于"已拍平的内容"，层内的作用于"绘制组"**。

---

## 4. 核心改动二：滚动 = 视口层 + shift blit

### 4.1 为什么不要 Tile / Stripe

v4 §11.2 的 Tile 化是为了让"整层缓存"的内存可控（1080×10000×4 ≈ 41MB）。v6 用单列 `Striped` 缓解，但仍然继承了完整的条带索引 / 淘汰 / 边界对齐逻辑，并且与自己的验收指标冲突（"滚动期光栅化像素 = 0" vs "滚动要光栅化新条带"）。

**观测事实：视口外的像素永远不会被看到。** 所以滚动内容层只需要**一张视口大小的画布**：

- 内存 = 1 个视口 / 滚动容器（不依赖内容长度）；
- 没有淘汰、没有索引、没有内存上界推理；
- 唯一的额外算法是 shift blit。

### 4.2 算法

```text
滚动 dy（物理像素对齐；|dy| < 视口高度）：
  ① 层画布 memmove：把 (h - |dy|) 行整体上移（用 Pixmap::data_as_u8_slice_mut + copy_within）
  ② 只光栅化新露出的那一条 |dy| 行：cull = 该条带，build_subtree 照旧
  ③ 合成损伤 = 视口（v4 §11.1）；最终画布从层画布 blit

|dy| ≥ 视口高度（拖动滚动条跳转）⇒ 整层重绘（②扩成整视口）
|dy| = 0                       ⇒ 什么都不做（静止零开销）
```

前置条件三条（缺一即错）：

1. **偏移必须对齐物理像素**（v4 §11.3），否则 shift 会累积重采样误差；
2. **层内不得有"按内容坐标绘制"的元素**（跨行阴影、粘性元素、跨行文字选中高亮）；有 ⇒ 该容器不启用层缓存；
3. 滚动条 / sticky header **不是层内元素**：它们由容器的兄弟节点绘制，本来就不平移（现状 `draw_scrollbar_overlay`，`widgets/mod.rs:1033-1049`）。

### 4.3 虚拟列表仍然会跑布局

`virtual_list`（`view.rs:622-684`）挂 `ScrollChanged` → `Signal::set` → 重跑 `view()` → `align` → 新行进 flex 树。**滚动帧的 `layout` 次数不为 0，且不应为 0** —— 它是在换窗，不是平移。所以指标必须分档（见 §9.2），不能写成"滚动零布局"。

### 4.4 成本下界（必须写进指标）

无论 shift 还是 blit，滚动帧的下界都是 **≈1 个视口的像素搬运**（1080p ≈ 8 MB）：

| 项 | 1080p 60fps 下的量级 |
|---|---|
| 层画布 shift | ~8 MB/帧 |
| 合成 blit（层 → 最终画布） | ~8 MB/帧 |
| 内存带宽占用 | ~1 GB/s（单核带宽的零头，但**不是零**） |

v6 的指标只约束"vello 光栅化像素"，会掩盖这一项。v7 把它显式化。

---

## 5. 核心改动三：控件 = 对象（契约修正版）

### 5.1 trait

```rust
// src/widget/mod.rs
pub trait Widget: Any + Debug {
    fn as_any(&self) -> &dyn Any;
    fn clone_box(&self) -> Box<dyn Widget>;
    /// 数据覆盖；返回**变的是什么**（对齐据此分级标脏）
    fn apply(&mut self, desc: &dyn Widget) -> Changed;
    fn widget_type(&self) -> TypeId { self.as_any().type_id() }

    fn measure(&self) -> Intrinsic { Intrinsic::Auto }
    fn draw(&self, cx: &mut DrawCtx<'_>) {}              // ← 只读
    fn on_event(&mut self, cx: &mut WidgetCtx<'_>, ev: &Event) -> bool { false }  // ← &Event
    fn paint_bounds(&self, rect: Rect) -> Rect { rect }
    fn focusable(&self) -> bool { false }
    fn on_attached(&mut self) {}
    fn on_detached(&mut self) {}
    fn debug_name(&self) -> &'static str;
}

pub enum Changed { None, Paint, Layout }
pub enum Intrinsic { Auto, Fixed(Size), Text(String, TextSpec) }
```

三处与 v6 的差别，都是**会编译不过或功能缺失**级别的：

| 项 | v6 | 为什么必须改 |
|---|---|---|
| `on_event` 事件类型 | `&EventView` | `EventView` 是 `Copy` 定长摘要（`event.rs:320-334`），**装不下 IME 的 preedit/commit 字符串**；`widgets::handle` 现在收的就是 `&Event`（`widgets/mod.rs:47-51`） |
| `draw` / `measure` | `&mut self` | `SceneBuilder::build(&Track)`（`scene.rs:513`）与 `hit_path(&Track)` 都必须是 `&Track`；`draw` 要 `&mut` 就等于给整棵树加 `RefCell` |
| `apply` 返回值 | `bool` | 现状 `align.rs:182` 把所有 desc 变化一律 `mark_layout_dirty + mark_paint_dirty`；`Changed` 让"Button 换了 label 但宽度没变"只重绘 |

### 5.2 借用模型：双 arena + 窄上下文

内置行为深度依赖框架服务（`slider_handle` 改 dragging、`scroll_handle` 改偏移、`input_handle` 改 caret/preedit 并要剪贴板）。若 `on_event` 从 `Track` 里借出 `&mut self`，就**不可能**再把 `&mut Track` 传进去。

```rust
pub struct Track {
    // ...
    nodes:   Vec<Option<Node>>,          // 只读视图（rect / content_size / styles / tab_* / bindings）
    widgets: Vec<Option<Box<dyn Widget>>>, // 同 index ⇒ 两个 Vec 可同时独立借用
}

pub struct WidgetCtx<'a> {
    pub node: &'a Node,        // 只读；**不含**任何可变入口
    pub theme: &'a Theme,
    pub now_ms: u64,
    pub enabled: bool,
    pub cmd: &'a mut CmdBuf,   // 一切副作用走命令
}
```

规则：

- **只读用 `node`，一切写操作走 `cmd`**（`Cmd::Damage` / `ScrollTo` / `SetFocus` / `Capture` / `Clipboard` 均已存在或加上）。
- 控件**不能**改别的节点、不能改样式、不能请求布局。需要时发命令，两段式（`cmd.rs` 的 `CmdBuf`）保持不变。
- `track.widget_mut::<T>(id)` 只是 downcast 助手（测试与内置行为共用），不是访问通道。

### 5.3 对齐契约

```rust
if node.widget_type() != desc.widget_type() {
    create_subtree(..);                        // 类型不同即重建，视图态随之丢弃
} else {
    match node_widget.apply(&*desc_widget) {
        Changed::None   => {},
        Changed::Paint  => mark_paint(id),
        Changed::Layout => { mark_layout(id); mark_paint(id); }
    }
}
```

`apply` 必须保留的既有语义（从 `track.rs:312-423` **平移**，不是重写）：f32 用 `to_bits()` 比较、Input 在 `preedit` 非空时不覆盖 text 且不重置 caret、Image 用 `Arc::ptr_eq`、Custom 用 `Rc::ptr_eq`。

### 5.4 测度契约（别丢两个既有优化）

`Intrinsic::Text` 必须同时绑定两件事，否则重构会静默丢掉已实测的收益：

| 现状特判 | 位置 | 收益 |
|---|---|---|
| `text_wrap` 写回（Text / Button） | `layout.rs:338-342` | 测度与绘制的换行宽度一致，否则顶对齐错位 |
| `measured_content` 复用（Text / Input） | `layout.rs:390-410` | D-a 修复：每轮布局少一次 `measure_text` |

规则：**返回 `Intrinsic::Text` 的控件自动获得**这两条；`measure()` 每轮只回答一次（不要在 `build` 和 `write_back` 各调用一遍）。

### 5.5 分界线：节点能力 vs 控件行为

一句话判据：*"这个状态换成一个别的控件类型后还有意义吗？"*

| 有 → 节点能力（留在 `Node` / `inter`） | 没有 → 控件内部（搬进 `widget/`) |
|---|---|
| 滚动偏移、内容尺寸、滚动条拖拽会话 | `InputState`（caret/anchor/preedit/scroll） |
| hover / pressed / focused / captures | `SliderState`（dragging） |
| `visible` / `hit_test_visible` / `clip` / `transform` / `tab_*` | `Checkbox`/`Switch` 的 checked 视觉过渡 |
| tooltip / context_menu | 各控件的 `on_event` 状态机 |

按这条线切，`Track` 立刻从 70 个方法掉到 ~50，`Node` 从 40 字段掉到 ~28（其中 3 个是死字段，见 §8.3）。

---

## 6. 核心改动四：`Track` 拆表

### 6.1 五张表（同一 `NodeId` 索引，不是五棵树）

| 表 | 内容 | 唯一写者 |
|---|---|---|
| `tree` | `parent` / `children` / `key` / `roots`（z 槽位） | `align` |
| `props` | `FlexStyle` / `PaintStyle` / `TextStyle` / `ImageStyle` / `visibility` / `clip` / `transform` / `tab_*` / `handlers` / `bindings` | `align` |
| `layout` | `computed` / `desired` / `text_wrap` / `content_size` / `flags` / `layout_epoch` | `layout` |
| `inter` | `hover` / `pressed` / `pressed_path` / `focused` / `captures` / `scroll_offset` / `scroll_drag` / `is_layer` | `input` / `event` |
| `damage` | `Vec<Rect>` + `damage_all` | `align` / `layout` / `event` |

签名即权限，编译器替你做访问控制：

```rust
layout(&Tree, &Props, &mut Layout, &mut Damage);                 // 不再需要 RefCell
handle(&Tree, &Props, &mut Widgets, &mut Inter, &mut Damage, &mut CmdBuf);
render(&Tree, &Props, &Layout, &Inter, &Damage);
```

**已知的共写点（必须显式处理，不要假装不存在）**：`scroll_offset` 被 `layout`（钳制，`layout.rs:364-370`）与 `input`（滚轮/拖拽）两边写。做法：布局只写 `content_size`，钳制抽成布局后的一个纯函数 `scroll::clamp(&mut Inter, &Layout)`，让 `layout` 保持单一写者。

### 6.2 迁移不变量（每个 PR 都要 grep 可验证）

- `scroll_offset` 只出现在 `inter.rs` 与 `widgets/scroll.rs`
- `computed` / `desired` 只出现在 `layout.rs` 与 `render/scene.rs`
- `borrow_mut()` 站点数单调下降（现状非测试 **31** 处）
- `track.rs` 行数单调下降（现状 **2096**）

---

## 7. 帧管线与脏通道合一

### 7.1 唯一判据

今天有**三条互不知情**的脏路径，这是 `widgets/mod.rs:395-402` 那段病历（"拖动无反应，松手才跳到最后位置"）的根因：

| 路径 | 谁写 | 谁读 |
|---|---|---|
| `Node.flags`（`MEASURE_DIRTY`/`PAINT_DIRTY`…） | `Track::mark_*_dirty` | `layout` / `render` |
| `Runtime` 位集（`Dirty::VIEW/LAYOUT/PAINT/PRESENT`） | `rt.mark_dirty` / `Cmd` | `needs_frame` / `frame` |
| `Track.damage`（矩形） | 各失效点 | `render` |

v7 收敛为：

```rust
impl Track {
    /// 唯一出帧判据：任何让画面变了的路径都必须落在这里
    pub fn needs_frame(&self) -> bool {
        self.dirty.view || self.dirty.layout
            || self.has_layout_dirty()          // layout 表 flags
            || !self.damage.is_empty()          // 含"层 origin 变了"
            || self.dirty_layers()              // 任一 is_layer 且 content_dirty
    }
}
```

`Runtime::peek_dirty` 降级为"外部是否来过事件"的输入，不再直接决定出帧。`Cmd::Damage` 只是"登记损伤"的语法糖。

### 7.2 帧循环

```text
WindowCtx::frame():
  ① VIEW    → view() + align()        ⇒ tree/props/widgets、dirty 位、损伤
  ② LAYOUT  → layout()                ⇒ layout 表、content_size、几何损伤
  ②.5       → scroll::clamp()         ⇒ inter.scroll_offset（钳制）
  ②.6       → ScrollChanged 派发       （时点不变，window.rs:566-575）
  ③ RENDER  → level_redraw() + bands 循环 + 上屏同源
  ④ 时钟    → next_clock / next_wakeup ⇒ Wait / WaitUntil
```

### 7.3 统计

`FrameStats`（`window.rs:217-239`）新增：

| 字段 | 含义 |
|---|---|
| `layers_redrawn` | 本帧重绘的层数 |
| `layers_moved` | 仅 origin/opacity 变化的层数 |
| `blit_pixels` | 合成期搬运的像素数（**新增硬指标**） |
| `raster_pixels` | 已有（`rasterized_pixels()`），层重绘与合成分别计数 |

`FrameStats.damage` 必须是**合成后**的损伤带（`present_with_damage` 直接消费它，`platform/mod.rs:561-583`）。

---

## 8. 改动清单

### 8.1 零改动

`hit.rs`（层不参与命中）、`input.rs`、`focus.rs`、`theme.rs`、`style.rs`、`event.rs`、`post.rs`、`timer.rs`、`transform.rs`、`menu.rs`、`crates/lieui-*`、`Scene`/`TextCache`、`damage_batches*`、`pack_xrgb`/`copy_damage_rects`、`present_with_damage`、`z_ordered_roots()`、`Dirty`/`next_wakeup`/`ControlFlow`。

### 8.2 必须改造

| 文件 | 改动 | 估计 |
|---|---|---|
| `render/scene.rs` | `Op::Layer` 变体；`build` → `build` + `build_subtree(id, cull, stop_at_layers)`；`walk` 遇 `is_layer` 停 | ~60 行 |
| `render/raster.rs` | 目标 Pixmap 参数化（`rasterize_into`）；`Op::Layer` 分支；抽出 `blit_premul`（统一供 `blit_image` 与层合成） | ~120 行 |
| `render/mod.rs` | `caches: Vec<LayerCache>`；`render()` 内部 ③.1 层重绘循环；`resize`/`set_scale` 时丢弃全部缓存 | ~80 行 |
| `render/cache.rs` | 新模块：`LayerCache` + shift blit | ~100 行 |
| `track.rs` | 删 `Kind` 三枚举 + 19 个控件方法；`Node` 减 12 字段；加 `is_layer`、`needs_frame()`、`dirty_layers()`、`layer_of()` | −600 行 |
| `widget/*.rs` | 新模块：`Widget` trait + 宏 + 11 个控件 | +700 行 |
| `align.rs` | 身份判据换 `widget_type()`；`apply` 三态标脏 | ~30 行 |
| `layout.rs` | `measure` 桥接（`Intrinsic`）；撤销 `-offset` 烘焙（`:353-377`）；`scroll::clamp` 外移 | ~40 行 |
| `view.rs` | 25 处 `push_desc` → `push_widget`；`scroll()` 置 `is_layer` | ~120 行 |
| `window.rs` | `needs_frame` 判据；`layer_redraw` 接线；统计 | ~60 行 |
| `platform/mod.rs` | 剪贴板 `Kind::Input` 判据 → widget 类型判据 | ~10 行 |

### 8.3 删除

| 项 | 位置 | 量级 |
|---|---|---|
| `KindTag` / `KindDesc` / `Kind` + `apply_to` + 两份 `tag()` + `from_desc` | `track.rs:206-531` | ~330 行 |
| 绘制 11 arm / 行为 11 arm / 测度 3 处 switch | `widgets/mod.rs:710-1022`、`:72-89`、`layout.rs:277-297,411-437` | ~340 行 |
| `Track` 上的 19 个控件状态 API | `track.rs:1529-1925` | ~200 行 |
| `kind_name` | `scene.rs` | ~30 行 |
| 死字段 `layout_rounding` / `allow_drop` / `can_drag` | `Node`（全库无读写） | 3 字段 |
| `crates/lieui-layout/src/measurable.rs` + `constraint.rs` | 主 crate 零引用（**但是该 crate 的公开导出，属 API 破坏**） | 2 文件 |

**净估算**：约 +1000 / −1500 行；核心概念从约 14 个（树/描述/Widget/Layer/LayerTable/LayerStorage/StripeCache/Compositor/带/脏位/脏区/Scene/Op/Pixmap）降到约 10 个（树/描述/Widget/`is_layer`/LayerCache/带/判据/Scene/Op/Pixmap）。

---

## 9. 落地顺序与验收

**结论先行：先花 2–3 天做一个可丢弃的原型，再决定后面三个季度值不值。**

| 阶段 | 内容 | 验收 | 成本 |
|---|---|---|---|
| **P0 原型（可丢弃）** | `Node.is_layer` + `Op::Layer` + `blit_premul` + 视口大小滚动层 + shift blit。**不碰 `Kind`/`Track`/`align`** | 见 §9.2 | 2–3 天 |
| **决策门** | 若"省下的布局时间 < 付出的合成带宽"，则**只保留滚动这一处**，`promote`/`opacity`/虚拟列表层全部砍掉 | `tests/compositor_probe.rs` 前后对比 | — |
| P1 Widget 化 | `Widget` trait + 宏 + 11 控件；下沉控件状态；`Changed`/`Intrinsic`；`Track` 双 arena | 497 条测试全绿；`clippy::all = deny`；像素回归无差异 | 中 |
| P2 Track 拆表 | 按写集拆 5 张表（每个 PR 一张） | §6.2 四条不变量 | 大 |
| P3 动画层 | `promote`/`demote`、层 `opacity`、`needs_frame` 合一 | 动画帧光栅化 0、静止 `Wait` | 小 |
| P4 清理 | 删三枚举 / 死字段 / 死模块；`prelude` 收敛 | 公开契约测试（`tests/api_contract.rs`）不变 | 小 |

### 9.1 为什么 P0 必须独立且可丢弃

`Op::Layer` 是**纯加法**：加一个变体、加一个分支、加一个缓存结构。删掉就回到今天，零风险。
它用一个周末回答"合成器到底值不值"这个价值百万的问题 —— 而 v6 把同样的问题留到 C3，中间要先押上 W0–W2 的全部工作量。

### 9.2 指标（P0 就要可测）

| 指标 | 目标 | 测法 |
|---|---|---|
| 非虚拟列表滚动的 `layout` 调用 | **0** | `tests/compositor_probe.rs` 的 `layout%` 列 |
| 滚动帧的 vello 光栅化面积 | ≤ 1/20 视口 | `RenderStats.raster.pixels` |
| 滚动帧的像素搬运 | ≤ 2 视口 | 新增 `blit_pixels` |
| 层像素总量 | ≤ 3 视口 + 活跃 promote 层 | `LayerCache` 面积求和 |
| 静止 | `ControlFlow::Wait`，`render()` 不被调用 | `tests/perf_regression.rs` |
| 像素正确性 | 逐像素无差异 | `src/render/render_tests.rs:13` 的 `px()` 传统 |

**必须写清的例外**：`virtual_list` 滚动会重跑 `view()` + `align` + `layout`（§4.3），它的指标是"新增节点数 ≤ 视口行数 + 1"，不是"layout = 0"。

---

## 10. 风险与缓解

| 风险 | 缓解 |
|---|---|
| **shift blit 的正确性**（跨行阴影 / 粘性元素 / 亚像素偏移） | 偏移强制物理像素对齐；层内"跨行绘制"的容器不启用缓存（§4.2 前置条件）；专测：带阴影的行滚动后残影检查 |
| **合成带宽被低估** | §4.4 的下界写进指标；`opacity == 1.0` 走 `copy_from_slice` 快路径；不做整窗合成（只做损伤带） |
| **层画布与最终画布的透明度语义** | 层重绘复用批次循环（`reset_and_resize` ⇒ 每批从透明开始），不做"叠加式"局部重绘；最终画布仍从底色 op 重建 |
| **`Op::Layer` 破坏 z 序**（与"图片盖住遮罩"同类） | 与 `Op::Image` 共用 `flush_segment`（`raster.rs:529-558`）；把 `primitive_after_an_image_are_painted_above_it` 扩成"层之后的原语在层之上"的等价测试 |
| **Widget 化改动面**（546 处 `Kind::` / 39 文件 / 497 测试） | P0 阶段完全不动它；P1 先立构造器助手与 `widget_of::<T>`，逐文件迁移；`prelude` 签名不变 |
| **Track 拆表把借用复杂度转成签名复杂度** | 每次只拆一张表；若 `WidgetCtx` 类结构数 > 拆出的表数，停手（说明拆过头） |
| **合成器收益整体被高估** | 决策门（§9）+ 明确降级路径："只保留滚动层" |

---

## 11. ADR

| 编号 | 决策 | 代价 |
|---|---|---|
| ADR-v7-001 | **层降维为装饰**：`Node.is_layer` + `Renderer` 像素槽 + `Op::Layer`，废除 `LayerTable` 与三段式 | 层不能嵌套；层的 `transform` 只能是平移（旋转留给层内 `Op::transform`） |
| ADR-v7-002 | **合成实现为一个 Op**，与 `Op::Image` 同构，复用 `flush_segment` 与软件裁剪栈 | 依赖"图片 blit 路径"的正确性；两者共享同一套坑 |
| ADR-v7-003 | **滚动用视口层 + shift blit**，废除 Tile 网格与 `Striped` 条带 | 层内不能有跨行绘制元素（否则该容器不缓存）；偏移必须物理像素对齐 |
| ADR-v7-004 | **不做层嵌套、不做自动层合并、不做 `blend_mode`** | 复杂动画需要应用侧手工组织层 |
| ADR-v7-005 | **`draw`/`measure` 只读**，`on_event` 收 `&Event` | 控件只能经 `CmdBuf` 产生副作用；IME payload 必须走完整 `Event` |
| ADR-v7-006 | **Track 拆 5 张同索引表**（按写集），不是多棵树、不是 ECS | 函数参数变长（用窄上下文包）；~96 个调用点 + 497 测试要改 |
| ADR-v7-007 | **先原型后实施**：P0 可丢弃原型作为决策门 | 若原型不达标，W/P1 的工作不会提前投入 |

---

## 12. 结语

v4 想要的东西 v7 一条不少：静止零提交、位移零光栅化、损伤驱动合成、层是缓存边界。
v7 拿掉的是 v4 在实现这些目标时**顺手引入的三样东西** —— 层的对象、层的表、合成的阶段。
它们不是目标的必要条件，而是"把缓存当成结构"的产物；而缓存本来是**装饰**。

于是净结果是：**树还是那棵树，`render()` 还是那个函数，命中还是那个纯函数**，
多出来的只有三行概念：一个 `bool`、一个像素槽、一个 `Op`。
