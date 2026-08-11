# LieUI 高性能重构设计文档

> 版本：v1.0-refactor | 状态：草案 | 面向目标：terminal（SSH/PTY）高频刷新场景
> 关联：`architecture.md`（v2-rewrite，现状基线）

---

## 一、重构目标与动机

### 1.1 为什么要重构

LieUI 当前是一个 **Builder 驱动 + 全量渲染** 的 C/S 框架，在传统 UI（按钮、列表、表单）场景下足够用，但面对 **terminal 类高频、大画布、局部变化** 的场景，现有架构存在结构性瓶颈。

**terminal 场景的典型特征**：
- **高频刷新**：SSH/PTY 数据到达频率可达每帧数 K~数十 K 字节（光标移动、字符输出），往往触发数十次/秒的刷新请求。
- **大画布**：终端缓冲区（如 120x40 cell ≈ 1200x840 px）需要常驻的像素缓冲，且**大部分区域在帧间保持不变**。
- **局部变化**：一次命令输出往往只影响屏幕的某一行或某几行，其余数千像素完全未变。
- **低延迟**：打字延迟、光标闪烁、滚屏都必须即时反馈，不能接受每次全量重建 + 全量光栅化的开销。
- **光标闪烁**：仅 1 个 cell 大小的区域需要周期性重绘（约 2~5 Hz），如果每次都全量 rebuild 整棵树 + 全量光栅化 + 全量 blit，CPU 会烧在毫无意义的重绘上。

### 1.2 现状瓶颈清单（基于 `architecture.md` 与源码调研）

| 编号 | 位置 | 问题 | terminal 影响 |
|------|------|------|--------------|
| **A** | `state.rs:532-540` / `widget/mod.rs:265-273` | 任何 `State::set` 都触发**全量 rebuild**（builder→reconcile→layout→render→raster） | 每次输出触发整条管线 |
| **B** | `app.rs` `build_and_render` | 每次 rebuild 递归重 build 整棵 ViewNode 树，大量 String/Vec 分配 | 每次数据变化重建整棵树 |
| **C** | `runtime/mod.rs:422-449` `build_render_tree` | 每次从零重建整个 `Vec<LayeredElement>` | 每帧重建全部 VisualElement |
| **D** | `render/engine.rs:62` | 每帧 `Pixmap::new` 全量新建像素缓冲 + CPU 全屏光栅化 | 每帧全屏光栅化 |
| **E** | `app.rs:802-833` `blit_to_window` | 每帧全量逐像素 pack + 拷贝到 softbuffer，无脏区 | 每帧全量 blit |
| **F** | `render/engine.rs:337-393` `blit_canvas` | Canvas 逐像素 CPU 循环（无 SIMD/无缓存 layer） | 终端画布每次全量逐像素拷贝 |
| **G** | `layout/context.rs:45-47` | 每次 layout 从零重建整棵 FlexNode 树 | 终端布局基本不变却全量重排 |
| **H** | `render/engine.rs:434-443` ShadowRoundedRect | 高斯模糊投影 CPU 代价最高 | 面板阴影在 resize/主题变化时昂贵 |

**核心结论**：LieUI 缺三样东西——**共享画布（shared surface）**、**脏区（dirty region）**、**按需重建（partial rebuild / state subscription）**。三者缺一，terminal 场景的性能就无法落地。

### 1.3 重构总原则

1. **默认不动，动则最小**：只有发生变化的像素/子树才被重算、重绘、上屏。
2. **渲染与 UI 逻辑解耦**：高频大画布（终端）走独立共享渲染通道，不经过 rebuild 循环。
3. **保持 API 兼容**：现有 `ViewNode`、`Widget`、`BuildContext`、`emit` 等公开接口尽量不动，新增能力采用"增量叠加 + 可选启用"。
4. **三阶段渐进**：先解决"共享 Surface + 脏区"（对应 80% 的 terminal 卡顿），再考虑"整体 Wayland C/S"（对应真正的多进程/安全/合成器架构）。

---

## 二、架构总览（重构后）

```
┌────────────────────────────────────────────────────────────────┐
│                        Widget / 业务层                           │
│  现有 Widget 系统 + 新增 TerminalWidget（直连 shared surface）   │
├────────────────────────────────────────────────────────────────┤
│                       ViewNode 层（增量）                       │
│  ViewNode 增加 dirty 标识；新增 SharedSurface 原语引用           │
├────────────────────────────────────────────────────────────────┤
│                     Runtime 层（按需管线）                       │
│  状态订阅(StateSubscription) → 局部 rebuild → 增量 diff → 脏子树 │
├────────────────────────────────────────────────────────────────┤
│                     布局引擎（脏子树剪枝）                       │
│  FlexNode 增量复用：仅重排 dirty 子树，写回受影响的 layout       │
├────────────────────────────────────────────────────────────────┤
│                   渲染层（脏区 + 共享表面）                      │
│  ┌─────────────────────────────┐  ┌──────────────────────────┐ │
│  │  UI 渲染通道（低频）         │  │  SharedSurface 通道(高频) │ │
│  │  Vec<LayeredElement>        │  │  Arc<SurfaceState>        │ │
│  │  VelloRenderer → Pixmap     │  │  终端独立维护 buffer      │ │
│  └──────────┬──────────────────┘  └────────────┬─────────────┘ │
│             │  dirty_rects                     │  dirty_rects  │
│             ▼                                  ▼               │
│        ┌──────────────────────────────────────────┐            │
│        │        Compositor（新）：脏区合并/重排     │            │
│        │  Blitter：仅对 union(UI dirty, Surf dirty)│            │
│        └──────────────────┬───────────────────────┘            │
│                           ▼                                    │
│                 softbuffer framebuffer → present               │
└────────────────────────────────────────────────────────────────┘
```

---

## 三、分阶段重构方案

### 阶段 A：共享 Surface + 脏区（P0，先做）

> 解决 terminal 卡顿的核心。目标：高频大画布（终端）与低频 UI 解耦，且上屏只刷新变化区域。

#### A.1 新增 `SharedSurface` 渲染通道

**动机**：终端这种"自己知道哪些像素变了"的组件，不应该每次都走 `builder → reconcile → layout → render-tree → raster` 全链路。它需要一块**自己拥有、自己绘制、按需提交**的共享像素表面。

```rust
// src/render/surface.rs (新增)

/// 一个可被 compositor 合屏的共享像素表面。
/// 由高频组件（如终端）自持，通过 dirty rect 增量提交。
pub struct SharedSurface {
    pub id: SurfaceId,
    /// RGBA8 像素缓冲，组件自己维护（terminal 只重绘 dirty cell 行）
    pub buffer: Arc<Mutex<Vec<u8>>>,
    pub width: u32,
    pub height: u32,
    /// 自上次提交以来发生变化的矩形集合（由组件自行标记）
    pub dirty: Mutex<Vec<Rect>>,
    /// 表面在窗口中的位置（由布局写入）
    pub origin: Cell<(f32, f32)>,
    /// 本帧是否被触及（决定是否需要合屏）
    pub touched: Cell<bool>,
}

impl SharedSurface {
    /// 组件标记某个区域变化（注意：只在变化时调用，勿每帧整屏标记）
    pub fn damage(&self, rect: Rect) {
        self.dirty.lock().unwrap().push(rect);
        self.touched.set(true);
    }

    /// 清空脏区（compositor 消费后调用）
    pub fn take_dirty(&self) -> Vec<Rect> {
        std::mem::take(&mut *self.dirty.lock().unwrap())
    }
}

/// 给高频组件提供的：只重绘一个矩形区域的像素
pub trait SurfacePainter {
    fn paint_into(&self, dst: &mut [u8], stride: usize, dirty: &[Rect]);
}
```

**关键设计**：
- `SharedSurface.buffer` 是**组件自持**的像素缓冲，与 lieui 的 `Pixmap` 解耦。
- 组件的 `paint_into` 只重绘 `dirty` 矩形（terminal 即"把发生变化的行重画出来"）。
- lieui 只负责把这个 surface 当作一个**合屏单元**合成到最终 framebuffer。

**ViewNode / ElementTree 集成**：

```rust
// src/view/node.rs 新增变体
ViewNode::SharedSurface {
    surface: Rc<SharedSurface>,
    layout: FlexStyle,
    key: Option<String>,
}
```

- Reconciler 对它用 `Rc::ptr_eq` 判断（surface 实例不变则不重建）。
- `build_render_tree` 产出 `VisualElement::SharedSurface { id, rect }`。

#### A.2 新增 Compositor（脏区合屏）

**动机**：当前 `blit_to_window` 每帧全量拷贝整块 Pixmap。引入 compositor 后，只对"本帧脏区并集"做 blit。

```rust
// src/render/compositor.rs (新增)

/// 统一收集本帧所有脏区，合并为少数矩形，仅对这些区域进行像素合成。
pub struct Compositor {
    pub dirty_rects: Vec<Rect>,
    pub frame_dirty: bool,
    pub backing: Vec<u8>,      // 最终 RGBA 帧缓冲（与 softbuffer 尺寸一致）
    pub width: u32,
    pub height: u32,
}

impl Compositor {
    /// UI 通道与 SharedSurface 通道都把各自的脏区上报到这里
    pub fn add_dirty(&mut self, rect: Rect) {
        self.dirty_rects.push(rect);
        self.frame_dirty = true;
    }

    /// 合并脏区：把分散矩形合拢成少量不重叠大矩形，减少拷贝次数
    pub fn merge(&mut self) -> Vec<Rect> {
        // 简单实现：AABB 合并（或扫线合并）
    }

    /// 仅将 dirty 区域从各 surface/UI pixmap 拷贝到 backing
    pub fn composite(&mut self, ui_pixmap: &[u8], surfaces: &[&SharedSurface]) {
        for rect in self.merge() {
            // 1. 从 UI pixmap 拷贝该区域（UI 通道已栅格化）
            // 2. 对覆盖该区域的每个 SharedSurface，做 blit_canvas 风格 alpha 合成
        }
    }

    /// 只 present dirty 区域（交给 softbuffer，或用 backing + 全量 present 但仅更新脏区内容）
    pub fn present(&self, buffer: &softbuffer::Buffer) {
        // softbuffer 无部分 present 时：将 dirty 区写入 backing 后整块 present
        // 具备 damage 能力的平台：直接提交 dirty 区
    }
}
```

**Compositor 职责与现有引擎的分工**：

| 现有 | 新增 |
|------|------|
| `VelloRenderer::render` → 全屏 Pixmap | 只对 `dirty` 区域调用引擎的光栅化原语 |
| `blit_to_window` 全量拷贝 | `Compositor::composite` 只合成 dirty 区 |

#### A.3 渲染引擎支持"部分光栅化"

`VelloRenderer::render` 目前每帧 `Pixmap::new(w,h)` 全量重画。重构后：

```rust
pub struct VelloRenderer {
    // ...
    /// 持久化的整帧 pixmap（跨帧复用，不再每帧 new）
    pub framebuffer: Pixmap,
    /// 本帧脏区（被 clip 到 framebuffer 尺寸内）
    pub dirty: Vec<Rect>,
}

impl VelloRenderer {
    /// 复用 framebuffer，仅重绘 dirty 区域内的元素
    pub fn render_partial(&mut self, elements: &[LayeredElement], dirty: &[Rect]) {
        // 1. 先重置 dirty 区为背景色（只清脏区，不整帧清）
        // 2. 对 dirty 区内的元素走原 render_element
        // 3. 保留脏区外的旧像素
    }
}
```

**注意**：由于 `VisualElement` 是"整帧"的列表，部分光栅化时需**把元素按 dirty 矩形裁剪后**再光栅化，避免覆盖脏区外像素。这一步是"剪裁光栅化"（clip-aware rasterization），是核心难点，需在引擎层支持 `clip_rect`。

#### A.4 blit 优化（SIMD + 通道复用）

- `blit_canvas`/`blit_image` 内层逐像素循环改用 **SIMD（`std::simd` 或 `wide` crate）**，对 RGBA 批量预乘。
- 引入**逐通道分支**：当 surface/UI 全部不透明时，跳过 alpha 预乘直接 `copy_within`（memcpy），这是终端场景最常见情况，能省大量乘法。
- Canvas 若整帧不变（如纯滚动），复用上次的合成结果。

#### A.5 terminal 接入流程（如何使用）

```
终端数据到达 (russh/PTY)
  → TerminalBackend 解析 VTE → 更新 Grid
  → 计算受影响 cell 行 → 标记 SharedSurface::damage(那些行的 Rect)
  → 请求"仅渲染"（request_redraw，不 request_rebuild）
  → RedrawRequested
    → 终端自己 paint_into(dirty)：只重画变化的行（复用已有字体 glyph 缓存）
    → SharedSurface 提交 dirty 到 Compositor
    → Compositor::composite 仅拷贝脏区 → present
```

**效果**：一次命令输出只重画变化的几行 + 拷贝那几行的像素，而不是整棵树 + 整屏。

---

### 阶段 B：脏子树 / 按需重建（P1）

> 解决阶段 A 之后剩余的"UI 层每次全量 rebuild"问题。terminal 场景下，UI 外壳（sidebar/tab/statusbar）状态变化时也不该重建整个终端 surface。

#### B.1 状态订阅（StateSubscription）

当前任何 `State::set` → `request_rebuild`（全量）。重构为**订阅式**：

```rust
// src/state.rs 新增
pub struct StateSubscription {
    /// 订阅者关心的 rebuild 目标（如某个 subtree 的 root key）
    pub targets: HashSet<String>,
    /// 是否需要整帧重绘（true 时跳过 diff 直接全量）
    pub full_rebuild: bool,
}
```

- `State::set` 时，**不立刻**全局 `request_rebuild`，而是标记"该状态已变更"，交由框架判断影响范围。
- `request_rebuild` 增加**作用域**参数：`request_rebuild(scope: RebuildScope)`，`RebuildScope::Subtree(String)` / `RebuildScope::Full`。
- `BuildContext::use_state` 记录状态所属的 key 路径，变更时只触发**该 subtree** 的 rebuild。

#### B.2 Reconciler 增量 diff（按子树剪枝）

当前 `diff()` 整树对比。重构后：

- 当收到 `RebuildScope::Subtree(key)` 时，只对以 `key` 为根的子树重新执行 `Widget::build` → `diff`，其余子树沿用上一帧结果。
- `tree_eq` 已有短路，在此基础上增加**子树级短路**：未触及的子树直接复用上一帧 ElementEntry（零拷贝）。

#### B.3 布局脏子树剪枝

- `perform_layout` 当前每次重建整棵 FlexNode 树（`build_flex`）。
- 重构为**脏子树布局**：只有 `dirty` 标记传播的子树重新 `build_flex` + `layout`，父级仅重排受影响分支，其余沿用 `ComputedLayout`。
- 配合 `LayoutContext` 现有的 intrinsic 缓存（`context.rs:69-75`），进一步跳过未变化叶子的 measure。

---

### 阶段 C：Wayland C/S 架构（P2，远期）

> 可选远期目标：把"共享 Surface + 脏区"推向真正的**多进程合成器架构**，为安全隔离、多窗口、GPU 加速打基础。

#### C.1 架构形态

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Compositor  │◄────│  App 进程 A   │     │  App 进程 B   │
│  (独立进程)   │     │  lieui 客户端 │     │  lieui 客户端 │
│  合屏/脏区/  │     │  共享 Surface │     │  共享 Surface │
│  wl_surface  │     │  / damage     │     │  / damage     │
└──────────────┘     └──────────────┘     └──────────────┘
      │ IPC: 共享内存(shm) + 协议
      ▼
  DRM/GPU/KMS (vello GPU 或 wgpu)
```

- **Compositor 进程**：持有全部窗口的帧缓冲，负责 dirty 合并、合成、present（对接 wlroots/smithay 或自研）。
- **App 客户端进程**：每个应用通过 IPC（共享内存 shm + 事件通道）把自己 `SharedSurface` 的像素 + dirty rect 交给 compositor。
- **协议层**：借鉴 Wayland 的 `wl_surface.attach / wl_surface.damage / wl_surface.commit` 语义，定义 `lie_surface.attach(buffer) / lie_surface.damage(rects) / lie_surface.commit()`。

#### C.2 关键收益

- **安全隔离**：App 崩溃不拖垮 compositor 和其他应用。
- **多窗口/多进程**：多个 lieui 应用可共享一个 compositor。
- **GPU 加速**：compositor 端可用 vello GPU/wgpu，CPU 光栅化不再是瓶颈。

#### C.3 与阶段 A/B 的关系

- 阶段 A 的 `SharedSurface` + `Compositor`（进程内）是阶段 C 的**进程外版**的雏形：把"组件自持像素 + 脏区合屏"抽象成"进程自持像素 + 进程外合屏"，接口可平滑演进。
- 阶段 C 的重心在 **IPC 协议层 + 合成器进程**，渲染与布局逻辑复用阶段 A/B 的成果。

---

## 四、terminal 场景专项设计

### 4.1 为什么 terminal 需要"共享 Surface"而不是普通 Canvas

现有 `Canvas` widget 已能承载像素（`Arc<Vec<u8>>` + `blit_canvas`），但它有两个致命缺陷：

1. **走 rebuild 全链路**：`Canvas` 只是 `ViewNode::Canvas`，更新 buffer 仍需 `emit → request_rebuild → 全量 build/reconcile/layout/render-tree`。终端高频刷新下，每次数据变化都重建整棵树。
2. **无脏区，全量 blit**：`blit_canvas` 每次把整块 buffer 逐像素合成，120x40 终端全屏每次都要拷贝约 1200x840x4 ≈ 4MB。

`SharedSurface` 把这两点都解决：**buffer 自持 + 脏区提交 + 跳过 rebuild 全链路**。

### 4.2 terminal 场景性能预算（目标）

| 指标 | 现状（估算） | 目标 |
|------|-------------|------|
| 命令输出（整行）延迟 | 全量 rebuild + 全屏 raster，明显卡顿 | 仅重画该行 + 拷贝该行像素，< 5ms |
| 光标闪烁（2~5Hz） | 全屏重绘 | 仅重绘 1 cell 矩形 |
| 滚屏 | 全量 | 仅滚入/滚出的行 |
| 空行 | 全量填充 | 0 开销（跳过） |

### 4.3 terminal 具体实现要点

**终端渲染器（`lieterm/src/render.rs` 侧）**：
- 维护一份 `Rc<SharedSurface>`，`width = cols*cell_w, height = rows*cell_h`。
- 每次 VTE 解析后，把**变更的 cell 行**收集为 dirty rect（x 可从最小变更列到最大列）。
- 收到 `request_redraw` 时调用 `paint_into`：对每个 dirty rect，用已有 glyph 缓存只重画那几行，写回 `SharedSurface.buffer`。
- 提交 `surface.damage(rects)` 给 Compositor。

**lieui 侧（`lib.rs`/`app.rs`）**：
- `RedrawRequested` 时，先检查是否有 `SharedSurface` 上报 dirty：
  - 若有 → 走 Compositor 只合成脏区（跳过 rebuild、跳过全屏 raster）。
  - 若无 → 走原有 UI 渲染通道。
- 终端 surface 在布局时把 `origin` 写回（布局变化时才会重定位）。

### 4.4 terminal 边界场景

| 场景 | 处理 |
|------|------|
| 终端 resize | 重建 surface buffer 尺寸，全量 damage 一次 |
| 终端滚动（主屏本地滚动） | 复用已有行像素上移/下移 + 只重画新露出的行，最终 damage 一个"滚动带"矩形 |
| 终端与 UI 重叠 | Compositor 按 z 序合成；surface 带 opacity，若 UI 覆盖终端则 UI 后画 |
| 焦点/IME | 终端 surface 区域外仍由 lieui 事件系统处理；光标闪烁只 damage 光标 cell |
| 多窗口 | `SharedSurface` 绑定所属窗口；Compositor 按窗口维护 framebuffer |

---

## 五、API 兼容与迁移路径

### 5.1 保持兼容的公开接口

- `ViewNode`（Text/Image/Div/Canvas）——**保持不变**，仅新增变体。
- `Widget` trait / `BuildContext` / `use_state`——保持不变，`State::set` 行为默认兼容（新订阅机制可选启用）。
- `emit` / 消息队列——保持不变。
- `request_rebuild` / `request_redraw`——签名不变，新增带作用域的重载。

### 5.2 新增公开接口（feature 或默认启用）

```rust
// 渲染/合成
pub use render::surface::{SharedSurface, SurfaceId, SurfacePainter};
pub use render::compositor::Compositor;

// 终端专用
pub struct TerminalSurfaceBuilder { /* 封装 SharedSurface + paint_into */ }
```

### 5.3 迁移顺序建议

1. **先落地阶段 A（P0）**：SharedSurface + Compositor + 部分光栅化 + blit 优化 → 直接解决 lieter 卡顿。
2. **再落地阶段 B（P1）**：状态订阅 + 增量 diff + 布局剪枝 → 提升 UI 层交互流畅度。
3. **阶段 C（P2）按需推进**：确认阶段 A 的 Surface/Compositor 抽象稳定后，再扩展为进程外 compositor。

---

## 六、性能验证与基准

### 6.1 新增基准

- `examples/perf_terminal.rs`：模拟 120x40 终端，随机 N 行文本刷新，统计 rebuild/layout/render-tree/raster/blit 各阶段耗时（复用 `src/perf.rs` 的分阶段打点）。
- `examples/perf_surface.rs`：纯 `SharedSurface` 脏区刷新压测（局部 vs 全屏）。
- `examples/perf_dirty.rs`：Compositor 脏区合并与只合成 dirty 区的耗时。

### 6.2 验收指标

| 场景 | 通过标准 |
|------|---------|
| 终端命令输出（50 行/s） | 无可见卡顿，单帧管线 < 8ms |
| 光标闪烁 | 只重绘 1 cell，不触发全屏重绘 |
| 终端滚动 | 只重绘滚动静，不整屏刷新 |
| UI 状态变化（sidebar 展开） | 只 rebuild 受影响子树，终端 surface 不动 |

### 6.3 复现问题对照

- 当前 `lieterm` 的"回车白屏""SSH 卡顿"在阶段 A 落地后应显著缓解：白屏源于全量重建丢帧/时序，卡顿源于全屏光栅化+blit；脏区+共享 surface 直接消除这两个根因。

---

## 七、风险与权衡

| 风险 | 说明 | 缓解 |
|------|------|------|
| 部分光栅化复杂度高 | `VisualElement` 整帧列表需按 dirty 裁剪，clip 处理易错 | 先用 AABB 粗裁（rect 相交），验证正确后再做精细裁剪 |
| softbuffer 无部分 present | 无 damage 能力的平台仍需整块 present | backing 缓冲 + 仅更新脏区内容，present 时整块但内容已增量 |
| 共享 surface 与布局耦合 | surface origin 由布局决定，布局变化需同步重定位 | surface 变化时标记全量 damage 一次 |
| 状态订阅复杂度 | 作用域 diff 可能引入隐藏 bug | 订阅机制默认不启用，`State::set` 保持全量兼容 |

---

## 八、实施路线图

| 里程碑 | 内容 | 预计 |
|--------|------|------|
| **M1（P0）** | `SharedSurface` + `VisualElement::SharedSurface` + Reconciler 集成 | — |
| **M2（P0）** | `Compositor` 脏区合并/合成 + `VelloRenderer::render_partial` | — |
| **M3（P0）** | blit SIMD + 不透明快速路径 + framebuffer 复用 | — |
| **M4（P0）** | lieter 接入 SharedSurface，验证 terminal 场景性能达标 | — |
| **M5（P1）** | `RebuildScope` + 状态订阅 + Reconciler 子树剪枝 | — |
| **M6（P1）** | 布局脏子树剪枝 | — |
| **M7（P2）** | Wayland C/S 协议层 + 进程外 compositor（可选） | — |

---

## 九、结论

- **最优先（M1~M4）**：落地 **共享 Surface + 脏区合成**，这是解决 lieter 卡顿的关键，也是 terminal 场景能否跑通的分水岭。
- **其次（M5~M6）**：**按需重建**，让 UI 层也摆脱全量 rebuild。
- **远期（M7）**：**Wayland C/S**，在 Surface/Compositor 抽象稳定后平滑演进，为多进程、GPU 加速、安全隔离铺路。

三阶段均**保持 API 兼容、渐进可验证**，建议以 M1 为起点逐个里程碑实施，并在每个里程碑后用 `perf_terminal` 基准对照验收。
