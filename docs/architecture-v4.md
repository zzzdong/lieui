# Rust 简易 GUI 库架构设计文档

**版本**：v0.1（设计稿）  
**状态**：草案  
**目标**：一个纯 Rust、CPU 渲染优先、支持局部更新与滚动/动画的简易 GUI 库

---

## 1. 概述与目标

### 1.1 项目定位

构建一个**轻量级、纯 Rust 实现**的 GUI 库，面向工具类桌面应用。核心约束：

- **持久控件树 + 持久渲染层树**，通过 Compositor 合成机制实现跨帧像素复用。
- **布局系统移植 Taitank**（采用 `taitank-rs` 纯 Rust 实现），提供完整 Flexbox 布局能力。
- **文本排版采用 Parley**，通过 measure 回调接入布局系统。
- **图形渲染使用 `vello_cpu`**（纯 CPU 光栅化），架构预留 `vello_gpu` 切换缝。
- **避免每帧重绘**：静止时零提交，滚动/动画时“每帧合成、几乎不重绘”。

### 1.2 非目标

- 不追求跨平台窗口系统抽象（早期聚焦单平台，平台宿主后续接入）。
- 不实现 CSS Grid 等 Taitank 之外的布局模型。
- 不提供 GPU 渲染路径（v0.1 仅 CPU，但预留抽象）。
- 不追求企业级控件库的完整度。

### 1.3 性能目标

| 指标 | 目标 |
| :--- | :--- |
| 静止状态 CPU 占用 | 0%（整帧跳过，事件驱动） |
| 局部内容更新 | 仅重绘脏层 + 合成损伤区域 |
| 滚动/位移动画 | 零 `vello_cpu` 光栅化，仅合成 |
| 单帧合成延迟（1080p 视口） | < 3ms（CPU） |

---

## 2. 设计原则

1. **控件树是唯一真实源**。布局树和渲染层树都是它的派生结构，不持有应用状态。
2. **变换与重绘分离**。位置、缩放、透明度变化只改层属性，不触发 Pixmap 重绘。
3. **失效驱动，而非轮询**。所有更新由脏标记驱动，无变化时整帧跳过。
4. **两级失效通道**。布局失效与绘制失效独立传播，互不误触发。
5. **层是缓存的边界**。层的划分决定缓存收益与合成开销的平衡。
6. **后端抽象前置**。`Pixmap` 不作为层缓存的硬编码类型，为 `vello_gpu` 预留关联类型。

---

## 3. 整体架构

### 3.1 分层视图

```
┌─────────────────────────────────────────────────────┐
│                    应用层（用户代码）                  │
│         声明控件、绑定状态、注册事件回调                 │
└─────────────────────────────────────────────────────┘
                          │
┌─────────────────────────────────────────────────────┐
│  控件树（Widget Tree）—— 持久，唯一真实源              │
│  SlotMap<WidgetId, WidgetNode>                      │
│  · 控件类型、父子关系、样式、内容、交互状态              │
└─────────────────────────────────────────────────────┘
          │                          │
          ▼                          ▼
┌──────────────────────┐  ┌──────────────────────────┐
│ 布局树（Taitank 镜像） │  │ 渲染层树（Layer Tree）     │
│ taitank-rs 节点       │  │ 持久层 + 每层缓存 Pixmap   │
│ · measure 回调接 Parley│  │ · LayerStorage: Single/Tiled│
│ · do_layout → 回写几何 │  │ · origin/opacity/transform │
└──────────────────────┘  └──────────────────────────┘
          │                          │
          │ geometry_dirty           │ content_dirty
          ▼                          ▼
┌──────────────────────┐  ┌──────────────────────────┐
│ 损伤追踪（Damage）     │  │ 层重绘（vello_cpu）        │
│ Tile 网格 + 矩形集合   │  │ RenderContext → Pixmap    │
└──────────────────────┘  └──────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────┐
│  Compositor —— 按损伤区域合成                          │
│  遍历脏 tile，按 z-order 收集相交层，clipped blit       │
└─────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────┐
│  平台宿主（窗口、事件循环、呈现）                       │
│  winit / 原生 Win32（后续接入）                        │
└─────────────────────────────────────────────────────┘
```

### 3.2 数据流

```
控件变更
  ├─ 结构/样式变更 → layout_dirty → Taitank 增量布局
  │                                      │
  │                                      ▼
  │                              几何回写 → 比较新旧
  │                                      │
  │                                      ▼
  │                              geometry_dirty → 损伤标记（旧∪新）
  │
  └─ 视觉属性变更 → paint_dirty → 层 content_dirty
                                        │
                                        ▼
                                 层重建 Scene → vello_cpu 光栅化
                                        │
                                        ▼
                                 损伤标记（层覆盖区域）
                                        │
                                        ▼
                              Compositor 合成损伤区域 → present
```

---

## 4. Crate 划分

| Crate | 职责 | 核心依赖 |
| :--- | :--- | :--- |
| `gui_core` | 控件树、状态、事件、`WidgetId` 句柄 | `slotmap` |
| `gui_layout` | Taitank 适配层、脏标记传播、几何回写 | `taitank-rs` |
| `gui_text` | Parley 文本整形、measure 回调、Layout 缓存 | `parley` |
| `gui_scene` | 层树、Scene 构建、`LayerStorage` | `vello_cpu`、`peniko`、`kurbo` |
| `gui_compositor` | 损伤追踪、tile 网格、合成器 | `vello_cpu` |
| `gui_runtime` | 事件循环、帧调度、平台宿主抽象 | `winit`（可选） |

**边界原则**：布局/文本/场景/合成之间通过 trait 和纯数据结构交互，不互相依赖具体实现。早期可合并为少量 crate 验证，但模块边界须提前确定。

---

## 5. 核心数据结构

### 5.1 控件树

```rust
struct WidgetTree {
    nodes: SlotMap<WidgetId, WidgetNode>,
    root: WidgetId,
}

struct WidgetNode {
    kind: WidgetKind,               // Panel / Text / Image / ScrollView ...
    parent: Option<WidgetId>,
    children: Vec<WidgetId>,
    style: Style,                   // 布局样式 + 视觉样式
    content: Option<Content>,       // 文本字符串 / 图像句柄
    layout_id: TaitankNodeId,       // 布局镜像句柄
    layer_id: LayerId,              // 渲染层句柄
    interaction: InteractionState,  // 焦点/悬停/拖拽
    flags: NodeFlags,               // layout_dirty / paint_dirty / subtree_dirty
}
```

### 5.2 布局节点

```rust
struct LayoutNode {
    taitank_node: TaitankNode,
    last_layout: Layout,            // 上次布局结果（用于新旧比较）
    layout_dirty: bool,
}
```

### 5.3 渲染层

```rust
struct LayerTree {
    layers: SlotMap<LayerId, Layer>,
    root: LayerId,
}

struct Layer {
    storage: LayerStorage,
    origin: PhysicalPosition<i32>,
    transform: Option<Transform>,   // 缩放/旋转（合成时应用）
    opacity: f32,
    blend_mode: BlendMode,
    clip: Option<Rect>,
    z_index: i32,
    content_dirty: bool,
    geometry_dirty: bool,
    scene: Option<RenderContext>,   // 该层的 Vello 绘制上下文
}

enum LayerStorage {
    Single(Pixmap),                 // 普通层
    Tiled(TileCache),               // 滚动内容层（按 256×256 分块）
}
```

### 5.4 损伤追踪

```rust
struct DamageTracker {
    tiles: TileGrid,                // 粗粒度 tile 网格（64×64 或 128×128）
    dirty_tiles: HashSet<TileId>,
    dirty_rects: Vec<Rect>,         // 精确矩形（用于小范围更新）
}

impl DamageTracker {
    fn mark_rect(&mut self, rect: Rect);
    fn mark_layer_move(&mut self, old: Rect, new: Rect);
    fn is_empty(&self) -> bool;
    fn drain_tiles(&mut self) -> Vec<TileId>;
}
```

---

## 6. 三棵树与同步机制

### 6.1 控件树 → 布局树

- **适配器模式优先**：不维护独立 `TaitankTree` 副本，让 Taitank 通过 trait 访问控件树数据。若 `taitank-rs` 不支持 trait 遍历，则退化为**复制模式**，但须保证结构变更时严格同步。
- 结构变更（增删子节点）→ 同步 Taitank 节点树。
- 样式变更 → 更新对应 Taitank 节点的样式，标记 `layout_dirty`。

### 6.2 布局树 → 控件树

- `do_layout` 完成后，遍历 Taitank 节点读取最终几何，回写到 `WidgetNode` 的布局结果字段。
- **新旧几何比较**：只有位置/尺寸真正变化的节点才向渲染层发出 `geometry_dirty`。

### 6.3 控件树 → 渲染层树

- 每个控件默认对应一个层；合成边界标记可提升子树为独立层。
- 布局结果驱动层尺寸与 `origin` 更新。
- 内容变更驱动 `content_dirty`。

### 6.4 层树 → Compositor

- 层树维护 z-order 排序（可按 `z_index` + 树序稳定排序）。
- Compositor 只读取层树的 `origin`、`opacity`、`Pixmap`，不修改层树。

---

## 7. 布局系统：Taitank 移植

### 7.1 技术选型

| 方案 | 说明 | 采用 |
| :--- | :--- | :--- |
| `taitank-rs` | 纯 Rust 实现，API 兼容 C++，通过全部 Flex 测试用例 | ✅ 首选 |
| `taitank-safe` | 官方 C++ 的安全 FFI 绑定 | 备选（引入 C++ 构建依赖） |

### 7.2 布局执行流程

```rust
fn run_layout(tree: &mut WidgetTree, root: WidgetId, viewport: Size) {
    // 1. 同步脏子树到 Taitank
    sync_dirty_subtrees(tree, root);

    // 2. 调用 Taitank 布局
    let taitank_root = tree[root].layout_id;
    taitank::do_layout(taitank_root, viewport.w, viewport.h, Direction::LTR, None);

    // 3. 回写几何 + 比较
    for (widget_id, node) in tree.nodes.iter_mut() {
        let new_layout = read_layout_from_taitank(node.layout_id);
        if new_layout != node.last_layout {
            node.last_layout = new_layout;
            mark_geometry_dirty(widget_id, new_layout);
        }
    }
}
```

### 7.3 文本测量桥接

Taitank 的文本叶子节点通过 **`measure` 回调**获取内容尺寸：

```rust
fn measure_text_callback(
    node: &TaitankNode,
    width: f32, width_mode: MeasureMode,
    height: f32, height_mode: MeasureMode,
) -> MeasureOutput {
    let text_node = node.user_data::<TextMeasureData>();
    // 约束未变且内容未脏 → 返回缓存
    if text_node.constraints_match(width, height) && !text_node.content_dirty {
        return text_node.cached_size;
    }
    // 调用 Parley 整形 + 换行
    let layout = text_node.parley_layout(width, height);
    MeasureOutput { width: layout.width(), height: layout.height() }
}
```

**约束**：字体必须在测量前加载到 `FontContext`，整个生命周期复用同一 `FontContext`。

---

## 8. 文本系统：Parley

### 8.1 职责

- 文本整形、换行、glyph 定位。
- 向布局系统提供 `measure` 结果。
- 向绘制系统提供已定位的 glyph run。

### 8.2 Layout 缓存

- Parley `Layout` 对象缓存在文本控件节点上。
- 仅当 `content_dirty` 或可用宽度变化时才重新整形。
- 绘制阶段遍历 `PositionedLayoutItem`，转为 Vello Scene 命令；**颜色在绘制时应用，布局不携带颜色**。

### 8.3 与渲染层的交互

```rust
fn build_text_scene(layer: &mut Layer, text: &TextNode) {
    layer.scene.reset();
    for item in text.parley_layout.items() {
        match item {
            PositionedLayoutItem::GlyphRun(run) => {
                // 将 glyph run 编码到 Vello RenderContext
                encode_glyph_run(&mut layer.scene, run, text.color);
            }
            _ => {}
        }
    }
}
```

---

## 9. 渲染与合成系统

### 9.1 层重绘（`vello_cpu`）

每个层持有独立的 `RenderContext` 和 `Pixmap`：

```rust
impl Layer {
    fn rebuild_if_dirty(&mut self) {
        if !self.content_dirty { return; }
        self.scene.reset();
        // 根据控件内容编码绘制命令
        encode_layer_content(&mut self.scene, self);
        self.scene.render_to_pixmap(&mut self.pixmap);
        self.content_dirty = false;
    }
}
```

**尺寸约束**：层的 `Pixmap` 尺寸 = 布局后的逻辑尺寸 × DPI 缩放，**不为每个层分配全屏 Pixmap**。

### 9.2 Compositor：损伤驱动合成

```rust
fn composite_damaged(root: &mut Pixmap, layers: &LayerTree, damage: &mut DamageTracker) {
    for tile in damage.drain_tiles() {
        let tile_rect = tile.to_rect();
        // 清空该 tile（或用最底层背景填充）
        clear_region(root, tile_rect);
        // 按 z-order 从底到顶合成相交层
        for layer in layers.iter_bottom_to_top() {
            if let Some(clipped) = layer.bounds().intersect(tile_rect) {
                blit_clipped(
                    root,
                    &layer.pixmap(),
                    layer.origin,
                    clipped,
                    layer.opacity,
                    layer.transform,
                );
            }
        }
    }
}
```

**关键规则**：
- 损伤集合为空 → **合成和呈现全部跳过**。
- 层移动 → 标记旧位置 ∪ 新位置，**不重绘该层**。
- 层内容变脏 → 重绘该层后，标记该层覆盖区域。
- 层 z-order 变化 → 标记受影响区域（通常为所有层覆盖区域的并集）。

### 9.3 双缓冲与呈现

- 维护一份持久的“最终画面” `Pixmap`。
- 损伤合成直接修改该 Pixmap。
- 呈现时整块提交（或只提交损伤矩形给平台，视平台能力）。
- 未损伤区域的内容与上一帧一致，无需每帧清空。

### 9.4 为 `vello_gpu` 预留的抽象

```rust
trait LayerRasterizer {
    type Target;
    fn rasterize(&mut self, scene: &RenderContext, size: (u32, u32)) -> Self::Target;
    fn composite(&self, targets: &[LayerTarget<Self::Target>], output: &mut Self::Target);
}
```

`vello_cpu` 实现用 `RenderContext + Pixmap`；未来 `vello_gpu` 可复用同一 Scene 描述，将稀疏条带上传 GPU 执行。**层缓存的类型不硬编码为 `Pixmap`**。

---

## 10. 失效传播管线

### 10.1 两条独立通道

| 通道 | 触发条件 | 后果 |
| :--- | :--- | :--- |
| **布局通道** | 控件结构/样式变更、窗口尺寸变化 | 标记祖先 `subtree_dirty`，Taitank 增量重算，回写几何 → `geometry_dirty` |
| **绘制通道** | 颜色/文本/圆角等视觉属性变更 | 标记层 `content_dirty`，不触发布局 |

### 10.2 传播规则

- `layout_dirty` → 不一定导致 `content_dirty`（如只改 margin，层内容不变）。
- `content_dirty` → **一定不**触发 `layout_dirty`（纯视觉变更）。
- `geometry_dirty` → 不重绘层，只标记损伤（旧∪新）。
- `subtree_dirty` → 父节点布局需要重算，但未变子树依赖 Taitank 缓存跳过。

### 10.3 失效入口

```rust
impl WidgetTree {
    fn set_style(&mut self, id: WidgetId, style: Style) {
        let node = &mut self.nodes[id];
        if node.style.layout_fields_changed(&style) {
            self.mark_layout_dirty(id);
        }
        if node.style.visual_fields_changed(&style) {
            self.mark_paint_dirty(id);
        }
        node.style = style;
    }
}
```

---

## 11. 滚动处理

### 11.1 架构

- 滚动容器作为独立层，内容层持有 `TileCache`。
- 滚动只改内容层的 `origin.y`（负滚动偏移），**不触发布局、不重绘任何层**。
- 损伤区域 = **整个视口矩形**（视口内每个像素都可能变化）。

### 11.2 Tile 化内容层

- 内容层按 256×256 或 512×512 分块，仅缓存视口 ±1 屏的 tile。
- 视口外 tile 可淘汰，避免长列表的整层 Pixmap 内存爆炸。
- 滚动时按顺序 blit 视口内 tile。

### 11.3 CPU 优化

- **滚动偏移对齐物理像素**，避免 blit 重采样。
- 滚动条、sticky header 作为兄弟层，不随内容平移。
- 惯性滚动期间每帧标记视口损伤；静止后损伤清空，整帧跳过。

---

## 12. 动画处理

### 12.1 按属性分类

| 动画类型 | 布局 | 重绘 Pixmap | 损伤区域 | 推荐度 |
| :--- | :--- | :--- | :--- | :--- |
| transform（位移/缩放/旋转） | ❌ | ❌ | 旧∪新包围盒 | ⭐⭐⭐ 首选 |
| opacity | ❌ | ❌ | 层覆盖区域 | ⭐⭐⭐ 首选 |
| width/height/padding | ✅ Taitank | ⚠️ 可能 | 受影响子树 | ⭐ 谨慎 |
| 颜色/文本内容 | ❌ | ✅ | 层覆盖区域 | ⭐⭐ 限制在小层 |

### 12.2 合成路径动画（零重绘）

```rust
fn tick_animation(layer: &mut Layer, t: f32, damage: &mut DamageTracker) {
    let old_bounds = layer.bounds();
    layer.transform = compute_transform(t);
    layer.opacity = compute_opacity(t);
    layer.geometry_dirty = true;
    damage.mark_rect(old_bounds);
    damage.mark_rect(layer.bounds());  // 旧 ∪ 新
    // content_dirty 保持 false → 不重绘
}
```

### 12.3 层提升策略

- 动画开始前，将动画子树**提升为独立层**；动画结束后可合并回父层。
- 布局动画尽量用 `transform` 替代布局属性（展开/收起用 `scale` 而非 `height`）。
- 内容动画限制在小面积独立层内（如进度条本身作为层）。

### 12.4 层数控制

- 同时活跃层数控制在**几十量级**。
- 每个脏 tile 的合成需遍历相交层，层数越多开销越大。
- 静态背景/大面积面板合并为单层。

---

## 13. 帧调度

```rust
loop {
    // 1. 处理事件（可能产生脏标记）
    handle_events(&mut tree, &mut damage);

    // 2. 推进动画时钟
    let has_active_animation = animation_clock.tick(&mut tree, &mut damage);

    // 3. 布局（仅 layout_dirty 时）
    if tree.has_layout_dirty() {
        run_layout(&mut tree, tree.root, viewport);
    }

    // 4. 重绘脏层（仅 content_dirty 时）
    for layer_id in tree.dirty_layers() {
        tree.layers[layer_id].rebuild_if_dirty();
    }

    // 5. 合成损伤区域（仅 damage 非空时）
    if !damage.is_empty() {
        composite_damaged(&mut final_pixmap, &tree.layers, &mut damage);
        window.present(&final_pixmap);
    }

    // 6. 等待策略
    if has_active_animation || scrolling_with_inertia {
        window.request_redraw();
    } else {
        control_flow = ControlFlow::Wait;  // 完全休眠
    }
}
```

**静止时**：不推进时钟、不布局、不重绘、不合成、不提交，CPU 占用为 0。

---

## 14. 关键决策记录（ADR）

### ADR-001：采用持久层树 + Compositor，而非纯 Display List

- **决策**：持久层树 + 损伤驱动合成。
- **理由**：CPU 渲染下，层移动/滚动只需 blit，无需重新光栅化；Display List 在位置变化时需重放整个序列。
- **代价**：内存开销高于纯 IR；需层管理逻辑。
- **缓解**：层数控制、tile 化滚动内容、静态层合并。

### ADR-002：布局采用 `taitank-rs` 纯 Rust 实现

- **决策**：首选 `taitank-rs`，`taitank-safe` FFI 作为备选。
- **理由**：避免 C++ 构建依赖，保持纯 Rust 工具链；已有完整 Flex 测试覆盖。

### ADR-003：变换与重绘分离

- **决策**：`transform` / `opacity` 动画只改层属性，不标记 `content_dirty`。
- **理由**：CPU 光栅化成本远高于 blit；动画期间 `vello_cpu` 应保持空闲。

### ADR-004：损伤区域用 Tile 网格而非精确矩形集合

- **决策**：粗粒度 tile 网格（64×64 或 128×128）为主，精确矩形为辅。
- **理由**：tile 网格合成循环简单，天然限制单次 blit 最小面积，避免碎片化。

### ADR-005：`Pixmap` 不作为层缓存的硬编码类型

- **决策**：`LayerStorage` 与 `LayerRasterizer` 抽象，为 `vello_gpu` 预留关联类型。
- **理由**：`vello_cpu` 与 `vello_gpu` 共享 CPU 端预处理，切换时不应重写层树和合成器调度。

### ADR-006：滚动内容层 Tile 化

- **决策**：滚动容器的内容层使用 `TileCache`，非单一 `Pixmap`。
- **理由**：长列表整层缓存内存不可控（1080×10000×4 ≈ 41MB）；tile 化后仅缓存视口附近。

---

## 15. 风险与未来工作

### 15.1 风险

| 风险 | 影响 | 缓解 |
| :--- | :--- | :--- |
| `taitank-rs` 成熟度 | 布局缺陷或 API 不稳定 | 保留 `taitank-safe` 备选；布局测试用例先行 |
| CPU 合成开销在高层数下上升 | 帧率下降 | 层数上限、tile 网格、静态层合并 |
| 损伤追踪粒度权衡 | 过度合并膨胀合成面积 / 不足导致碎片化 | tile 网格 + 精确矩形混合策略 |
| 双缓冲未损伤区域一致性 | 画面撕裂/残留 | 持久最终 Pixmap，损伤区域外不修改 |
| `vello_gpu` 切换的抽象泄漏 | 未来迁移成本 | `LayerRasterizer` 提前定义，`Pixmap` 不硬编码 |

### 15.2 未来工作

1. **`vello_gpu` 后端**：复用 Scene 描述，将稀疏条带上传 GPU；`LayerRasterizer` 新增实现。
2. **平台宿主抽象**：`winit` 接入，后续支持原生 Win32。
3. **无障碍与输入法**：文本输入、屏幕阅读器桥接。
4. **多窗口**：每个窗口独立的层树与 Compositor。
5. **层合并优化**：动画结束后自动将独立层合并回父层，减少常驻层数。
6. **性能基准**：建立滚动、动画、局部更新的基准测试套件。

---

## 16. 术语表

| 术语 | 含义 |
| :--- | :--- |
| **控件树** | 持久化的 UI 对象树，应用状态的唯一真实源 |
| **布局树** | Taitank 节点的镜像结构，负责 Flex 布局计算 |
| **渲染层树** | 持久化的层树，每层缓存自己的 Pixmap |
| **损伤区域** | 本帧需要重新合成/呈现的像素区域集合 |
| **Compositor** | 按 z-order 将各层 Pixmap 合成到最终画面的组件 |
| **合成路径动画** | 只改层 transform/opacity、不重绘 Pixmap 的动画 |
| **布局路径动画** | 触发布局重算的动画（width/height/padding 等） |
| **Tile 网格** | 将画面划分为固定大小方块，用于粗粒度损伤追踪 |
| **层提升** | 将子树标记为独立层，以隔离其重绘/变换影响范围 |

---

**文档结束**。本设计以“持久层缓存 + 损伤驱动合成”为核心骨架，以 Taitank/Parley/vello_cpu 为三大技术支柱，通过变换与重绘分离、两级失效通道、tile 化滚动等机制，在 CPU 渲染管线中实现“静止零开销、滚动/动画几乎不重绘”的目标。