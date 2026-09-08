# dirty-surface 分支审查与改进方案

> 版本：v1.2 | 日期：2026-09-08 | 分支：`refactor/dirty-surface`（基线 `origin/main`）
> 进度：**阶段 1（A1~A7）已实施**（§六）、**阶段 2 第一批（B1 持久 Pixmap + 多线程光栅化 + B5′ UI 脏区探测）已实施**（§七）；B2/B3/B4 待排期。
> 关联文档：`performance-refactor.md`（阶段 A/B/C 蓝图）、`complexity-audit-and-simplification.md`（方案 B / M0~M7）
> 本文定位：**执行基线**。先写文档再开工，阶段 1 的每一项都可对照本文验收。

---

## 一、分支现状：做了什么

相对 `origin/main`：21 文件 +3193/−144。目标是给 terminal 这类"高频、大画布、局部变化"组件打通**共享表面 + 脏区**通道。

| 类别 | 内容 | 位置 |
|---|---|---|
| 新机制 | `SharedSurface`：组件自持 RGBA8 缓冲（`Arc<Mutex<Vec<u8>>>`，可跨线程写）+ widget 级脏区 `damage(rect)` + 自增 `SurfaceId` + thread_local `Weak` 注册表 | `src/render/surface.rs`（新 229） |
| 新机制 | `Compositor`：backing 帧缓冲、脏区收集、条带合并 `merge()`、`composite_surface()` 只拷脏区 | `src/render/compositor.rs`（新 288） |
| 新机制 | `ExternalSource` / `ExternalEvent` / `wake()`：跨线程投递 + `EventLoopProxy` 唤醒（事件驱动替代轮询） | `src/external.rs`（新 98） |
| 管线集成 | `ViewNode::SharedSurface` → `VisualElement::SharedSurface{id,bounds}` → vello 跳过光栅化 | `view/node.rs` +49 / `runtime/mod.rs` +21 / `render/engine.rs` +3 / `render/visual.rs` +15 / `runtime/element.rs` +14 |
| 双通道合屏 | `present_frame`：UI pixmap 拷 backing 时**跳过 surface 覆盖区**（防白屏）→ 只合脏区 → backing 上屏；`blit_to_window` 改吃 `&[u8]` | `src/app.rs` +247 |
| 新 API | `Application::external_source()/external_data_handler()/on_resize()`；`WindowContext.compositor`；resize 时重建 compositor 并强制 re-layout | `src/app.rs` |
| 组件/示例 | `SharedSurfaceView`（8 个事件 builder）+ `examples/shared_surface.rs`（33ms 动画驱动脏区色带） | `widget/shared_surface.rs`、`examples/shared_surface.rs` |
| 依赖 | `vello_cpu` 0.1→0.2、`tungstenite` 0.24→0.30 | `Cargo.toml` |
| 设计资产 | 三阶段性能蓝图 + 方案 B 复杂度审计 + 3 个原型（14 测试） | `docs/performance-refactor.md`、`docs/complexity-audit-and-simplification.md`、`tests/prototype_*.rs` |

**总评**：机制骨架与设计论证到位（甚至自带自审计），但**性能收益尚未兑现**——脏区未接入管线、部分上屏未启用。二、三节给出证据与修复路径。

---

## 二、一帧链路（审查基线）

```
RedrawRequested                                  app.rs:636
├─ 全量分支（首帧/无 surface/rebuild 请求/尺寸变）  app.rs:642-644
│    builder() → root.build()                    app.rs:806-807
│    submit_view_tree → Runtime::frame           runtime/mod.rs:56,84
│      Reconciler diff/apply                     runtime/mod.rs:96-102
│      perform_layout                            runtime/mod.rs:180,252  → layout/context.rs:45-101（整棵 FlexNode 重建）
│      build_render_tree                         runtime/mod.rs:218,422  → 层表 clone+sort core/layers.rs:406
│    VelloRenderer::render                       render/engine.rs:58-107（Pixmap::new 每帧全屏新建 :61，每 z 层 render_with :92）
│    present_frame                               app.rs:868-964
│      ├─ 全窗逐像素写回 backing（含逐像素 any() 覆盖判定）  :911-949
│      ├─ composite_surface 只合脏区             :952-959
│      └─ backing.clone() → blit_to_window       :962 / :1009-1045（逐像素 pack + present :1044）
└─ 仅视觉分支                                     app.rs:651-652 → runtime/mod.rs:235 frame_visual_update
   （redraw 只置全局标记，需"补一次 request_render()" 兜底 —— app.rs:646-651）
```

已确认可利用但**尚未使用**的底层能力：

- `softbuffer 0.4`：`Buffer::present_with_damage(&[Rect])` —— 真正的部分上屏。
- `vello_cpu 0.2`：`Pixmap::as_mut() -> PixmapMut` + `RenderContext::new(&mut PixmapMut, RectU16)` —— 子区域光栅化。
- `Pixmap::data_as_u8_slice()`：直接拿 RGBA8 字节，免逐像素转换。

---

## 三、问题清单

### P0 性能：脏区形同虚设，多数场景比 main 更慢

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| 1 | 每帧多一趟全屏拷贝：backing 写回无条件整窗 | `app.rs:911-949` | main 只有 blit 一次 O(W·H)，现在两趟 |
| 2 | 逐像素覆盖判定 `surface_rects.iter().any()` | `app.rs:919-926` | O(W·H·S)，surface 越大越慢 |
| 3 | `backing.clone()` 每帧整帧堆分配 | `app.rs:962` | 1080p ≈ 8MB/帧，纯为绕开借用 |
| 4 | 脏区合并不在管线里：`merge()/add_dirty()/copy_region()/composite()` 零生产调用；`frame_dirty`、`touched/is_touched`、`unregister_surface`、`SurfacePainter` 无实现/调用 | `compositor.rs:45,61,155,179`；`surface.rs:52,160,185` | 半成品（审计文档 §六第 6 条已自认） |
| 5 | 未用 `present_with_damage` | `app.rs:1044` 仍 `present()` | 最大单点收益未兑现 |
| 6 | 未用局部光栅化，仍 `Pixmap::new` 全屏 | `render/engine.rs:61` | 每帧全屏重栅 |

### P0 正确性

| # | 问题 | 证据 | 后果 |
|---|---|---|---|
| 7 | **z 序被破坏**：surface 区域被无条件从 UI 拷贝中排除、之后再覆盖 | `app.rs:905-959` | z 更高的 popup/tooltip/确认弹窗被 terminal 遮住 |
| 8 | **残影**：surface 位置/尺寸变化时旧矩形像素不被清除（只 skip 不回填） | `app.rs:939-948` | resize/滚动后旧位置留残影 |
| 9 | **alpha 语义混用**：UI 通道是 premul（`PremulRgba8`），SharedSurface 用户写 straight RGBA，而合成是覆盖拷贝不做混合 | `app.rs:944`、`compositor.rs:145`（示例 `examples/shared_surface.rs:36`） | 半透明/抗锯齿边缘发黑或被全盖 |
| 10 | 注册表泄漏 + 静默失效：`unregister_surface` 无调用者；`resolve_surface` 失败静默跳过；thread_local 注册表 | `surface.rs:40-56` | Weak 项只增不减；surface drop 后该区域留旧像素；多窗口/多线程不成立 |

### P1 架构与易用性

| # | 问题 | 证据 |
|---|---|---|
| 11 | 三个帧入口 + 补丁式补调用，`request_redraw` 与 `runtime.needs_render` 语义分裂 | `app.rs:646-651`；`runtime/mod.rs:228,235` |
| 12 | external 半成品：`Data` 无来源标识；`wake()` 进程级 `OnceLock` 单例 | `external.rs:29,60` |
| 13 | 样板重：`Box<dyn Widget>` + `'static Fn`；`State<T>` 外泄且全量 rebuild；`use_state` 错位即 panic（examples 零使用） | `app.rs:203`；`state.rs:430-453`；`widget/mod.rs:250,259` |
| 14 | 回调 API 碎片化，10+ widget 各写一份且签名不一致 | `menu.rs:272` vs `button.rs:290` |
| 15 | `Animation` 必须变量保活；错误一律 panic/no-op | `animation.rs:78-82`；`app.rs:324,351,720` |
| 16 | 每帧固定成本未动：整树 build + `tree_eq`、整棵 FlexNode 重建、层表 clone+sort | `runtime/mod.rs:61`；`layout/context.rs:45-101`；`core/layers.rs:406` |

---

## 四、改进方案

### 阶段 1：让脏区真正兑现（✅ 已实施）

设计总纲：**present 改为脏区驱动**。一帧的脏区集合定义为

```
damage = UI 通道脏区 ∪ ⋃ surface 脏区 ∪ ⋃ (surface 旧矩形 ∪ 新矩形，仅当位置/尺寸变化时)
```

| # | 动作 | 具体做法 | 验收 |
|---|---|---|---|
| A1 | `present_frame` 脏区驱动 | ① 维护 `prev_surface_rects: HashMap<SurfaceId, Rect>`，与当前比对得出"移动/缩放"surface，把旧矩形并入脏区；② UI 像素**只拷脏区**（`copy_region`），且按 `z_index` 决定 UI 与 surface 的先后（surface 的 z 由 `VisualElement::SharedSurface` 携带）；③ 旧矩形先用 UI 像素回填 | 修 7、8；无 surface 时行为与 main 等价 |
| A2 | 去掉 `backing.clone()` | 把 `ensure_surface`/`buffer_mut` 的借用拆开（先取 `&backing[..]` 的裸切片作用域，或用 `take`+回填），blit 直接读 backing | 每帧省一次 W·H·4 分配+拷贝 |
| A3 | 接 `present_with_damage` | 合并后的脏矩形转 `softbuffer::Rect` 传给 `present_with_damage`；空脏区直接跳过上屏 | 光标级小更新只提交小矩形 |
| A4 | 覆盖判定改 span 段拷 | 每行预计算不重叠覆盖区间（surface 矩形按 x 排序合并），行内按 span `copy_from_slice` | 去逐像素 `any()`，O(W·H) 常数大幅下降 |
| A5 | 统一像素契约 | backing 统一 **premul RGBA**；`composite_surface` 实现 src-over 混合 + `a==255` 的 memcpy fast path；SharedSurface 文档明确"写入 straight RGBA，合屏时转 premul" | 修 9；半透明边缘不再发黑 |
| A6 | 注册表收口 | `SharedSurface` 实现 `Drop` 调 `unregister_surface`；`resolve_surface` 对失效 Weak 惰性移除；启用 `merge()` 接到 A3 之前（或删除死代码） | 堵泄漏；`merge()` 不再是死代码 |
| A7 | 基准 | 新增 `examples/perf_surface.rs`：1200×840 surface，对比"单点 20×20 脏区" vs "全屏脏区"的 ms/帧，输出 compositor 阶段耗时 | 可度量，写进本文 §五 |

### 阶段 2：部分光栅化 + 帧调度统一（1~2 周）

- **B1** `VelloRenderer` 持跨帧 `Pixmap`（`Pixmap::resize` 复用），去掉每帧 `Pixmap::new`。
- **B2** `Pixmap::as_mut()` + `RenderContext::new(&mut pm, RectU16)` 按合并条带重画，脏区外保留旧像素（先做包围盒，再逐条带）；z 层循环需能在子区域上正确合成。
- **B3** 三个帧入口收敛为 `FrameCommand` + `FrameScheduler`（`complexity-audit-and-simplification.md` §六），删掉 `app.rs:646-651` 补丁。
- **B4** 布局剪枝：无 dirty 子树复用上次 `ComputedLayout`；`FlexNode` 增量复用。
- **B5** 事件/reconciler 产出 UI 侧脏矩形，未变化整帧 Noop（替代整树 `tree_eq` 短路）。

### 阶段 3：易用性（可并行）

- **C1** `Application::new` 提供 `impl Widget + 'static` 泛型入口（自动 Box），builder 支持 `FnMut`。
- **C2** 统一 `.listen(EventType, cb)` 单一入口，现有 `on_click/...` 降级为语法糖并统一签名。
- **C3** `Animation::spawn()` 挂 Application 生命周期；`Animation::new` 加 `#[must_use]`。
- **C4** 公开 API 收 `Result<_, LieuiError>`（至少 `run`/`Image::from_*`/外部源注册）；`wake()` 返回 bool 或下发 proxy 句柄，去掉全局单例。
- **C5** `ExternalEvent::Data { source_id, payload }` + handler 注册表（或 `ExternalSource::on_event(&mut self, &mut EventContext)`）。
- **C6** 补 `examples/multi_window.rs`、`custom_widget.rs`、`external_source.rs`（模拟 PTY 线程 + `wake`）；`lib.rs` 加 crate 级 `//!` 文档；`Cargo.toml` 显式声明全部 example。

### 阶段 4：方案 B（Widget 树持久化）

按 `complexity-audit-and-simplification.md` 的 M2→M5 推进（已有 3 原型 14 测试），与性能工作解耦，建议在阶段 2 后启动。

---

## 五、验收标准

| 项 | 标准 |
|---|---|
| 功能 | 8 个 example（hello/counter/window/gallery/pdfkit/shared_surface/perf_click/perf_gallery）目视无回归；无残影、无遮挡错位 |
| 正确性新增用例 | ① surface 上方 z 更高的弹窗可见；② surface 移动后旧位置无残影；③ surface 半透明像素颜色正确；④ surface drop 后不留旧像素 |
| 性能 | `perf_surface`：1200×840 surface 单点脏区刷新 ≤ 全屏刷新耗时的 20%；无 surface 场景与 main 持平或更快 |
| 工程 | `cargo test` 全绿、`cargo clippy --all-targets` 无警告、`cargo fmt --check` 通过 |

## 六、阶段 1 实施记录（2026-09-08）

| 项 | 落地方式 | 位置 |
|---|---|---|
| A1 脏区驱动 | `composite()` 只处理合并后的条带：UI 铺底 → 按 z 升序合成 surface → 回填 z 更高的 UI 遮挡者（`Group` 会递归展开，浮层不漏） | `render/compositor.rs::composite`、`app.rs::collect_surface_info/collect_ui_rects` |
| A1 残影 | `sync_surfaces()` 比对上一帧矩形，移动/缩放/消失时把**旧矩形**标脏由 UI 回填；`surfaces_changed_from()` 供调用方判断 | `render/compositor.rs` |
| A1 z 序 | 新增 `SurfaceEntry{id, rect, z}`，surface 与 UI 遮挡者按 z 混排处理 | `render/compositor.rs` |
| A2 去整帧 clone | `std::mem::take(&mut backing)` → blit → 回填，取消每帧 8MB 级分配 | `app.rs::present_frame` |
| A3 部分上屏 | `present_with_damage(&[softbuffer::Rect])`；`age()==0`（缓冲区内容未定义）或脏区 ≥70% 时退化为全量 `present()`。win32 后端按矩形 `BitBlt`，X11/Wayland/KMS 同样生效 | `app.rs::blit_to_window` |
| A4 去逐像素判定 | 原 `covered(px)` 逐像素 `any()` 判定整体删除，改为按矩形 `copy_from_slice` 段拷贝 | `app.rs` |
| A5 像素契约 | backing/UI 通道为 premul；SharedSurface 为 straight RGBA，`blit_surface` 做 src-over 混合 + `a==255` 的 memcpy 快速路径 | `render/compositor.rs::blit_surface`、`render/surface.rs` 文档 |
| A6 注册表收口 | `SharedSurface: Drop` 自动注销（`try_with` 容错）；`resolve_surface` 惰性移除失效 `Weak`；`is_touched()` 用于跳过未触及表面；`merge_rects()` 进入管线 | `render/surface.rs`、`render/compositor.rs` |
| A7 基准 | `examples/perf_surface.rs`（`-- full` 切换整屏模式，`LIEUI_PERF=1` 看分阶段耗时） | `examples/perf_surface.rs`、`Cargo.toml` |
| 帧调度修补 | `request_redraw()` 旧语义（重画 UI）保留：仅当"UI 无 layout/render 标记 **且** 确有 surface 脏区"时才走免光栅化路径 | `app.rs::RedrawRequested` 分支、`runtime::needs_visual_update` |

**新增单测**（`render/compositor.rs`）：脏区外不受影响、surface 移动标旧脏区、高层 UI 遮挡回填、半透明 src-over 混合 —— 共 8 例。

**验证**：`cargo test` 16 个套件全绿（100 例）；`cargo clippy --all-targets` 对本次改动无警告。

**遗留（不属本次范围）**：
- `src/widget/menu.rs` 3 条 clippy 警告（`clone_on_copy` / 嵌套 `if let` / 临时值 `take()`）——分支既有，建议随 C2 回调 API 统一一起清。
- `external.rs` 的 `Data` 无来源标识、全局 `wake()` 单例 —— 阶段 3 的 C4/C5。
- `use_state` 错位 panic、`State<T>` 全量 rebuild —— 阶段 3 的 C1/C2 + 阶段 4。
- **B2 局部光栅化**：方案已验证（见 §七），待与 B5′ 联调。
- **B3 帧调度统一（FrameCommand）/ B4 布局剪枝**：待排期，方案见 §四 阶段 2。

## 七、阶段 2 第一批实施记录（2026-09-08）

| 项 | 落地方式 | 位置 |
|---|---|---|
| **B1 持久 Pixmap** | `VelloRenderer` 持有跨帧 `Pixmap`，尺寸不变时不重建（省每帧 `Pixmap::new` 的分配 + 清零，1080p ≈ 8MB）；`Renderer` trait 改为返回 `&Pixmap`（借用渲染器，调用方同语句消费）；`pixmap_bytes()` 直接供合屏消费（免逐像素转换） | `render/engine.rs`、`render/renderer.rs` |
| **多线程光栅化** | 新增 lieui feature `parallel`（**默认开启**）→ `vello_cpu/multithreading`（rayon 按区域切分）；`VelloRenderer::with_threads(w,h,n)` / `render_threads_from_env()`；`LIEUI_RENDER_THREADS` 环境变量：未设置=自动（核数-1 上限 8），`1`=强制单线程（极小场景规避调度开销） | `Cargo.toml`、`render/engine.rs` |
| **B5′ UI 脏区探测** | 元素内容签名（`LayeredElement::signature`：z/几何/颜色/文本/图像指针，`DefaultHasher`）帧间比较 → 变化元素的矩形并集（含旧矩形便于擦除）作为 UI 脏区；上屏随之只提交变化区域 | `render/visual.rs::signature`、`render/engine.rs::update_ui_dirty` |
| 保守退化策略 | 任何**无法定位矩形**的变化（Group、无边界元素）或元素数量变化 → 整屏脏（宁可多画不可漏更新）；`LIEUI_UI_DIRTY=0` 可关闭（等价旧行为） | `render/engine.rs` |
| 合屏入口拆分 | `present_frame` 拆为自由函数 `composite_frame(&mut compositor, ...)` + `WindowContext::blit_damage`，解决 `renderer.pixmap_bytes()` 借用与 `&mut self` 的冲突（字段级 disjoint borrow） | `app.rs` |

**B2（局部光栅化）调研结论（未实施）**：vello_cpu 0.2 的 `RasterizerSettings.offset` + `PixmapMut` 支持"场景边界 ≠ 目标区域"的局部渲染——整宽水平条带可表示为连续内存的 `PixmapMut::new(w, band_h, &mut bytes[y0*w*4..])`，配合 `ctx.set_transform(translate(0,-y0))` 平移坐标系即可只光栅化条带。**未实施原因**：① UI 脏区目前只能靠签名比较得到，条带化光栅化需与之联调；② z 层循环内每条带重复遍历元素 + `blit_image` 需增加 y 偏移参数；③ 无 GUI 像素级回归环境。计划与 B5′ 联调后作为 B2 落地（默认仅在脏区面积 < 全屏 50% 时启用）。

**新增单测**（`render/engine.rs`）：pixmap 跨帧复用（指针不变）、多线程与单线程输出逐像素一致、线程数可配置、UI 脏区三态（无变化为空 / 单元素变化覆盖其矩形 / 结构变化退化整屏）—— 共 6 例。

**验证**：`cargo test` 16 个套件全绿（116 例）；`cargo clippy --all-targets` 对本次改动无警告；冒烟：`perf_surface` 小脏区 17.1ms/帧、`perf_click` 首帧 15.2ms（201 元素）。

## 八、风险与回滚

| 风险 | 缓解 |
|---|---|
| `present_with_damage` 在部分后端被忽略 | 保留全量 `present()` 兜底路径，按后端能力探测切换 |
| 局部光栅化后 z 层合成错乱 | 阶段 2 先只做"整帧持久 Pixmap + 脏区包围盒"，验证后再逐条带 |
| 像素契约改动引入色偏 | A5 单独一个 commit，配 alpha 混合单测 + gallery 目视 |
| 阶段 1 改动面集中在 `app.rs` | 每完成一项即 `cargo test` + 跑 shared_surface/hello/counter/gallery 四个 example |
