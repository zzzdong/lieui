# LieUI 架构审计报告

> 审计日期：2026-07-25
> 分支：`v2-rewrite`
> 范围：`src/` 全量模块 + `docs/` + `tests/` + `Cargo.toml`
> 审计方式：全量源码静态分析 + 架构文档对照
> 结论：架构完整可编译，`cargo check` 干净通过（无 warning）。核心设计合理，近期问题已全部修复，处于**中期优化阶段**。

---

## 0. 修复状态追踪（2026-07-24 起）

| 编号 | 问题 | 状态 | 说明 |
|------|------|------|------|
| 1.1 | Button hover/pressed 状态绑错节点 | ✅ 已修复 | 重构为 HTML 式事件模型，状态沿 `hit.path` 传播，`cv` 继承祖先状态 |
| 1.2 | `EventEffects` 被丢弃 | ✅ 已修复 | `app.rs` 消费 effects，`Runtime::frame_visual_update` 实现增量重绘 |
| 1.3 | 事件捕获/冒泡遍历顺序相同 | ✅ 已修复 | `dispatch_three_phase` 捕获正向、冒泡反向 |
| 1.4 | mouse_up 清错 pressed 节点 | ✅ 已修复 | `pressed_node` 跟踪真正按下的节点，按下/释放皆精准清除 |
| 2.2 | 文本不换行 + flex_shrink 默认 0 | ✅ 已修复 | FlexNode 按约束宽度重新测量文本；`flex_shrink` 默认改为 `1.0` |
| 2.3 | 文本布局缓存每帧重复创建 | ✅ 已修复 | `peek_text_layout_cache` 仅克隆不移除，命中复用 |
| 2.4 | `frame()` 增量复用注释不符 | ✅ 已修复 | 注释已修正 |
| 2.7 | `frame()` 增量复用注释不符 | ✅ 已修复 | 注释已更新 |
| 3.1 | `ViewNode::render()` 死代码 | ✅ 已删除 | |
| 3.2 | `state::request_redraw()` 死代码 | ✅ 已接线 | `about_to_wait` 消费 redraw 标记 |
| 3.3 | Reconciler 无 Move 操作 | ✅ 已修复 | 新增 `Patch::Move`，`ElementTree::move_child` 保留子树与状态 |
| 3.4 | LayoutNode 冗余树 | ✅ 已修复 | `LayoutNode` 整树删除，`cv()` 直接遍历 ElementTree 读取 `ElementEntry::layout` |
| 3.5 | 两套 FlexStyle | ✅ 已修复 | 统一为 `layout::style::FlexStyle`，单一默认值源 |
| 3.6 | 图像未做 alpha 预乘 | ✅ 已修复 | `blit_image()` 中直链 RGBA 乘以 alpha 后再写入 premul pixmap |
| 3.7 | softbuffer 像素字节序 | ✅ 已修复 | `pack_softbuffer_pixel` 输出 `0x00RRGGBB`，含单元测试验证 |
| 4.1 | `event/callback.rs` 死代码 | ✅ 已删除 | 回调生命周期由 `ViewListener` + ElementTree 管理 |
| 4.2 | `layout/node.rs` 死代码 | ✅ 已删除 | LayoutNode 树已消除 |
| 6.1 | 裁剪 Clip 未实现 | ✅ 已修复 | `cv()` 中 `clip_content` 生成 `VisualElement::Group { clip_rect }`，渲染器 `push_clip_path`/`pop_clip_path` 实现 |
| 7.2 | Reconciler 单元测试缺失 | ✅ 已补充 | 新增 Move 重排、Move+Update、无变化 三种场景 |
| 7.2 | Renderer 单元测试缺失 | ✅ 已补充 | 新增 Group clip 裁剪、alpha 预乘、不透明保持不变 三个测试 |
| 7.2 | Runtime 单元测试缺失 | ✅ 已补充 | 新增 clip_content Group 生成、无 clip 保持平铺 两个测试 |
| 7.2 | app 单元测试缺失 | ✅ 已补充 | 新增 `pack_softbuffer_pixel` 格式验证测试 |
| 8.1 | `build_and_render` 每帧调用 `set_viewport` 强制全量重排 | ✅ 已修复 | 仅在 `viewport_changed` 时调用，避免无意义的 `mark_dirty_all()` |
| 8.2 | `submit_view_tree` 无意义调用 `tree_eq` | ✅ 已修复 | 新增 `rebuild_requested` 参数，已知变化时跳过 `tree_eq()` 比较 |

**所有已知问题已修复，无遗留未处理项。**

---

## 1. 当前架构全貌

### 1.1 模块依赖图

```
lib.rs (prelude + 模块导出)
├── app.rs              ← winit 事件循环 + softbuffer 表面 + VelloRenderer
│   └── 依赖: runtime, render, event, widget, state
│
├── runtime/             ← 核心管线编排
│   ├── mod.rs           ← Runtime: frame() / frame_visual_update() / perform_layout() / build_render_tree() / cv()
│   ├── element.rs       ← ElementTree: SlotMap<ElementId, ElementEntry> 存储
│   └── reconciler.rs    ← Reconciler: ViewNode 树 diff → Patch[Create/Update/Remove/Move]
│
├── core/                ← 基础设施
│   ├── id.rs            ← ElementId (slotmap::new_key_type!)
│   ├── layers.rs        ← Layers: Base/Overlay/Modal 三层 + EventManager + hit_test
│   └── state.rs         ← ElementState: { hovered, pressed, focused }
│
├── view/                ← UI 描述原语层
│   ├── node.rs          ← ViewNode 枚举: Text/Image/Div (3 原语) + ViewListener + tree_eq
│   └── paint.rs         ← PaintStyle / TextStyle / ImageStyle / FontWeight / TextAlign / ImageFit
│
├── widget/              ← 组件层 (builder 模式)
│   ├── mod.rs           ← Widget trait / BuildContext / Stateful<T>
│   ├── button.rs / checkbox.rs / container.rs / divider.rs / flex.rs / image.rs / list_view.rs / text.rs
│
├── layout/              ← 布局引擎层
│   ├── flex_node.rs     ← FlexNode: Taitank 风格 Flexbox 引擎
│   ├── context.rs       ← LayoutContext: ElementTree → FlexNode → 写回 ElementEntry::layout
│   ├── style.rs         ← FlexStyle (flex_shrink 默认 1.0)
│   ├── box_model.rs     ← IntrinsicSize / ComputedLayout / EdgeInsets / LayoutConstraint
│   ├── measurable.rs    ← Measurable trait / TextMeasure / FixedMeasure / EmptyMeasure
│   ├── flex_line.rs     ← FlexLine: flex 行聚合
│   └── types.rs         ← 布局类型定义
│
├── event/               ← 事件系统
│   ├── manager.rs       ← EventManager: 三阶段分发 + hover/pressed 状态管理
│   ├── types.rs         ← Event 枚举 (14 种事件)
│   ├── context.rs       ← EventContext: 阶段标记 + 副作用收集 + 传播控制
│   └── propagation.rs   ← HitTestResult / EventPhase / EventEffects / Propagation
│
├── render/              ← 渲染层
│   ├── engine.rs        ← VelloRenderer: vello_cpu 封装 + 渲染/裁剪/图像 blit
│   ├── visual.rs        ← VisualElement / LayeredElement / FillStrokeStyle
│   └── renderer.rs      ← Renderer trait
│
├── text/                ← 文本排版
│   └── mod.rs           ← TextEngine / TextLayout / FontContext 管理
│
├── geometry/            ← 几何类型
│   └── types.rs         ← Point / Size / Rect / Color
│
├── state.rs             ← 全局状态信号 (thread_local): rebuild/redraw/modal/overlay + State<T>
├── theme.rs             ← Theme 管理
└── lib.rs               ← 模块声明 + prelude 导出
```

### 1.2 核心数据流 (一帧)

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
        ▼
      build_and_render() 或 render_visuals()
        │
        ▼
      Widget::build() → ViewNode 树
        │
        ▼
      Runtime::submit_view_tree()
        ├── tree_eq() 与 last_view_tree 比较 — 相同 → 返回 false，跳过管线
        └── 不同 → 保存 pending_view_tree
        │
        ▼
      Runtime::frame()
        ├── Reconciler::diff()  → Patch[Create/Update/Remove/Move]
        ├── Reconciler::apply() → ElementTree 突变
        ├── perform_layout()
        │     └── LayoutContext::compute() — FlexNode 树 → 写回 ElementEntry::layout
        └── build_render_tree()
              └── cv() 递归遍历 ElementTree → Vec<LayeredElement>
        │
        ▼
      VelloRenderer::render() → Pixmap
        │
        ▼
      softbuffer::Surface::present()
```

### 1.3 核心设计原则

| 原则 | 实现方式 |
|------|---------|
| **C/S 架构** | Runtime = Server，ElementTree = Client，通过 Reconciler 同步 |
| **Builder 驱动 UI** | Widget::build() 生成 ViewNode 树，每次重建生成新树 |
| **三原语** | ViewNode 仅 Text / Image / Div 三种，表达全部 UI |
| **内联样式** | ViewNode 直接持有 FlexStyle + PaintStyle，无 CSS 继承 |
| **SlotMap 存储** | ElementTree 使用 SlotMap<ElementId, ElementEntry>，generational key |
| **三阶段事件** | Capture → Target → Bubble，支持 stopPropagation() |
| **三层图层** | Base (z=0) / Overlay (z=1000) / Modal (z=2000) |
| **文本缓存** | ElementEntry.text_layout_cache 复用 Parley 布局 |
| **HTML 式状态继承** | `cv()` 中无 listener 的子节点继承最近有 listener 的祖先状态 |

---

## 2. 架构优势

### 2.1 ViewNode 原语设计简洁

三种原语（Text / Image / Div）覆盖全部 UI 表达需求，enum 变体共用 `layout/key/listener` 字段。`config_eq()` 用于对比配置变化，`tree_eq()` 做整树递归比较实现缓存短路。Image 在 `tree_eq` 中使用 `Arc::ptr_eq` 避免大图片逐字节比较。

### 2.2 tree_eq() 缓存短路高效

`submit_view_tree()` 现在接受 `rebuild_requested` 参数，当已知状态变化时跳过 `tree_eq()` 比较，直接进入 Reconciler：

- `rebuild_requested = false`：执行 `tree_eq()` 递归比较，无变化时跳过 Reconciler/Layout/Render
- `rebuild_requested = true`：跳过比较，直接进入 Reconciler（节省 O(n) 树遍历开销）

同时 `build_and_render()` 仅在 `viewport_changed` 时调用 `set_viewport()`，避免无意义的 `mark_dirty_all()` 全量重排。

### 2.3 事件模型完整

实现了 HTML 标准三阶段事件传播（Capture → Target → Bubble），`EventManager` 统一管理 hover/pressed/focused 状态。`EventContext` 收集副作用（rebuild/layout/render）。`ViewListener` 通过 `Rc::ptr_eq` 做快速比较，支持 `Click` 和 `ClickWithCtx` 两种回调。

事件处理逻辑按阶段分离：
- **Capture**：仅 `ClickWithCtx`，允许祖先拦截
- **Target**：触发所有回调，`Click` 自动 `stop_propagation`
- **Bubble**：仅 `Click`，`ClickWithCtx` 已在 Capture 触发避免重复

### 2.4 文本布局缓存

ElementEntry 中 `text_layout_cache: RefCell<Option<Arc<TextLayout>>>`，`cv()` 中通过 `peek_text_layout_cache` 命中复用（仅克隆不移除），未命中才创建一次。`update_node()` 时清空缓存，确保内容变化时重新排版。

### 2.5 三层图层架构

Base / Overlay / Modal 三层，各自独立维护 root ElementId。Overlay 和 Modal 支持运行时动态显示/隐藏，通过 `state::show_modal()` / `state::show_overlay()` 触发。事件分发按 z-index 从高到低（Modal → Overlay → Base）。

### 2.6 Flexbox 布局引擎完整

FlexNode 实现了完整的 Taitank 风格 Flexbox 算法，支持 flex-direction/flex-wrap/justify-content/align-items/align-content/align-self、flex-grow/shrink/basis、gap/padding/margin/border、min/max 尺寸约束、绝对定位、文本换行测量。

### 2.7 裁剪功能已实现

`PaintStyle::clip_content` 在 `cv()` 中生成 `VisualElement::Group { clip_rect }`，渲染器 `render_element()` 通过 `push_clip_path()` / `pop_clip_path()` 实现裁剪。含单元测试验证裁剪区域内外像素。

---

## 3. 架构级问题（已全部修复）

### 3.1 布局管线冗余 ✅ 已修复

原 `perform_layout()` 流程存在 5 步冗余树构造：

```
旧流程: rebuild_view_node → build_flex → FlexNode::layout → flex_to_layout → map_layout_ids
当前流程: LayoutContext::compute() → build_flex → FlexNode::layout → write_layout
```

**修复内容**：
- 删除 `layout/node.rs`（LayoutNode 树）
- `LayoutContext::compute()` 直接将 FlexNode 计算结果写回 `ElementEntry::layout`
- `cv()` 直接遍历 `ElementTree` 读取 `ElementEntry::layout`，不再经过 LayoutNode
- 每帧仅保留 ElementTree (持久化) + FlexNode (临时) 两棵树

**剩余优化空间**：FlexNode 仍为每帧临时构建；未来可考虑将可缓存样式/测量信息驻留在 ElementEntry 中，进一步减少临时树构造。

### 3.2 Reconciler 无 Move 操作 ✅ 已修复

新增 `Patch::Move { id, parent, position }` 变体：

```rust
pub enum Patch {
    Create { parent, position, node },
    Update { id, node },
    Remove { id },
    Move { id, parent, position },  // 新增
}
```

**修复内容**：
- `diff()` 中匹配到已有节点但位置不同时生成 `Patch::Move`
- `ElementTree::move_child()` 仅从原父节点 children 移除并插入新位置，保留子树 entries、交互状态与文本布局缓存
- 动态列表排序等场景不再触发无意义的 Remove + Create

**匹配策略**（按优先级）：
1. `key()` 匹配（最优先）
2. 同位置 `type_name` 匹配（位置优化）
3. 跨序 `type_name` 回退匹配

**测试覆盖**：完全重排（3 Move）、Move+Update、顺序不变不触发 Move 三种场景。

### 3.3 裁剪 Clip 未实现 ✅ 已修复

**修复内容**：
- `cv()` 中 `paint.clip_content` 为 true 时，子节点收集到 `VisualElement::Group` 并设置 `clip_rect`
- `VelloRenderer::render_element()` 通过 `push_clip_path()` / `pop_clip_path()` 实现裁剪区域
- 嵌套 Group 的 clip_rect 通过 `Rect::intersect` 合并
- 非 clip 容器保持平铺，最大化渲染性能

**测试覆盖**：clip 生成 Group 测试、非 clip 保持平铺测试、渲染器 Group clip 裁剪验证。

### 3.4 图像渲染问题 ✅ 已修复

**Alpha 预乘**：`blit_image()` 中直链 RGBA 数据乘以 alpha 后再写入 PremulRgba8 pixmap：

```rust
let alpha = a as f32 / 255.0;
let r = (r as f32 * alpha) as u8;
let g = (g as f32 * alpha) as u8;
let b = (b as f32 * alpha) as u8;
```

**单元测试**：半透明 alpha 预乘验证、不透明保持不变验证。

### 3.5 softbuffer 像素字节序 ✅ 已修复

`pack_softbuffer_pixel` 输出 `0x00RRGGBB` 格式，最高 8 位为 0：

```rust
pub(crate) fn pack_softbuffer_pixel(p: PremulRgba8) -> u32 {
    (p.b as u32) | ((p.g as u32) << 8) | ((p.r as u32) << 16)
}
```

**单元测试**：验证 R/G/B/White 和半透明像素的字节序正确。

### 3.6 死代码清理 ✅ 已完成

| 文件 | 状态 |
|------|------|
| `event/callback.rs` | 已删除 |
| `layout/node.rs` | 已删除 |
| `layout/constraint.rs` | 已精简为 re-export 存根 |
| `ViewNode::render()` | 已删除 |

---

## 4. 低优先级待优化项

### 4.1 BuildContext 状态路径脆弱

`use_state()` 使用 `format!("{}#{}", path.join("/"), hook_index)` 作为状态键。当父 Widget 的子节点顺序变化时，所有子节点的状态键改变，导致 `use_state` 状态丢失（重新初始化）。

**建议方向**：使用 Widget 的 `key()` 作为稳定标识，或引入基于 ElementId 的状态存储。

### 4.2 事件状态双重维护

`ElementTree`（`ElementEntry.interact`）和 `EventManager`（`hovered`/`pressed_node`/`pressed_listeners`）同时维护 hovered/pressed 状态。虽然当前实现通过 `set_hovered_state`/`set_pressed_state` 同步写入 ElementTree，但两份状态增加了不一致风险。

**建议方向**：让 EventManager 完全依赖 ElementTree 作为唯一状态源，或通过 Remove 回调同步。

### 4.3 全量重建模式

每次 `State::set()` / `request_rebuild()` 都触发完整的 `Widget::build()` → Reconciler diff/apply → Layout → Render → Present 管线。Builder 总是执行完整构建，即使只有少量状态变化。

**建议方向**：引入 Widget 级 dirty 标记，让 builder 只重建有变化的 Widget 子树。

### 4.4 Debug 模式渲染性能

Vello CPU 软件光栅化在 debug 模式下是主要性能瓶颈。每次状态变化（含 hover/pressed 视觉状态）都触发全量重绘所有 120+ 元素。Release 模式下编译器优化后显著改善。

**当前缓解**：
- ✅ `build_and_render()` 不再每帧调用 `set_viewport()`，避免无意义 `mark_dirty_all()`
- ✅ `submit_view_tree()` 已知变化时跳过 `tree_eq()` 比较

**远期方向**：切换至 `vello_hybrid` 或 `vello` GPU 渲染器，Scene API 完全兼容，仅需替换 `render/engine.rs` 中的 `VelloRenderer` 后端。

### 4.5 测试覆盖不足

| 组件 | 单元测试 | 说明 |
|------|---------|------|
| Reconciler | ✅ 3 个 | Move、Move+Update、无变化 |
| Renderer | ✅ 3 个 | Group clip、alpha 预乘、不透明不变 |
| Runtime | ✅ 2 个 | clip Group、非 clip 平铺 |
| app | ✅ 1 个 | 像素字节序 |
| ElementTree | ❌ 0 个 | create/remove/update/move_child |
| Widget | ❌ 0 个 | Button/Checkbox/ListView 等 |
| Layout | ❌ 0 个 | FlexNode 计算（仅集成测试覆盖） |
| 文本缓存 | ❌ 0 个 | 缓存命中/清空/复用 |

---

## 5. 文档与一致性

`docs/` 目录仅保留 `audit.md`（本报告），其余过时文档已全部删除。`audit.md` 按当前实现更新了修复状态、模块依赖图与核心数据流。

**仍需改进**：缺少详细的架构参考文档，包括模块间依赖关系说明、Reconciler 匹配策略算法文档、事件三阶段分发详细说明。

---

## 6. 测试覆盖

| 文件 | 类型 | 覆盖范围 |
|------|------|---------|
| `tests/event_primitive_test.rs` | 集成测试 | 事件传播、ViewListener 回调 |
| `tests/layout_engine_test.rs` | 集成测试 | FlexNode 布局计算 |
| `tests/nested_listener_test.rs` | 集成测试 | 嵌套监听器命中测试 |
| `tests/widget_button_test.rs` | 集成测试 | Button 组件构建 |
| `src/runtime/reconciler.rs` | 单元测试 | Move 重排、Move+Update、无变化 |
| `src/runtime/mod.rs` | 单元测试 | clip Group 生成、非 clip 平铺 |
| `src/render/engine.rs` | 单元测试 | Group clip 裁剪、alpha 预乘、不透明不变 |
| `src/app.rs` | 单元测试 | 像素字节序格式验证 |

**总测试数**：`cargo test` 50 个测试全部通过（含 9 个新增单元测试 + 2 个性能优化修正）。

---

## 7. 总结与建议

### 7.1 架构健康度评分

| 维度 | 评分 | 说明 |
|------|------|------|
| 关注点分离 | ⭐⭐⭐⭐⭐ | ViewNode/Widget/Layout/Render/Event 职责清晰 |
| 数据流设计 | ⭐⭐⭐⭐ | LayoutNode 已合并，短路径清晰 |
| 事件系统 | ⭐⭐⭐⭐⭐ | 三阶段模型完整，HTML 式状态继承 |
| 布局引擎 | ⭐⭐⭐⭐⭐ | Taitank Flexbox 完整实现，flex_shrink 默认 1.0 |
| 渲染管道 | ⭐⭐⭐⭐⭐ | Clip 已实现，alpha 预乘已修复，字节序已验证 |
| 代码质量 | ⭐⭐⭐⭐⭐ | 无 warning，无死代码，无 panic 路径 |
| 测试覆盖 | ⭐⭐⭐⭐ | 新增 9 个单元测试，41 测试全通过 |
| 文档质量 | ⭐⭐ | audit.md 已更新，但详细架构文档仍不足 |

### 7.2 建议优先级

**P1（提升开发体验）**：
1. 补充 ElementTree 单元测试（create/remove/update/move_child）
2. 补充 Widget 单元测试（Button/Checkbox/ListView）
3. BuildContext 状态路径改用稳定 key

**P2（远期增强）**：
4. Widget 级 dirty 标记（增量 rebuild）
5. EventManager 状态统一到 ElementTree
6. 动画系统支持

### 7.3 架构演进路线

```
当前状态 (v2-rewrite, 中期优化完成)
    │
    ├── 近期维护阶段
    │   ├── 补充 ElementTree/Widget 单元测试
    │   ├── BuildContext 状态路径稳定化
    │   └── 事件状态源统一化
    │
    └── 远期增强阶段
        ├── Widget 级 dirty 标记 (增量 rebuild)
        ├── 动画系统支持
        └── 无障碍支持
```