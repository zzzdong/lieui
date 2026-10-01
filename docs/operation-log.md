# lieui 操作 / 变更日志

> 约定：每次施工追加一节（按时间正序）。每条写清「做了什么 / 为什么 / 怎么验证的」，并记录**决策**与**遗留**。
> 设计基线：[`docs/architecture-v3.md`](./architecture-v3.md)（v3.4-draft）。
> 里程碑：M0 抽 crate 骨架 → M1 v3 地基 → M2 布局/命中 → M3 渲染/脏区 → M4 窗口/多窗口 → M5 组件迁移。

---

## 2026-09-30 · M0 抽 crate 骨架 —— ✅ 完成

### 决策

1. **新分支 `refactor/v3`**（从 `feature/mvp` 切出）。现有分支不动，随时可回退。
2. **v3 代码放 `v3/` 子目录自成工作区**（`v3/Cargo.toml`）：根 `Cargo.toml` / `src/` / `tests/` / `examples/` **一字未动**，
   因此现有 10 个测试套件与 7 个 example 完全不受影响（设计文档 §六"不改动现有分支"的落地方式）。
   M5 尾声再把 `v3/crates/*` 提回根、切换 `prelude`。
3. **crate 切分**：`lieui-geom`（零依赖）→ `lieui-text`（parley）→ `lieui-layout`（Flex 引擎）→ `lieui`（M1 起填充）。
4. **文本排版规格独立成 `TextSpec`**（从旧 `view::paint::TextStyle` 拆出，只留影响整形/测度的字段）：
   layout 因此不必依赖 view 层；**颜色不在 spec 内**，由绘制层在 `create_text_layout(text, spec, color)` 时给。
5. `lieui-layout` **不抽 `context.rs`**：它把旧 `ElementTree` 编译成 `FlexNode`，与旧树强耦合；v3 在 M2 写新适配器。

### 变更清单

| 文件 | 动作 |
|---|---|
| `v3/Cargo.toml` | 新增：工作区（4 成员）+ 与根一致的 `dev` profile |
| `v3/crates/lieui-geom/{Cargo.toml,src/lib.rs}` | 新增：从 `src/geometry/types.rs` 拷贝；**删除 `Color::as_vello()`**（M3 放到渲染层，避免 geom 依赖 `vello_cpu`） |
| `v3/crates/lieui-text/{Cargo.toml,src/lib.rs}` | 新增：从 `src/text/mod.rs` **重写适配**——去 `crate::view::paint::TextStyle`，改 `TextSpec`；`create_text_layout`/`apply_plain_editor_style`/`create_plain_editor` 增加 `color` 参数；新增 `TextEngine::clear_measure_cache()` |
| `v3/crates/lieui-text/src/spec.rs` | 新增：`TextSpec` + `FontWeight` + `TextAlign` |
| `v3/crates/lieui-layout/src/{box_model,constraint,flex_line,flex_node,measurable,style,types}.rs` | 从 `src/layout/*` 拷贝；路径改写（`crate::layout::x → crate::x`、`crate::geometry::Rect → lieui_geom::Rect`、`crate::text::TextEngine → lieui_text::TextEngine`、`crate::view::paint::TextStyle → lieui_text::TextSpec`）；**删除 `FlexStyle::scroll_state` 与 `bind_scroll_state`**（v3 滚动态归 `Kind::Scroll` 节点） |
| `v3/crates/lieui-layout/src/lib.rs` | 新增：原 `layout/mod.rs` 去掉 `context` / `LayoutContext` |
| `v3/crates/lieui-layout/tests/smoke.rs` | 新增：4 个冒烟测试 |
| `v3/crates/lieui/src/lib.rs` | 新增：占位（re-export 三个 crate），M1 填充 |
| `.gitignore` | 追加 `/v3/target`、`/v3/Cargo.lock` |

### 验证

- `cargo check --manifest-path v3/Cargo.toml --workspace`：**一次通过，0 warning**。
- `cargo test --manifest-path v3/Cargo.toml --workspace`：`tests/smoke.rs` **4 passed / 0 failed**
  （row 定长布局、column 堆叠、`flex_grow` 撑满主轴、文本叶在约束宽度下经 parley 重测度）。

### 环境备注（踩坑，后续所有构建都要用）

本机 `~/.cargo/bin/cargo.exe` 是**指向 `rustup.exe` 的符号链接**（`LinkType=SymbolicLink`, `Length=0`），
沙箱拒绝穿越（报"无法遍历该路径，因为它包含不受信任的装入点"），`cmd /c cargo` 也不行。
**必须**直接调用工具链里的真实 exe：

```powershell
$tc = Join-Path $env:USERPROFILE '.rustup\toolchains\1.92-x86_64-pc-windows-msvc\bin'
$env:PATH = "$tc;$env:PATH"
& "$tc\cargo.exe" test --manifest-path v3/Cargo.toml --workspace
```

### 遗留

- `lieui-text` 仍有 **3 个 thread_local**（`FONT_CONTEXT` / `LAYOUT_CONTEXT` / `MEASURE_CACHE`），
  与设计目标"0 thread_local"不符 → 规划到 **M2** 收敛为显式 `TextService` 对象（`&mut TextService` 穿透到 measure 调用链）。
- `Color::as_vello()` → M3 放进渲染层。
- 运行时（Runtime/Signal 等）：无 thread_local / 无全局单例，见 M1。

---

## 2026-09-30 · M1 v3 地基 —— 🚧 进行中（3/8 模块）

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `src/reactive.rs` | `Runtime`（每窗口脏标志表 `Vec<(WindowId, Dirty)>`；无 thread_local / 无全局单例）、`Dirty`（手写位集，免 `bitflags` 依赖）、`Signal<T>`（`Rc` 句柄，**不要求 `T: Clone`**；`get` / `with` / `set` / `update` / `take`）、`act` / `act1` | 12 |
| `src/window.rs` | `WindowId(u32)`（M4 与 winit 的 `WindowId` 做映射，M1 不引入 winit） | — |
| `src/style.rs` | `TextStyle`（= `lieui_text::TextSpec` + `color`/`hover_color`/`pressed_color`）、`PaintStyle`、`ShadowSpec`、`ImageStyle` / `ImageFit` | 5 |

### 关键实现决策（写下来免得回退）

1. **`Signal` 持 `Runtime`（`Rc` 包装）而非裸 `Rc<RuntimeInner>`**：否则 `mark_all` / `is_in_view` 等方法要在
   `RuntimeInner` 上重复实现一遍。
2. **`Signal::set/update/take` 在 `view()` 期间 panic（debug）**：`Runtime::begin_view/end_view` 置位，
   `set` 内 `assert_not_in_view` 拦下 —— 这是"状态变更只能来自事件闭包 / 生命周期钩子"的运行时保险（设计 §3.3 约束 2）。
3. **`impl Clone for Signal<T>` 手写**（不能 derive：会强加 `T: Clone`）。测试里专门用**非 `Clone` 的 `T`** 验证这一点。
4. **保守传播**：`set` 给**所有**已注册窗口置 `VIEW`（R1 不追踪依赖，无法知道谁读了它）。
   测试 `batching_is_free_one_flag_for_n_sets` 断言"一次事件改 N 个 signal，脏标志仍只有 `VIEW` 一位" ⇒ 批处理免费。
5. `begin_view/end_view` 暂标 `#[allow(dead_code)]`（帧循环在 M1 后续 / M4 接入后去掉）。
6. `style.rs` 不引入聚合 `Style` 结构：`Node` 侧保持 `layout` / `paint` / `text` 三字段并列（设计 §3.2），
   构造期由 `TextRef` / `ButtonRef` 这类句柄分别访问，避免一个类型承担三种语义。

### 验证

- `cargo test --manifest-path v3/Cargo.toml --workspace`：**21 passed / 0 failed**（`lieui` 17 + `lieui-layout` smoke 4），**0 warning**。
- `reactive::tests::set_inside_view_panics`（`#[should_panic]`）确证自我触发保险生效；
- `reactive::tests::get_inside_view_is_fine_and_no_dirty_is_marked` 确证"只读不置脏"。
- ⚠️ **`cargo clippy` 本环境不可用**：工具链 `1.92-x86_64-pc-windows-msvc/bin` 下**没有 clippy 组件**
  （只有 cargo/rustc/rustdoc），`.cargo/bin/cargo-clippy.exe` 又是被沙箱拒绝的符号链接。
  因此当前门禁 = `cargo test` + 编译 **0 warning**；clippy 待环境修复（`rustup component add clippy`）后补跑。

### 待办（M1 剩余）

- [x] `track.rs` / `view.rs` / `align.rs` / `event.rs`（类型层）—— 见下"第二段"
- [x] 路由分发（两段式）+ `cmd.rs` + 帧驱动 + `WindowView` 擦除 —— 见下"第三段"
- [ ] `view.rs` 缺的糖：`stack`（局部水印用）、`*_bind` 双向绑定（依赖 Input/Slider 的真实行为，M5）
- [ ] 动态开关窗（`cx.open_window` / `pending_windows` 队列）—— 需要 winit 真正建窗口，挪到 **M4**
- [ ] 内置行为分派（`widgets/handle`）—— 随组件一起在 M5 落地；`Track::add_builtin_handler` 已就位

---

## 2026-10-01 · M1 第二段：track / view / align / event —— ✅ 完成

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `src/event.rs` | `EventKind`（40 个变体，裁剪自 WinUI）+ `Routing`（Tunnel/Bubble/Direct）+ `PointerId` + `Handler = Rc<dyn Fn(&mut Ctx)>` + `Ctx`（`invalidate`/`request_repaint`/`runtime`/`window`；**不提供保留树访问**） | 3 |
| `src/track.rs` | arena（`NodeId` = index:32\|gen:32，`destroy` 自增 generation）+ `Node`（UIElement 属性集 + 视图态 + 布局结果 + handlers）+ `Kind`/`KindDesc`（desc 组 vs state 组）+ `Flags` + 层（`Root`/`Layer` 6 值/`LayerOpts`/`Anchor`/`Placement`/`FocusPolicy`）+ 窗口级视图态（hover/pressed/focus/captures）+ 脏区（`damage`/`damage_all`） | 16 |
| `src/view.rs` | 描述 arena（`ViewBuf`，`begin()` 只重置游标）+ 构造 DSL（`column`/`row`/`container`/`scroll`/`spacer`/`text`/`button`/`checkbox`/`slider`/`progress`/`image`/`keyed_list`）+ 层入口（`modal`/`overlay`/`popup_at`/`tooltip_at`/`drag_preview`，**嵌套声明**）+ `DescRef` 链式样式（布局/绘制/文本/可视/事件五组） | 7 |
| `src/align.rs` | 描述 ↔ 保留树对齐（**位置 + 一层 key**）+ 内部 patch + 脏区登记 + `AlignStats` | 13 |
| `Cargo.toml` / `lib.rs` | 模块接线与再导出 | — |

### 验证

`cargo test --manifest-path v3/Cargo.toml --workspace`：**59 passed / 0 failed**（`lieui` 55 + `lieui-layout` smoke 4），**0 warning**。

关键行为已被单测钉住：

- `identical_desc_is_a_no_op`：同一份描述重复对齐 ⇒ `created/destroyed/patched = 0`，且**零脏区**（"未变化节点零操作"）。
- `text_change_patches_one_node_and_keeps_ids`：只改一个文本 ⇒ `patched = 1`，节点身份不变，其他节点不受影响。
- `slider_value_update_preserves_dragging_state`：`Slider.value`（desc）被对齐，`dragging`（state）跨帧保留 ⇒ **设计 §3.4.1 的核心承诺成立**。
- `keyed_list_reuses_nodes_on_reorder_and_destroys_missing`：`[1,2,3] → [3,1,4]` ⇒ 复用 2 个、新建 1、销毁 1，顺序正确。
- `layers_appear_and_disappear_and_nesting_cascades`：Modal 里声明的 Popup 随 Modal 一起消失（`roots_removed = 2`）。
- `modal_appearance_marks_whole_window_dirty` / `layer_opts_change_marks_whole_window_dirty`：backdrop 与锚点变化 ⇒ 整窗脏。
- `damage_uses_old_rect_of_patched_node`：脏区用的是**旧**矩形（新矩形由 M2 布局阶段补登）。
- `set_inside_view_panics`（reactive）：`view()` 内改状态被拦下。

### 踩到的四个真问题（都已修，写下来免得重犯）

1. **闭包不能返回借用** ⇒ 容器/层入口一律 `FnOnce(&mut Self)`，链式句柄 `DescRef` 只能作为**语句**用
   （`c.text("x").font_size(48.0);`）。试过 `impl FnOnce(&mut Self) -> R`，编译器直接报
   "returning this value requires that `'1` must outlive `'2`" —— 这是类型系统硬约束，不是取舍。
2. **`keyed_list` 的 key 必须写两处**：父节点的 `child_keys`（顺序对齐）**和**子节点自身的 `key`
   （保留树侧按它匹配复用）。只写前者会导致复用全部失败（每次全量重建）。
3. **"类型不同即重建"必须在调用方处理，且必须"先建后销毁"**：销毁会把旧节点从父 `children` 摘掉，
   位置信息（下标）随之丢失 ⇒ 先 `replace_child_at(parent, i, fresh)` 再 `destroy(old)`。
4. **新建的层根要立刻登记为已认领**，否则 `align` 末尾的"清理未认领根"会把它当垃圾立刻删掉
   （症状：第一个测试全部 `有内容根` panic）。

### 与设计文档的四处偏差（已按实现修正认知）

- `DescNode` 需要 `tab_stop` / `tab_index`（设计 §3.2 的 `Node` 里有，描述侧漏了）。
- `modal()` **不占用 `anchor`**（居中由层语义给出）；`anchor` 只服务按 key 锚定的层（Popup/Tooltip）。
- `Node` 不重复存 `opacity`（`PaintStyle.opacity` 是唯一来源），避免两个真相。
- 代码里描述 arena 叫 **`ViewBuf`**（不是文档的 `View<'_>`）：强调它是"可复用的缓冲"，也避开将来可能出现的 `View` trait 命名冲突。

---

## 2026-10-01 · M1 第三段：cmd / 路由分发 / 帧驱动 —— ✅ M1 逻辑层完成

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `src/cmd.rs` | `Cmd`（22 个纯数据变体）+ `CmdBuf` + `apply_cmds(track, cmds) -> Dirty`。**唯一**的延迟写入通道，取代旧实现里 5 个散落通道 | 11 |
| `src/event.rs`（分发） | `Event`（Pointer/Wheel/Key/Simple/Tick）+ `EventView`（`Copy` 摘要，避免每处理器 clone payload）+ `HandlerSlot { kind, handler, handled_events_too }` + `RoutePlan` / `collect_route`（Tunnel / Bubble / Direct 三态）+ `dispatch`（**两段式**）+ `Ctx` 扩成"改状态 / 攒 `Cmd` / 请求重绘 / `mark_handled`" | 11 |
| `src/app.rs` | `ViewModel` trait（`view(self: &Rc<Self>, ..)` + `on_tick` / `on_external` / `on_close_request`）+ `WindowView` 对象安全擦除（`VmAdapter` + `erased()`）+ `WindowConfig` / `CloseAction` / `ExternalData` + `WindowCtx`（帧驱动 / 分发 / tick / external / 关闭请求）+ `App`（**无泛型**，多窗口） + `FrameStats` | 9 |
| `track.rs` / `view.rs` 补丁 | `add_builtin_handler`（`handled_events_too = true`）、`has_layout_dirty`、`replace_child_at`、`HandlerSlot` 接线、`DescRef::on_always` | — |

### 验证

`cargo test --manifest-path v3/Cargo.toml --workspace`：**87 passed / 0 failed**（`lieui` 83 + `lieui-layout` smoke 4），**0 warning**。

M1 的端到端承诺（响应式回路 + 分发 + 对齐）已被这些测试钉住：

- `first_frame_mounts_the_tree`：首帧 `view()` → `align` 建出 5 个节点、整窗脏。
- `second_frame_is_idle_without_state_change`：**无状态变化时第二帧完全空闲**（`view_ran = false`、零 patch、零脏矩形）。
- `signal_change_reruns_view_and_patches_only_that_node`：`signal.set()` → 重跑 `view()` → `align.patched == 1`（只改那一个文本节点），节点身份与数量不变。
- `button_closure_changes_state_and_next_frame_renders_it`：命中链 → 按钮闭包 → `Signal` 变化 → 下一帧只 patch 一行文本（**这就是 §九 counter 例子的完整逻辑闭环**）。
- `handler_can_request_repaint_without_rerunning_view`：`cx.request_repaint()` 只标 `PAINT|PRESENT`，不重跑 `view()`。
- `two_windows_share_signals_but_keep_separate_trees`：共享 `Signal` 变一次 ⇒ **两个窗口都重跑**（R1 保守传播），两棵树各自独立。
- `handled_stops_the_bubble_but_not_handled_events_too`：内层 `mark_handled` 后，中间层被跳过、最外层 `handledEventsToo` **仍然执行** —— `.builtin()` 排序 hack 的正面替换成立。
- `bubble_goes_inner_to_outer` / `tunnel_goes_outer_to_inner` / `direct_only_reaches_the_target`：三态路由顺序正确。
- `handlers_can_queue_commands_that_apply_after_dispatch`：处理器只攒 `Cmd`，**分发结束、借用释放后**才 `apply_cmds` 落树（`focus` 生效）。
- `tick_hook_runs_without_rerunning_view` / `external_data_reaches_the_view_model` / `close_request_can_be_cancelled`：三个生命周期钩子都不触发 `view()`。

### 踩到的四个真问题（都已修）

1. **`Rc<V> → Rc<dyn WindowView>` 不能直接强转**。unsizing 要求 `V: WindowView + Sized`，给 `Rc<V>` 实现 trait 帮不上忙
   （编译器报 `the trait bound V: WindowView is not satisfied`）。而 `ViewModel::view` 的 receiver 是 `&Rc<Self>`，
   拿不到 `Rc` 就调不了。解法：`struct VmAdapter<V>(Rc<V>)` 实现 `WindowView` + `pub fn erased<V>(Rc<V>) -> Rc<dyn WindowView>`。
2. **`NodeId` 的 `Default` 不能 derive**：derive 会给 `NodeId(0)`，而那是**合法身份**（index 0 / gen 0），
   用来表达"没有节点"会埋雷。手写 `Default` 返回 `NodeId::NULL`。
3. **`FrameStats::layout_pending` 的语义**：一开始写成 `dirty.contains(LAYOUT) || track.has_layout_dirty()`，
   而 M1 不跑布局 ⇒ 节点上的 `MEASURE_DIRTY` 永不清除 ⇒ "空闲帧"测试永远失败。
   正确口径是"**本帧产生的义务**"：`dirty.contains(LAYOUT) || (view_ran && track.has_layout_dirty())`。
4. **`Ctx` 不能持有 `&mut CmdBuf`**：那会给 `Ctx` 引入生命周期参数，`Handler = Rc<dyn Fn(&mut Ctx)>`
   就得写成 HRTB（`for<'a> Fn(&mut Ctx<'a>)`），对象化与调用点都会变复杂。
   改为 **`Ctx` 自带一个 `CmdBuf`**，`dispatch` 结束后 `take_cmds()` 交给调用方统一 `apply_cmds`。

### 明确挪到后续里程碑

- **动态开关窗**（`cx.open_window` / `Runtime.pending_windows` 队列）→ **M4**：它需要真实的 winit 窗口创建，
  现在造一个类型擦除的队列只会被重写一遍。静态多窗口（`App::window` / `window_erased` / `close_window`）M1 已可用。
- **内置行为分派**（`widgets/handle(track, id, ev, cmd)`）→ **M5**：`Track::add_builtin_handler` 与
  `handled_events_too` 机制已就位，缺的是组件本身。
- **`*_bind` 双向绑定 / `stack` 容器** → 随 Input/Slider 与 M2 布局一起做。

### 当前规模（含单测）

`track 1130 / event 775 / view 742 / app 677 / align 640 / cmd 436 / reactive 396 / style 298 / lib 55 / window 20`
≈ 5.2k 行（其中约 1.5k 是单测）。对照设计预估的 8~9k 总量，M1（地基）已基本收敛，后续主要是 M2 布局适配器、M3 渲染、M5 组件。

---

## 2026-10-01 · M2：布局适配器 + 命中 + 输入 + 焦点 —— ✅ 逻辑层完成

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `src/layout.rs` | 保留树 ⇄ Taitank 的适配器：`build()`（叶子带 `measure_text`/`intrinsic_size`）、`write_back()`（写 `computed`/`desired`、旧∪新 bounds 入脏区、滚动内容尺寸 + 钳制 + 子原点平移）、`layout()`（**边界集合**重排）、`layout_subtree()`、`has`/`rect_of`/`contains` | 10 |
| `src/hit.rs` | 命中测试（纯函数）：层序（DragPreview→Content，同层逆序）、`Visibility` 三态、`hit_test_visible` 子树穿透、`Clip`、**变换逆矩阵**（父变换作用于子树）、`blocks_below` 无条件吸收、`hit_path_for`（捕获优先）、`path_to` | 16 |
| `src/transform.rs` | `Affine`（2D 仿射）+ `Transform::matrix(rect)`（origin/scale/rotate/translate 合一）+ `inverse`/`bounding_box` | 9 |
| `src/focus.rs` | `set_focus`（保证旧焦点必清）、`FocusChange`、`focusable_ancestor`、`tab_order`/`next_tab`（`(tab_index, 树序)` 稳定排序 + 循环） | 9 |
| `src/input.rs` | 指针状态机：hover 传播（旧∪新链，Entered/Exited/Moved）、`Tapped` 合成、捕获路由、`Cancel` 必发 `PointerCaptureLost`、`default_wheel_scroll`（嵌套滚动链） | 13 |
| `src/app.rs` | 帧里接上布局；新增 `set_size`/`hit`/`hit_target`/`pointer`（状态机→分发→默认滚动）/`key`/`tab`/`focus`；`PointerOutcome` | 8 |
| `Track` 补丁 | `mark_flow_dirty`、`mark_all_layout_dirty`、`clear_layout_flags`、`layout_boundaries`、`node_ids`；`add_root`/`destroy` 补标脏 | — |
| `lieui-geom` | 补 `impl Default for Rect`（布局写回需要"零矩形"起点） | — |

### 验证

`cargo test --manifest-path v3/Cargo.toml --workspace`：**151 passed / 0 failed**（`lieui` 147 + `lieui-layout` smoke 4），**0 warning**。

> 说明：本机工具链没装 `clippy`（`cargo clippy` → `no such command`），所以这轮只跑了 `cargo test`。

M2 的端到端承诺（**真实命中 → 点击 → 状态 → 下一帧只 patch 一行**）已被钉住：

- `app::tests::click_reaches_the_button_through_hit_testing`：用**命中测试算出来的坐标**（不是手写 path）
  驱动 `Down`/`Up` → `Tapped` → 闭包改 `Signal` → 下一帧 `align.patched == 1`。
- `app::tests::hover_follows_the_pointer_and_marks_damage`：hover 走完整链路、链路每个节点都置 `pointer_over`、
  **不重跑 `view()`**、只产生脏区。
- `app::tests::wheel_scrolls_the_container_when_no_handler_claims_it`：无处理器认领 ⇒ 框架默认滚动；
  滚动**不触发重排**（`layout.ran == false`）。
- `app::tests::frame_runs_layout_then_goes_idle` / `resize_relayouts_the_whole_window`：帧内重排 + 空闲帧 + 尺寸变化整窗重排。
- `layout::tests::fixed_size_subtree_is_a_boundary`：改固定尺寸卡片里的文本 ⇒ **边界只有 1 个、只重建 2 个节点**
  （不含根），卡片尺寸不变 —— 这就是"脏边界重排"的收益（旧实现是整窗重建 flex 树）。
- `hit::tests::*`：层序/Modal 吸收/Overlay 穿透/`Collapsed` 补位/`Hidden` 占位不可命中/`Clip`/缩放平移/
  父变换作用于子树/退化变换不可命中。
- `input::tests::*`：hover 链迁移、`Tapped` 合成的两种情形（同目标 / 释放落在子节点）、
  捕获期间事件回到捕获链、`Cancel` 清态并必发 `PointerCaptureLost`。

### 本轮最重要的架构修正：**两种"脏"**

设计稿只写了"`mark_layout_dirty` + 边界冒泡"，实现时发现一个分类错误：

| 脏类型 | 含义 | 影响范围 | API |
|---|---|---|---|
| **自身尺寸脏** | 我的内容/尺寸可能变了（文本、`Kind::desc`） | 只有"尺寸未确定"的节点会把影响传给祖先 | `mark_layout_dirty` |
| **流脏** | 我在父的**流**里变了（`Collapsed`、被增删、`FlexStyle`/margin 变了） | **必然**让兄弟重排 ⇒ 必须连父一起标脏 | `mark_flow_dirty` |

如果只有前者，会出现："把固定尺寸的子节点 `Collapsed`，兄弟不补位"（因为标记停在它自己身上）。
测试 `hit::tests::collapsed_child_shifts_siblings_and_is_unhittable` 与
`layout::tests::collapsed_children_are_excluded_from_the_flex_tree` 现在把这条钉住了。
`align` 也相应拆成 `size_changed / layout_changed / flow_changed` 三种触发。

### 与设计文档的偏差（已按实现修正认知）

1. **没有 `LayoutTree` trait**。设计稿写"新写适配器实现 `LayoutTree`"，但 M0 抽出的引擎里
   `FlexNode` 自带 `measure_text: Option<(String, TextSpec)>` 与 `intrinsic_size`，
   而且 `build()` 本来就是**自底向上持有**整棵 flex 树（`children: Vec<FlexNode>`）——
   中间再加一层 trait 回调只是纯开销。所以直接"Track → FlexNode 树 → 求解 → 写回"。
2. **`MEASURE_DIRTY` / `ARRANGE_DIRTY` 不分段**（设计稿已预告"v1 两个 flag 先只用于跳过整棵边界子树"）：
   现在两者同置同清，真正的 arrange-only 优化留待基准验证后做。
3. **`Hidden` 定为"占位但不绘制 ⇒ 也不可命中"**（命中跟随渲染，与 WPF 一致）；
   `Collapsed` 连布局都不参与。
4. **Modal 无条件吸收**（不看层根矩形）：否则"点在小对话框旁边"会漏给背后的内容，
   而 backdrop 是全窗绘制的。水印类 `Overlay` 则整层穿透（`LayerOpts.hit_test_visible = false`）。
5. **`Transform` 只作用于绘制/命中，不作用于布局**（对齐 WinUI 的 `RenderTransform`）：
   被放大的元素的**兄弟不会挪位**，它的子树随它一起缩放（命中测试按逆矩阵递归）。层级/锚点定位在 M4/M5。
6. **`desired`（≈`DesiredSize`）的当前口径**：叶子 = 固有测度（文本重新测一次、命中 parley 缓存）；
   容器 = 引擎分配尺寸（滚动容器的真实内容尺寸在 `Node.content_size`）。后续若有布局需要再细化。

### 顺带修掉的两个 M1 遗留 bug（M2 的测试暴露的）

1. **`add_root` 没有标脏**：新挂载的层（Popup/Modal）永远等不到一次布局 ⇒ rect 恒为全零。
   现在 `add_root` 里调 `mark_layout_dirty(node)`。
2. **`destroy` 没有标脏父节点**：删掉一个子节点后兄弟位置不重排。现在补 `mark_layout_dirty(parent)`。

### 当前规模（含单测）

`track 1198 / app 976 / event 801 / view 742 / align 647 / input 577 / layout 459 / cmd 447 / reactive 396 / hit 311 / style 298 / transform 207 / focus 193 / lib 72 / window 20`
≈ 7.3k 行（其中约 2.5k 是单测）⇒ 有效实现约 4.8k 行。

### 下一步：M3 渲染

draw list 展开（`Track` → `Scene`，含 hover/pressed 配色解析、`Transform` 应用、`Clip` 入栈）+
持久 `Pixmap` + 脏区光栅化（vello_cpu）+ `present_with_damage`（移植 dirty-surface 的契约与单测）。
`FrameStats.damage`/`damage_all` 已经从 M2 起就是现成的输入。

---

## 2026-10-01 · M3：draw list + 纯 CPU 光栅化 + 脏区 —— ✅ 像素级跑通

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `render/scene.rs` | `Op`（Rect/Shadow/Border/Text/PushClip/PopClip/PushOpacity/PopOpacity，**op 自带已组合好的 `transform`**）、`Scene`（扁平列表 + 成对性自检）、`SceneBuilder::build`（树 → 列表，含 `Cull` 剔除）、`TextCache`（`(内容, 规格, 颜色)` → 排版，FNV 规格哈希） | 14 |
| `render/raster.rs` | `Rasterizer`（持久 `Pixmap` + 复用 `scratch` + `RenderContext`）、`damage_batches`（脏区 → 批次）、`to_vello`（straight → premultiplied）、全部原语提交（含 glyph run、模糊阴影、裁剪、不透明度层） | 16 |
| `render/mod.rs` | `Renderer`（SceneBuilder + Rasterizer 的门面）、`RenderStats` | — |
| `widgets/mod.rs` | 按 `Kind` 枚举分派的**绘制**：Box/Text/Button/Checkbox/Slider/Progress + 交互配色解析（pressed > hover > 常态）+ 禁用降级（alpha 减半） | 8 |
| `track.rs` 补丁 | `Node::paint_bounds()`（文本按内容范围）、`Track::damage_bounds()`（沿祖先链组合变换） | — |
| `lieui-geom` | `Rect::{intersects, intersect, union, inflate, is_empty, center, right, bottom}` + `GREEN/BLUE/GRAY` 常量 | — |
| `v3/crates/lieui/examples/render_counter.rs` | 无窗口跑完整管线 + 导出 PNG | — |

### 验证

`cargo test --manifest-path v3/Cargo.toml --workspace`：**196 passed / 0 failed**（`lieui` 192 + `lieui-layout` smoke 4），**0 warning**。

`cargo run -p lieui --example render_counter`（**debug 档**）实测：

```
首帧       : view_ran, align.created=5, layout{boundaries=1, nodes=5, moved=5}, damage_all
展开 op    : 5，光栅化像素 120000（整窗 400×300）
set 后脏标志: Dirty(1)                    ← 只置位，不立即干活
第二帧     : view_ran, align{patched=1, unchanged=4}
  对齐 patch=1，重排边界=1，光栅化 3120 像素（占整窗 2.6%）   ← 脏区就是那一个文本节点的矩形
空闲帧     : is_idle=true                 ← 什么都不做
pixmap     : 400×300，非底色像素 3499，不同颜色 260 种        ← 文字真的画出来了
已写出     : lieui_counter.png（4832 字节）
```

对照设计稿的验收线（"小脏区 ≤ 全量的 20%"）：**2.6%** ✓（且这是 debug 档；release 只会更好）。
对照旧实现：旧的是"每帧全量重建 + 全量光栅化"，这里是"**一次文本改动 → 只光栅化那 38×77 的矩形**"。

### 本轮踩到的四个真问题（都已修，其中两个是真正的设计缺陷）

1. **"全宽行带"是错的优化**。最初实现按"全宽 × 若干行"做局部渲染（为了能直接用持久 pixmap 的
   连续字节切片做 `PixmapMut` 视图）。但这把 38×77 的文本脏区放大成 400×77 —— 收益直接减半，
   而且 `damage_bands` 的 45% 兜底经常被触发（多个小脏区面积一累加就退化整窗，
   测试 `click_only_repaints_a_band` 抓到了：3 个等价脏区 + 一个全宽脏区 = 58% ⇒ 整窗）。
   **改为"任意脏矩形批次"**：每个脏区单独渲染进复用的 `scratch` `Pixmap`，再逐行拷回持久 pixmap。
   开销 ∝ **脏区面积**（新测试 `narrow_damage_only_rasterizes_its_own_area`：2×3 脏区只光栅化 **6** 个像素）。
2. **整窗大小的容器一进 hover 就把整窗标脏**。`set_pointer_over` 原先无条件 `mark_paint_dirty`，
   而内容根是 400×300 ⇒ 鼠标一进入窗口就"整窗脏"。改成：**只有声明了交互视觉的节点才标脏**
   （`PaintStyle::is_interactive()` / `TextStyle::is_interactive()`）。这是旧实现同款毛病的正面修复。
3. **文本节点的脏区要按内容范围算**。文本常被 `align_items: stretch` 拉伸到容器宽度，
   但字形只占 `desired`（固有测度）那一块。新增 `Node::paint_bounds()`（文本用 `desired` + `text_align`
   定位，其余用整矩形），脏区从"整条宽度"收到"字形范围"——上面 2.6% 就来自这里。
   顺带新增 `Track::damage_bounds()`：脏区**沿祖先链组合变换**，修掉"被变换的元素改了但脏区还在原位"的残影隐患。
4. **`RasterizerSettings::offset` 在 vello_cpu 0.2 里是 `(u16, u16)`**（不能为负），
   所以"把批次原点移到 (0,0)"不能靠 offset，改由**场景变换**完成：`shift = translate(-x0, -y0)`
   与每个 op 自身的变换组合后 `set_transform`。

另外记录一条实现约束：`#![forbid(unsafe_code)]` 下做局部光栅化的可行路径是
`Pixmap::data_as_u8_slice_mut()` + `PixmapMut::new(..)`（都在 vello 的公开 API 里），
拷回用 `PremulRgba8` 切片的 `copy_from_slice` —— 全程无 unsafe、无 bytemuck。

### 与设计文档的偏差

1. **依赖 `vello_cpu` 0.2**（根 crate 在 0.1）。0.2 的 `RasterizerSettings::offset` /
   `Pixmap::data_as_u8_slice_mut` / `push_opacity_layer` 正好够用；差异记录在此，切换版本时需复核。
2. `Color::as_vello()` 按 M0 的计划落在渲染层（`render::to_vello`），`lieui-geom` 不依赖 vello ✓。
3. **`Kind::Image` 暂不绘制**（需要 `ImageFit` + 圆角裁剪的像素级 blit，约 130 行，随 M5 一起做）。
4. 组件的**默认视觉**（按钮灰底 + hover/pressed、checkbox 18×18、slider 140×20、progress 120×8）
   暂时写在 `view.rs` 的 DSL 里；M5 收敛到主题（`Theme` 作为 `Track`/`Renderer` 的字段）。
5. `widgets/` 目前只有**绘制**；行为（`handle`）仍在 M5。

### 当前规模（含单测）

`track 1300 / app 1160 / event 801 / view 790 / align 647 / input 577 / raster 560 / scene 700 /
layout 459 / cmd 447 / reactive 396 / widgets 560 / hit 311 / style 298 / transform 207 /
focus 193 / render/mod 110 / lib 84 / window 20` ≈ **9.6k 行**（其中约 3.6k 是单测）⇒ 有效实现约 6k 行。

对照设计预估（`lieui` 约 6k + 复用 3k）：基本吻合。M1~M3 已把"逻辑 + 布局 + 渲染"三层打通，
剩下 M4（winit/IME/拖拽/关闭守卫/上屏）与 M5（组件行为 + 自定义逃生舱）。

---

## 2026-10-01 · M4：winit 事件循环 + softbuffer 上屏 —— ✅ **真窗口跑通**

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `platform/mod.rs` | `Runner`（winit `ApplicationHandler`：建窗/事件翻译/帧循环/上屏）、`AppEvent`（Wake / External）、`RepaintHandle`（`EventLoopProxy` 包装，**不是全局单例**）、`pack_xrgb`（premul→`0x00RRGGBB`）、`softbuffer_damage`（逻辑脏区→物理 `softbuffer::Rect`） | 6 |
| `app.rs` | `drain_requests`（开/关窗队列 → 真实窗口同步）、`open_window_erased`、`App::run` / `run_with_handle`、`set_scale_factor` / `physical_size`、`ExternalData` 改 `Send` | 3 |
| `reactive.rs` | `RequestQueue`（类型擦除的待处理请求槽）+ `Runtime::requests()` | — |
| `render/` | DPI 支持：`Rasterizer{logical, scale}`，物理 pixmap = 逻辑 × scale，批次变换里组合 `scale`、脏区先乘 `scale` | — |
| Cargo | winit/softbuffer 设为**可选特性**（`default = ["winit"]`）⇒ `--no-default-features` 可完全无头编译 | — |
| `examples/window_counter.rs` | 真窗口示例：点击 +1、Tab 切焦点、后台线程每秒投递数据 | — |
| `lieui-text` | 文本测试：拉丁/中文换行、泰语不换行的 characterization、UTF-8 边界 | 5 |

### 验证

`cargo test --workspace`：**209 passed / 0 failed**（`lieui` 200 + `lieui-text` 5 + `lieui-layout` 4），**0 warning**。
`cargo build -p lieui --all-targets`：干净。`cargo build -p lieui --no-default-features`：**通过**（核心不依赖窗口系统）。

**真窗口运行证据**（`LIEUI_TRACE=1` + `examples/window_counter.exe`，运行 5 秒后手动结束）：

```
[lieui] WindowId(1) view=true  align.patched=0 layout[边界=1 移动=5] paint=153600px batches=1 present=true
[lieui] WindowId(1) view=false align.patched=0 layout[边界=1 移动=0] paint=153600px batches=1 present=true
[lieui] WindowId(1) view=true  align.patched=1 layout[边界=1 移动=0] paint=3120px   batches=1 present=true
[lieui] WindowId(1) view=true  align.patched=1 layout[边界=1 移动=0] paint=3120px   batches=1 present=true
```

- 第一帧：整窗 480×320 = **153600px**（首次必须全画）；第二帧重画一次是因为窗口管理器随后报了真实尺寸/DPI（`Resized` ⇒ 整窗脏）。
- 之后每秒钟：`view=true`（后台线程投递了数据）+ `align.patched=1`（只有一个文本节点变）+ **`paint=3120px`（整窗的 2%）** ⇒ **脏区 CPU 渲染在真窗口里生效**。窗口持续出帧 6 秒无崩溃。

我没有截图能力（无法"看到"窗口），所以 M4 的证据是"**进程活着 + 帧统计**"，像素正确性由 M3 的单测覆盖。

### 本轮踩到的真问题（前两个只有真跑才会暴露）

1. **softbuffer 的 surface 建出来是 0×0，必须先 `resize` 才能 `buffer_mut()`** ——
   我原先把 `WinSurface.size` 初始化成窗口尺寸（其实是谎话，surface 并没 resize 过），
   首次上屏直接 panic：`Must set size of surface before calling buffer_mut()`。改成 `Option<(u32,u32)>`，
   初值 `None` ⇒ 首次必 resize ✓。
2. **softbuffer `Buffer` 的像素访问是 `DerefMut<Target = [u32]>`**，没有 `pixels_mut()` 方法；
   像素格式是 `0x00RRGGBB`（**忽略 alpha**）⇒ 需要从我们的 premultiplied pixmap **反预乘**再打包
   （`a==255` 走直通、`a==0` 视为背景，避免除零）。
3. **DPI 的坐标分工**：布局/命中吃**逻辑**像素，pixmap 与 softbuffer surface 吃**物理**像素
   （逻辑 × scale）。所以 winit 的光标位置进 `InputEvent` 前要除以 scale；
   `ScaleFactorChanged` 只重建 pixmap、**不重排**（有测试断言 `layout.ran == false`）。
4. **`Signal` 是 `!Send`，跨线程只能"投递 + 唤醒"**：后台线程用 `RepaintHandle::post_external(window, data)`，
   数据经 `AppEvent` 回到 UI 线程后由 `on_external` 落到 `Signal`。因此 `ExternalData` 必须是 `Send`（已改）。
   另外 `EventLoopProxy` 只能由 `EventLoop` 创建 ⇒ 句柄只能在 `run` 之前拿到，
   所以加了 `App::run_with_handle(on_ready)`（而不是回调进 VM 构造函数）。
5. **`Ctx` 要能请求开窗/关窗，但不能反向依赖 `app` 层**：加了一个**类型擦除**的 `RequestQueue`
   （`push<T>` / `take<T>`），`Ctx::request(payload)` 写入、`App::drain_requests()` 取出并解释。
   好处：无窗口环境（单测）也能验证动态开关窗 —— 有两条测试守它。
6. **parley 的换行不消费"复杂脚本分词"**：泰语（无空格 + 无 UAX#14 断点）不会自动换行。
   我一度通过"特性合并"打开了 `icu_segmenter` 的 `auto`/`lstm`（`cargo tree` 确认已开），
   但换行行为**没变** ⇒ 那条额外依赖没有收益，**已撤销**。现状写成 characterization 测试：
   泰语不换行（变通：插 `\u{200B}`，测试里也验了这条路）；中文照常换行（UAX#14 允许汉字间断）。
7. **中日韩的 `No segmentation model for complex script: Chinese/Japanese` 提示**：缺 `cjdict` 词典数据
   （不在 `compiled_data` 里）。对 UI 换行**无影响**（汉字间可断），属已知的 cosmetic 提示。

### 与设计文档的偏差 / 顺延

1. **剪贴板（`arboard`）与 IME 文本的"消费方"顺延到 M5**：本层已把 `Ime::Commit` 翻成逐字符的
   `CharacterReceived`、`Preedit` 翻成 `TextCompositionChanged` 并派发到焦点链 —— 但真正"吃掉"这些事件的
   是 Input 组件（M5）。没有消费方时把剪贴板接进来无法验证，故一并顺延。
2. **层锚点定位（popup 跟随锚点 / 视口不足翻转）仍未做**：这是 M2 顺延下来的（`Anchor` + `Placement` 已在
   `Track`/`LayerOpts` 里，缺的是"布局后解析锚点 key 的 rect 并把整棵层子树平移"的那一步 + "最多两轮"）。
   它属于布局范畴而非窗口层，和 M5 的 Menu/Popup 一起做更自然。
3. **拖拽（DnD 源侧与目标侧）**：`can_drag`/`allow_drop`/`Drag*` 事件位已在，行为留 M5。
4. `winit` 作为默认特性：核心 crate 可在没有窗口系统时编译（CI 友好），这是设计稿没写但值得的做法。

### 当前规模（含单测）

`track 1300 / app 1330 / event 810 / view 790 / scene 721 / align 647 / platform 780 / raster 640 /
input 577 / widgets 548 / cmd 447 / layout 459 / reactive 460 / hit 311 / style 298 / transform 207 /
focus 193 / render/mod 130 / lib 95 / window 20` ≈ **10.8k 行**（含约 4k 单测）⇒ 有效实现约 6.8k 行。

### 下一步：M5（组件行为 + 自定义逃生舱）

- `widgets::handle(track, id, ev, cmd)`：内置交互（Input 编辑/IME/选区、Slider 拖拽、Scroll 滚动条、
  Checkbox/Radio/Switch 的点击、Menu 开合）；
- 层锚点定位（popup 跟随锚点 + 视口翻转）；
- `Kind::Custom` + `CustomNode`（用户扩展逃生舱）；
- 剪贴板（`Ctrl+C/V`）与 IME 的真实消费者；
- 主题（token 结构 → `Renderer`/`Track` 字段，去掉散落在 DSL 里的默认视觉）。

---

## 2026-10-01 · M5-A：内置行为 + 双向绑定 —— ✅ 机制与两个组件落地

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `widgets/mod.rs` | **内置行为分派**：`handle_route`（按路由策略沿命中链跑）+ `handle`（按 `Kind` 枚举分派）+ Slider（按下取捕获 / 拖拽改值 / 松开清态）+ Checkbox（`Tapped` 翻转） | — |
| `track.rs` | `Bindings`（`checked` / `value` / `text` 三个绑定槽）+ `Node.bindings`；`find_by_key`（锚点用）；`slider_drag_to` / `toggle_checked` / `set_dragging`；`KindDesc::Slider { value, min, max }` | — |
| `view.rs` | `slider_range` / `slider_bound` / `checkbox_bound`（≈ `v-model`）；`DescNode.bindings` | — |
| `align.rs` | 绑定随描述覆盖到节点（**不置脏**） | — |
| `app.rs` | `WindowCtx::dispatch` 变成三段：**① 内置行为（直接 `&mut Track`）→ ② 用户处理器（`&mut Ctx`）→ ③ 落 `Cmd`** | 6 |
| `examples/window_counter.rs` | 加了一个绑定滑块 + 绑定复选框（真窗口里演示双向绑定） | — |

### 验证

`cargo test --workspace`：**215 passed / 0 failed**（`lieui` 206 + `lieui-text` 5 + `lieui-layout` 4），**0 warning**；
`--all-targets` 干净；真窗口示例（含绑定控件）跑 4 秒无崩溃：首帧 `移动=8`，之后每秒只重绘 3120px（整窗 2%）。

M5-A 的验收测试（都在 `app.rs`，走完整链路：`view()` → 对齐 → 布局 → 命中 → 分发 → 内置行为 → `Signal`）：

- `slider_drag_writes_back_the_bound_signal`：拖到中点 ⇒ `Signal<f32>` = 5.0；越界拖到右端外 ⇒ 钳到 `max`；
  拖拽期间**指针捕获**在滑块上；松开后捕获释放、`dragging` 归位。
- `slider_drag_and_the_next_view_do_not_fight`：拖完下一帧 `align.patched == 0` ——
  证明"立即写节点 + 写 signal"两条路**不打架**（这是双向绑定最容易踩的坑：值回弹）。
- `unbound_slider_ignores_the_pointer` / `unbound_checkbox_is_left_to_user_handlers`：
  **只在有绑定时**框架才接管（≈ `v-model` 语义）；未绑定时 `desc` 仍是唯一真相，用户处理器照常收到 `Tapped`。
- `bound_checkbox_toggles_on_tap`：`Tapped` ⇒ 框架翻转绑定的 signal，再点一次翻回来。
- `disabled_slider_ignores_the_pointer`：`enabled(false)` 时内置行为整体不执行。

### 两个设计决定（都写进了代码注释）

1. **内置行为不走 `HandlerSlot` 闭包，而是 `dispatch` 里独立的一遍**。
   M1 时我按 `handledEventsToo` 把内置行为做成了"标记位 + 闭包槽"，但 M5 的内置行为（滑块拖拽、
   文本编辑）必须**直接访问保留树**（算几何、改视图态），而闭包只拿得到 `&mut Ctx`。
   于是按设计稿 §3.5 的原样恢复成"内置行为先跑一遍"：`dispatch` = 内置行为（`&mut Track`）→
   用户处理器（`&mut Ctx`）→ 落 `Cmd`。好处：组件行为写起来是普通函数（`fn slider_handle(track, id, ev, cmd)`），
   不需要为每个行为包一层闭包，也不需要新的 `Cmd` 变体来绕路。
   `HandlerSlot.handled_events_too` 仍保留（用户侧要用"即使已处理也调用"时有用）。
2. **"能改模型"的内置行为只在有绑定时激活**。`slider(0.5)` 是"显示这个值"；`slider_bound(&sig)` 才是
   "能拖且改模型"。这不是洁癖：若允许拖未绑定的滑块，就会得到一个"**下次 `view()` 才回弹**"的中间态，
   而它何时回弹取决于"下一次 `view()` 何时发生"——不可预测。测试把这条规则钉住了。

### 顺带的结构变更

`KindDesc::Slider { value, min, max }`（原来固定 0..1 映射，`min/max` 现在属于**数据**）、
`Kind::Slider { value, min, max, dragging }`（`dragging` 仍属视图态）。`apply_to` 会同时比较三者。

### 当前规模

`lieui` 约 **11.3k 行**（含约 4.4k 单测）⇒ 有效实现约 6.9k 行。

### M5 剩余（下一段）

- ~~**Input 组件**~~ → ✅ 已完成（见下一段）
- **滚动条**（`show_scrollbar` 的 thumb 绘制 + 拖动滚动；滚轮滚动 M2 已有）；
- **层锚点定位**（popup 跟随锚点 + 视口不足翻转；`Anchor`/`Placement`/`find_by_key` 都已就位，缺"布局后解析并平移整棵层子树"这一步 + 最多两轮）；
- **`Kind::Custom` + `CustomNode`**（用户扩展逃生舱）；
- **主题**（token → `Renderer`/`Track` 字段）。

---

## 2026-10-01 · M5 第二段：Input 组件 —— ✅ 输入闭环完成

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `track.rs` | `Kind::Input { text, placeholder, caret, anchor, preedit }`（desc/state 划分见架构文档）+ `KindDesc::Input` + 编辑 API（`input_insert` / `input_backspace` / `input_delete` / `input_move_caret` / `input_set_caret` / `input_select_all` / `input_set_preedit` / `input_selection` / `input_selected_text`，全部按 char 边界） | 8 |
| `event.rs` | `Modifiers`（SHIFT/CTRL/ALT/META 手写位集）+ `Event::Key.modifiers` + `EventView.modifiers` + `Event::char_received(ch)`（IME 提交也走这条） | 1 |
| `widgets/mod.rs` | `input_handle`（点击聚焦+置光标 / 拖选 / 方向键·退格·删除·Home·End / Ctrl+A / 字符插入 / IME preedit / 失焦清组合串）+ `input_draw`（正文或占位提示、选区高亮、光标、组合串下划线，文本走排版缓存） | 5 |
| `layout.rs` | Input 的固有尺寸：空文本时按 placeholder 测度（盒子不塌） | — |
| `view.rs` | `input(value)` / `input_bound(&Signal<String>)` / `.placeholder(s)`；默认 200×28、可 Tab 聚焦、白底圆角边框、超宽裁剪 | — |
| `platform/mod.rs` | 键事件带修饰键；**Ctrl+C/X/V 在平台层拦截**（`arboard` 作为 winit feature 的可选依赖，核心不碰 OS 服务） | — |
| `examples/gallery.rs` | 组件陈列馆：按钮 / 滑块 / 复选框 / 进度条 / **输入框+回显**，真窗口验证 4 秒不崩 | — |

### 验证

`cargo test --workspace`：**231 passed / 0 failed**（`lieui` 222 + layout smoke 4 + 其余 5），**0 warning**。

端到端测试（全部走真实输入路径：指针→焦点链、键盘→焦点链）：

- `typing_writes_back_to_the_bound_signal`：点进输入框打字 ⇒ `Signal` 实时更新，**下一帧零补丁**（光标不被值回弹打断）；
- `click_places_the_caret_at_the_clicked_glyph`：用 `TextEngine` 实测 "ab" 前缀宽度后点击 ⇒ 光标精确落在 2；插入 'X' 得 "abXcd"；
- `shift_arrows_select_and_typing_replaces_the_selection` / `ctrl_a_selects_all_...` / `dragging_extends_the_selection`（拖拽期间**指针被捕获**，松手释放）；
- `ime_preedit_then_commit_inserts_text`：预编辑只进组合串（文本/模型不动），提交后组合串清空；
- `losing_focus_clears_the_preedit` / `unfocused_input_ignores_characters` / `unbound_input_ignores_editing`（三条防串字/防回弹的护栏）；
- `model_change_replaces_the_buffer_and_puts_the_caret_at_the_end`（外部同步覆盖编辑缓冲）；
- `tab_moves_focus_away_from_an_input`（两个输入框 + Tab 迁移 + 分别写各自的 Signal）；
- 绘制：占位提示、光标（聚焦才画）、选区高亮、组合串下划线、**文本起点 == 左内边距**（钉住 `CSSDirection::Left == 0` 的索引约定）。

### 踩到的三个真问题

1. **`EventView` 放不下字符串**：IME 预编辑/提交带文本 payload，而 `EventView` 是 `Copy` 定长摘要。
   `widgets::handle_route` 改收 `&Event`（内置行为要什么拆什么，用户处理器仍拿 `Copy` 摘要）。
2. **IME 提交与组合串的衔接**：平台的 `Ime::Commit` 逐字符发 `CharacterReceived`，
   组合串还挂在 `preedit` 里 ⇒ `input_insert` 的第一个字符**先清组合串**再插入（组合串从未进过 `text`）。
3. **点击测试的语义**：先 Down 再 Move 不构成拖选——必须"按下未松手"（指针捕获中）才扩选；
   测试因此用 `Down → Move → Up` 三段模拟，而不是"点击 + 移动"。

### 已知欠账（诚实记录）

- **光标不闪烁**：闪烁需要"聚焦期间逐帧重绘"，与空闲帧零功耗冲突；随主题/动画一起做（聚焦才开 tick）。
- **文本超宽只裁剪不滚动**：需要 Input 内的水平 scroll offset + 光标随动，随 Scroll 组件补。
- 剪贴板写失败仅 `eprintln!`（无用户可见提示）；未做右键菜单。

### 当前规模

`lieui` 约 **12.4k 行**（含约 5.0k 单测）⇒ 有效实现约 7.4k 行。

### M5 剩余

- ~~**层锚点定位**~~ → ✅ 已完成（见下一段）
- **滚动条**（thumb 绘制 + 拖动滚动；滚轮滚动 M2 已有）；
- **`Kind::Custom` + `CustomNode`**（用户扩展逃生舱）；
- **主题**（token → `Renderer`/`Track` 字段）。

---

## 2026-10-01 · M5 第三段：层锚点定位 —— ✅ popup/tooltip 落位闭环

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `layout.rs` | `place_anchored_layers(track, window)`：布局**之后**解析每个带 `anchor` 的层根——按锚点 rect + `Placement` 求目标原点；默认侧放不下且另一侧放得下 ⇒ 翻转（Below↔Above / LeftOf↔RightOf，一次决策）；仍放不下 ⇒ 钳到视口内（`Fixed` 除外）；平移整棵层子树（`descendants` 含自身），旧 ∪ 新矩形登记脏区 | 9 |
| `layout.rs`（引擎接口） | **锚定层根用未定义可用空间布局**（`VALUE_UNDEFINED` ⇒ Taitank 收缩到内容）。此前层根作为根节点被拉伸成整窗，"锚点定位"无从谈起 | — |
| `align.rs` | **修复**：新建层根时带上描述里的 `LayerOpts`（`add_root` 只给默认 opts ⇒ 锚点/backdrop 曾被丢掉） | — |
| `app.rs` | 帧驱动 ②.5 步：布局后落位（在 `take_damage` 之前 ⇒ 挪层本帧就被重绘）；`FrameStats.anchored_layers` | 1（端到端） |

### 验证

`cargo test --workspace`：**241 passed / 0 failed**，0 warning；gallery 冒烟通过。

单元测试（`layout.rs`）：Below 留 4px 间距且左对齐、内容自适应（不再填满整窗）、下方放不下翻转到上方、两侧都放不下钳到视口内、右缘 RightOf 翻 LeftOf、ScreenCenter 居中、锚点 key 失效保持原位（moved=0）、`Fixed` 按字面落位、挪层登记旧 ∪ 新脏区。

端到端（`app.rs`）：`popup_opens_on_tap_and_follows_the_anchor` —— 点按钮 ⇒ popup 层出现且在按钮下方留间距、左对齐；再点 ⇒ 层消失。整条链：tap → `Signal` → `view()` 声明层 → `align` 建根（带 opts）→ 布局（内容自适应）→ 落位。

### 踩到的三个真问题（都修了）

1. **`align` 新建层根时丢 `LayerOpts`**：`add_root` 用 `LayerOpts::for_layer(layer)` 的默认值，
   描述里的 `anchor` / `backdrop` / 命中策略全被丢掉。表现：popup 根被当普通根拉伸成整窗、
   `place_anchored_layers` 拿不到锚点。已有根的 opts 变更有同步路径（`opts_changed`），唯独**新建**漏了。
2. **锚定层根会被拉伸成整窗**：根节点以窗口为可用空间，Taitank 对未定义宽高做"填满"。
   解法：锚定根用 `VALUE_UNDEFINED` 可用空间布局 ⇒ 引擎收缩到内容（显式定宽的层不受影响）。
3. **`descendants` 含自身**：`collect_subtree` 先 push 自身（destroy 的语义）。
   平移子树时我又 `once(node).chain(descendants)` ⇒ 根被平移两次（探针里 y=68=2×34 暴露）。
   顺便验证了探针驱动调试的价值：三个问题里两个是探针一眼看出来的。

### 设计取舍

- **翻转只做一次决策 + 钳制**，不做"最多两轮"的全局 pass：两侧都放不下时钳到视口贴边，
  结果等价于两轮收敛，但少一次遍历；"防止抖动"由"放不下**且**另一侧放得下才翻"保证。
- **落位放在帧驱动的布局之后**（而不是布局函数内部）：锚点 rect 属于别的子树，
  布局按边界集合处理时序不保证锚点先算完；帧级别统一做一次是确定性最高的位置。

### 当前规模

`lieui` 约 **12.7k 行**（含约 5.2k 单测）⇒ 有效实现约 7.5k 行。

### M5 剩余

- ~~**主题**~~ → ✅ 已完成（见下一段）—— **M5 全部完成**。

---

## 2026-10-01 · M5 第六段：主题 —— ✅ M5 完成

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `theme.rs`（新） | `Theme`（17 个语义 token）+ `light()` / `dark()` 预设 | 2 |
| `reactive.rs` | `Runtime.theme()` / `set_theme(t)`（同值 no-op；切换 ⇒ 所有窗口 `VIEW|PAINT|PRESENT`，即"重跑 view + 整窗重绘"——§3.10 原文方案） | — |
| `view.rs` | `ViewBuf` 携带主题快照（`frame()` 注入）；DSL 烘焙：`text`/`button` 文字色 = `theme.text`、按钮底/hover/pressed/圆角、输入框底/边框/圆角 | — |
| `render/scene.rs` + `widgets` | `SceneOptions.theme` 快照 → `widgets::draw` 全部绘制期颜色走 token（滑块轨道/选中/光标/占位提示/滚动条/未覆盖 accent 兜底）；散落的 5 组颜色常量全部删除 | — |
| `app.rs` | `WindowConfig.background` 改 `Option<Color>`：显式设置优先于主题；未设置则跟随 `theme.window_background`（主题切换时同步） | 2 |

### 设计对齐（§3.10 逐条）

- ✅ `Theme` 是 Runtime 的普通字段，**0 个 thread_local**；
- ✅ widget 构造时把 token 烘焙进节点样式；切换主题 = `view()` 重跑 + 全屏 `PAINT`；
- ✅ 无属性表/继承（拒绝 `PropertyStore` 路线）。
- 唯一的补充：绘制期颜色（光标/选区/滚动条）不经过 `view()`，走 `SceneOptions.theme` 快照
  ——否则它们没法随主题变（这些状态本就不在描述里）。

### 测试钉住的语义

- 像素级：浅色下按钮底 = `light.control`；`set_theme(dark)` 后下一帧 = `dark.control`，且 `view_ran == true`；
- **同值切换是 no-op**（不重跑 view、不重绘）；
- 显式 `WindowConfig.background` 在主题切换后**保持不变**（用户选择优先）。

### 当前规模

`lieui` 约 **13.7k 行**（含约 5.7k 单测）⇒ 有效实现约 8.0k 行。

---

## 2026-10-01 · M5 里程碑达成

M5 六段全部完成：滑块/复选框/进度条（绑定）→ **输入框**（编辑/IME/选区/剪贴板）→ **层锚点定位** →
**`Kind::Custom` 扩展点** → **滚动条** → **主题**。功能对齐 + 扩展点可用的验收目标达成：

- `255 passed / 0 failed`，0 warning（`lieui` 246 + layout smoke 4 + text 5）；
- 交互组件全部有像素级/端到端测试（真窗口冒烟：counter + gallery）；
- 双向绑定、指针捕获、焦点链、IME、锚定浮层、自绘扩展、滚动、明暗主题——
  WinUI 日常 UI 的核心交互面在 v3 架构上全部落地。

---

## 2026-10-01 · M6 修复：滚动平移 / 弹层透明 / hover / 居中 —— ✅ 陈列馆反馈修复

用户跑陈列馆反馈的四个问题，全部定位并修复：

### 1. 滚动条动了、内容没动（真 bug，根因修复）

**根因**：子原点的 `-offset` 平移是**布局时烘焙**的（`write_back`），而 `set_scroll_offset`
只标 PAINT ⇒ 滚动后布局不重跑 ⇒ 子节点停留在旧位置。
**修复**：`set_scroll_offset` 改标 LAYOUT（容器是尺寸确定的重排边界 ⇒ 其子树按新偏移
整体重新平移）。旧测试 `scroll_offset_change_marks_paint_only` 的断言按新语义更新，
并新增两个回归测试：偏移变化 ⇒ 子节点 y 平移 -offset；滚轮后内容真的上移。
（已知代价：滚动事件触发滚动子树的重排——对虚拟列表是 ~13 节点，可接受；
绘制期平移是更优解，留待滚动架构演进。）

### 2. 菜单/下拉弹层是透明的（真 bug）

`popup_at`/`tooltip_at` 的层根是无视觉的 Box——弹层内容直接叠在下层内容上。
**修复**：`layer()` 给 Popup/Tooltip 层根烘焙默认视觉（主题 `input_background` 底 +
边框 + 投影 + 6px 内边距）。像素测试：弹层内边距区域 = `input_background`（不透明）。

### 3. hover 看不到显示（部分是缺视觉，机制本身正常）

机制正常（`set_pointer_over` 对声明了交互视觉的节点标脏，按钮有 hover 色并新增
**像素级回归测试**：hover 后按钮 = `control_hover`）。真正缺的是**弹层菜单项**没有
hover 反馈——已给 gallery 的菜单项/下拉项加 `hover_background` + 圆角。

### 4. 陈列馆内容居中

滚动主体改为 `align_items(Center)` + 固定 360px 内容列；分隔线随列拉伸；图片给显式尺寸。

### 验证

274 tests 全绿 0 warning；gallery 冒烟通过。
另：一次 link 1104（gallery.exe 被残留进程占用）——杀进程即恢复，与代码无关。

### M6 方向（全部完成）

1. ~~**Switch/Radio**~~ → ✅（见上）
2. ~~**ComboBox（含轻关闭接线）**~~ → ✅（见上）
3. ~~**MenuBar（嵌套弹层）**~~ → ✅（见上）
4. ~~**VirtualList（含 ScrollChanged 接线）**~~ → ✅（见上）
5. ~~**组件陈列馆 + Image 绘制**~~ → ✅（见上）
6. ~~**光标闪烁 + 输入框水平滚动 + custom_mut 了结**~~ → ✅ 已完成（见下一段）—— **M6 全部完成**

---

## 2026-10-01 · M6 第六段：光标闪烁 + 水平滚动 + custom_mut 了结 —— ✅ M6 完成

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `app.rs` + `platform/mod.rs` | **光标闪烁**：`WindowCtx::animate(now)`（有键盘聚焦的输入框才翻相位并标脏）+ `next_wakeup()`（平台层据此 `ControlFlow::WaitUntil`）+ 打字重置相位（输入期间常亮）。**聚焦才动，其余零功耗**——没有聚焦输入框的窗口永远是纯 Wait | 1 |
| `track.rs` + `widgets` | `Kind::Input.scroll`（水平滚动视图态）+ `input_ensure_caret_visible`（前缀实测宽 ⇒ `scroll ∈ [caret_x - inner, caret_x]`，接入插入/删除/光标移动/全选）+ 绘制应用偏移 + 点击映射补偿滚动 | 1 |
| `widgets` | 光标绘制门控 `track.blink_on`（闪烁相位；相位为灭时只省一个 1.5px 矩形，成本可忽略） | — |

### `custom_mut` 欠账的了结（设计决定，非实现）

设计稿的 `Ctx::custom_mut(key)` 逃生舱**不需要了**：`v.custom(&cell)` 要求 ViewModel 持有
`CustomCell` —— 实例本来就在用户手里，直接改自己的 `RefCell` 再 `cx.request_repaint()`
（`on_event` 里则 `cmd.damage(id)`）即可。Ctx 不持树的设计反而让逃生舱多余，据此关闭
（架构文档已更新）。

### 测试钉住的语义

- `caret_blinks_only_while_an_input_is_focused`：未聚焦 ⇒ animate no-op；聚焦 ⇒ 常亮起步；
  静止一个周期翻转（亮→灭→亮）；未到周期不动；
- `input_horizontally_scrolls_to_keep_the_caret_visible`：打 40 字符（超宽）⇒ scroll > 0
  且光标可见；Home ⇒ 光标 0、滚动归 0。

### 当前规模

`lieui` 约 **15.0k 行**（含约 6.4k 单测）⇒ 有效实现约 8.6k 行。

---

## 2026-10-01 · M6 里程碑达成

M6 六段全部完成：Switch/Radio → ComboBox（轻关闭）→ MenuBar（嵌套弹层）→ VirtualList
（ScrollChanged）→ **组件陈列馆**（全量 gallery + Image 绘制）→ **光标闪烁 + 水平滚动**。

`cargo run -p lieui --example gallery` 可交互体验全部组件。v3 至此覆盖 WinUI 日常 UI
的完整交互面：声明式响应、双向绑定、指针/键盘/IME、焦点链、层与弹层、虚拟化列表、
自绘扩展、明暗主题——且每个能力都有像素级或端到端测试钉住。

---

## 2026-10-01 · M6 第五段：组件陈列馆 —— ✅ 全部 widget 可展示

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `render/scene.rs` + `raster.rs` | **补上 `Kind::Image` 的绘制**（M3 起悬置的欠账）：新 `Op::Image` + 光栅层手动 blit（RGBA8 直通 alpha → premultiplied SrcOver、contain 等比缩放、最近邻采样、按批次裁剪）。已知限制：blit 在批次内 vello 原语之后 ⇒ 同批次内图片总在最上层（对陈列馆无影响，记录在案） | 1（像素级） |
| `examples/gallery.rs` | **全量重做**：单一内容根 = 固定菜单栏 + 可滚动主体（解决了"内容根只能声明一次"的约束），12 类组件全部分区展示 | — |

### 陈列馆清单（全部真实交互）

菜单栏（文件/编辑 + 轻关闭 + 声明式互斥）｜按钮（计数）｜**主题切换**（明/暗）｜
滑块+进度条（绑定+随动）｜复选框+开关（**共享同一个 Signal**）｜单选组｜
输入框（IME/剪贴板/选区）｜下拉选择（锚定+轻关闭）｜嵌套滚动区（滚轮+滚动条）｜
自绘波形（CustomNode）｜图片（程序生成渐变）｜Tab 焦点迁移。

### 发现的约束（已写进 DSL 文档）

**内容根只能声明一次**：`view()` 顶层第二个声明会 panic（提示改用层）。
页头固定 + 主体滚动的正确姿势是外层 column 包裹——这本来就该写进入门文档。

### 当前规模

`lieui` 约 **14.9k 行**（含约 6.3k 单测）⇒ 有效实现约 8.6k 行。

---

## 2026-10-01 · M6 第四段：VirtualList —— ✅ 滚动事件接线 + 虚拟化组合

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `event.rs` | `Event::Scroll { offset }` 变体 + `EventView.scroll` 字段 + `Ctx::scroll_offset()` —— `ScrollChanged` 终于有了 payload（又是"定义了但没人派发"的欠账，本次接线） | — |
| `track.rs` | `set_scroll_offset` 变化时标记 `needs_scroll_event` + `take_scroll_changes()`（Track 无事件队列只排队，派发归 app 层） | — |
| `app.rs` | 帧驱动 ②.6 步：布局后派发 `ScrollChanged`（Direct，发给容器自身） | 2 |
| `app.rs`（组合配方） | `virtual_list(v, first, count, item_h, viewport_h, label)`：顶部占位 + 可见行 + 底部占位（总高恒定 ⇒ 滚动条诚实）；`ScrollChanged` 把首行写回 signal ⇒ view 重跑换窗 | — |

### 原理（无新机制，两件事组合）

1. **`ScrollChanged` → 信号**：容器挂处理器，把新偏移换算成"窗口首行"写回 signal；
2. **view 只渲染可见窗口**：`first..first+visible` 的行 + 上下占位（占位高 = 行数 × 行高）。
   滚动位置在保留树里（视图态），跨帧不丢；换窗时对齐器就地更新行文本（零重建）。
   派发时序：布局后派发 ⇒ 信号标记 VIEW ⇒ **下一帧** view 重跑（与 React 的 setState 一帧延迟同构）。

### 测试钉住的语义（1000 行 × 24px，视口 240）

- `virtual_list_materializes_only_the_visible_window`：**1000 行只物化 11 行**（Item 0..10）；
  内容尺寸 = 24000（滚动条诚实）；滚 240 ⇒ 下一帧窗口变为 Item 10..20，节点数不变；
  滚动位置与信号一致、不回弹；
- `scrolling_back_reveals_earlier_items`：直接滚到最深 ⇒ 最后一行是 Item 999。

### 踩坑记录

- 又一次"声明了但从未派发"（`ScrollChanged` 与上段的 `Dismissed` 同款欠账）——
  设计稿里的事件清单要在接线时逐个核对；
- 调试时浪费一轮：测试 helper 假设"滚动容器是 content_root 的 children[0]"，
  而当 `v.scroll` 在 view 顶层声明时**它就是 content_root 本身**。探针打印树结构一次定位。

### 当前规模

`lieui` 约 **14.8k 行**（含约 6.2k 单测）⇒ 有效实现约 8.6k 行。

---

## 2026-10-01 · M6 第三段：MenuBar（嵌套弹层）—— ✅

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `app.rs`（测试承载的组合配方） | 菜单条 = 一排锚点按钮 + `open_menu: Signal<String>` 声明式互斥（同 Radio 原理）+ 每个菜单一个 `popup_at` 弹层 + **子菜单**（锚点在弹层内、RightOf）——验证嵌套弹层 | 3 |

### 组合配方的三个要点（都是可复用经验）

1. **互斥零机制**：`open_menu` 是唯一真相；点另一个菜单按钮 ⇒ 信号变 ⇒ 旧弹层消失、新弹层出现。
   弹层甚至复用同一个节点（对齐器按 (layer, ordinal) 匹配，就地换内容与锚点）。
2. **轻关闭守卫**："只关自己"——`Dismissed` 处理器先比对 `open_menu` 再清空，
   否则点"编辑"打开时，"文件"弹层的 dismiss 会把刚写入的信号清掉（次序竞态的声明式解法）。
3. **子层级**：子菜单锚点（`item-查找`）在父弹层内，`find_by_key` 跨层解析 + `RightOf` 落位
   直接工作，无任何嵌套特殊处理——`place_anchored_layers` 的"按 key 解析"设计得到验证。

### 测试钉住的语义

- `menu_bar_opens_one_menu_at_a_time_and_reanchors`：文件→编辑直接切换（不闪关）；
  弹层**跨帧重新锚定**到编辑按钮下方（复用节点 + 锚点跟随）；
- `menu_item_fires_and_submenu_nests_to_the_right`：项触发并关闭；子菜单出现在锚点项
  **右侧 4px、顶对齐**；点子菜单项全部关闭（含子菜单状态）；
- `clicking_outside_closes_the_open_menu`：点外部轻关闭。

### 踩到的测试问题（非框架 bug）

菜单条按钮被 flex 拉伸到整窗高（行没有固定高）——菜单条配方需要 `r.height(28.0)`。
这暴露了"根容器默认填满、按钮在无界行里跟随拉伸"的 flex 语义，文档配方里要写明。

### 当前规模

`lieui` 约 **14.4k 行**（含约 6.1k 单测）。

---

## 2026-10-01 · M6 第二段：ComboBox + 轻关闭接线 —— ✅

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `input.rs` | **补上从未接线的 `dismiss_on_outside_click`**：点击落在弹层子树之外 ⇒ 给层根发 `Dismissed`（Direct）。**排在 Tapped 之后**——锚点 toggle（关）与 dismiss（关）幂等一致，不会"先关又被锚点重开" | （随端到端覆盖） |
| `view.rs` | `ViewBuf::on(kind, f)`：给当前容器/层根挂处理器（弹层根的 Dismissed 处理器用） | — |
| `examples/gallery.rs` | 下拉选择演示（锚定按钮 + 弹层三个选项，选中即回填并关闭） | — |

### ComboBox = A 档组合函数（零新机制）

`combo(vm, v, anchor_key, options)`：锚定按钮（`.key()`）+ `popup_at(key, Below, ..)` +
选项行（`on_tap` 写回选择并翻 open）+ 层根 `Dismissed` 处理器。**这正是设计 §3.12 的
"A. 组合覆盖 ~90%"**——下拉菜单不需要成为 `Kind`，层锚点 + 轻关闭 + 声明式 view 组合即可。

### 端到端测试钉住的语义

- `combo_dropdown_opens_selects_and_light_dismisses`：
  ① 点锚点 ⇒ 弹层出现（锚点下方 4px、左对齐，三个选项）；
  ② 点"香蕉" ⇒ `choice` 更新 + 弹层消失（选项自己翻 open）；
  ③ 再展开，点弹层之外 ⇒ `Dismissed` ⇒ 关闭；
  ④ 锚点在关闭态再点 ⇒ 重新展开（toggle 与 dismiss 不打架）。
- `anchor_click_while_open_closes_the_dropdown`：**开着时再点锚点 = 关闭**（Tapped 的 toggle
  先执行 → false；随后的 Dismissed 再置 false，幂等）。这是 light-dismiss 最容易出错的次序问题，
  靠"dismiss 排在 tapped 之后"一次排对。

### 发现的真缺口

`LayerOpts.dismiss_on_outside_click` 自 M2 声明以来**从未被消费**——`Dismissed` 事件定义了、
路由策略定了，但没有任何代码派发它。本次接线是它的第一个真实消费者（ComboBox）。

### 当前规模

`lieui` 约 **14.3k 行**（含约 6.0k 单测）。

---

## 2026-10-01 · M6 第一段：Switch / Radio —— ✅ 机制可组合性验证

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `track.rs` | `Kind::Switch { on }` / `Kind::Radio { selected, value }`（纯 desc，无 state 组——交互全走"写 signal ⇒ view 重跑"）+ `toggle_switch` / `select_radio` | — |
| `widgets` | `switch_handle`（点击翻转 + 写 `checked` 绑定）/ `radio_handle`（点击写 `text` 绑定）+ 绘制（药丸+thumb / 圆环+圆点，全走主题色，on/off 与选中态用 accent） | 2 |
| `view.rs` | `switch` / `switch_bound(&Signal<bool>)` / `radio` / `radio_bound(&Signal<String>, value)`（可 Tab 聚焦） | — |
| `examples/gallery.rs` | 新增：开关与复选框**共享同一个 Signal**（两个控件互相同步——单一真相的直接演示）+ 单选组 | — |

### 关键设计：单选互斥零机制

Radio 的组互斥**不遍历兄弟、不发命令**：点击只把本项 `value` 写回组 `Signal<String>`，
信号变化 ⇒ `view()` 重跑 ⇒ 同组所有 radio 的 `selected` 由对齐器自然更新。
这是"状态是唯一真相 + 声明式 view"架构的自然推论——WinUI 需要 RadioButton 事件互踩的逻辑
在 v3 里**不存在**。复选框与开关绑定同一个 Signal 能互相同步，是同一原理的另一个演示。

### 测试钉住的语义

- `switch_bound_toggles_the_signal_and_the_desc_follows`：点击写回 signal、desc 跟上零回弹；未绑定的只显示形态点不变；
- `radio_group_is_mutually_exclusive_via_the_view_rerun`：点 green ⇒ signal="green" ⇒ 下一帧 red=false/green=true（**框架没有一行互斥代码**）；点回 red 同样只需一次写入；
- `unbound_radio_ignores_taps`（"能改模型"只在有绑定时激活的规则继续成立）；
- 绘制：on 轨道 = accent / off = 边框色；选中才有圆点。

### 当前规模

`lieui` 约 **14.0k 行**（含约 5.8k 单测）。

---

## 2026-10-01 · M5 第五段：滚动条 —— ✅ thumb 绘制 + 拖动滚动

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `widgets/mod.rs` | `vscroll_parts` / `hscroll_parts`（几何：比例 thumb、最小长度 24、右/下缘内缩 2px）+ `scroll_handle`（按下抓 thumb → `capture` → 移动按比例映射 → 松手释放，**抓取点不跳**）+ `draw_scrollbars`（半透明圆角 thumb，拖拽时加深）+ 命中带外扩 6px（触摸容差） | 4 |
| `track.rs` | `Node.scroll_drag: Option<ScrollDrag>`（拖拽会话：指针 id / 轴向 / 抓取偏移） | — |
| `examples/gallery.rs` | 新增滚动演示区（12 行内容，滚轮 + 拖动右侧滚动条） | — |

### 设计要点

1. **滚动条是"虚拟部件"**：不占节点、不进对齐器，绘制挂在滚动容器的 draw 里、
   行为挂在容器的 `handle` 里（`overflow_scroll && show_scrollbar` 才激活）。
   理由：thumb 不是可聚焦/可对齐的元素，做成节点会污染 `view()` 描述与命中语义（WinUI 也是模板部件而非元素）。
2. **映射公式**：thumb 行程 → 滚动量的比例 = `max_scroll / (track_len - thumb_len)`；
   按下时记录"抓取点到 thumb 缘的距离"，拖动时按抓取点反推 thumb 位置 ⇒ 抓哪都不跳。
3. **滚轮**：M2 的 `default_wheel_scroll` 原样复用（含"到边界交给上层容器"的滚动链）。

### 测试钉住的语义

- 几何：thumb 长度 = 视口比例（100/300 → 32px）、贴右缘、offset=0 时在顶端；**不溢出 ⇒ 无 thumb**；
- 拖动：按下 ⇒ 进入会话 + 指针被捕获；拖到 track 中点 ⇒ offset ≈ max_scroll/2；松手 ⇒ 会话结束、捕获释放、**滚动位置保留**；
- 抓取点不跳：先滚到一半、抓 thumb 顶端、只挪 1px ⇒ offset 只动 ≈一步（不是跳回顶部/底部）；
- 绘制：溢出 ⇒ 场景里有 thumb 矩形（右缘、宽 = `SCROLLBAR_WIDTH`）。

### 当前规模

`lieui` 约 **13.5k 行**（含约 5.5k 单测）⇒ 有效实现约 8.0k 行。

---

## 2026-10-01 · M5 第四段：`Kind::Custom` + `CustomNode` —— ✅ 扩展逃生舱开放

### 已完成

| 模块 | 内容 | 单测 |
|---|---|---|
| `custom.rs`（新） | `CustomNode` trait（`intrinsic_size` / `draw` / `on_event(id, ev, cmd)` / `on_attached` / `on_detached`，**全部不拿 `&mut Track`**）+ `CustomCell`（`Rc<RefCell<dyn CustomNode>>` 包装，判等按 **Rc 指针**）+ `cell()` / `from_rc()` 工厂 + `fill_rect` 助手 | 6 |
| `track.rs` | `KindDesc::Custom` / `Kind::Custom`；`apply_to` 指针判等（同 cell = 实例跨帧保留）；`create ⇒ on_attached`、`destroy ⇒ on_detached` | — |
| `layout.rs` / `widgets` / `view.rs` | 固有尺寸挂钩、绘制整块转交用户（窗口坐标系）、事件转交状态机、`v.custom(&cell)` DSL、prelude 补导出（`CustomNode`/`CustomCell`/`custom::`/`Scene`/`Affine`/`CmdBuf`/`NodeId`/`Placement`/`EventView`） | — |
| `examples/gallery.rs` | 新增"自绘波形"（B+C 档）：点击前进一格，`on_event` 自改状态 + `cmd.damage(id)` 自标脏 | — |

### 设计决定（与设计稿 §3.12 的差异，都有理由）

1. **不做 `update_desc(&dyn Any)` downcast**：设计稿是"用户每帧给新 desc、框架调 `update_desc` 更新实例"；
   实现改为**用户直接持有 `CustomCell` 并跨帧复用**——同一个 cell 指针相等 ⇒ 对齐器视为没变，
   实例一直留在保留树里。省掉整个 downcast 协议（及其 debug 断言），类型安全从 `Rc<RefCell<T>>` 天然获得。
2. **`on_event` 带 `NodeId`**：自定义节点改了自己的状态后要能**自标脏**——`Signal::set` 只触发
   `view()`（对齐器不会碰这个节点），不 `cmd.damage(id)` 就永远不重绘。这是 C 档能自洽工作的关键。
3. **B 档读 Signal 的诚实说明**：自绘内容读 `Signal` 在 `draw` 里可行（UI 线程），但数据变化只会
   触发 `view()`，不会自动重绘自绘区域——需要伴随一个重绘触发（自绘节点自己收事件、或将来
   的 `cx.custom_mut` 逃生舱 / tick 动画）。文档里写明了这条限制。

### 测试钉住的语义

- 自绘原语出现在场景里且坐标 = 节点 rect、宽度随内部状态变化；
- 固有尺寸：父关闭交叉轴拉伸后 120×24 精确生效（顺带钉住"`align_items` 设在**父**上约束孩子"）；
- 同 cell 跨帧稳定（`apply_to` 返回 false）+ 多节点共享实例（一处改、处处生效）；
- 换 cell = 数据变化（`apply_to` 返回 true）；
- `create ⇒ on_attached`、`destroy ⇒ on_detached` 各一次；
- 端到端：`view()` 声明 → 帧渲染 → Tapped 到达状态机 → **view 重跑后实例不重建**（`attached` 仍为 1）。

### 当前规模

`lieui` 约 **13.1k 行**（含约 5.4k 单测）⇒ 有效实现约 7.7k 行。

## M5 修复：整窗回退把脏区外元素擦成底色（2026-10-01）

**症状**（gallery 实测）：拖「嵌套滚动区」滚动条时，页面其他元素整片消失（只剩滚动内容 + 底色）。

**根因**：`Renderer::render` 里**场景剔除**与**光栅批次**各算各的——场景按原始脏区剔除（脏区外的
原语全部不进绘制列表），而 `Rasterizer::rasterize` 内部的 `damage_batches` 在**碎片 >8 或面积
>45%** 时把批次退化成整窗。嵌套滚动区 16 行内容一滚 ⇒ 每行旧∪新 32 个脏矩形 ⇒ 触发整窗回退 ⇒
用"只剩滚动元素的场景"重画整窗 ⇒ 页面其他元素被擦成底色。行数少的滚动容器不触发（≤8 碎片），
所以只有嵌套区（16 行）必现。

**修复**（`render/mod.rs`）：批次**先**算——`damage_batches` 判定退化整窗 ⇒ 场景也不剔除
（`scene_all`）；场景剔除矩形改用**批次反推的逻辑矩形**（floor/ceil 取整外扩），保证"实际重画的
面积 ⊆ 场景包含的面积"，顺带消掉 1px 边缘擦除。脏区全在窗外 ⇒ 提前返回零统计。

**回归测试** `render::tests::full_window_fallback_does_not_erase_elements_outside_damage`：
滚动容器（10 行蓝条）+ 右侧红箱（与滚动脏矩形零交集），滚 20px 产生 >8 碎片 ⇒ 断言红箱像素
不被擦、滚动内容正确位移。

**顺带清零** 8 条既有 clippy 警告（needless_borrow ×2 / match_single_binding / collapsible_if ×2 /
clone_on_copy / unnecessary_fallible_conversions ×2），均在本次改动之外的一行修复。

验证：`cargo test --lib` 266 全绿；`cargo clippy --lib --examples` 0 警告。
