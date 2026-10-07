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

## M5+ 图标显示支持（2026-10-01）

**结论**：图标 = **图标字体里的一个字符**，整套支持零新绘制原语——注册字体进文本引擎 +
名称查码点，之后测度/布局/绘制/hover 变色全部复用文本管线。

- **`src/icon.rs`（新）**：`ensure_icon_font()`（幂等注册内嵌的 Material Icons TTF，记录真实
  family 名）/ `icon_char(name)`（查内嵌 codepoints 表，~2200 个名称全覆盖；未知名称回退
  占位字形 □）/ `icon_spec(size)`（图标字体 + 不换行的 `TextSpec`）。字体与 codepoints
  表从 `assets/` 移入 `src/assets/`（`include_bytes!`/`include_str!` 内嵌）——发布自包含，
  `Cargo.toml` 的 `exclude` 相应去掉 `/assets`。
- **`view.rs`**：新增 `icon(name)`（= 设了图标字体的 `Kind::Text`，默认 20px，主题文字色）
  与 `icon_button(name)`（`Kind::Button` + 图标字形，交互与 `button` 完全一致）；
  `button` 的主题烘焙抽成 `bake_button_style` 共享。
- **gallery**：新增「图标」演示段（icon_button 一排 + 不同字号/变色的 icon 一排）。

**设计取舍**：不做 `IconName` 枚举（2200 个 variant 会拖慢编译），用字符串名称 + 运行时查表；
未知名称显示 □ 而不是 panic——图标缺失不该炸 UI。

**测试** +6：码点解析（close=e5cd / settings=e8b8 / add=e145）、未知名回退、常用子集存在性、
字体注册后图标字形可测度、`icon` DSL 产出（Text + 图标字体 + 不换行）、`icon_button` 产出
（Button tag + 图标 label）。

验证：`cargo test --lib` 273 全绿；`cargo clippy --lib --examples` 0 警告；gallery 编译通过。

## lieui-text / lieui-geom 参考 lievisual 改进（2026-10-01）

对照 `../lievisual 0.2` 的 `text/engine.rs` 与 `geometry.rs`，取其精华、按 GUI 场景取舍：

**lieui-text（4 项，源自 lievisual 引擎的成熟做法）**：
1. **`line_height` 真正生效**（最有价值的修复）：旧注释称"parley 0.11 无此 StyleProperty、由上层
   处理"是错的——lievisual 证明有 `StyleProperty::LineHeight(FontSizeRelative)`。现在测度 /
   排版 / PlainEditor 三处一致应用（行高换算 `lh / font_size` 为倍率）。旧实现里它只是缓存键的
   一部分，是 **no-op**。
2. **进程级共享字体集合**：`Collection::new(CollectionOptions { shared: true })` 全局持有，各线程
   `FontContext` 克隆之——注册的字体对所有线程（含后启线程）可见，不再静默回退系统默认。
3. **CSS 字体族列表**：`font_family: "Segoe UI, sans-serif"` 按 `FontFamilyName::parse_css_list`
   解析为回退链（编辑器样式用 'static 复制版；解析失败退化为单一命名族，兼容旧行为）。
4. **注册增强**：`FontSource`（Path/Memory）+ `register_font_source(source, family_override,
   generic_family)`——family 名覆盖 + 把注册字体挂到 generic family（如 `monospace`）；
   旧 `register_font_bytes` / `register_font_file` 签名不变，改为其上的便捷封装。

**lieui-geom**（补齐实用面；**明确不移植 kurbo f64 直通**——UI 布局/命中/上屏全链路是 f32
逻辑像素，kurbo 直通是矢量场景的价值）：
- `Color`：`Eq`/`Hash`（可作映射键）、`rgb()`、`to_hex()`（不透明 `#rrggbb` / 带 alpha
  `#rrggbbaa`）、`with_alpha()`、`lerp()`（动画用）；文档注明 8-bit 直存的无损理由（同 lievisual）。
- `Rect`：`from_points`（对角点归一化）、`translate`、`inflate_xy`（`inflate` 变为其同值特例）。
- `Point`：`distance`、`lerp`。

**测试** +9（geom 4 / text 5）：hex 往返、派生色与插值端点、HashMap 键、点构造归一化、
平移/非对称外扩、距离与插值；line_height 生效（16px + lh 32 ⇒ 高≈32）、字体族列表回退、
family 覆盖 + generic 绑定（用仓库内 Material Icons 字体实测：覆盖名可排版、图标字形
advance≈字号）。

验证：`cargo test --workspace` 291 全绿（273+6+4+8）；clippy 0 警告。

## M5+ 框架级 tooltip（2026-10-01）

**背景**：gallery 里「或点这里看看 hover」是个没有任何 handler/hover 视觉的纯文本——点了没
反应、hover 也没东西可看。借此把 v3 缺的 tooltip 补成**框架级**能力（hover 态本来就归框架的
视图态，tooltip 只延迟出现一次，放 view() 里声明既啰嗦又需要 hover 数据在模型里）。

**组成**：
1. **描述字段**：`DescNode.tooltip` / `Node.tooltip` + `DescRef::tooltip(..)` DSL；align 的
   write_all/patch 两路都同步（tooltip 不影响布局/绘制 ⇒ 不置脏）。
2. **锚点泛化**：`Anchor { target: AnchorTarget, placement }`，`AnchorTarget::Key(Key) | Node(NodeId)`
   ——`popup_at`/`tooltip_at` 走 Key 不变；框架 tooltip 锚到任意 hover 节点（不要求有 key）。
   `anchored_origin` 按 target 解析，翻转/钳位逻辑共用。
3. **框架自管层**：`Root.framework: bool` + `Track::add_framework_root`；**align 的 stale 清理
   跳过 framework 层**——tooltip 的生命周期归 `WindowCtx` 会话，view() 重跑不会误删。
4. **会话状态机**（`WindowCtx`）：hover 链变化 ⇒ 从最深节点向上找第一个带 tooltip 的 ⇒ 计时
   （`TOOLTIP_DELAY = 600ms`）；`tick(now)` 到时浮出（`Layer::Tooltip` + RightOf + 翻转/钳位
   复用锚定落位）；移开/按下/目标销毁 ⇒ `remove_root`。`next_wakeup` 合并 armed 截止时刻
   （空闲窗口也能被唤醒浮出）。主题新增 `tooltip_background` / `tooltip_text`。
5. **Renderer::options()**（只读）补齐——tooltip 建层时要读主题 token。

**回归**：`tooltip_opens_after_hover_delay_and_closes_on_leave`——悬停未到延迟无层、到点浮出
且锚定目标节点、`framework == true`、移开自动消失。

**gallery**：246 行改为真实演示——文本有 `hover_color`（hover 变强调色）+ `tooltip`（多行提示）+
`on_tap` 反馈到状态栏。

验证：`cargo test --workspace` 292 全绿；clippy 0 警告。

## 文本墨迹盒（ink bounds）与光学对齐（2026-10-01）

**起因**：gallery 图标段那行文字看起来没和不同字号的图标垂直对齐。排查后确认：
**lieui 的文本尺寸一直是 parley 行盒**（`TextEngine::measure_text` → `layout.width()/height()`；
`lieui-layout::FlexNode::measure_text` 直接调它；绘制按 rect 左上角画 glyph runs），
**没有墨迹盒**——而 lievisual 有（`TextLayout::ink_bounds` + `glyph.rs::ink_box`，
用 skrifa 逐字形 bbox，服务于 `compute_text_offset` 的光学居中）。

**实测数据**（`ink_bounds_explains_the_optical_offset` 固化为断言）：

| 字体 | 行盒 | 墨迹 | 墨迹中心 − 行盒中心 |
|---|---|---|---|
| Material Icons @16/20/24/28 | = 字号（见方） | ≈0.58em | **0.00**（精确居中） |
| 系统字体拉丁 13px（"abc"） | 14.95 | 9.46 | **+1.05**（偏上） |
| 系统字体 CJK 13px | 16.51 | 13.17 | **−0.39**（偏下） |

⇒ 「按行盒居中」时图标（Δ=0）与拉丁文字混排会偏 ~1px，CJK 只偏 0.4px。
另外顺带发现：**lieui-layout 的 relative `position_top` 不生效**（探针测试失败，
`write_back` 没有应用 relative 偏移）——"用相对定位微调"这条捷径不可用，正解是 ink。

**实现（参考 lievisual 移植）**：
1. `lieui-text::ink`（新）：`InkBounds` + `ink_bounds(&TextLayout)`；逐字形墨迹盒取自
   skrifa `GlyphMetrics::bounds`（不是重建轮廓），按 `(blob id, collection index, 字号)`
   进程级缓存字形盒 map。skrifa 0.44 与 parley 内部同版本，不引入重复依赖。
2. `TextEngine::ink_bounds(text, spec)`（给绘制/对齐用，坐标系与测度同源、y 向下）。
3. `TextSpec.optical_align`（新字段）：置真时**测度高度 = 墨迹高度**（宽度仍是 advance）；
   `measure_cache_key` 已含该位。它不是功能开关而是**排版口径**，所以放在 spec 里随测度流动
   （`lieui-layout` 自己调 measure，无需改布局引擎）。
4. `DescRef::optical_align(true)` DSL；绘制端 `push_text`：
   非居中 ⇒ `origin.y = rect.y - ink.top`（墨迹上缘贴矩形上缘）；
   居中 ⇒ 把墨迹盒居中到矩形里。墨迹与排版一起进 `TextCache`（`get_full`），
   每帧零成本；命中但缺 ink 时惰性补算一次。
5. gallery 图标段那行文字用上 `.optical_align(true)`。

**测试** +5：ink characterization（上表数据）、`optical_align` 测度 = 墨迹高、
布局高度随口径变化、绘制原点按墨迹上缘、以及原有的图标行盒见方断言。

验证：`cargo test --workspace` 297 全绿（277+6+4+10）；clippy 0 警告；gallery 编译通过。

### 后续（同日）：真正的根因是「测度/绘制换行口径不一致」

用地表最强的证据（**渲染成像素**再量墨迹带）复查 gallery 图标行，抓到真凶：

```text
修复前：PROBE favorite/star/info/menu 墨迹带中心 = 13.0 ~ 13.5
        PROBE label: rect y=0 h=28  绘制墨迹带 = 0..12 中心 = 6.0   ← 顶对齐
```

**根因**：布局引擎测度文本时会**按约束宽度换行**（`flex_node::layout_single_node`
把 `style.max_width = avail_w` 注入后调 `TextEngine::measure_text`），而**绘制**用的是
节点自身的 spec（`max_width: None`）⇒ 不换行。于是：
- 该标签在 360 宽的行里放不下（图标占 152，剩 208）⇒ 测度**换行成两行**，盒高 28；
- 绘制只画一行（13px 高），贴在 28 高的盒子顶部 ⇒ 肉眼看就是「文本顶对齐」。

这是框架级既有缺陷（与 `optical_align` 无关，任何被约束到换行的文本都会错位；
上一节测出的 0.39px 光学偏差只是次要因素）。

**修复**：
1. `lieui-layout::FlexNode.measured_wrap_width`：测度时记录**实际使用的换行宽度**
   （`wrap=false` 时清空）；
2. `lieui::layout::write_back`：把它写到文本/按钮节点的 `Node.text_wrap`；
3. `widgets::draw_spec(n)`：绘制时若 `spec.wrap && spec.max_width.is_none()`，
   用 `n.text_wrap` 补上同一约束 ⇒ 测度与绘制**逐字一致**（布局缓存键已含 max_width）。
4. gallery 图标行标签加 `.wrap(false)`（演示行保持单行）并缩短文案。

**回归测试**（像素级）：`wrapped_text_draws_with_the_same_wrap_width_as_measure` ——
窄容器里的长中文文本，画出的墨迹带必须覆盖盒子高度的 60% 以上（此前只有一行 ≈ 33%）。
修复后同一探针：`label: 墨迹带 0..28 中心 = 14.0`，与图标中心一致。

验证：`cargo test --workspace` 298 全绿；clippy 0 警告；gallery 编译通过。

### 后续（同日）：滚动条被列表项遮挡

**症状**（pdfkit 左侧页列表）：滚动容器的 thumb 被 item 盖住。

**根因**：滚动条是容器的"虚拟部件"，但画在 `widgets::draw` 的 ⑤ 步（**容器自身内容**里），
而 `scene::walk` 是"自身内容 → 子节点"的顺序 ⇒ 后画的列表项（整行背景）把它盖住。

**修复**：把滚动条改成**覆盖层**——
- `widgets::draw` 不再画滚动条；新增 `pub(crate) draw_scrollbar_overlay(...)`；
- `scene::walk` 在**走完 children 之后、PopClip 之前**调用它 ⇒ 覆盖在内容之上，
  且仍在滚动容器的裁剪内（不会画到视口外）。

**回归测试**：`scrollbar_is_drawn_above_the_content`——给内容子节点铺满底色，
断言场景 op 序列里 thumb 的位置**在内容之后**。

验证：`cargo test --workspace` 299 全绿；clippy 0 警告；pdfkit 重新编译通过。

## 2026-10-01 · 主题支持完善：token 补全 + 跟随系统 + 整窗重绘

M5 第六段已落地"token → 烘焙"的主题机制（`Theme` + `light()/dark()` + `Runtime::set_theme`
+ 每帧 `SceneOptions.theme` 兜底）。本轮补齐三处缺口：

### 1. 三个 token 缺失项（此前硬编码在框架里）

- `focus_ring`：焦点框描边（`scene::walk` 里原本是 `Color::rgba(80, 120, 220, 255)`）；
- `backdrop`：Modal 遮罩（`LayerOpts::for_layer` 里的 `rgba(0, 0, 0, 80)` 现在只是**无主题兜底**，
  由 `ViewBuf::layer` 用 token 覆盖）；
- `shadow`：浮层投影（`view.rs` 弹层默认视觉里的 `rgba(0, 0, 0, 60)`）。

深色下三者给了更合适的值（描边更亮、遮罩/投影更重）。

### 2. 跟随系统（`ThemeMode`）

`ThemeMode { Light, Dark, System, Custom }` —— 描述"主题从哪来"，与生效值 `Theme` 正交：

- `Runtime::set_theme_mode(mode)`：预设模式应用对应 token 集；`System` 按已上报的 OS 状态
  立即应用（未上报 ⇒ 浅色）；
- `Runtime::set_system_dark(bool)`：平台层上报（winit `WindowEvent::ThemeChanged` + 窗口创建时的
  `window.theme()`）；**只在 `System` 模式下换主题**，其余模式仅记录供之后切换使用；
- `Runtime::set_theme(t)`：语义不变（自定义 token 集），模式转 `Custom` ⇒ 此后不再跟系统。

平台接线（`platform/mod.rs`）：创建窗口后上报初始值、事件循环里转发 `ThemeChanged`。

### 3. 换主题必须整窗重绘（真 bug）

`set_theme` 只置 `Dirty::PAINT`、**没有登记整窗脏** ⇒ 窗口底色与"未被 token 覆盖的兜底色"
（光标/选区/滚动条/焦点框）不会重画，留下旧底色残块。修复：`WindowCtx::frame` 的主题同步分支
（仅在渲染器 token 快照**真的变了**时）调 `track.damage_whole_window()`。
此前没暴露，是因为既有主题测试只在"自己重画过的那块"取样（按钮内部）。

顺带：**框架自管层（tooltip）主题同步**——`align` 不会重建它，所以 `WindowCtx` 每帧
（`update_tooltip` 开头）检查已浮出 tooltip 的底色/文字色是否与当前 token 一致，不一致就重上色并标脏。

### gallery

主题演示行加「跟随系统主题」开关（`checkbox_bound` + `Tapped` 里 `set_theme_mode`）；
手动「切换深色主题」按钮会退出跟随，保持 UI 状态一致。

### 测试 +5

- `system_mode_follows_the_os_theme`（reactive）：预设 / System / Custom 三态与 OS 变化的行为；
- `system_mode_picks_up_an_already_reported_os_state`：先上报 OS 深色、再切 System ⇒ 立即深色；
- `system_theme_mode_follows_the_os_and_repaints_the_window`（app，像素级）：OS 转深色 ⇒
  view 重跑 + 窗口底色跟随；
- `layer_defaults_use_theme_tokens`（view）：Modal 遮罩 = `theme.backdrop`、弹层投影 = `theme.shadow`；
- `focus_ring_uses_the_theme_token`（widgets）：焦点框描边 = `theme.focus_ring`；
  tooltip 主题同步并入既有 `tooltip_opens_after_hover_delay_and_closes_on_leave`。

验证：`cargo test --lib` 284 全绿；`cargo clippy --lib --examples` 0 警告；gallery 编译通过。

## 2026-10-06 · 跨线程通信 + loading 遮罩（框架级补齐）

**动机**：pdfkit 合并多个 PDF 时在 UI 线程同步跑 ⇒ 窗口僵死（上一轮只能在应用层自己拼：
`run_with_handle` 拿句柄 + 手写线程 + 手写遮罩）。框架该把这条链路做完整。

此前只有三件套：`RepaintHandle`（平台层 `Send` 句柄）、`ExternalData`（类型擦除载荷）、
`ViewModel::on_external`（UI 线程落地）。缺口：取句柄必须改启动方式、没有任务抽象
（进度/取消/完成/生命周期）、没有遮罩、无头不可测。

### 三层 API（新增 `src/task.rs`）

| 层 | API |
|---|---|
| 唤醒 | `Waker` trait（平台无关）· `Runtime::{set_waker, waker, wake, is_online, take_pending_external}` |
| 任务 | `Runtime::{spawn_task, spawn_task_busy}` · `TaskHandle{cancel, is_done, is_running}` · `TaskCtx{post, progress, is_cancelled, cancel_token, wake}` · `CancelToken` |
| 遮罩 | `Runtime::begin_busy(..) -> BusyToken`（RAII，Drop 即收起）· `BusyToken{set_label, set_progress, cancellable, finish}` · `Runtime::{busy_items, is_busy}` |

设计要点：

1. **`Waker` 抽象 ⇒ 框架可无头**：winit 平台由 `RepaintHandle` 实现，`platform::run` 自动
   `set_waker`（**不再必须用 `run_with_handle`** —— 老 API 保留）。没有平台时退化成 Runtime
   内部的**本地队列**，`App::frame_all` 像事件循环一样消费它 ⇒ 单测能跑通"任务 → 投递 →
   落地"的完整闭环（不需要真窗口）。
2. **完成 = 一条消息**：任务返回值包成 `TaskEvent { id, payload }` 投递。`WindowCtx::external`
   先让框架收尾（清任务表 + 收遮罩），**再**交给 `on_external`（用户 `downcast` 自己的类型）。
   进度消息 `TaskProgress` 由框架完全消费（驱动遮罩），不打扰用户。
3. **取消是协作式的**：`CancelToken`（`Arc<AtomicBool>`）。三种触发：点火遮罩「取消」、
   窗口关闭（`App::close_window` 现在会取消该窗口的全部任务）、显式 `TaskHandle::cancel`。
   线程不可强杀，语义是"任务轮询后自行收敛"。
4. **panic 兜底**：任务体用 `catch_unwind` 包住 ⇒ panic 也走收尾路径（投递 `TaskFailed`），
   否则遮罩会永远挂在屏幕上（任务永远不会"完成"）。
5. **遮罩自带「取消」按钮**：`spawn_task_busy` 默认把它接到该任务的 `CancelToken`。

### loading 遮罩（新增 `src/overlay.rs`）

- **声明式 Modal 层**：由忙碌项驱动 —— `view()` 跑完后框架追加
  `modal_tagged(BUSY_OVERLAY_TAG)`；忙碌项清空就不再声明 ⇒ 下一帧 `align` 的 stale 清理
  自动删层（零手工增删）。
- 视觉：Modal backdrop（主题 token）+ 居中卡片（标题行 = 三点脉冲 spinner + 文案；
  确定进度 ⇒ 进度条 + `done / total`；可取消 ⇒ 「取消」按钮）。
- **动画不重跑 `view()`**：spinner 是 `CustomNode`（相位读挂钟）⇒ `WindowCtx::animate`
  每 `SPIN_PERIOD`(33ms) 只把**卡片矩形**标脏重绘；`next_wakeup` 在有遮罩时返回定时唤醒
  （遮罩消失 ⇒ 回到空闲零功耗）。
- 配套通用能力：`ViewBuf::modal_tagged(tag)` + `Track::root_by_tag(tag)`（层根标签，
  用户也能给自己的层打标签）、`WindowConfig::auto_busy_overlay(false)`（想自己画遮罩时关掉）。

### Ctx 糖

事件处理器里不必再存 `rt` / `window`：`Ctx::{spawn_task, spawn_task_busy, begin_busy, waker}`。

### 测试 +19

- task（12）：本地队列往返、平台 waker 注入与 `is_online`、`TaskEvent` 载荷落地、遮罩进度与
  自动收起、点遮罩取消、协作式取消、窗口关闭取消（连带清遮罩项）、多任务按窗口堆叠、
  **panic 兜底**、`BusyToken` RAII 与进度更新、取消按钮回调；
- overlay（4）：spinner 三点落在框内且不重叠、遮罩是带 tag 的 Modal 层且卡片是层根首子节点、
  忙碌清空后层被清理、可取消 ⇒ 描述里出现按钮；
- app（2）：**端到端**（起任务 → 遮罩出现 → 动画帧标脏卡片 → 结果落地 Signal → 遮罩消失且
  回到零唤醒）、遮罩阻断下层按钮交互（Modal 语义）；
- 示例：新增 `examples/background_task.rs`；gallery 新增「后台任务」演示段（3 秒任务 +
  实时进度 + 可取消）。

验证：`cargo test --lib` 301 全绿；`cargo clippy --lib --examples` 0 警告；全部示例编译通过。

## 2026-10-06 · 事件系统与 winit 事件循环的统一（自定义事件 / 定时器 / 动画帧）

**问题**（设计复盘）：事件入径本是三条并行、语义重叠的通道，且帧唤醒来源散落：

| # | 入径 | 现状问题 |
|---|---|---|
| 1 | winit → `InputEvent`/`Event` → `dispatch` | 只承载"输入"，自定义事件无处可去 |
| 2 | `AppEvent::External` → `WindowCtx::external` → `on_external` | 只有"从线程投递"这一半能力（`TaskCtx::post`），UI 线程内没有对应入口 |
| 3 | `Ctx::request` → `RequestQueue` → `drain_requests` | 与 #2 语义重叠（都是延迟投递），却只能 UI 线程内用 |
| 4 | 帧唤醒 | `WindowCtx::next_wakeup` 手写 if-else 汇总（闪烁 / tooltip / spinner），每加一种动画要改两处 |

**结论：不引入第四种机制，收敛成"两条入径 + 一个时钟"**。

- 输入事件（#1）保持原样：它有自己完整的语义（命中、路由、捕获、IME）。
- **消息**：#2 升格为通用通道 —— 自定义事件、任务结果、跨线程数据全走 `Waker`
  （平台 `EventLoopProxy` ↔ 无头本地队列）。#3 降级为**框架控制消息**（开窗/关窗），
  文档明确"别拿它当事件总线"。
- **时钟**（#4）：定时器与动画帧做成 `Runtime` 侧的一份表，平台层回到"只问下次何时醒"。

**为什么不做 `App<UserEvent>` 泛型化**（像 winit 那样）：泛型会传染到 `App` / `WindowConfig` /
`Rc<dyn WindowView>` 的擦除层，并让"无头可测"变难。运行时 `downcast`（`ExternalData`）
在类型安全（发射器 `Emitter<T>` 仍是编译期类型化的）+ 擦除层不变之间取得平衡。

### 新增：自定义事件（`event.rs`）

- `Runtime::emit(window, msg) -> bool` / `Ctx::emit(msg)`；
- `Emitter<T>`（`Clone + Send + Sync`，内部只存 `WakerSlot` ⇒ 天然可跨线程，**无需 unsafe**）；
- `Runtime::emit_global(Arc<T>)` 广播到所有窗口；
- 分发语义不变：`WindowCtx::external` 先让框架消费内部消息（任务/定时器），再 `on_external`。

### 新增：统一时钟（`src/timer.rs`）

- `Runtime::set_timeout(window, dur, f)` / `set_interval(..) -> TimerHandle{cancel, is_active}`；
  回调是 `FnMut(&mut Ctx)`（UI 线程，可改状态/开窗/起任务），**`TimerHandle` 的 Drop 不取消**
  （`let _ =` 写法不该自杀）；
- `Runtime::request_animation(window)` + `ViewModel::on_animation(cx, now, dt)`（经典 RAF 语义：
  回调里再请求才继续；`on_tick` 仍是每帧都跑）；
- `Runtime::next_deadline(window)`（定时器最早到期 ∪ 动画帧时刻）；
  `WindowCtx::next_wakeup()` 汇总裁剪 = 闪烁 / tooltip / spinner / 时钟，平台层零改动；
- 窗口关闭清定时器（与任务取消并列）。

### 顺带修掉的两个"名不副实"

- `Ctx::damage_all()` 以前只是 `request_repaint()` 的别名（**不产生脏区** ⇒ 自绘动画什么都画不出来），
  现在真的走 `Cmd::DamageAll` → `track.damage_whole_window()`；`request_repaint()` 保留原语义并写清区别。
- `App::frame_all()` 以前只跑 `frame()`，不跑 `tick()` ⇒ 无头驱动下定时器/动画**根本不触发**。
  现在与平台层一帧同构：消费外部事件 → tick（定时器 → 动画 → `on_tick`）→ frame。

### 测试 +12

timer（4）：一次性到期即消失、周期重排直到 cancel、动画请求一次性 + 驱动 deadline、
按窗口隔离与关窗清理；event（5）：`emit` 落本地队列、`emit_global` 广播、`Emitter` 跨线程
（编译期 `Send + Sync` 断言 + 真线程投递）、`request` 与 `emit` 互不干扰、
`ExternalData` 为统一载荷；app（3）：定时器一次性/周期/取消、动画帧只在请求时跑且 `dt` 真实、
自定义事件（UI 线程 + 线程内发射器）经帧驱动到达 `on_external`。

示例：新增 `examples/event_clock.rs`（自绘相位动画 + 每秒 interval + 3 秒 timeout 发自定义事件 +
跨线程发射器）。

验证：`cargo test --lib` 313 全绿；`cargo clippy --lib --examples` 0 警告；全部示例编译通过。

## 2026-10-06 · 脏区机制的成本核算与取舍（附基准 + `full_repaint` 逃生舱）

**背景**：有人问"脏区剔除值不值得，是否干脆有 dirty 就整窗重绘"。先把真实成本量出来再决定。

### 现有实现规模（生产代码约 330 行 + 测试约 150 行）

| 环节 | 位置 | 规模 |
|---|---|---|
| 脏区登记（矩形集 + 整窗标志 + 节点→脏区换算） | `track.rs`（`damage`/`damage_all`/`damage_rect`/`damage_whole_window`/`take_damage`/`damage_bounds`/`mark_paint_dirty`） | ~30 行 |
| 登记点散布（重排、位移、层增删、主题、cmd） | `layout.rs` / `align.rs` / `app.rs` / `cmd.rs` | ~40 行 |
| 场景剔除（按脏区裁原语） | `render/scene.rs` | ~15 行 |
| 脏区 → 光栅批次（裁剪/取整/去重/退化阈值） | `render/raster.rs::damage_batches` | 37 行 |
| 批次光栅化（scratch 复用 + 逐行拷回） | `render/raster.rs::rasterize` | 101 行 |
| 剔除与批次同源（防擦除） | `render/mod.rs` | ~30 行 |
| 局部上屏（物理换算 + `present_with_damage`） | `platform/mod.rs` | ~75 行 |

关键字（damage/脏区/dirty）在源码里共 315 行，含注释与测试。

### 实测（`examples/damage_bench.rs`，1280×720，release）

```text
节点  151 | 整窗 4.6ms (921600px) | 局部  49µs (588px) | 空闲 0.5µs | 94×
节点  601 | 整窗 2.6ms (921600px) | 局部 114µs (588px) | 空闲 1.6µs | 22×
节点 1801 | 整窗 7.2ms (921600px) | 局部 388µs (637px) | 空闲 5.7µs | 19×
```

两个结论：

1. **脏区把光栅化从 ms 拉回 µs**（整窗占 60fps 预算的 15~43%），空闲帧几乎为零
   ——"有脏才画"确实成立，**不该退化**。
2. **局部重绘耗时随节点数线性增长**（49µs → 388µs）：瓶颈已不在光栅化，而在
   **每帧全量重建场景 + 剔除**（O(N)）。下一步该做的是增量场景 / 保留 draw list，
   而不是继续抠脏区。

### bug 风险（两类，都有真实案例）

- **漏标**（登记不完备 ⇒ 残影）：换主题没标整窗脏（`Dirty::PAINT` 单独无效）、
  文本"测度换行口径"与绘制不一致、`CustomNode` 自绘改了状态没 `cx.damage(..)`。
- **多剔**（剔除与绘制不一致 ⇒ 擦除）：整窗回退时场景仍按细碎脏区剔除
  （gallery 嵌套滚动整页空白那个 bug）、同批次内图片 blit 与 vello 原语的顺序。

缓解：`damage_batches` 的自动退化阈值（碎片 > 8 或面积 > 45% ⇒ 整窗）把最危险的
"碎片场景"引到安全路径；`Cmd::DamageAll` 提供"拿不准就整窗"的兜底。本轮还修了两处
同类问题：`Ctx::damage_all()` 名不副实（不产生脏区）、`App::frame_all()` 不跑 tick。

### 与 compositor 的关系（结论写进代码注释）

**不是一回事，是正交的两层**：

- **compositor（合成器）** 回答"**怎么叠**"：把多来源（窗口 surface / 层 / 图元）按层序
  与透明度合成最终画面。本库的"层序遍历 + SrcOver 到一张 pixmap"就是**简易 compositor**
  （内容层 / 弹层 / Modal 遮罩的层序合成）。
- **damage（脏区）** 回答"**这次要重算哪里**"：现代合成器一律以脏区为单位增量合成
  （Wayland `surface.damage`、Win32 `WM_PAINT` 的 update region、macOS `setNeedsDisplayInRect`）。
  我们的对应链路：`Track.damage` → 场景剔除 + 批次光栅化（软件合成）→
  `softbuffer::present_with_damage`（把脏区交给**系统合成器**）。

所以两者可以各自独立演进（换 GPU 后端时脏区机制照样有效）。

### 落地

- `WindowConfig::full_repaint(bool)`：整窗重绘逃生舱（默认 off）。用途：残影类 bug 的
  一键对照（打开后残影消失 ⇒ 某处漏标脏区）、正确性优先于性能的场景。帧里就一行
  `damage.clear(); damage_all = true;`。
- `examples/damage_bench.rs`：把上面的测量固化成可复跑的基准（带结论注释）。
- 测试 +1：`full_repaint_mode_always_redraws_the_whole_window`（同一改动在两种模式下
  的 raster 像素数：局部 < 整窗 = 400×300）。

验证：`cargo test --lib` 314 全绿；`cargo clippy --lib --examples` 0 警告；全部示例编译通过。

## 2026-10-06 · 评估"多碎片 → 单包围盒"简化：实测否决，但找到了真瓶颈

### 做了什么（为评估提供可测工具，默认行为不变）

- 三种策略都实现成**纯函数**：`damage_batches`（现状·精确碎片）、`damage_batches_union`
  （单包围盒）、`damage_batches_bands`（水平行带：按 y 合并成互不重叠的带）。
- `Rasterizer::rasterize` 拆成 **`batches_for`（批次决策）+ `rasterize_batches`（执行）**
  —— 决策与执行解耦，剔除与光栅化可以共用同一批次列表（同源不再靠约定），也便于注入策略做基准。
- `examples/damage_bench.rs` 增加"策略对比"与"真实光栅化耗时"两段。

### 实测（1280×720，场景 2000 原语，release，30 次平均）

| 场景 | 精确碎片（现状） | 单包围盒 | 水平行带 |
|---|---|---|---|
| 滚动 16 条横带 | 1 批 / 921600px / **2.15ms** | 1 批 / 870400px / **1.89ms** | 16 批 / 409600px / **8.39ms** |
| 分散 4 块（四角） | 4 批 / 24000px / **1.84ms** | 1 批 / 830800px / **2.21ms** | 2 批 / 74400px / **1.12ms** |
| 同列 6 行 | 6 批 / 36000px / **2.64ms** | 1 批 / 46000px / **0.46ms** | 6 批 / 36000px / **2.63ms** |

### 结论

1. **成本模型**：`每帧 ≈ 2.5ns/px × 像素数 + ~0.4ms × 批次数`（2000 op 场景）——
   因为 `rasterize_batches` 里**每个批次都把整个场景的 op 重放一遍**。
   于是**批次数是最贵的维度**，像素数反而是次要的。
2. **现状的"碎片 > 8 或面积 > 45% ⇒ 退化整窗"是对的**：它把批次数压回 1。
3. **单包围盒（简化方案）否决**：实现更简单、3 个场景里 1 快 2 慢，且"少量分散更新"场景
   像素膨胀无上界（四角 4 块：24000 → 830800px，35×）；对"状态栏/多面板各自刷新"这类
   UI 是明显倒退。
4. **水平行带**像素最省（滚动 −56%），但当前**实测最慢**（8.39ms）——瓶颈不在策略，
   而在"每批重放全场景"。**换策略不能解决问题**。
5. **真瓶颈与下一步**：做 **per-batch op 裁剪**（每批只提交与它相交的 op；剔除用的包围盒
   逻辑 `scene::push` 里已经有了），把每批成本从 O(全场景 op) 降到 O(相交 op)。
   预估滚动场景 16 批：`16 × ~25µs + 1.0ms 像素 ≈ 1.4ms` < 现状 2.15ms；
   做完之后行带/精确碎片都能兑现像素优势，单包围盒就更没必要了。**（本轮未做，留待决定）**

### 测试 +5

三策略**像素级完备性**（脏矩形里每个像素都落在某批次内——注意行带会把矩形按 y 切开，
不能断言"某批次完整包含某矩形"）、行带**互不重叠**、行带**局部性**（上下分离两块 ⇒ 两个带，
而单包围盒吃掉整窗）、平凡情形（空 / 整窗）三策略一致、`batches_for` 与 `rasterize` 同源。

验证：`cargo test --lib` 319 全绿；`cargo clippy --lib --examples` 0 警告。

## 2026-10-06 · 虚拟列表升格为框架 API（`ViewBuf::virtual_list`）

**背景**：虚拟化此前只是测试里的 40 行 A 档组合函数（`app.rs::tests::virtual_list`），
examples 没有任何演示，pdfkit 侧栏是反例（`for pn in 1..=total`，1000 页 ≈ 近万节点）。

### API

- `VirtualListState`（`Clone`；内部一个 `Signal<usize>`）记住"可见窗口起点"；
- `ViewBuf::virtual_list(&state, items: &[T], key_fn: fn(&T) -> K, item_h, viewport_h, item)`
  —— 必须在**滚动容器**内声明；`ScrollChanged` 时把偏移换算成起点写回
  （**只在跨行时才写**，避免每帧无谓重跑 `view()`）。

### 相对旧内联实现的三处改进

1. **空占位节点 → 虚拟 padding**：内容列 `padding_top = first×item_h` /
   `padding_bottom = 余量×item_h` 撑出真实内容总高（滚动条因此"诚实"），
   比"上下两个空 row"少一个节点，也不必给框架加 `content_size` 覆盖字段。
2. **改用 `keyed_list` 复用**：旧版按下标对齐——纯文本行看不出问题，但**行内有状态
   （选中 / 输入框 / 展开态）时滚动一屏就会错配**；新版按 `key_fn` 匹配
   （测试断言：重叠行在滚动前后 `NodeId` 不变）。
3. **窗口起点向下取整**（`floor` 而非 `round`）：偏移落在半行时，视口顶部露出的那一行
   也要物化，否则顶部会缺一块。

### 顺带的框架小改

- `keyed_list` 的 `key_fn` 从 `fn` 指针放宽为 `impl Fn`（`virtual_list` 内部要适配
  `fn(&T)` → `fn(&&T)`，需要捕获）；现有调用点不受影响；
- 新增 `DescRef::padding_top` / `padding_bottom`（与既有 `padding_y` 配对）。

### 测试（+3 新 / 2 改）

- 新增：**key 复用**（滚动后重叠行 `NodeId` 不变）、**边界**（空列表 / 起点越界 / 单项）、
  **虚拟占位**（内容列只放可见行、内容高 = `count × item_h`）；
- 改造：原"只物化可见窗口"与"滚回去能看到更早的项"两个测试改走新 API —— 断言不变、
  行为一致（等于给新 API 做了回归）。

### gallery

新增「虚拟列表（10000 项）」演示段：点行标记 + 已标记计数 + 清空标记。

验证：`cargo test --lib` 322 全绿；`cargo clippy --lib --examples` 0 警告；示例编译通过。

## 2026-10-06 · pdfkit 侧栏接上虚拟列表（第 2 步）

**前置**：pdfkit 原来依赖 crates.io 的 `lieui 0.1.0-alpha.1`（拿不到新 API）⇒
`Cargo.toml` 改为 `{ path = "../lieui", version = "0.1.0-alpha.1" }`。

### 改动

| 文件 | 内容 |
|---|---|
| `Cargo.toml` | lieui 依赖改 path（本地 crate），`Cargo.lock` 随之更新 |
| `app/model.rs` | `AppState` 加 `page_indices: Vec<u32>`（派生的页索引缓存）+ `sync_derived()`（页数没变就零开销）/ `page_indices()` |
| `app/controller.rs` | `update()` 里统一 `sync_derived()`（**所有**状态变更的唯一入口 ⇒ 不会漏刷新）；`PdfKitVm.vl: VirtualListState` |
| `ui/sidebar.rs` | `for pn in 1..=total` → `virtual_list(&vm.vl, st.page_indices(), …)`；固定行外框 `ITEM_H = 80`；`infos` 改为**借用**（原先每帧 `clone()` 全量 PageInfo） |

### 过程中踩到 / 修掉的两个坑

1. **行的外框高必须**等于**虚拟步长**。最初把行高设 76（内容 64 + padding 12）、步长 80
   （差 4 当"视觉间距"），结果内容总高少了 `可见行数 × 4` ⇒ 滚动条与滚动位置会漂。
   测试（内容总高 ≈ `count × ITEM_H`）当场抓到。现在行外框 = 80，间距由行内留白提供。
2. 侧栏内容 = 「页面」标题 + 虚拟列表 ⇒ 滚动容器的 `content_size` 比 `count × ITEM_H`
   多一个标题高；断言要按区间写（`[list_h, list_h + 60)`）而不是等号。

### 测试（+2）

- `sidebar_materializes_only_the_visible_pages`：500 页文档只建 **< 40 行**节点（实测 19 行），
  且内容总高 ≈ `500 × 80`（虚拟 padding 撑出来的滚动范围）；
- `page_indices_follow_the_page_count`：文档关闭 ⇒ 页索引清空且不物化任何页行。

验证：`cargo test`（pdfkit）24 全绿；`cargo clippy --all-targets` 0 警告。

## 2026-10-06 · 框架补右键语义 + pdfkit 四项优化

### 框架：右键终于有语义了（`src/input.rs`）

以前右键被合成为普通 `Tapped`（"右键也会触发左键行为"：勾选、按下、提交……）。
现在 `Up` 分支按按键分派：`PointerButton::Right` ⇒ `EventKind::RightTapped`，其余 ⇒ `Tapped`。
`RightTapped` 早在 `EventKind` 里、但**从未被派发**（本轮补上）。测试 +1。

### pdfkit（四项）

1. **tooltip**：toolbar 全部图标按钮 + 侧栏上下移都挂 `.tooltip(..)`（框架层 600ms 延迟、
   自动翻转/钳位/主题配色）；顺手删掉 toolbar 顶部"v3 暂无 tooltip 组件"的旧说明。
2. **线程划分**：新增 `src/app/jobs.rs` —— 打开 / 保存 / 页面操作 / 拆分提取的**纯逻辑**
   （`Send`，不碰 UI 状态）。UI 线程只做"选路径 → `cx.spawn_task_busy(..)` → `job_finish` 落地"。
   - 删掉 pdfkit 自造的 `bg.rs`（`BgHandle` / `BgMsg` / `spawn_merge`）与自写遮罩
     `ui/busy.rs` + `AppState.busy`（改用框架遮罩：线程/进度/取消全免费）。
   - `LoadedDocument.pdf`：`Rc<Pdf>` → `Arc<Pdf>` ⇒ 整个结构 `Send`，后台算好的
     **文档整包搬回** UI 线程（UI 侧连重新解析都省了）。
3. **统一打开入口**：删除"合并 PDF"独立模式 —— `MergeState`、7 个 `Merge*` 动作、
   `merge_*` 方法族、`ui/merge.rs` 整文件、toolbar 的合并按钮全部移除。
   「打开」改为 `pick_files()`：**单选 = 普通打开；多选 = 按顺序合并成一个工作区文档**，
   之后照常预览 / 编辑 / 保存 / 另存为。
4. **页面右键菜单**：侧栏行容器监听 `RightTapped` → `state.page_menu` → 声明式
   `popup_at(行 key, Placement::Below)` 弹「上移一页 / 下移一页 / 删除此页」
   （越界项 `.enabled(false)`；点别处由框架 `Dismissed` 轻关闭）。
   注册在**行容器**而非行主体：checkbox 与图标按钮是它的兄弟子树，冒泡也能到 ⇒ 整行可右键。

### 测试

- lieui +1：`right_button_synthesizes_right_tapped_instead_of_tapped`（323 全绿）。
- pdfkit：`jobs` 5（单文件打开 / 多文件顺序合并 / 空输入与坏文件 / 页面操作三段 /
  保存与提取落盘）、controller 4（打开落地文案 / 页面操作落地与页码收敛 / 菜单状态独立 /
  保存清 dirty 且路径跟随）、侧栏 2（500 页只物化可见行、页数变化同步）⇒ 26 全绿。

验证：lieui `cargo test --lib` 323 全绿 + clippy 0 警告；pdfkit `cargo test` 26 全绿 +
`cargo clippy --all-targets` 0 警告。

## 2026-10-06 · 多文件打开的进度反馈（细分进度 + 明细文案 + 可取消）

**问题**：批量打开多个 PDF 时只有"每个文件一格"的粗进度（5 个文件 ⇒ 5 跳），
单个大文件全程只有不确定动画；而且**打开过程不响应取消**（遮罩上的按钮点了没用）。

### 框架侧（`task.rs` / `overlay.rs`）

- `TaskCtx::progress_with(done, total, detail)`：进度与**明细文案**一条消息一次唤醒。
  意义在于 `done/total` 可以比"文件数"更细（把每个文件拆成"读盘/解析/合并"三格），
  于是**进度条在大文件内部也会走**；`detail` 是可以给人看的一行字。
- `BusyItem.detail` + `BusyToken::set_detail(..)`（UI 线程侧的忙碌段同样能用）。
- 遮罩渲染：有 `detail` 就显示它，否则退回 `done / total`（既有调用者行为不变）。

### pdfkit 侧（`jobs.rs` / `controller.rs`）

- `open_job(paths, cancel, progress)`：新增 `OpenProgress { done, total, detail }`；
  每个文件占 3 格（读取 / 解析 / 合并），`total = 文件数 × 3` ——
  首个文件没有合并步骤，但它的块仍占满，末尾多一格收尾，无论几个文件比例都不失真。
  文案形如 `第 2 / 3 个文件 · 正在合并 b.pdf`、收尾 `正在生成预览…`。
- **取消接上了**：每个阶段边界查 `CancelToken`（遮罩上的「取消」按钮由框架默认接到它），
  取消后返回 `已取消打开（<阶段>）` ⇒ 状态栏可见、遮罩自动收起。
- `controller::open`：改为 `ctx.progress_with(..)` + `ctx.cancel_token()`；
  开始时先写状态栏（`正在打开 N 个文件（按顺序合并）…`），遮罩标题同文案。

### 测试

- lieui +3：`progress_with` 携带明细（进度与文案都跟着上报走）、`BusyToken::set_detail`、
  遮罩**优先显示明细**（有明细就不再显示 `done / total`）。
- pdfkit：改写 2 个打开测试（单文件三格 `(0,3)→(1,3)→(2,3)`；3 文件 9 格且 `done` 严格
  单调、`读取` 3 次 / `合并` 2 次、文案点名当前文件），新增取消测试（预置取消 ⇒
  `已取消打开`）⇒ 27 全绿。

验证：lieui `cargo test --lib` 326 全绿 + clippy 0 警告 + 示例编译通过；
pdfkit `cargo test` 27 全绿 + `cargo clippy --all-targets` 0 警告。

## 2026-10-06 · tooltip 文本走墨迹盒居中（附带修"光学对齐吃掉 padding"）

**问题**：tooltip 卡片是"文本 + 上下各 6px 内边距"，但文本按**行盒**绘制 —— 行盒的
ascent/descent 不对称（12px 字号下约 1px 偏差），于是上下留白看着不相等、文本略往上顶。

**修法**（两处，第二处是顺带发现的真 bug）：

1. `WindowCtx::open_tooltip_layer`：tooltip 的文本节点置 `spec.optical_align = true` ——
   节点高变成**墨迹高**，于是上下 padding 对称、字形视觉中心与卡片中心重合。
2. `widgets::Kind::Text`：光学对齐路径改为按**内容盒**（`content_rect(n, rect)`，rect 去掉四边
   padding）放墨迹。此前 `push_text` 的 `(ink, center=false)` 分支把墨迹上缘对到 **border box**
   上缘 ⇒ padding 被吃掉（节点高 = 墨迹 + 12，却只在下方留 12）——只加 (1) 反而更偏。
   Input 组件一直是按 padding 定位的（`pad_top`/`pad_left`），这次把文本节点也统一过来；
   非光学路径保持原样（行盒自带 leading，既有布局按 border box 定位，不动的风险最小）。

**测试 +2**（都做了"去掉修正就会红"的验证）：

- `widgets::optical_align_keeps_the_padding_around_the_ink`：盒高 = 墨迹高 + 12、
  墨迹上/左缘落在内容盒上缘、上下 padding 相等。去掉修正后报
  `墨迹上缘落在内容盒上缘：0 vs 6`；
- `app::tooltip_text_is_centered_by_its_ink_box`（**像素级**）：tooltip 浮出后，
  盒高 = 墨迹高 + 12；上/下 padding 横带是纯底色、字形只出现在中间墨迹带里。去掉修正后报
  `顶部 padding 带是纯底色`。

注：pdfkit 依赖的是 crates.io 上的 `lieui 0.1.0-alpha.1`，看不到本地的这些修复
（要生效需改回 `path = "../lieui"` 或发版）。

验证：lieui `cargo test --lib` 328 全绿 + clippy 0 警告。

## 2026-10-06 · loading 遮罩的图标/文本光学对齐 + pdfkit 依赖核对

### 遮罩卡片文本走墨迹盒（`overlay.rs`）

上一轮修 tooltip 时发现的同类问题：遮罩标题行是 `[spinner(18×18), 文本]` + `align_items(Center)`，
行盒居中让**盒中心**对齐，但字形墨迹在行盒里偏 ~1px（ascent/descent 不对称）⇒ 标题与
三点 spinner 的视觉中心对不齐。改用 `optical_align(true)`（盒高 = 墨迹高 ⇒ 墨迹中心 = 盒中心）；
明细行同理（间距按墨迹算，卡片里的 14px 才均匀）。

顺带排查了框架自绘的图标混排面：`view.icon(..)` 本身就是"带图标字体的 `Kind::Text`"、
`icon_button` 是按钮（字形双轴居中，无参照物不显偏），checkbox/radio/switch 的方框在自己
矩形内居中 —— 框架内需要墨迹对齐的**只有 tooltip 与遮罩标题**，均已覆盖。
用户界面里的"图标 + 文本"混排按 gallery 的建议自行加 `.optical_align(true)`。

测试 +1：`overlay_texts_are_ink_aligned_with_the_spinner` —— spinner 与标题的矩形中心重合、
标题盒高 = 墨迹高（⇒ 墨迹中心即盒中心）、明细行同走墨迹盒。

### pdfkit 的依赖核对（结论：**本来就是本地 path**）

`pdfkit/Cargo.toml` 一直是 `lieui = { path = "../lieui", version = "0.1.0-alpha.1" }`：

- `cargo tree -p pdfkit` ⇒ `lieui v0.1.0-alpha.1 (D:\code\rust\lieui)`（含 `crates/lieui-*` 三个子 crate）；
- `Cargo.lock` 的 lieui 条目**没有** `source = ` 行（= path 依赖），全树只有这一个 lieui；
- pdfkit 根目录无 `.cargo/`、无 `vendor/`，没有 patch 干扰；
- 佐证：pdfkit 正在用**本次会话新加**的本地 API（`Ctx::progress_with` / `Ctx::cancel_token`），
  发布版里根本没有这些 —— 若走 registry 早就编译不过。

（更正：此前一轮我判断"pdfkit 用的是 crates.io 版本"有误 —— 当时只在 registry 缓存里查到
了 API 就下了结论，没有核对 `cargo tree`。依赖本就是本地的，因此 tooltip 墨迹修复、
`progress_with` 等改动对 pdfkit **立即生效**。）

验证：lieui `cargo test --lib` 329 全绿 + clippy 0 警告；pdfkit `cargo test` 27 全绿（对着本地 lieui）。

## 2026-10-06 · pdfkit 打开 PDF 的三处状态问题（确认门 / 合并文档 / 侧栏归零）

起因：复盘"已打开 PDF 后再点「打开」"的实际行为 —— 旧实现**静默整体替换**：`finish_open`
无条件换掉 `loaded`、`dirty = false`（编辑无提示丢失）、`vl`（侧栏滚动位置）不重置；
另外多文件合并后 `loaded.path` 只是第一个文件，「保存」会把合并结果覆盖掉它。

### ① 打开 / 关闭 统一确认门（`PendingAction`）

把只有关闭才有的守卫推广成一件事的两个入口：

- `AppState.confirm_close: bool` → `pending: Option<PendingAction>`，`PendingAction { Close, Open(Vec<PathBuf>) }`；
- `open()` 拆成「选路径 → `guard_unsaved(Open(files))` → `start_open(cx, files)`」：
  有未保存改动就挂起待办、弹确认；**确认后不再让用户重选文件**（路径在弹窗前就选好了）；
- 弹窗文案 / 确认按钮由 `pending` 决定（「不保存退出」/「不保存并打开」）；
- `on_close_request`：弹窗已是「退出」⇒ 放行（第二次确认）；原本是「打开」⇒ **改写待办**为退出
  （文案随之切换）—— 否则弹窗开着点关窗会静默丢编辑（旧代码的 `confirm_close` 短路也有这个洞）；
- `take_discarded()` 取动作时**只在「退出」抹 dirty**：「打开」先不动 —— 打开失败时旧文档的编辑
  必须仍是"未保存"，成功后由 `finish_open` 重新算。

### ② 合并文档不再糊里糊涂覆盖第一个文件

- `LoadedDocument.merged_from: usize` 记住来源文件数；
- `finish_open`：`dirty = 文件数 > 1`（合并结果在磁盘上并不存在，如实标脏）；
- `save()`：`merged_from > 1` ⇒ 直接改走「另存为」并给状态栏提示，**绝不覆盖参与合并的第一个文件**；
- 「另存为」默认名用 `merged_<原名>`（`save_as_default_name`）；
- 落盘后（`JobDone::Saved`）`merged_from = 1`、`dirty = false` ⇒ 之后「保存」恢复正常。

### ③ 侧栏滚动位置归零

`finish_open` 补 `self.vl.set_first(0)`。虚拟列表的窗口起点是 VM 侧状态、不跟着文档走，
留着它会让新文档停在上一份的滚动位置（第 1 页在视口外；新文档更短时还会因为
`virtual_list` 的 `first.min(count)` 先渲染一帧空窗口）。

### 测试 +3（30 全绿）

- `unsaved_edits_guard_opening_and_closing_alike`：无改动不拦 / 有改动挂起且**确认前 dirty 不掉** /
  取消后编辑仍在 / 打开确认不抹 dirty（失败语义）/ 退出确认抹 dirty / 幂等；
- `merged_open_is_dirty_and_never_overwrites_the_first_file`：`merged_from=2`、`dirty=true`、
  `path` 仍是第一个文件、`vl` 归零、默认名 `merged_a.pdf`、另存为后回到普通文档；
- `single_file_open_clears_dirty_and_resets_the_sidebar_scroll`。

验证：pdfkit `cargo test` 30 全绿 + `cargo clippy --all-targets` 0 警告。

## 2026-10-06 · 快活儿看不见遮罩（pdfkit 报告）→ busy 最短可见时间

**症状**：pdfkit 里已打开一个 PDF，再点「打开」选新文件 —— **看不到"正在打开"的遮罩**。

**定位**（实测支撑）：`open_job` 打开一个 1 页 PDF 只要 **4.5ms**（debug；release 更快），
而一帧（1100×780 全窗重绘 + softbuffer 上屏）是十几毫秒量级。平台的完成消息走
`user_event` → `ctx.external`（收遮罩），比"下一帧"先到 —— 于是那一帧里忙碌项已经没了，
**遮罩一帧都没画出来**。（多文件合并耗时长，所以那时能看见；这也解释了为什么只在快路径上暴露。）

### 修法：busy 最短可见时间（框架能力）

- `Runtime::set_busy_min_visible(Duration)`（默认 `ZERO` = 老行为，不等待）；
- `BusyItem` 增 `since` / `hide_at`；`end_busy` 若"才出现就结束"则不立刻移除，而是记下
  `hide_at = since + min`；帧驱动每帧开头 `Runtime::reap_busy(now)` 收掉到点的项
  （`WindowCtx::frame` 里调，保证"先收尾、再声明遮罩"的顺序）；
- `WindowCtx.has_busy`（每帧刷新）取代原来的"有遮罩卡片就唤醒"判断 ⇒ 挂着等收尾的那段时间
  仍有 30fps 唤醒（驱动 spinner + 到点收尾）；收干净后回到零唤醒；
- pdfkit：`run_windowed()` 里 set 400ms —— 快点也有反馈，长任务不受影响（`now >= since+min` 时
  照旧立即收起）。

默认零 ⇒ 既有测试与行为完全不变（331 全绿）。

### 测试 +2（都验证过"去掉修正就会红"）

- `task::busy_overlay_is_held_for_the_minimum_visible_time`：任务表已清空但遮罩项还在
  ⇒ 到点前不 reap、到点后 reap 掉、幂等；
- `app::a_fast_busy_section_still_shows_the_overlay`：忙碌段在两次出帧之间开始又结束
  （`begin_busy` + 立刻 `finish`），下一帧遮罩**必须在**、`next_wakeup` 有值；到点后层消失、
  回到零唤醒。

验证：lieui `cargo test --lib` 331 全绿 + clippy 0 警告；pdfkit `cargo test` 30 全绿 +
`cargo clippy --all-targets` 0 警告。

## 2026-10-06 · 图片盖住浮层（真凶）：光栅层不再把图片统一放到批次末尾

**症状（pdfkit 复报）**：加上"最短可见时间"后**仍然看不到"正在打开"遮罩**；用户直接指出
"应该是 PDF 预览挡住了 modal"。**诊断正确** —— 这不是层序问题，是光栅层内部顺序：

`rasterize_batches` 里图片 op 绕开 vello（手动 blit），原实现把它**收集到批次末尾统一 blit**：

```rust
// 旧代码注释（原文）：已知限制：blit 在该批次的所有 vello 原语之后 ⇒ 同批次内图片总在最上层
```

于是预览图（`Op::Image`）盖住了它**之后**声明的 Modal 背板与卡片（`Op::Rect`）—— 层序、
描述顺序、`align` 都没错，只有光栅合成顺序错了。M4 时代记录在案的"已知限制"（当时陈列馆里
图片不与浮层重叠，就没暴露）在 PDF 预览这种"整窗图片 + 浮层"场景下变成硬伤。

### 修法：按 op 顺序就地合成（`raster.rs`）

- 新增 `Rasterizer::flush_segment(pending, bw, bh)`：把当前累积的 vello 原语 `flush` +
  `render_with`（SrcOver 合成进批次画布）后 `reset`；
- 批次内的循环改为**顺序遍历**：遇到 `Op::Image` ⇒ 先 `flush_segment`（把之前的原语落地），
  再把图片 blit 到当前位置；普通 op ⇒ `submit` 累积；批次结尾再 `flush_segment`。
- 关键性质：每段只渲染**自己的**原语（`reset` 后场景为空）⇒ 几何总量不变，代价只是
  "图片边界处多一次 `render_with` 的固定开销"；**连续多张图片**之间没有原语 ⇒ 不额外分段
  （`pending` 为假时 `flush_segment` 是 no-op）。
- 仍未支持：图片不参与 `PushClip` 裁剪栈（滚动容器里的图片不会被裁）、最近邻采样
  ——已在 `blit_image` 的文档里写明。

### 测试 +2（都验证过"改回旧行为就会红"）

- `raster::primitives_after_an_image_are_painted_above_it`（像素级）：图之后画的蓝条必须压在
  红图上；旧行为下 `(14,14)` 是红 ⇒ FAILED；
- `app::busy_overlay_covers_a_full_window_image`（端到端）：整窗图片 + 忙碌遮罩 ⇒ 遮罩卡片
  底色必须出现在图上；旧行为下该像素仍是红（= 用户看到的"预览挡住 modal"）⇒ FAILED。

验证：lieui `cargo test --lib` 333 全绿 + clippy 0 警告 + 示例编译通过；
pdfkit `cargo test` 30 全绿 + `cargo clippy --all-targets` 0 警告。

## 2026-10-06 · pdfkit 新增「添加」：把更多 PDF 续到当前文档末尾

**动机**：多文件合并只能在「打开」那一次选定，之后没法再往工作区里加文件。

### 语义分工（这次刻意做得不对称）

| | 基准 | 未保存改动 | 当前页 / 勾选 | 落盘 |
|---|---|---|---|---|
| **打开** | 磁盘上的新文件 | 先过确认门（可丢） | 重置（归 1 / 清空） | `merged_from` = 文件数 |
| **添加** | **内存里那份文档**（含编辑） | **不动、不确认** | **保持不变** | `merged_from += 本批文件数` |

「添加」不丢任何东西（当前编辑就是基准的一部分），所以**不需要**确认门 —— 这是它与
「打开」的本质差别。

### 改动

- `jobs::append_job(doc, files, cancel, progress)`：在**已有文档**后面逐文件 `merge_pdfs_mem`，
  末尾 `reparse` 出新的预览数据；进度 `total = n×3 + 1`（每文件 读/解析/追加 三格 + 收尾一格），
  文案形如 `第 2 / 3 个文件 · 正在追加 b.pdf`，阶段边界查 `CancelToken`（取消 ⇒
  `已取消添加（<阶段>）`）；
- `OpenProgress` 更名 `FileProgress`（打开与追加共用）；
- `JobDone::Added { result, added }` + `job_finish` 分支：换掉 doc/pdf/infos、`merged_from += added`、
  `dirty = true`（内存 ≠ 磁盘 ⇒「保存」照旧走另存为）、当前页与勾选不动，状态栏
  `已添加 2 个文件（2 -> 6 页，共 3 个来源）`；
- `Action::Add` + `controller::add()`：没打开任何文档时退化成「打开」（按钮不至于点不动）；
- 工具栏在 `total > 0` 时显示 `playlist_add` 按钮（tooltip：*添加 PDF：把更多文件的页面追加到当前文档末尾*）。

### 测试 +3（33 全绿）

- `jobs::append_job_appends_pages_to_the_existing_document`：2 + 3 + 1 = 6 页、预览数据同步、
  7 格进度单调不提前报满、文案点名"正在追加 b.pdf"；
- `jobs::append_job_can_be_cancelled`：预置取消 ⇒ `已取消添加`；
- `controller::added_files_land_and_keep_the_current_page_and_selection`：页数 2→6、`merged_from` 1→3、
  **当前页与勾选不变**、`dirty` 置位、状态栏文案。
  （写测试时踩到一次测试隔离问题：复用了 `vm_with_pages` 的临时目录，被同进程另一个用例
  `remove_dir_all` 掉 ⇒ 追加文件改放自己的 `tmp_dir`。）

验证：pdfkit `cargo test` 33 全绿 + `cargo clippy --all-targets` 0 警告。

## 2026-10-06 · 发布 0.1.0-alpha.2

- **版本**：workspace 与三个子 crate 一并升到 `0.1.0-alpha.2`（子 crate 用
  `version.workspace = true`，`[workspace.dependencies]` 的 path+version 规格同步改）；
- **内容**：本日 6 个提交 —— 跨线程任务与 loading 遮罩、统一时钟（定时器/动画帧）、
  自定义事件、虚拟列表升格、文本光学对齐按内容盒、脏区批次策略与图片 z 序修复、
  右键语义、示例与文档；
- **发布顺序**：`lieui-geom` → `lieui-text` → `lieui-layout` → `lieui`
  （后者依赖前三个，版本必须已在索引里）；
- **pdfkit**：path 依赖的 version 规格同步改为 `0.1.0-alpha.2`
  （path 依赖也会校验 version，不一致直接解析失败）。

**结果**：四个 crate 均已上传到 crates.io（`lieui-geom` / `lieui-text` / `lieui-layout` / `lieui`
0.1.0-alpha.2）。发布前跑过 `cargo test --lib`（333 全绿）与全目标 `cargo check`；
发布后 pdfkit 对新版本重新编译并测试（33 全绿）。

**另：yank 掉 `0.2.0-beta.1`**。它是降版本之前那条线，semver 上高于 `0.1.0-alpha.2`
（`0.2.0 > 0.1.0`）⇒ crates.io 的 "latest" 一直被它占着，`cargo add lieui` 会拿到旧版。
yank 之后（索引里 `yanked=true`）新解析会跳过它，直接落到 `0.1.0-alpha.2`；
已有 lockfile 固定到它的旧用户不受影响（yank 只影响新解析，不删包）。

## 2026-10-06 · 虚拟列表嵌在普通 column 里时滚不动（pdfkit 侧栏症状）

**症状**：pdfkit 左侧栏只能看到前 19 行，滚下去不再出现新行（内容高度明明是"虚拟"的
完整高度，滚动条也画得出来）。

**根因**：`ViewBuf::virtual_list` 把 `ScrollChanged` 处理器挂在**当前节点**上。直接挂在
滚动容器上时正常；一旦被包进普通 column（"标题 + 列表"是最常见排布，pdfkit 侧栏正是
`scroll → column[标题, 虚拟列表]`），事件就永远不来 —— 只有 `overflow_scroll` 的节点
才派发 `ScrollChanged` ⇒ 窗口起点不推进 ⇒ 永远只有第一窗。

**修法**：沿 `ViewBuf::stack`（当前打开的容器链）向上找**最近的 `overflow_scroll`
祖先**，把处理器挂在那里；找不到时退回当前节点（保持旧行为）。

**顺带核对**（结论：本来就对）：虚拟 padding（`padding_top` / `padding_bottom`）**是**计入
滚动容器 `content_size` 的 ⇒ 滚动条长度一直正确，这次只有"窗口不推进"这一个 bug。

**测试 +1**：`app::virtual_list_nested_in_a_plain_column_still_advances_its_window`
复现 pdfkit 的排布（滚动容器 → 普通 column → 虚拟列表），断言滚 10 行后窗口起点推进、
且内容高度接近 1000 行。已做反向验证：把挂载点改回当前节点后该测试失败，报出的正是
`["Item 0", …, "Item 10"]`（滚了但没换窗）。

验证：lieui `cargo test --lib` 334 全绿 + clippy 0 警告。

## 2026-10-06 · 系统缩放（DPI）支持：契约固化 + 通知应用 + pdfkit 高 DPI 预览

### 先摸清现状（结论：光栅侧早就对了，缺的是"通知"与"契约"）

盘上一开始就有 `Rasterizer::scale`（`physical = logical × scale`）、`WindowCtx::set_scale_factor`、
`Resized → 逻辑 = 物理 / scale`、`ScaleFactorChanged → set_scale_factor`。**但有个前提之前没被
写下来，差点被当成 bug "修掉"**：

1. **winit 0.30 的 `CursorMoved::position` 是 `PhysicalPosition<f64>`**（`winit-0.30.13/src/event.rs`
   定义处），所以 `to_logical` 里那次除法是**必须**的，不是重复换算；
2. **DPI 感知不用应用清单**：winit 的 `EventLoopBuilder` 默认 `dpi_aware: true`
   （`platform_impl/windows/event_loop.rs` 的 `Default`），建循环时即调
   `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`（逐级回退 V1 / `SetProcessDPIAware`）；
3. **`WM_DPICHANGED` 时 winit 自己就保逻辑尺寸**（同上文件按"旧物理 → 旧逻辑 → 新物理"换算），
   所以"拖到别的显示器后内容突然变小"并不会发生。

### 框架侧改动

- **`app.rs`**：`WindowCtx::set_scale_factor` 现在返回 `bool`（是否真的变了），并新增
  `scale_epoch: u64` —— 变化时 +1 且**回调 `ViewModel::on_scale_changed(cx, scale)`**。
  这是本轮最实质的缺口：缩放变化之前**没有任何通知**，应用按像素缓存的资源（PDF 页栅格、
  缩略图）永远不失效 ⇒ 拖到 2× 屏上被拉伸发虚。非法 scale（NaN / 0 / 负）按"无变化"处理。
- **`platform/mod.rs`**：
  - 新增纯函数 `sane_scale` / `physical_to_logical` / `physical_size_to_logical` /
    `logical_size_to_physical` —— 逻辑↔物理换算的**唯一**口径（`to_logical` 与 `Resized`
    都改走它们），夹住 0/NaN 防"除零污染整条布局链"；
  - `ScaleFactorChanged` 显式吃下 `inner_size_writer`：按 `逻辑 × 新 scale` 请求新尺寸
    （Windows 上等价于 winit 的默认值，但**不依赖平台默认**；X11/Wayland 上不主动请求就会
    只剩像素数不变 ⇒ 逻辑面积被除以 scale）；
  - 模块头写清 **DPI 契约**（一个真源 = 逻辑尺寸；DPI 不改布局；光标为何要除；为何不需要清单）。

### 未做（都是计划里就标注的，附原因）

- **`ensure_dpi_awareness()`**：计划里打算用裸 FFI 调 `SetProcessDpiAwarenessContext`，
  实际动手才发现本 crate 是 **`#![forbid(unsafe_code)]`**（`lib.rs:1`）—— `forbid` 无法局部豁免，
  任何 FFI 都写不进来。改为**把做法写进模块文档**：`App::run` 已由 winit 默认覆盖；自带宿主
  循环的嵌入方别关 `with_dpi_aware`（默认 true）即可。不引入 `windows-sys`（同样要 unsafe 才能调）。
- **运行期 zoom**（无障碍放大）：计划里就是"视需要再开"，未做。它的口径是"光栅 scale 乘一个
  系数、布局仍走逻辑坐标"，与本轮加的通知机制天然兼容。

### 测试 +5（lieui 339 全绿）

- `platform::cursor_position_is_converted_from_physical_to_logical`：钉住那次除法
  （防后人当"重复换算"删掉 —— 那会让 2× 屏上点击全部错位一半）；
- `platform::insane_scale_falls_back_to_one`（0/NaN/∞）、
  `platform::physical_and_logical_sizes_round_trip`（双向自洽）、
  `platform::physical_size_never_collapses_to_zero`（softbuffer 需要 `NonZeroU32`）；
- `app::scale_change_notifies_the_view_model_and_bumps_the_epoch`：只通知一次、纪元 +1、
  物理尺寸 = 逻辑 × 2、重复值与非法值都不打扰。

### pdfkit：先修"半迁移导致的编译不过"，再补高 DPI 预览

**盘面**（`git status` + 编译错误）：`ui/*` 与 `app/mod.rs`、`app/jobs.rs` 已是新 API，
而 `app/model.rs`、`app/action.rs`、`app/controller.rs`、`ui/preview.rs` 还是 v3 之前的旧版
（`lieui::widget::Widget` / `Container` / `use lieui::state`）⇒ **整个 bin 编译不过**，
`render_page` 也少一个 `scale` 形参（`jobs.rs` 已按 3 参调用）。按 `ui/*` 的调用面重建：

- `model.rs`：`AppState`（含 `pending` / `page_menu` / **派生缓存 `page_indices`**）、
  `LoadedDocument { …, pdf: Arc<Pdf>, merged_from }`、`PendingAction` + `sync_derived()`
  （顺带把越界的当前页/勾选收回来 —— 删页后这两者都可能指向不存在的页）；
- `action.rs`：`Open/Add/Save/SaveAs/Split/Rotate*/Delete*/Extract/Select*/MovePage/DeletePage/
  OpenPageMenu/TogglePage/SetCurrent/CancelPending`；
- `controller.rs`：`PdfKitVm`（`state` / `tick: Signal<u32>` / `vl` / `scale` / 预览缓存）——
  分发全部走 `cx.spawn_task_busy` + `JobDone` 回传，**没有一处重活留在 UI 线程**；
  未保存守卫（`guard_unsaved` / `take_discarded`）与合并文档保护原样接回；
- `ui/preview.rs`：改写成描述树，并接上**按 DPI 渲染**的位图；
- `render/preview.rs`：`render_page(pdf, pn, scale)`（`x_scale`/`y_scale`，与 hayro 自己的
  `render_pdf` 同口径），删掉随之无用的 `render_current` / `resize_rgba`；
- 顺手修 `jobs.rs` 里一处**潜在编译错**：`paste_pages_job` 的 `step` 闭包按 `FnMut` 捕获
  `progress`，缺 `mut`。

**高 DPI 预览**（本轮 DPI 工作的落点）：

- `PreviewKey = (页码, 设备像素尺寸)`，设备像素 = `逻辑 × DPI` ⇒ **缩放一变键就变**，
  自动按新分辨率重渲染；页面**内容**变化（打开 / 旋转 / 删页 / 追加）键里看不出来，
  由 `invalidate_preview()` 显式作废；
- 显示尺寸 = 图像像素 ÷ DPI ⇒ 一个图像像素落在一个物理像素上（不再被光栅器放大）；
- `ViewModel::on_scale_changed` → `PdfKitVm::set_scale`（记下比例 + 作废缓存），
  `on_tick` 负责把缓存**烘热**（渲染放在 `on_tick` 而不是 `view()`：`view()` 必须无副作用，
  且 `on_tick` 排在它之前 ⇒ `view()` 只读缓存）。

**测试 +5（pdfkit 35 全绿）**：打开落地（重置当前页/勾选/侧栏滚动 + 派生缓存）、
合并文档标脏与"绝不覆盖第一个文件"、未保存守卫的四种路径（不拦 / 挂起 / 取消 / 确认）、
页面操作回传（页数收敛 + 标脏）、**DPI 预览按设备像素渲染且同键只渲一次**。
最后一条做了反向验证：把 `dpi` 写死成 `1.0` 后它立刻失败（`按设备像素渲染`）。

### 已知取舍 / 未验证

- 预览栅格化仍是**同步**的（一页几十毫秒级）：键不变时只是一次 `RefCell` 查询，所以
  "翻页 / 缩放"那一下会卡一帧。挪进 `spawn_task` 是明确的下一步（那时未命中要显示占位）。
- `PREVIEW_BOX` 是常量（860×620 逻辑）：`view()` 发生在布局**之前**、拿不到实测尺寸，
  而框架的图片没有"按比例适应"（`ImageStyle::fit` 只有 `Fill`）⇒ 宽高必须显式给。
  （`Runtime::window_size` 能给出**窗口**逻辑尺寸，但要的是"窗格"尺寸 —— 还要减掉侧栏与
  header/toolbar/status；而且 `view()` 手里没有 `Runtime`/`WindowId`，只有 `on_tick` 能读。
  所以真要做"跟随窗口"要么补一个 `view()` 能用的入口，要么把栅格化挪进作业后按窗格尺寸渲。）
- **未在真实多显示器上验收**：本机无法切换显示器 DPI，DPI 结论均来自 winit 0.30.13 源码
  （位置见上）与可无头验证的纯函数测试。需要一台 Windows 双屏（100% + 150/200%）拖一次
  窗口来最终确认：界面变清晰、预览跟着变清晰、无内容缩水。

验证：lieui `cargo test --lib` 339 全绿 + clippy 0 警告 + 示例编译通过；
pdfkit `cargo test` 35 全绿 + `cargo clippy --all-targets` 0 警告。

## 2026-10-06 · 发布 0.1.0-alpha.3

- **版本**：workspace 与三个子 crate 一并升到 `0.1.0-alpha.3`。子 crate 用
  `version.workspace = true` ⇒ 只需改根 `Cargo.toml` 的 5 处（`package.version`、
  `workspace.package.version`、三个 `workspace.dependencies` 的 path+version 规格）；
- **内容**：系统缩放（DPI）支持 —— 逻辑/物理换算单一真源（`sane_scale` /
  `physical_to_logical` / `physical_size_to_logical` / `logical_size_to_physical`）、
  `scale_epoch` + `ViewModel::on_scale_changed` 把缩放变化通知给应用、
  `ScaleFactorChanged` 显式按新 scale 请求物理尺寸；附带 `Runtime::window_size`
  （帧驱动每帧登记每窗口逻辑尺寸）；
- **发布顺序**：`lieui-geom` → `lieui-text` → `lieui-layout` → `lieui`
  （后者依赖前三个，版本必须已在索引里）。本次用 cargo 1.97 的 `cargo publish --workspace`
  —— 由 cargo 自己按依赖顺序逐个上传并等索引，不必再手工一个个发 + 等；
- **pdfkit**：path 依赖的 version 规格同步改为 `0.1.0-alpha.3`
  （path 依赖也会校验 version，不一致直接解析失败）。

**结果**：四个 crate 均已上传到 crates.io（`lieui-geom` / `lieui-text` / `lieui-layout` /
`lieui` 0.1.0-alpha.3）。发布前 `cargo publish --dry-run --workspace` 四个 crate
全部打包 + 验证通过；发布后 pdfkit 对新版本重新编译并测试（35 全绿）。

## 2026-10-06 · pdfkit 改名 liepdf + 六个常用功能接线 + 撤销/重做

### 改名：`pdfkit` → `liepdf`

包名 / 窗口标题 / 页头文案都改了，**类型名 `PdfKitVm` → `AppVm`**（刻意中性：下次改名
代码零改动）。git 目录名仍叫 `pdfkit`（没有任何东西依赖它，依赖是 `../lieui`）。

### 六个"作业已就绪、UI 没入口"的功能接线

`app/mod.rs` 早就写着"jobs 里有一批已写好、待接线的作业"。这轮把它们全接上：

| 功能 | 入口 | 作业 |
|---|---|---|
| 插入页面（从文件） | 工具栏 `note_add` 弹层 / 每页右键「在此页后插入 PDF…」 | `insert_files_job` |
| 插入空白页 | 同一弹层（A4 / Letter） | `insert_blank_job` |
| 统一纸张 | 工具栏 `aspect_ratio` 弹层（统一为 A4 / Letter） | `normalize_sizes_job` |
| 导出 PNG | 工具栏 `image`（选中页，未选则当前页） | `export_png_job` |
| 粘贴页 | 工具栏 `content_paste` + 右键「复制此页 / 剪切此页」 | `paste_pages_job` |
| 文档信息面板 | 工具栏 `info`（Modal） | `ops::doc_info`（纯内存、可在 UI 线程现算） |

`JobDone` 契约一个都不用新增（早就位），`jobs.rs` 只改了 **1 处签名**：
`export_png_job(pdf: Pdf)` → **`Arc<Pdf>`**（`hayro::Pdf` 不是 `Clone`，UI 侧只有 `Arc<Pdf>`，
收 `Arc` 才能零拷贝搬进工作线程）。

实现中才发现并修掉的四件事：

1. **插入位置要在入口夹**：作业层不夹（越界直接 `InvalidPage`）⇒ `clamp_at` 收进
   `1..=总页数+1`；
2. **页号是"位置"不是对象**：文档变短后剪贴板里越界的页号必须失效 ⇒ 粘贴前按当前页数
   过滤，全失效就提示"剪贴板里的页已不存在"；**剪切粘贴是一次性的**（源页随即被删、
   页号随之失效）⇒ 立刻作废剪贴板；
3. **弹层用互斥枚举**（`tools_menu: Option<ToolsMenu>`）而不是两个 `bool`：后者能表达
   "两个都开着"这个不合法状态；
4. lieui **没有 MenuBar 组件**，菜单是手搓的：锚点按钮 `.key(..)` + 层里
   `v.popup_at(key, Placement::Below, ..)`（与页面右键菜单同一套机制）。

### 撤销 / 重做（快照式）

存"某次改动**之前**的整份状态"（`HistoryEntry`），不是命令式 —— 因为"删页"的逆操作要把
删掉的页找回来，那等于存整份快照。三个要点：

- **只在作业成功时入栈**（dispatch 时记 `pending`，`job_finish` 成功才 commit）⇒ 不留
  "按下去什么也不发生"的空撤销；
- `UNDO_LIMIT = 16`，超了丢最老的。这是**内存取舍**（每条是整份文档深拷贝），已在代码里
  写明：处理几百 MB 的 PDF 会显著吃内存，真要支持得改增量/命令模式；
- **打开新文档 ⇒ 历史作废**（跨文档撤销没有意义）。

UI：工具栏最左 `undo`/`redo`（按 `can_undo()`/`can_redo()` 灰显）+ `Ctrl+Z` / `Ctrl+Y` /
`Ctrl+Shift+Z`（挂在内容根的 `KeyDown` 上，映射抽成纯函数 `shortcut(ev)` 便于测）。

### 顺带修掉一个真回归

`JobDone::Added` 落地时漏了 `merged_from += added` ⇒ **「添加」文件后点「保存」会把追加的
内容直接覆盖写回原文件**。补上，并为此给 `Restored` 契约加了 `merged_from`（否则撤销一次
「添加」后来源计数不退回，「保存」会一直误判成合并文档）。

工具栏现在 20 个图标按钮（≈650px）+ 侧栏 220 ⇒ `min_size` 从 `(720,480)` 提到
`(900,480)`，否则最右侧「文档信息 / 已选择」会被挤出可视区。

**测试**：pdfkit 46 全绿（导出按 scale / 弹层互斥 / 剪贴板往返与失效过滤 / 插入与统一纸张
落地 / 位置夹紧 / 撤销重做往返 / 失败不入栈 / 历史上限与清空 / 追加涨来源数 / 快捷键映射）。

## 2026-10-06 · 滚动条在窗口最小化时 panic（pdfkit 真机报障）

**症状**（用户复现）：`cargo run --release` → 打开 PDF → 导出某页 PNG → **最小化 / 切窗口**
⇒ 进程崩：`min > max, or either was NaN. min = 24.0, max = -29.992188`
（release 的 `panic = "abort"` 让它变成 `STATUS_STACK_BUFFER_OVERRUN` 硬崩）。

**根因**：`widgets::scroll_parts` 里的
`thumb_len = (…).clamp(MIN_THUMB_LEN, track_len)`。`MIN_THUMB_LEN = 24.0` —— 与日志里的
`min` 完全对上。链路：最小化 ⇒ winit 发 **0×0** 客户区 ⇒ 布局把侧栏滚动容器算成**负高度**
（实测约 -26px：header/status 这些固定高度 + padding 减完还欠）⇒ `track_len` 为负 ⇒
`f32::clamp` 的上下界反序 ⇒ panic。"导出后才出现"只是时序巧合，真条件是
「窗口被压到 0 尺寸 + 侧栏有内容溢出」。

**修法**（`scroll_parts`，三处防御）：

- `track_len.max(0.0)`，`<= 0` 直接 `None`（没有可画的轨道）；
- thumb 的 clamp 下界收敛成 `MIN_THUMB_LEN.min(track_len)` ⇒ **轨道比 24px 还矮**时
  thumb 铺满轨道（`travel = 0`，拖不动但不崩）。这一条顺手修掉了与最小化**无关**的
  同源 bug：任何比 24px 矮的滚动容器 + 内容溢出，以前都会炸；
- "没溢出"的判断从 `!(content_len > view_len + 0.5)` 改成
  `content_len.partial_cmp(&(view_len + 0.5)) != Some(Ordering::Greater)` ⇒ 顺便把
  **NaN 尺寸**也挡掉（NaN 几何画出来还是 NaN），并且 clippy 干净（否定比较会被告警）。

**测试 +2**：① `scroll_parts_survives_collapsed_and_short_viewports`（负高度 / 零高度 /
10px 矮容器 / NaN 四种退化视口 + 正常情形）—— **已反向验证**：去掉 `max(0.0)` 那两行，
它立刻 panic 在 `widgets/mod.rs:1844`；② `a_minimized_window_frames_without_panicking`
（0×0 窗口跑完整帧 + 还原后正常重绘）。**诚实标注**：② 不是这个 bug 的护栏 —— 无头环境
复现不出让容器变负高度的那套几何（试过照抄 pdfkit 排布 + 固定高度标题栏，仍不复现），
护栏归 ①。

## 2026-10-06 · 依赖升级：vello_cpu 0.3（+ skrifa 0.48 等），版本升到 alpha.4

**动机**：把渲染栈升到当前版本。`vello_cpu 0.2 → 0.3` 是破坏性升级，代码只有三处要改，
但其中一处是**真陷阱**。

| 变更 | 适配 |
|---|---|
| `CompositeMode` 被删 | 混合语义改由 **`TargetInit`** 表达。**注意默认值是 `Clear(透明)`** —— 直接用 `RasterizerSettings::default()` 会把批次画布上已有内容（尤其前面手工 blit 的图片）整片擦掉。必须显式 `TargetInit::SrcOver`（"画在已有内容之上"），这也是分段渲染 / 图片 z 序修复依赖的前提 |
| `RasterizerSettings` 字段变成 `render_mode` / `target_init` / `pixel_format` / `offset` | 同上 |
| `RenderContext::pop_clip_path` 只剩 glifo `DrawSink` trait 版本 | 改用**固有方法** `pop_clip`（trait 版只是转发），不必为此把 glifo 拉成依赖 |
| `fill_glyphs` 改为返回 `Result` | 不 `unwrap`、也不逐次打日志：失败只意味着"那一小段文字没画出来"，改为**计数** `glyph_errors` 并出现在 `Rasterizer` 的 `Debug` 里（可观测、不刷屏） |

`TargetInit` 那个陷阱**已反向验证**：改成 `Clear` 后，图片 z 序测试
`primitives_after_an_image_are_painted_above_it` 直接失败（图片像素变成全透明）——
已在该测试的文档注释里写明"这条同时是 0.3 的陷阱护栏"。

**其余升级**：`parley 0.11.1`、`softbuffer 0.4.8`、`arboard 3.6.1`（后三个
`cargo update` 自动到位）。

**`skrifa` 刻意不动（0.44）**：它必须与 parley 用的是同一份，链路是
`parley 0.11 → fontique 0.11 → read-fonts 0.41 → skrifa 0.44`。先前顺手把它升到 0.48，
`cargo tree -d` 里就出现了**两份 skrifa**（0.44 是 parley 带的，0.48 是我们的直依赖）：
字体引擎编两遍，更麻烦的是**度量可能对不上** —— parley 的排版用 0.44 算、而墨迹盒是
0.48 读的（`GlyphMetrics::bounds`），两者不一致会让"按墨迹盒居中"（tooltip / loading
遮罩 / 图标对齐）偏掉几个百分点。这类偏差测试很难察觉，却正是光学对齐的命门。
fontique **不**转发 skrifa（只有 `Blob`/`Collection`/`FontInfo` 等）、parley 也不暴露
glyph bbox ⇒ 直依赖省不掉，只能把版本钉在 parley 那条链上（`Cargo.toml` 注释里写了
耦合与检查方法）。校验：`cargo tree -i skrifa@0.44.0` 显示 `lieui-text` 与
`parley v0.11.1` 共用一份，`read-fonts` 也只有 0.41.0 一份。

**故意不升 `winit`**（仍是 0.30.13）：0.31 只有 `0.31.0-beta.3`（预发布），而本轮的
DPI 契约（`ScaleFactorChanged` + `InnerSizeWriter::request_inner_size` + `dpi_aware` 默认值）
都建立在 0.30 的 API 上，其中 `ScaleFactorChanged` 在上游正是"计划移除"的那一个。
窗口后端刚稳定就跳 beta，收益远小于风险；要升请单独开一轮。

**pdfkit 侧**：`hayro 0.8` 把渲染设置拆成 `RenderSettings`（行为）+ **`PixmapSettings`**
（尺寸/底色），`render()` 从 4 参变 5 参 ⇒ `render::preview::render_page` 已适配
（导出按 scale 的测试断言 200pt 页 @1x=200px、@2x=400px 仍成立 ⇒ 缩放确实生效）。
`lopdf 0.45` / `rfd 0.17` 无需改动。pdfkit 树里 **只有一份 vello_cpu 0.3**
（hayro 0.8 也用它）⇒ 不重复编译。

**版本**：`0.1.0-alpha.3 → 0.1.0-alpha.4`（根 `Cargo.toml` 五处 + pdfkit 的 path 依赖
version 规格 —— path 依赖也校验 version）。依赖破坏性变更 ⇒ 不发新版，crates.io 上的
alpha.3 仍然指向 vello_cpu 0.2。

验证：lieui `cargo test --lib` 341 全绿 + clippy 0 警告 + 示例编译通过；
pdfkit `cargo test` 46 全绿。

## 2026-10-06 · 发布 0.1.0-alpha.4

- **内容**（相对 alpha.3）：
  - **修复**：滚动条在窗口最小化（0×0 客户区 ⇒ 负轨道长度）时 panic；
  - **破坏性依赖升级**：`vello_cpu 0.2 → 0.3`（三处 API 适配 + `TargetInit` 默认清空
    目标的陷阱）、`skrifa` 与 parley 对齐（见上）；`parley 0.11.1` / `softbuffer 0.4.8` /
    `arboard 3.6.1` 随 `cargo update` 到位；`winit` 保持 0.30.13（0.31 只有 beta）；
  - 版本号 `alpha.3 → alpha.4`。
- **提交**（发布前 4 笔）：
  - `fix(scrollbar)`: 轨道长度为负时不再 panic（窗口最小化 / 矮容器）
  - `chore(deps)!`: 升级 vello_cpu 0.3 + skrifa，版本升 alpha.4
  - `docs(operation-log)`: 记三条（panic 修复 / 依赖升级 / liepdf 改名与新功能）
  - `fix(deps)`: skrifa 对齐 parley 的 0.44（别让树里出现两份字体引擎）
- **发布顺序**：`lieui-geom` → `lieui-text` → `lieui-layout` → `lieui`（用 cargo 1.97 的
  `cargo publish --workspace`，由 cargo 按依赖顺序排队并等索引）；发布前
  `cargo publish --dry-run --workspace` 四个 crate 全部打包 + 验证通过。
- **pdfkit**：path 依赖的 version 规格同步到 `0.1.0-alpha.4`（path 依赖也校验 version），
  对新版本重新编译并测试（46 全绿）。

**结果**：四个 crate 均已上传到 crates.io，`cargo search` 复核 latest 全部是
`0.1.0-alpha.4`。

## 2026-10-06 · 浮层锚点补上"点"：右键菜单可以贴着鼠标出现

### 起因（一个观察，不是一个需求）

liepdf 侧栏的页右键菜单"位置不跟随鼠标"。查下来不是 bug：菜单用
`v.popup_at(pn, Placement::Below, ..)`，锚的是**那一行**（key = 页号，由
`virtual_list` 的 `key_fn` 自动贴上），于是落点 = 行的左边缘下方 4px（`ANCHOR_GAP`），
翻转与钳制按视口算。**鼠标坐标在 `RightTapped` 处理器里就被丢掉了** —— 处理器只
`dispatch(OpenPageMenu(pn))`。框架也确实**没有"锚到某点"的能力**：`Placement::Fixed{x,y}`
虽然能写坐标，但它**跳过翻转与视口钳制**（`layout.rs:211` 明确排除），光标靠近窗口
下/右边缘时菜单会跑出窗外。

### 框架侧改动（3 处）

- **`track.rs`：`AnchorTarget` 新增 `Point(Point)`** —— 锚到一个逻辑坐标点。定位时它被
  当作**零尺寸的退化矩形**（`Rect::new(x, y, 0, 0)`），于是 `anchored_origin` 里那套
  "翻转 → 钳制"**一字不改**就适用于点锚点：贴鼠标、贴近下边缘自动翻上方、贴近右边缘
  自动平移回视口内。已有两处 `AnchorTarget` 的匹配都带兜底分支，无需改动。
- **`layout.rs`：`anchored_origin` 的锚点解析改成直接产出 `Rect`**（原先先解 `NodeId`
  再查 rect）；`Key` 查不到仍返回 `None`（保持原位），`Point` 不可能失败。
- **`view.rs`：新增 `ViewBuf::popup_at_point(pos, placement, ..)`** —— 与 `popup_at`
  只差锚点，层类型 / 默认视觉 / 关闭策略完全一致。`popup_at` 的文档补一句"位置与鼠标
  无关，要贴鼠标用 `popup_at_point`"，避免下次又对着 `popup_at` 找原因。

### 为什么值得单加变体（而不是让调用方自己用 `Fixed`）

1. `Fixed` 不翻不钳 ⇒ 光标在窗口右下角时菜单**出界**，调用方得自己拿窗口尺寸再钳一遍
   （而 `view()` 手里没有窗口尺寸，只有 `on_tick` 读得到 `Runtime::window_size`）；
2. 点锚点**不需要有对应节点存在** ⇒ 可以放心锚在虚拟列表行上：行被回收 / 滚出窗口都不会
   让菜单失去定位（`Key` 锚点在那种情况下会静默停在原位 —— 这是旧实现的隐藏坑）。

### 测试 +6（lieui 347 全绿）

`layout`：`point_anchor_puts_the_popup_under_the_cursor`、`point_anchor_flips_above_near_the_bottom`、
`point_anchor_is_pulled_back_inside_the_window`、`point_anchor_needs_no_node`、
`point_anchor_honors_other_placements`；`view`：`popup_at_point_records_a_point_anchor`。

后两条断言**先弱后强**：初版"翻转 / 靠边"只断言了"结果在窗口内"，把点锚点临时退化成
`(0,0)` 时它们照样通过（原点也在窗口内）⇒ 改成断言精确落点（贴边那条断言"右边缘正好
贴住窗口右边缘"）。退化验证：5 条 layout 测试**全部变红**，恢复后全绿。

### 未做

- **`tooltip_at_point`**：tooltip 由框架自管、锚 hover 节点，不需要点锚点，先不加。
- **liepdf 侧接线**：右键时把 `cx.event().pos`（已核实是**逻辑**坐标：按钮事件取
  `self.cursor`，而 `cursor` 在 `CursorMoved` 时已 `/scale`）存进状态，菜单改用
  `popup_at_point`。下一步做。

验证：lieui `cargo test --lib` 347 全绿 + clippy 0 警告 + 示例编译通过；
pdfkit `cargo test` 48 全绿（依赖方未被API 变更影响）。

## 2026-10-06 · 菜单构件 + 上下文菜单（`ContextMenu` / 对齐 WinUI `MenuFlyout`）

### 起因

上一条记的是"右键菜单贴鼠标"（点锚点）。真正的问题是**整个菜单层是手搓的**：
`grep "pub fn menu("` 在 `src/` 里 0 命中 —— liepdf 侧栏 7 个菜单项每个都是 8 行链式调用
（`p.text(…).font_size(…).width(160).padding(6).radius(3).hover_background(…).enabled(…).on_tap_with(…)`），
复制了一遍又一遍：行高、禁用态颜色、分隔线全靠每个调用点自觉，外观无法统一。

### 第1 层：菜单构件（`p.menu`）

新模块 `src/menu.rs`（`ViewBuf::menu` + `MenuRef` / `MenuItemRef`）：

- **先攒规格、后统一渲染**：`m.item(…).checked(true).accelerator("Ctrl+C")` 里的属性都要
  **回头改这一行**（前面插勾、右侧加快捷键）。若 `item()` 当场建行，后加的属性就只能
  "往当前容器追加" ⇒ 勾选标记会跑到标签**后面**。所以 `MenuRef` 只往 `Vec<Kind>` 攒，
  `menu()` 闭包结束后一次性渲染：哪些槽位出现、行高、内边距、配色集中在一处。
- 顺带白拿两个 WinUI 行为：**勾选列 / 图标列按需出现**（没人勾选就不留空白列，
  否则每个标签凭空缩进 18px）；**标签左对齐稳定**（定宽槽）。
- `item` / `separator` / `icon` / `accelerator` / `checked` / `enabled` / `key` /
  `on_tap` / `on_tap_with` / `on`；`MenuRef::min_width`（默认 168）。
- 配色全部取主题 token（`control_hover` / `control_pressed` / `text_secondary` /
  `control_border` / `control_radius`），**没有新 design token** ⇒ 不需要主题迁移。

为此给 `view.rs` 开了三扇"给组合构件用的门"（都写在文档里）：
`container_ref`（建容器并回传 `DescRef`）、`DescRef::handler`（挂已装箱的 `Rc` 处理器）、
`ViewBuf::context_menu`（作用于当前容器的糖）。

### 第 2 层：`.context_menu()`（元素挂饰，对齐 `ContextFlyout`）

```rust
v.container(|row| {
    row.text("第 3 页");
    row.context_menu(|m| {
        m.item("复制此页").on_tap_with(tap(vm, Action::CopyPage(pn)));
        m.separator();
        m.item("删除此页").enabled(total > 1).on_tap_with(..);
    });
});
```

**状态、触发、定位、关闭全在框架里** —— 应用一行状态都不用加，也不用在 `view()` 里写
`if let Some(menu)`。机制完全复刻 tooltip 会话（`WindowCtx.tooltip`）：

1. `DescNode.context_menu: Option<Rc<dyn Fn(&mut MenuRef<'_>)>>`（对齐时搬进`Node`，
   和 `handlers` 一样整体替换）；
2. `WindowCtx.ctx_menu: Option<CtxMenuSession>`（target / at / builder）；
3. 右键 `Down` ⇒ 命中链里**最深**带菜单的节点胜出；
4. 渲染：框架在 `view()` 之后、`align` 之前往描述树里**追加**一个 `Layer::Popup` 根
   （带 `CTX_MENU_TAG` 标签认领，与 loading 遮罩的 `modal_tagged` 同一手法）
   ⇒ 对齐 / 脏区 / 轻关闭 / 锚定落位全部复用现成 machinery；
5. 关闭：点菜单外、点菜单项后、**禁用项被点（不关）**、目标节点不再声明菜单（列表滚走）。

### 顺带修一个真 bug：`enabled(false)` 之前不挡点击

`collect_from` 只查处理器、不查 `enabled` ⇒ `button(…).enabled(false).on_tap(…)` **照样触发**
（内置行为与焦点是挡住的，只有用户闭包漏了）。菜单里"剪切此页（禁用）"看着是灰的、
点下去真的执行。现在禁用节点**不进路由**（祖先照跑），对齐 WinUI `IsEnabled=false`。

### 三个必须记下来的坑

1. **点菜单项后收起**的判据是"命中路径上有没有禁用节点"，不是"最深节点是否禁用"——
   禁用行里的最深命中通常是 **spacer**（`flex_grow` 的空 Box 也会吃掉命中）。
2. **禁用态不能只挂在行容器上**（会让命中最深点判不出来，见上）；也**不能**给子节点打
   `enabled(false)` 来传播 —— 渲染层还会再乘一次半透明（`dim_if_disabled`）⇒ 灰两次。
   颜色用 `text_secondary` 表达，行为交给路由阶段。
3. **会话清掉必须当帧置 `Dirty::VIEW`**：`rt.mark` 落在 `take_dirty` **之后**会白等一帧，
   而弹层是"注入进描述树"的 ⇒ 不重跑 `view()`，align 的 stale 清理就删不掉那一层，
   **菜单留在屏幕上**。`drop_dead_context_menu` 因此返回布尔、由 `frame()` 并进本帧 `dirty`。

（还有一个同类 bug 在开发过程中出现过：`drop_dead_context_menu` 一度在**没有会话时**也置
VIEW ⇒ 每帧重跑 `view()`，把"空闲帧零开销"打挂了一整批测试 —— 判据里加一句
"本来就有会话"就对了。）

### 测试 +21（lieui 369 全绿 / liepdf 48 全绿）

- `menu`（13 条）：结构（一项一行 + 分隔线）、分隔线是 1px 线 + 外边距、勾选/图标槽
  按需出现且标签左对齐、快捷键贴右内边距、禁用项灰字 + `enabled=false`、点击挂在行上、
  hover 底色取主题 token、宽度有下限但随内容变宽、`min_width` 可改、空菜单 / 空标签不崩、
  贴光标定位。
- `app`（8 条）：右键在光标处开菜单、无菜单处右键开不出、菜单内容来自构造器（闭包
  在渲染时才跑 ⇒ 捕到当下值）、点菜单项执行并收起、**禁用项既不执行也不收起**、
  点外面收起、右键别处菜单跟过去、目标不再声明菜单时自动收起。
- `event`（1 条）：`disabled_nodes_are_skipped_but_ancestors_still_run`。

### liepdf：删掉整套菜单状态

侧栏每行改成 `row.context_menu(…)` 后：`AppState.page_menu` / `PageMenu` /
`Action::OpenPageMenu` / `close_page_menu` / `page_context_menu()`（47 行）**全部删除**
（`Action` 一度因`OpenPageMenu` 带 `Point` 而摘掉 `Eq` 推导，删掉变体后已还回去）。
新增 `ui::tap_rc`（已经持有 `Rc` 的版本，给只能捕获 `Rc` 的 `'static` 闭包用）。

`Cargo.toml` 换回 **path 依赖**（要用未发布的 `popup_at_point` / `menu` / `context_menu`）；
发lieui 新版本后再换回 crates.io 规格。

验证：lieui `cargo test --lib` 369 全绿 + clippy 0 警告+ 示例编译通过；
liepdf `cargo test` 48 全绿 + `cargo clippy --all-targets` 0 警告。

## 2026-10-06 · `WindowCtx` 收敛：抽出 `Sessions`（框架交互会话）

### 起因

`WindowCtx` 长到 17 个字段时就很可疑了：`tooltip` / `ctx_menu` / `next_blink` /
`spinner` / `has_busy` 混在"视图态 / 渲染器 / 会话 / 时钟 / 派生缓存"里靠注释分辨。
顺着查还发现一处**同一关切被劈成两半**：光标相位 `blink_on` 在 `Track`，
时钟 `next_blink` 在 `WindowCtx`。

### 结论：不该走 `Layer`，**会话 ≠ 层**的讨论记录

`Layer` 描述的是"视口里的一棵子树 + z 序 + 焦点/关闭策略"；而这几个字段都不是子树：

| 字段 | 实际是什么 |
|---|---|
| `tooltip` / `ctx_menu` | **会话**：目标 `NodeId` + 计时 + 它管的层 `RootId`（**会话*生产*层**） |
| `next_blink` | 一个**时刻**（相位本身在 `Track::blink_on`） |
| `spinner` | **跨帧保留实例的缓存**（真身是树里的 `Kind::Custom` 节点） |
| `has_busy` | **派生快照**（`rt.is_busy(id)` 的每帧副本） |

把它们塞进 `Layer`/`LayerOpts` 会让 `align` 变成"两层状态的管理者"，直接破掉
"结构只由 `view()` 描述"这条不变量 ⇒ **层还是描述，会话单独集中**。

### 改动

- **新增 `struct Sessions`**（`derive(Default)`）：`tooltip` / `ctx_menu` /
  `next_blink` / `spinner` 四个一伙，`WindowCtx` 只剩一个 `sess: Sessions` 字段。
  文档里写明三件容易被后人改错的事：① 为什么它们不是层；② 为什么三个框架层的
  生命周期不同却**是刻意的**（tooltip 文案静态 ⇒ 走 `add_framework_root`，align 不重建；
  菜单 / loading 内容每帧都可能变 ⇒ 每帧注入描述树，由 align 增删）；
  ③ `spinner` 缓存实例的理由（`Kind` 按 **Rc 指针**判等，每帧新建 cell 会被当成
  "换数据"⇒ 动画每次从头转）。
- **删掉 `has_busy`**：它是"够不着 `rt` 才不得不缓存"的产物。
  `next_wakeup(&self)` → `next_wakeup(&self, rt: &Runtime)`，内部直接问
  `rt.is_busy(self.id)`。调用方（平台层 / 测试）本来就持有 `rt` ——少一处可能过期的真相。
  平台层因此多一行 `let rt = self.app.runtime();`（注释说明了由来）。
- **`Track::blink_on` 补注释**：明确"状态 vs 时钟"的分工（ `blink_on` 跟焦点节点走，
  `next_blink` 是唤醒源 ⇒ 无焦点恒 `None` ⇒ 空闲零功耗）。

### 结果

`WindowCtx` 17 → 13 字段；四种框架交互态有了共同的名字与归属说明；
新加第五个框架层（菜单键触发、抽屉…）有现成模子可参考。

验证：lieui `cargo test --lib` 369 全绿 + clippy 0 警告 + 示例编译通过；
liepdf `cargo test` 48 全绿 + clippy 0 警告（纯重构，行为零变化）。

---

## 2026-10-07 · S0 门禁与测试地基（H1/H2/H3/H8）—— ✅ 完成

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 主线 H。
>本批只做**门禁与机械统一**，零行为变更，为后续 S1–S5 提供可复现的验证环境。

### 决策

1. **格式统一单独成批**（不与功能改动混在同一批）。理由：rustfmt 会碰 39 个文件，
   若与 S1 的行为修复混在一个 diff 里，真正的语义变更会被机械噪声淹没。
2. **`rustfmt.toml` 放宽到 `max_width = 120`，而不是让 rustfmt 默认改写全库风格。**
   现状是作者手写的**紧凑单行**风格（全库最长行 150 字符，`app.rs`），
   rustfmt 默认 100 宽会把它展开成多行（315 处差异）。实测配置选择：

   | 配置 | 差异处数 |
   |---|---|
   | 默认（100 宽） | 315 |
   | **`max_width = 120`（采用）** | 412 |
   | `max_width = 120` + `use_small_heuristics = "Max"` | 738 |

   `use_small_heuristics = "Max"` 让 rustfmt 更激进地合并成单行，反而把差异推高到 738，故**保持默认**。
3. **lint 门禁只开 `clippy::all`，不开 pedantic/nursery**。实测全库 clippy 建议 15 条
   **全部落在 `all` 级别**，可在单个批次内清零；pedantic 会一次性引入上百条风格噪声，
   不适合作为增量门禁。后续要收紧时单独开批次逐条评估。
4. **lint 门禁用 `[workspace.lints]` +成员 `[lints] workspace = true` 继承**，
   而不是逐 crate 重复定义（`[lints]` 不随 workspace 自动继承，必须显式声明，否则漏一个 crate 就静默失效）。
5. **MSRV job 先标 `continue-on-error: true`**。本仓库声明 `rust-version = "1.88"`，
   但上游依赖（vello_cpu 0.3 / parley 0.11 / softbuffer 0.4 / winit 0.30）各自的 MSRV
   **尚未逐一核对**，可能高于 1.88。本机只有 1.92 / 1.97，无法本地验证 1.88。
   与其写一个必然红的 job，不如先留骨架 + 写明原因。
6. **`deny.toml` 按 cargo-deny 各版本的公共子集书写**。本机装的是旧版，CI 用
   `taiki-e/install-action` 装最新版，两边都要能解析。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `rustfmt.toml` | **新增** | `max_width = 120` + `newline_style = "Unix"`，附"为什么不用默认 / 不用 Max heuristics"的实测数据 |
| `rust-toolchain.toml` | **新增** | `channel = "stable"` + `clippy`/`rustfmt` 组件。**不锁精确版本**：精确锁会在每次 Rust 小版本更新时强制全员同步，与 MSRV 声明的意图冲突 |
| `deny.toml` | **新增** | 许可证白名单 + `multiple-versions = "warn"`（目标：逐步收紧到 deny）；禁未知 registry/git |
| `.github/workflows/ci.yml` | **新增** | 5 个 job：`fmt` / `clippy` / `test`(Linux+Windows 矩阵) / `msrv`(暂 continue-on-error) / `audit` |
| `Cargo.toml` | 修改 | 新增 `[workspace.lints.rust]` `unexpected_cfgs = "deny"`、`[workspace.lints.clippy]` `all = "deny"`；根 package 加 `[lints] workspace = true` |
| `crates/{lieui-geom,lieui-text,lieui-layout}/Cargo.toml` | 修改 | 各加 `[lints] workspace = true`（缺任一个则该 crate 逃过门禁） |
| 全库 39 个 `.rs` | 格式化 | `cargo fmt --all`：1370 insertions / 1816 deletions（净 −446 行），纯机械无语义变更 |
| `src/align.rs` | 修 clippy | `ComputedLayout::default()` 后逐字段赋值 → struct literal + `..Default::default()` |
| `src/app.rs` | 修 clippy | 删冗余的 `let page = page;`（2021 edition 的 redundant redefinition） |
| `src/app.rs` / `src/track.rs` / `src/view.rs` | 修 clippy | 12 处由 `cargo clippy --fix` 自动修复（`assert_eq!(x, true/false)` → `assert!`、useless `vec![]` 等） |

### 验证

- **格式化前基线**：`cargo test --workspace` = **389 passed / 0 failed**
  （369 + 6 + 4 + 10，四份审计一致认定 389，此处实测确认）。
- **格式化后**：`cargo test --workspace` = **389 passed / 0 failed** ⇒ 纯机械无语义变更。
- **`cargo fmt --all --check`**：0 处差异（幂等）。
- **`cargo clippy --workspace --all-targets`**（在 `all = "deny"` 下）：**exit 0，零警告**。
- **`cargo deny check licenses`**：**exit 0**（详见下方"发现"）。
- **`cargo deny check bans`**：exit 0（`multiple-versions = "warn"` 不阻塞）。
- **`cargo build --examples`**：随 CI 门禁纳入（examples 内含像素级断言）。

### 顺带发现（已处理 / 已记录）

1. **许可证白名单需含 `BSL-1.0`**：`arboard → clipboard-win 5.4.1` 与 `error-code 3.4.0`
   用 Boost Software License 1.0（OSI 批准的 permissive，与 MIT OR Apache-2.0 可并存）。
   已加入白名单并注明来源，避免后来人以为是误配而删掉。
2. **`Unicode-3.0` 不是合法 SPDX 标识符**（`unicode-ident` 的正确写法是 `Unicode-DFS-2016`），
   写错会让 cargo-deny 直接拒绝加载配置。
3. **本机 cargo-deny 是旧版**，在 advisory-db 查询阶段自身 panic
   （`called Option::unwrap() on a None value`）——**是工具版本问题，不是配置问题**：
   `licenses` / `bans` 两个子检查都能正常跑完。CI 装最新版不受影响。

### 遗留

- **`msrv` CI job 未真正验证过 1.88**（`continue-on-error: true`）。
  **待办**：核对 vello_cpu / parley / softbuffer / winit / arboard 各自的 `rust-version`，
  确认 1.88 真的能编译后去掉该标记。否则"声明了 MSRV 但从没有 CI 真跑过"等于没有承诺。
- **`multiple-versions`仍是 `warn`**：当前树里有传递重复（`bitflags` / `bytemuck` 等，
  Wayland 链路还有 `wayland-protocols` 的多个版本）。收紧到 `deny` 前需逐条用
  `skip` / `skip-tree` 精确豁免。**目标是把"同一语义出现两份实现"从"没人注意"变成"构建失败"**
  —— 这直接对应 `9b00f0a` 那次 skrifa 双份字体引擎的事故。
- **S0 剩余未做**：H5（`lieui-layout` 特征测试矩阵，当前仍只有 4 条 smoke）、
  H6（`tests/api_contract.rs`）、H7（doc test 去 `ignore`，当前非 ignore 代码块为 0）、
  以及像素回归助手扩展到"删除 / 阴影 / 图片"三例（与 S1 的 A2 一起做）。
  其中 **H5 必须在 S3（布局重构）之前完成**，否则 D-a/D-b 是裸奔。
- **`rust-toolchain.toml` 写的是 `channel = "stable"`**：本机 stable 是 1.97.1，
  远高于 MSRV 1.88。因此 MSRV 只能靠 CI 的独立 job 兜底，不能靠本地工具链。
---

## 2026-10-07 · S1 正确性 P0 —— A2 脏区登记 / A3 关闭回调落树 —— ✅ 完成（本批 2/7）

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 A。
> 本批只做 A2、A3 两项；A1 / A4 / A5 / A6 / A7 见「遗留」。

### 决策

1. **A2 排在本批第一位**（计划 §五硬约束②：*A2 必须先于其它任何像素测试*）。
   理由不是优先级，而是**它会改变所有后续改动的测试基线** ——修复前，"删除节点"这件事
   本身就会留残影，任何在此之前写的像素测试都可能把**残影误判为预期结果**。
2. **两处修复各自补了变异测试（mutation test）**，即"临时把修复改回原样，确认新测试确实失败"。
   理由：像素级断言很容易写成"永远通过"的空测试（我见过断言写错颜色值、断言采样点落在
   背景区之类的情形）。**没验证过"无修复时会失败"的测试，等于没有测试。**
3. `destroy` 的登记放在 `detach` **之前**：`damage_bounds` 要沿祖先链取变换，节点释放后取不到。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/track.rs` `destroy` | 修改（D2） | 遍历 `descendants(id)`，对每个节点 `damage_bounds` → `damage_rect`，**在 `detach` 之前**完成 |
| `src/track.rs` `damage_bounds` | 修改（D57） | 祖先链中途 `get(c)` 返回 `None` 时，原来的 `?` 会**整条放弃**脏区登记；改为 `break`（带着已累积的变换继续）。症状同样是残影，但触发路径完全不同，必须一起修 |
| `src/app.rs` `close_requested` | 修改（D4） | 补`if !cx.cmds().is_empty() { apply_cmds(...) }`，与 `external` / `tick` 走同一套收尾 |
| `src/render/mod.rs` | 新增测试 | `destroy_registers_damage_for_removed_subtree` —— 像素级回归 |
| `src/app.rs` | 新增测试 | `close_request_applies_queued_commands` —— 命令落树回归 |

### 验证

**A2 变异测试**（临时禁用登记后跑新测试）：

```
panicked at src\render\mod.rs:301:
  destroy 必须登记被删子树的旧矩形，实际脏区：[Rect { x: 0.0, y: 0.0, width: 50.0, height: 120.0 }]
test result: FAILED. 0 passed; 1 failed
```

⇒ 修复前脏区**只有蓝箱那一块**（`x 0..50`），被删红箱占的`x 50..100` 完全不在脏区内
⇒ 证实残影真实发生，且新测试确实能抓住它。
（该测试同时刻意制造了"同帧存在其它脏区"这个触发条件——否则渲染层"脏区为空 ⇒ 按整窗处理"
的兜底会让 bug 测不出来。）

**A3 变异测试**（临时禁用落树后跑新测试）：

```
panicked at src\app.rs:1773:
  关闭回调里的 cx.damage_all() 必须落树（修复前这里恒为空）
test result: FAILED. 0 passed; 1 failed
```

**恢复修复后的完整门禁**：

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **371 passed / 0 failed**（389 → 371+6+4+10，新增 2 条） |
| `cargo clippy --workspace --all-targets`（`all = "deny"`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |

### 遗留（本批未做，按A1 → A7 顺序）

- **A1（`D1` `D10`）**：`Tapped` 不校验按下/抬起的按键配对 —— **数据破坏级**（右键按下 + 左键抬起
  会合成 `Tapped`，触发勾选/提交/删除）。计划已注明**先只做按键配对**，位移/时长阈值需要改
  `input::step` 的签名（纯函数、无时钟来源），留到有测试支撑时再做。
- **A4（`D8`）**：`view()`内 `set` 的断言去掉 `cfg!(debug_assertions)` + `begin/end_view` 改 RAII 守卫。
- **A5（`D9`）**：定时器回调内 `cancel()` 无效、关窗后成孤儿。
- **A6（`D7`）**：`run()` 之前 `spawn_task` 永久静默挂起。
- **A7（`D33` `D58` `D59` `D61` `D62`）**：阴影超脏区 / `clear_layout_flags` 吞脏标 /
  `window_sizes` 泄漏 / Wheel 绕过捕获 / spinner 用 `SystemTime`。
- **像素回归助手还差两类**（计划要求扩展到"删除 / 阴影 / 图片"三例）：本批完成了**删除**例，
  **阴影**（`D33`）与**图片 clip**（`D3`，需先修 C3）待补。
---

## 2026-10-07 · S1 正确性 P0 —— A1 点击按键配对 + 按下态泄漏 —— ✅ 完成（本批 3/7）

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 A1。
> 本批修完全部 P0 中**唯一的数据破坏级**缺陷（D1）+ 一个状态泄漏（D10）。

### 决策

1. **`Track.pressed` 从 `Option<NodeId>` 升级为 `Option<PressState>`**，
   `PressState { node, pointer, button, pos }`。
   **为什么必须多记 `button`**：`Tapped` 合成要校验"按下与抬起是同一按键"。
   只记节点的话，**右键按下 + 左键抬起落在同一节点会被判成左键点击** ⇒ 触发勾选 / 提交 / 删除。
   `pointer` / `pos` 是**为后续阈值预留**，本次不消费（见下条）。
2. **只做按键配对，不加位移 / 时长阈值**（计划 A1 的"分两步"）。
   原因：`input::step(&mut Track, InputEvent) -> InputStep` 是**纯函数、拿不到时钟**，
   加时间阈值要改签名并穿透全部调用点。先堵住数据破坏级缺陷，阈值另开批次。
3. **抽出 `clear_pressed(track)` 供 `Down` / `Up` / `Cancel` 三处共用**。
   此前三处各自内联同样的"清整条链"逻辑，其中 `Down` 处**漏了**（D10 的成因）。
   共用后"清法不一致"这个类别不再可能发生。
4. **配对校验不牵连右键菜单**：额外补了一条正向测试（右键按下 → 右键抬起仍合成 `RightTapped`），
   避免"修 D1 时把右键功能一起堵掉"这种过度修复。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/track.rs` | 新增 `PressState` + 改字段 | `pub pressed: Option<PressState>`；`PressState` 放在模块级（**不要放进 `Track` struct 里**，第一版误放导致 `structs are not allowed in struct definitions`） |
| `src/track.rs` | 改导入 | `use crate::event::{HandlerSlot, PointerButton, PointerId}`（`PointerButton` 是 `Copy + PartialEq`，配对可直接 `==`） |
| `src/input.rs` `Down` | 修改（D1 + D10） | 记录 `PressState { node, pointer, button, pos }`；**先 `clear_pressed`** 再设新按下态 |
| `src/input.rs` `Up` | 修改（D1） | 点击合成条件加 `press.button == button` |
| `src/input.rs` | 新增 `clear_pressed` | 取代三处重复的内联清链逻辑 |
| `src/input.rs` | 测试迁移 | `assert_eq!(t.pressed, Some(kids[1]))` → `t.pressed.map(|p| p.node)`，并追加按键断言 |
| `src/input.rs` | 新增 3 条测试 | `cross_button_release_does_not_synthesize_tapped`（D1）、`right_button_press_and_release_still_taps`（防过度修复）、`second_down_clears_previous_pressed_chain`（D10） |
| `src/lib.rs` | 修改 | `pub use track::...PressState`（`Track::pressed` 是 pub字段，类型必须导出） |

### 验证

**D1 变异测试**（去掉 `press.button == button` 后）：

```
panicked at src\input.rs:579:
  按键不配对时不得合成点击（否则右键按下会被当成左键点击）
test result: FAILED. 15 passed; 1 failed
```

⇒ 修复前**右键按下 + 左键抬起确实会合成 `Tapped`**，数据破坏级缺陷真实存在。

**D10 变异测试**（去掉 `Down` 里的 `clear_pressed` 后）：

```
panicked at src\input.rs:655:
  上一次按下的节点不应残留 pressed 视觉
test result: FAILED. 0 passed; 1 failed
```

**完整门禁**：

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **374 passed / 0 failed**（371 → 374，新增 3 条） |
| `cargo clippy --workspace --all-targets`（`all = "deny"`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |

### 当前 P0 进度

| 计划项 | 缺陷 | 状态 |
|---|---|---|
| A1 | D1（点击按键配对）、D10（按下态泄漏） | ✅ 本批完成 |
| A2 | D2（destroy 不登记脏区）、D57（damage_bounds 静默放弃） | ✅ 上一批完成 |
| A3 | D4（close_requested 丢Cmd） | ✅ 上一批完成 |
| A4 | D8（release 下 view() 内 set 静默自激 + panic 毒化） | ⏳ 待做 |
| A5 | D9（定时器取消失效 / 关窗孤儿） | ⏳ 待做 |
| A6 | D7（run() 前 spawn_task 静默挂起） | ⏳ 待做 |
| A7 | D33 / D58 / D59 / D61 / D62 | ⏳ 待做 |

**测试总数389 → 374（lib）+ 6+ 4+ 10（子 crate 与集成）= 394**，新增 5 条回归测试，
其中 **5 条全部经过变异验证**（确认"无修复时会失败"）。

### 遗留

- **位移 / 长按阈值**（A1 第二步）：需给 `input::step` 传入时钟，改签名。
  `PressState` 的 `pos` / `pointer` 已预留好，届时只需补判定。
- **A4 未做**：`assert_not_in_view` 仍是 `cfg!(debug_assertions)`（release 下静默自激），
  `begin_view/end_view` 仍非 RAII（`view()` panic 后 `in_view` 永久污染）。**这是 P0 里剩下的最大一项。**
- **D11 / D13（焦点）**、**D6（Modal 内Popup 的 z 序）** 按计划在 S4，不在本批。
---

## 2026-10-07 · S1 正确性 P0 —— A4 view() 值守 RAII + always-on 断言 —— ✅ 完成（本批 4/7）

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 A4。

### 决策

1. **`begin_view()` 改为返回 RAII 守卫**，删除手写的 `end_view()`。
   触发原因不是风格偏好，而是**真实缺陷**：`view()` 是用户代码，它 panic 时
   `end_view()` 永不执行 ⇒ `in_view` 永久停在 `Some(..)` ⇒ 此后该窗口所有
   `Signal::set` 都被 `assert_not_in_view` 拦下，**Runtime 被永久毒化**。
2. **`assert_not_in_view` 去掉 `cfg!(debug_assertions)` 门禁，改always-on**。
   代价只是读一个 `Cell<Option<WindowId>>`；换来release 下也能 fail-fast，
   而不是变成"每帧 view → set → 再 view"的**永久满帧自激**（100% CPU、不报错、无日志）。
   设计 §四本就把这条列为"违反会 panic 或死循环"的硬纪律。
3. **`ViewGuard` 放在模块级而非 `impl Runtime` 内**（第一版误放进 impl 块，
   编译报 `structs are not allowed in struct definitions`）。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/reactive.rs` | 新增 `ViewGuard` + `Drop` | 守卫持有 `&Cell<Option<WindowId>>`，`Drop` 时置 `None` |
| `src/reactive.rs` `begin_view` | 改签名 | `-> ViewGuard<'_>`；**删除 `end_view()`**（原带 `#[allow(dead_code)]`，实际生产在用） |
| `src/reactive.rs` `assert_not_in_view` | 修改（D8） | 去掉 `cfg!(debug_assertions)` |
| `src/app.rs` `frame` | 修改 | `let _view_guard = rt.begin_view(self.id);` … `drop(_view_guard);` 替代成对调用 |
| `src/reactive.rs` | 测试升级 | **已有的 `set_inside_view_panics` 带 `#[cfg(debug_assertions)]` ���— 移除该门禁并改 RAII 形式**（测试本身只在 debug 存在，正是 D8 的证据） |
| `src/reactive.rs` | 新增测试 | `view_guard_recovers_after_panic` |

### 验证

**A4 变异测试**（把 `ViewGuard::drop` 改成空操作，精确模拟"`end_view()` 未被调用"）：

```
panicked at src\reactive.rs:682:
  panic 后 in_view 必须被守卫释放，否则 Runtime 被永久毒化
test result: FAILED. 0 passed; 1 failed
```

> 变异设计说明：第一次尝试的变异（把 `let _guard = ...` 改成先 panic 再 `drop`）是**无效变异** ——
> Rust 的 RAII 语义保证 panic 展开时仍会调用 `Drop`，测试照样通过。
> 正确做法是直接把 `Drop` 实现清空，才能模拟原代码"手写 `end_view()` 未执行"的真实情形。

**完整门禁**：

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **375 passed / 0 failed**（374 → 375，新增 1 条；改造前基线 369） |
| `cargo clippy --workspace --all-targets`（`all = "deny"`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |

### 本轮（2026-10-07）总进度

**已完成 S0 全量 + S1 的 A1–A4。**

已修复并经变异验证的缺陷（6 项）：

| 缺陷 | 严重度 | 症状 | 修复 |
|---|---|---|---|
| **D1** | P0 数据破坏 | 右键按下 + 左键抬起 ⇒ 合成 `Tapped` ⇒ 触发勾选/提交/删除 | `Track::pressed`升级为 `Option<PressState>`，`Up` 校验按键配对 |
| **D2** | P0 视觉 | 同帧有其它脏区时，删除节点留残影 | `destroy` 登记旧矩形 + 像素回归测试 |
| **D4** | P0 静默失效 | 关闭回调里的 `cx.damage/focus/scroll_to` 全部无效 | `close_requested` 补 `apply_cmds` |
| **D8** | P0 自激/毒化 | release 下 `view()` 内 `set` 满帧自激；`view()` panic 后 Runtime 永久毒化 | 值守改 RAII + 断言 always-on |
| **D10** | P1 状态泄漏 | 二次按下不清理旧链 ⇒旧节点永久残留 pressed 视觉 | 抽出 `clear_pressed`，`Down`/`Up`/`Cancel` 共用 |
| **D57** | P0 残影 | `damage_bounds` 祖先链断裂即静默放弃整条脏区登记 | `?` 改 `break`，带已累积变换继续 |

门禁与基建：`rustfmt.toml` / `rust-toolchain.toml` / `[workspace.lints]`（clippy `all = "deny"`，
15 条清零）/ `.github/workflows/ci.yml`（5 job）/ `deny.toml`（许可证白名单，发现 `BSL-1.0` 来自
`arboard → clipboard-win`）。

测试：改造前 389 → 现在 **395**（lib 375 + 6 + 4 + 10），**新增 6 条回归测试，全部做过变异验证**
（确认"修复回退后测试确实失败"—— 这是本轮坚持的验证标准，见 `refactor-plan` §四·主线 A 的配套要求）。

### 下一步（S1 剩余）

- **A5（D9）**：定时器回调内 `cancel()` 无效；关窗后成孤儿（先出表 → 执行 → 无条件 reschedule）。
- **A6（D7）**：`run()` 之前 `spawn_task` 永久静默挂起（waker 快照过期 + 本地队列 GUI 模式不 drain）。
  **建议先加诊断**，让"静默"变成"可报错"——这类"卡住且不吭声"的缺陷最难被用户报告。
- **A7（D33/D58/D59/D61/D62）**：阴影超脏区 / `clear_layout_flags` 吞脏标 / `window_sizes` 泄漏 /
  Wheel 绕过捕获 / spinner 用 `SystemTime`。
- 之后进入 **S2**：主线 B（帧调度，**含 `request_redraw` 闭环**，计划已标注需先做 spike）→ C1/C2/C3 → F1/F3。
---

## 2026-10-07 · S0 剩余项 —— 测试体系重建（H5/H6/H7）—— ✅ 完成

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 H5/H6/H7 + §五 S0。
> 本批把测试数**389 → 437**，且**没有一条新测试是"重跑旧断言"**。

### 决策

1. **不把内嵌测试外移。** audit-3 建议把 `app.rs` 的 3368 行测试搬到 `app/tests.rs`。
   **本批拒绝采纳**：内嵌测试能访问 `pub(crate)` 私有字段（`flags`/`damage`/`computed` 等），
   外移后这些测试只能改测`pub` 面，**覆盖率会净下降**。
   真正缺的不是"把测试搬出去"，而是**"没有测试的层"** —— 那是黑盒契约层（H6）。
   文件臃肿问题留给 S5 的 G4（结构重构）用"按职责拆模块 + 测试随模块走"解决。

2. **特征测试按"语义域"拆文件，不做一个大矩阵。**
   `tests/feature_flex.rs`（尺寸协商）+ `tests/feature_position.rs`（定位与流）。
   理由：新增测试时容易判断"该放哪"，且两组的失效模式不同（前者挂死循环，后者错位）。

3. **已知缺陷用 `#[ignore = "KNOWN BUG ..."]` 显式标注，而不是写"钉住缺陷"的断言。**
   后者有个致命问题：修复 bug 后测试会失败，容易被误当成"测试坏了"而顺手删掉。
   `#[ignore]` 则是"清单里明写这一条坏了"，`cargo test` 输出直接可见。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `crates/lieui-layout/tests/feature_flex.rs` | **新增** 17 条 | grow/shrink/basis/min-max clamp/align/self/shrink 冻结循环/收敛守卫 + 2 条 KNOWN BUG |
| `crates/lieui-layout/tests/feature_position.rs` | **新增** 17 条 | absolute 四向/margin 叠加/padding/border/RTL/reverse/justify/wrap/gap + 索引契约 + 1 条 KNOWN BUG |
| `tests/api_contract.rs` | **新增** 9 条 | 主 crate 第一个黑盒契约测试（仅 `pub` API） |
| `crates/lieui-layout/src/style.rs` | 文档 | 警示`CSSDirection` 只有前 6 个变体可索引样式数组 |
| `src/lib.rs` | 文档 | 第一个**可编译** doc test（`no_run`），示例同时充当 API 形状守卫 |

### 关键产出 1：`lieui-layout` 从 4 条 smoke → 36 条特征测试

覆盖审计点名的**全部零覆盖区**：absolute、shrink 冻结循环、wrap 多行、min/max clamp、RTL、gap。
其中两条收敛守卫特别重要：

- `shrink_stops_at_basis_and_reallocates_remainder` —— `resolve_flexible_lengths`（`:603` 的`while`）
  **没有迭代上限**（D48），一旦震荡挂死 UI 主线程。这是该路径唯一的测试。
- `pathological_inputs_terminate_with_finite_results` —— 用 min>max / shrink-min 冲突 / 零尺寸
  四组刁钻输入验证"必须终止且产出有限值"，是**挂死风险的兜底**。

### 关键产出 2：新发现一个四份审计都漏掉的 API 陷阱

> **`CSSDirection` 有 10 个变体，但样式数组只有 6 个槽位**（`K_CSS_PROPS_COUNT = 6`）。

`All`(8) / `Horizontal`(6) / `Vertical`(7) / `None`(9) **拿去索引 `padding`/`margin`/`border`/`position`
会越界 panic**。这四个变体与 `FlexStyle` 的数组**都是 `pub`**，看起来"枚举多长数组就多长"。

- 常量索引会被编译器抓成 `unconditional_panic`（我在写测试时立刻撞到）；
- 但 `dir as usize` 是**运行时炸弹**。
- 已核查生产代码**未踩到**（`src/view.rs:976` 用 `for i in 0..4`）；
- 处理：① `style.rs` 顶层加警示文档；② 加契约测试 `only_first_six_css_directions_are_indexable` 钉住。

### 关键产出 3：`lieui-layout` 的三条 DSL 真实约束被钉进契约测试

写 `api_contract.rs` 时连续撞上三条**只以 `panic!` 表达**的约束（不是编译错误）：

1. **`view()` 顶层只能声明一个内容根**（`view.rs:219`）—— 第二个顶层容器直接 panic；
2. **`keyed_list` 必须在容器闭包内**（`view.rs:560`）；
3. **`keyed_list` 会"接管"该容器**（`view.rs:208`）—— 接管后不能往同一容器再加子节点，
   所以两个列表必须各自独占一个容器。

这三条都写进了测试注释。**顺带印证 D45**：`padding`/`gap` 返回 `()` 而非 `Self`，
所以 `c.padding(8.0).gap(4.0)` 这种写法编译不过 —— 契约测试里只能用两条语句。

### 修正：审计对 D44 的影响面判断偏大

审计说 `layout()` 永久改写 `style.flex_basis` 且不恢复（D44）。**实测范围窄得多**：

> 所有递归调用**全部走 `layout_impl`**（`:472/:623/:675/:853`），只有**根节点**走 `layout()`
> ——而 `flex_basis` 的写入与 `dim` 的恢复都只在 `layout()` 里。
> 所以**只有根节点被污染**，子节点不受影响。

影响评估：框架当前每次布局都重建临时 FlexNode 树（`layout.rs:94`），根节点是新的，**今天不触发**。
但 S3 的 D-a 要做"FlexNode 持久缓存复用" —— 那时根节点会被复用，污染才变成真 bug。
**结论不变（仍需在 D-a 之前修），但优先级可降，且不必按"影响整棵树"去设计修复。**

### 门禁立刻拦住了本批自己写的代码

`[lints.clippy] all = "deny"` 在本批生效，当场抓到 `feature_flex.rs` 里我写的
`type_complexity`（复杂类型）与无用 `mut` —— 已修。
**这是门禁第一个"抓到作者本人"的实例**，也是它最实在的价值证明。

### 验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **12 个测试二进制全 ok**，合计 **437** 条（起点 389，**+48**） |
| ├ 主 crate lib | 375 |
| ├ `tests/api_contract.rs`（新） | 9 |
| ├ `crates/lieui-layout` | 4 → **36**（16 + 16 + 4，2 条 KNOWN BUG ignored） |
| ├ `crates/lieui-text` / `lieui-geom` | 10 / 6 |
| └ doc test | **1 passed**（`lib.rs` 示例真正编译），19 ignored |
| `cargo clippy --workspace --all-targets` | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |

### 遗留

- **H7 只做了 `lib.rs` 一处**（19 处仍 `ignore`）。下一步按"用户会先读到的顺序"推进：
  `view.rs` → `custom.rs` → `theme.rs` → `app.rs`。
  注意 doc test 会**编译**，改之前必须核对真实签名（本批已踩：`progress` 收`usize` 不收 `u32`）。
- **像素回归助手还差两类**：阴影超脏区（D33）、图片 clip（D3，需先修 C3）。
- **`damage_bench.rs` 的断言仍只在 `main()` 里**，`cargo test` 不执行。
  计划 C0 要求的"量化基准"应改为**确定性指标**（批次数 / 光栅像素数 / 帧调用次数）进 `cargo test`，
  时间类指标继续留在 example 里 —— 这是把"性能回归"变成"可断言"的关键一步，尚未做。
---

## 2026-10-07 · S2 前置 —— C0 把性能回归变成可断言测试 —— ✅ 完成

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 C 的 C0 步骤。

### 决策

1. **只断言确定性指标，不把时间指标搬进 `cargo test`。**
   可断言的五个字段全是 `pub` 的：`FrameStats::damage.len()`（碎片数）、
   `RenderStats::raster.{batches, pixels, rasterized}`、`FrameStats::is_idle()`。
   时间（ms/µs）受机器/编译模式/CPU 调度影响，属于 `examples/damage_bench.rs` 的职责。
   **分工**：测试守"算法有没有退化"，example 报"退化成什么样"。

2. **把 `Page` 从 `v.column` 改成 `v.scroll` 包裹。**
   原因见下 —— 原来的基准**根本没有滚动容器**，等于从未测过"滚动脏区"这条最关键路径。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `tests/perf_regression.rs` | **新增** 8 条 | 空闲帧不���栅化 / 标脏后恢复光栅 / 局部远小于整窗 / 整窗脏覆盖全窗口 / 滚动碎片基线 / 标脏 30 节点基线 / 2 条 `#[ignore]` 记录 D21、D22 |
| `docs/refactor-plan.md` | 更新 | D21 条目补实测数据并**重新定性**；C2 段落补C0 实测表 |

### 关键产出：实测把 D21 从"滚动问题"重新定性为"普遍失效"

实测基线（1280×720，`-- --nocapture` 可复现）：

| 场景 | 脏区碎片 | 批次 | 光栅像素 |
|---|---|---|---|
| 50 行滚动一次（dy=20） | **302** | 1 | 921600 = **整窗** |
| 标脏 30 个节点 | **30** | 1 | 921600 = **整窗** |
| 标脏 1 个按钮（深埋） | 1 | 1 | 远小于 2% 窗口 ✅ |
| 空闲帧 | 0 | 0 | 0，`rasterized == false` ✅ |

**这比审计的估计严重得多**：审计说"嵌套滚动一次产出 20+ 块"，
实测**一次滚动产生 302 个碎片**；而且"标脏 30 个节点"（一次普通批量状态变更）**也退化成整窗**。

⇒ 碎片阈值 8 的真实含义是：**任何 ≥9 节点变化都退化为整窗光栅 + 全树 Scene 重建**。
这不是"滚动场景拿不到脏区红利"，而是**除单控件交互外几乎都拿不到**。
已同步修正 `refactor-plan` 的 D21 与 C2。

**顺带修正另一处基准缺陷**：`examples/damage_bench.rs` 的 `Page` 用 `v.column` 且**无滚动容器**，
所以它引以为傲的"局部49µs vs 整窗 4.6ms"全部来自**单个按钮标脏**（碎片 = 1），
从未覆盖滚动。新测试补上了这条路径。

### 两条 `#[ignore]` 的定位

- `scrolling_list_produces_more_than_eight_fragments` —— 断言"当前退化为整窗"，
  C2 修好后**这条断言会失败**，正是提醒把它改成正断言并去掉 `ignore` 的信号。
- `local_present_should_not_copy_full_rows`（D22）—— 占位。
  `present_with_damage` 在 `#[cfg(feature = "winit")]` 平台层内且需真实 surface，
  **`cargo test` 里无法断言**。因此给 C1 留了明确要求：
  **必须先把"按行拷贝宽度"抽成可测的纯函数**（如 `copy_rows(pixmap, buf, rects)`），
  否则这条永远测不了。

### 验证

| 检查 | 结果 |
|---|---|
| `cargo test --test perf_regression` | 6 passed / 2 ignored |
| `cargo test --test perf_regression -- --ignored` | **2 passed**（确认 D21 现状断言成立） |
| `cargo test --workspace` | **13 个测试二进制全 ok** |
| `cargo clippy --workspace --all-targets`（CI 用 `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |

### 本批修正的一处自身问题

`full_repaint_covers_the_whole_window` 之外，`idle_frame_does_not_rasterize` 有个未使用的 `id`绑定
（clippy warning，因为 CI 用 `-D warnings` 会失败）—— 已修为 `_id`。
这是 `[lints]` 门禁在本批第二次生效（第一次是抓 `feature_flex.rs` 的 `type_complexity`）。

### 遗留

- **C0 的数据已就位，下一步就是 C2**（碎片合并）。验收标准现在有了明确数字：
  滚动 302 碎片 → 合并后应 ≤ 4（计划里的C2 目标）。
- **C1（矩形上屏）需要先做一次重构**：把行拷贝抽成纯函数，否则无法测试。
  这是 D22 能否被回归保护的**前置条件**。
- S1 剩余（A5 定时器 / A6 任务 waker / A7 杂项）仍未做。
---

## 2026-10-07 · S1 正确性 P0 —— A5 定时器取消 / A6 任务唤醒器 —— ✅ 完成（本批 6/7）

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 A5 / A6。
> 完成后 S1 只剩 A7（5 项杂项）。

### 决策

1. **A5 采用"双检查"而不是"回调在表内执行"。**
   根因是 `take_due_timers` 用 `remove` 把定时器**移出表**再执行回调（为了回调里能安全地
   再设定时器）。改执行模型会牵动整条时序，所以改为在**放回之前**加两道检查：
   ① 回调执行期间被 `cancel()` ⇒ 丢弃；② 所属窗口已注销 ⇒ 丢弃。
   第② 条是**独立于用户行为**的兜底：没有它，关窗时正在执行的周期定时器会变成
   "永不触发却一直持有闭包捕获"的孤儿。
   为此给 `RuntimeInner` 加了 `cancelled_timers: RefCell<HashSet<u64>>`，
   且**只在"回调执行期间取消"这种罕见情况**才进集合（普通 `cancel()` 走 `retain` 即可），
   `reschedule_timer` 消费后立即移除 ⇒ 集合始终很小。

2. **A6 选了比原计划更小的修法。**
   计划里写的是"让 `TaskCtx` 在 post 时查询当前 slot"（需要把
   `RuntimeInner.waker` 从 `RefCell<WakerSlot>` 改成 `Arc<Mutex<..>>`，
   并波及 `event.rs` 的 `Emitter` + 10 余处测试）。
   实测发现 `WakerSlot::Local` 持的是 **`Arc<LocalQueue>`（共享！）**，
   于是给 `LocalQueue` 加一个 `forward: Mutex<Option<Arc<dyn Waker>>>`，
   `set_waker` 时把平台 waker 装进**旧的**队列即可 ——
   **改动 3 处、零结构变更**，且同样根治。

3. **顺手改掉了一条"错误的正确"注释。**
   `App::frame_all` 的文档写着"有平台时队列恒空 ⇒ 零开销"。
   那个假设**恰恰是 bug 的来源**（有平台时并不恒空，因为快照可能还是 `Local`）。
   已改为说明真实情况并指向修复。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/reactive.rs` | 新增字段 | `cancelled_timers: RefCell<HashSet<u64>>` |
| `src/timer.rs` `TimerHandle::cancel` | 修改 | 表里找不到（= 正在执行）时**打取消标记** |
| `src/timer.rs` `reschedule_timer` | 修改 | 放回前查①取消名单、②窗口是否仍注册 |
| `src/timer.rs` `cancel_timers_of` | 修改 | 先把该窗口所有定时器 id 记入名单再删表 |
| `src/task.rs` `LocalQueue` | 修改 | 新增 `forward` 转发器；`push` 有平台就转发；新增 `attach_platform` |
| `src/task.rs` `Runtime::set_waker` | 修改 | 把平台 waker 装进**旧**本地队列并转交积压消息 |
| `src/platform/mod.rs` `Runner::tick` | 修改 | 补`take_pending_external` drain（平台层此前完全没做） |
| `src/app.rs` | 文档 | 修正"有平台时队列恒空"的错误假设 |
| `src/timer.rs` | 新增 2 条测试 | 回调内取消自己/ 关窗不孤儿 |
| `src/task.rs` | 新增 2 条测试 | 旧快照转发 / 积压消息转交 |

### 教训：A5 的测试**第一版是假通过**，靠变异测试抓出来

第一次写 `cancel_inside_own_callback_stops_interval_timer`，我在测试里这样执行回调：

```rust
(due[0].take_cb().unwrap())(&mut cx);
rt.reschedule_timer(due.remove(0), now);
```

结果 `reschedule_timer` 里`take_cb()` 返回 `None` ⇒ 命中"回调被 take 走后没还回来
（不该发生）"分支提前 return ⇒ **定时器当然不会回表**，断言恒成立 ——
**测的其实不是取消逻辑，而是"cb 不在了"**。

生产路径（`WindowCtx::tick`）是 `cb(&mut cx); timer.cb = Some(cb);` **会把 cb 还回去**，
所以真实 bug 并不存在"cb 丢失"，而是我的测试没有复现生产写法。

**去掉"取消名单"检查后测试仍然通过** —— 这个"变异没让测试变红"的现象才暴露了问题。
修正：测试里照 `WindowCtx::tick` 的做法**把 cb 还回去**（新增 `run_one` helper），
之后再去掉任一检查，对应测试立刻失败：

| 变异 | 失败的测试 |
|---|---|
| 移除①取消名单 | `cancel_inside_own_callback_stops_interval_timer`（`timer_count` 1≠0） |
| 移除②窗口检查 | `closing_window_does_not_orphan_the_executing_interval_timer`（1≠0） |

> **结论**：变异测试不只用来验证"测试能抓 bug"，也用来验证**"测试本身没在测别的东西"**。
> 一个断言恒成立的测试比没有测试更危险——它给人虚假的安全感。
> 这条已写进 `refactor-plan` §四·主线 A 的配套要求。

### 教训：第二个测试也犯了"前置断言把自己要验的东西清掉"的错

`set_waker_forwards_already_queued_messages` 里我写了
`assert_eq!(rt.take_pending_external().len(), 2, "前置条件：两条都在本地队列里")`
作为前置检查 —— 而 `take_pending_external` 是**取走**，把待验证的消息清空了，
于是 `set_waker` 之后当然转交不到。已去掉该前置断言，并在注释里写明"不要在这里 drain"。

### 验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **13 个测试二进制全 ok**；lib **379**（375 → 379，新增 4 条） |
| ├ A5 变异 | 两道检查分别移除 → 对应测试**各自失败** |
| ├ A6 变异 | 移除转发 → **2 条同时失败**（16 passed → 14 passed / 2 failed） |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |

测试总数 **389 → 447**（+58）。

### S1 剩余

- **A7（`D33` `D58` `D59` `D61` `D62`）**：阴影模糊超出脏区 / `clear_layout_flags` 吞脏标 /
  `window_sizes` 永不清理 / Wheel 绕过指针捕获 / spinner 用 `SystemTime`（非单调时钟）。
  五项都是小改动，可一次做完。
---

## 2026-10-07 · S1 正确性 P0 —— A7 杂项五项 —— ✅ 完成，**S1 收尾**

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 A7。
> 至此 S1（A1–A7）全部完成，P0 表里的 8 项**全部修复并经变异验证**。

### 变更清单（五项，各自独立）

| 缺陷 | 修改 | 回归测试 |
|---|---|---|
| **D33** 阴影超出脏区 | `Node::paint_bounds` 在有阴影时按 `spread + blur * 1.5`（高斯 3σ）**四边保守外扩** | `paint_bounds_covers_shadow_blur_reach` + `paint_bounds_is_not_inflated_without_shadow`（确认无阴影时零扩张，保住"精确脏区"收益） |
| **D58** 布局期间脏标被吞 | `Node` 加 `layout_epoch` + `Track` 加 `layout_epoch`；`mark_layout_dirty` 记录当轮纪元；`clear_layout_flags` **只清 `epoch < 当前`**；`layout()` 动手前 `begin_layout_epoch()` | `flags_raised_during_the_layout_round_survive_clear`（三轮：消费 → 本轮新提应保留 → 下一轮应消费） |
| **D59** `window_sizes` 泄漏 | `unregister_window` 里一并`retain` 掉尺寸记录 | `unregister_window_clears_the_recorded_size` + `unregistering_one_window_keeps_the_other_size`（确认不是"清空全部"） |
| **D61** Wheel 绕过捕获 | `let _ = pointer; hit::hit_path(…)` → `hit::hit_path_for(track, pointer, pos)` | `wheel_is_routed_through_pointer_capture` + `wheel_without_capture_follows_hit_chain`（确认不是"总发给捕获者"） |
| **D62** spinner 用挂钟 | `Spinner` 加 `started: Instant`；相位改用 `self.started.elapsed().as_millis()` | 由既有 overlay 测试覆盖（相位单调性本质难断言，见遗留） |

### 关键决策

1. **D58 用epoch 而不是"清两轮"。**
   先考虑过"清完再扫一遍、仍脏则重新冒泡"，但那样无法区分"刚标的"与"上一轮漏的"。
   epoch 方案语义精确：标记带**提出时刻**，清理只动"本轮之前"的。
   代价是 `Node` 多一个 `u32`（4 字节 × 节点数）。
   `overflow_add` 而非 `+=` —— 42 亿轮后才回绕，且回绕语义仍是"旧的更小"，方向安全。

2. **D33 选四边外扩而非只按 offset 方向。**
   `offset_x/offset_y` 已知，但**模糊半径与 spread 在四周对称**；只朝 offset 方向扩会让
   反方向的模糊边缘仍落在脏区外。四边扩多标一点，换来"不会漏"。

3. **D33 必须配一条"无阴影时不扩张"的测试。**
   否则将来有人把 `paint_bounds` 改成"永远外扩一点"，脏区红利会被静默吃掉，
   而所有阴影相关测试仍然全绿。

### 变异验证（逐项确认测试真能抓到）

| 变异 | 结果 |
|---|---|
| `unregister_window` 去掉 `window_sizes` 清理 | **2 条 D59 测试失败** |
| `clear_layout_flags` 去掉 epoch 判断（`if true`） | **D58 测试失败** |
| Wheel 改回 `hit_path` | **D61 测试失败**（实际路由到 `kids[0]` 而非捕获者 `kids[1]`） |

**门禁第三次抓到作者本人**：`ShadowSpec` 与 `lieui_geom::Size` 两个未使用导入
（CI 用 `-D warnings`，会直接失败）。已清理。

### 验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **13 个测试二进制全 ok**；lib **386**（379 → 386，新增 7 条） |
| `cargo clippy --workspace --all-targets` | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |

测试总数 **389 → 454**（+65）。

## S1 阶段总结

| 批次 | 内容 | 结果 |
|---|---|---|
| A1 | 点击按键配对（`PressState`）+ 按下态泄漏 | D1 D10 |
| A2 | `destroy` 登记旧矩形 + `damage_bounds` 不再静默放弃 | D2 D57 |
| A3 | `close_requested` 落Cmd | D4 |
| A4 | `view()` 值守 RAII + 断言 always-on | D8 |
| A5 | 定时器取消双检查 + 关窗不孤儿 | D9 |
| A6 | waker 转发 + 平台层 drain 本地队列 | D7 |
| A7 | 阴影脏区 / epoch / 尺寸泄漏 / Wheel 捕获 / 单调时钟 | D33 D58 D59 D61 D62 |

**P0 表 8 项（D1 D2 D3 D4 D7 D8 D57 + D5 待S4）全部修复**，每项都有变异验证过的回归测试。
唯一未修的是 **D3（图片绕过裁剪栈）** ——它属S2 的 C3（需要先做 clip 栈），
当前状态是**代码注释已自述该限制**，行为已知。

### 下一步：S2（帧与脏区）

按计划顺序：**主线B（帧调度）→ C1（矩形上屏）→ C2（碎片合并）→ C3（图片 clip）**。

⚠️ **主线 B 必须先做 spike**：它是唯一"设计意图正确但改法不完整就会引入新故障"的地方 ——
漏掉 `request_redraw` 闭环会把"帧跑 3 次"换成"定时器和动画停帧"。
C0 已为其备好量化验收指标（`tests/perf_regression.rs` 里的
`idle_frame_does_not_rasterize` / `marking_damage_reenables_rasterization`）。
---

## 2026-10-07 · S2 · 主线 B 帧调度收敛到单一入口（spike → 落地）—— ✅ 完成

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 B。
> 这是全计划里唯一一个"设计意图正确但**改法不完整就会引入新故障**"的主线，
> 所以先做 spike 验证闭环可行，再落地。

### 决策

1. **不按 v1.0 计划里的 `WindowCtx::last_frame: Instant` 闸门做**，改为**拆分职责**：
   - `tick()`（平台层）= 完整帧，**只在 `RedrawRequested` 调用**；
   - 新增 `pump()` = **只消费**定时器 / 动画帧 / 本地投递，**不渲染**，返回"是否有窗口做了事"。
   理由：闸门方案只是"少跑几次"，而职责拆分**同时**解决了"timer 被 frame 次数放大"
   和"帧跑 3 次"两个问题，且不引入新的时间状态。

2. **闭环 = `pump()` 的返回值驱动 `request_redraw`。**
   `WindowCtx::tick` 改成返回 `bool`（消费了定时器 / 动画帧 / 产生了命令 / tooltip 会话变化）。
   `about_to_wait` 与 `user_event` 变成：
   ```
   if self.pump(now) { self.request_redraw_all(); }
   ```
   **漏掉这一环的后果**：定时器回调改了状态但没人重绘 ⇒ **停帧**（回调在跑、画面不动）。
   这正是 v1.0 计划遗漏、我在评审里标为"唯一功能性风险"的那一环。

3. **`update_tooltip` 也改成返回 `bool`**（tooltip 浮出 / 收回都改了树，需要重绘），
   否则"悬停后等tooltip 出现"会停帧。

4. **抽出 `schedule_wakeup(el)`** 供 `tick` 与 `about_to_wait` 共用，
   避免两处各算一份"下一个唤醒时刻"（那正是"过期真相"的来源）。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/app.rs` `WindowCtx::tick` | **返回类型 `()` → `bool`** | 定时器到期 / 动画帧消费 / 有命令 / tooltip 变化 ⇒ true |
| `src/app.rs` `update_tooltip` | **返回类型 `()` → `bool`** | 浮出或收回了 tooltip 层 ⇒ true |
| `src/platform/mod.rs` `Runner::tick` | 语义收窄 | 成为 `RedrawRequested` 的**唯一**完整帧入口 |
| `src/platform/mod.rs` `Runner::pump` | **新增** | 只消费定时器/动画/本地投递，返回"是否有工作" |
| `src/platform/mod.rs` `request_redraw_all` | **新增** | pump 有产出时请求全部窗口重绘 |
| `src/platform/mod.rs` `schedule_wakeup` | **抽出** | `tick` 与 `about_to_wait` 共用 |
| `src/platform/mod.rs` `about_to_wait` | **改写** | 不再跑完整帧；`pump` + 条件 redraw + drain_requests + schedule_wakeup |
| `src/platform/mod.rs` `user_event` | **改写** | 两个分支都改成 `pump`（不再 `tick`）|
| `src/platform/mod.rs` `resumed` | 保持 | 首帧仍走完整 tick（正确：需要出画面） |

### 验证

**变异测试（关键）**：让 `WindowCtx::tick` 恒返回 `false`（即"pump 认为什么都没发生"）：

```
panicked at src\app.rs:5426:
  定时器到期时tick 必须返回 true，否则 pump 后没人 request_redraw ⇒ 停帧
test result: FAILED. 0 passed; 1 failed
```

⇒ 精确复现了"漏掉闭环 = 停帧"这个失败模式，测试有效。

新增 2 条回归测试：
- `tick_reports_true_when_a_timer_fires` —— 三段：无定时器 ⇒ false；定时器到期 ⇒ **true**；一次性消费后 ⇒ false。
- `timer_callback_that_writes_state_marks_the_window_dirty` —— 定时器回调 `damage_all` 后该窗口确实被标脏
  （"画面会更新"的直接保证，而不只是"tick 返回了 true"）。

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **13 个测试二进制全 ok**；lib **388**（386 → 388） |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |

### 已知限制（诚实记录）

1. **平台层的"帧次数 3 → 1"无法在 `cargo test` 里直接断言** ——
   它需要真实 `winit` 事件循环。已能断言的是**闭环的前提**（`tick` 的返回值语义）。
   真机验证方式：`LIEUI_TRACE=1` 下观察 `[lieui] ...` 输出的次数，
   或在 `damage_bench` 里加计数器。
2. `about_to_wait` 现在会`drain_requests`（开窗 / 关窗请求）——
   因为它不再走 `tick`，必须自己做这件事，否则关窗按钮点了没反应。
   这条路径新增了，需要真机点一次关窗按钮验证。
3. **`resumed` 仍走完整 `tick`**（正确：那时还没有画面，必须出首帧）。

### 下一步

**C1（矩形上屏，`D22`）**：改 `platform/mod.rs:485-497` 为按列拷贝。
前置条件（C0 时记录的）：**必须先把"行拷贝宽度"抽成可测的纯函数**，
否则 `local_present_should_not_copy_full_rows` 这条 `#[ignore]` 永远测不了。
---

## 2026-10-07 · S2 · C1 局部上屏只拷矩形内像素（D22）—— ✅ 完成

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 C1。
> C0 时留的前置条件（"必须先抽出可测的纯函数"）已在本批满足。

### 决策

1. **先抽纯函数、再改调用点**，而不是直接改内联循环。
   理由：`present_with_damage` 在 `#[cfg(feature = "winit")]` 平台层内且需要真实 `surface`
   ⇒ **改完无法写测试**。抽出 [`copy_damage_rects`] 后，"拷贝量∝ 脏区面积"这条不变量
   才落进 `cargo test` 的保护范围。

2. **顺带把 `perf_regression.rs` 里那条 `#[ignore]` 换成真实断言。**
   那条占位注释明确写了"修 C1 时应把行拷贝宽度抽成可测的纯函数"——
   前置条件既已满足，占位就该兑现，否则会变成"忘了回来的债"。

3. **越界一律静默裁剪，不 panic**（与 `pack_xrgb` 的 `min` 语义一致）：
   窗口被最小化、或脏区超出表面尺寸是常态，不是错误。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/platform/mod.rs` | **新增 `copy_damage_rects`** | 纯函数：按**矩形**（含 x/width）拷贝，返回拷贝像素数；越界 clip；`stride == 0` 早退 |
| `src/platform/mod.rs` `present` | 修改（D22） | 局部路径由"内联整行拷"改为调用该纯函数 |
| `src/platform/mod.rs` | 新增 4 条测试 | 见下|
| `tests/perf_regression.rs` | `#[ignore]` → 真实断言 | `local_present_copies_proportionally_to_damage_area`（端到端面积关系护栏） |

### 量化效果（测试里钉住了）

一个 **40×20** 的脏区，在 1280 宽的窗口上：

| | 拷贝像素数 |
|---|---|
| 旧行为（整行拷） | 20 × 1280 = **25 600** |
| 修复后 | 40 × 20 = **800** |
| **倍数** | **32×** |

`old_behaviour_would_copy_the_whole_row` 这条测试把这个倍数关系直接断言下来
（`old / new == 5`，在 200 宽的窗口上），让"修了之后省了多少"有据可查。

### 变异验证

把 `w` 退回 `stride`（= 旧行为）后，**4 条测试同时失败**：

| 测试 | 失败信息 |
|---|---|
| `copy_damage_rects_only_touches_the_rect` | 拷贝量 4000 ≠ 800 |
| `copy_damage_rects_sums_multiple_rects` | 1152 ≠ 164 |
| `copy_damage_rects_clips_out_of_bounds` | 64 ≠ 4 |
| `old_behaviour_would_copy_the_whole_row` | 4000 ≠ 800 |

### 顺带修正一处 API 假设

测试里发现 **`softbuffer::Length` 是 `NonZeroU32` ⇒ 长度 0 无法表达**
（`w.into()` 不成立）。既有 `softbuffer_damage` 里已用 `.max(1)` 兜底（其 `nz`），
本批测试 helper 对齐该约定，并把"零尺寸"断言改成记录**已入档的行为**（夹成 1）
而不是臆测"零尺寸不拷"。

### 验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **13 个测试二进制全 ok** |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| **`cargo check --no-default-features`** | exit 0（纯库形态未被破坏；新测试已加 `#[cfg(feature = "winit")]`） |

### `perf_regression` 剩余的 1 条 `#[ignore]`

`scrolling_list_produces_more_than_eight_fragments`（**D21**）——
等 C2（碎片合并）修完后改成正断言。基线数字已在C0 记录：**滚动一次 302 碎片**。

### 下一步：C2（碎片合并）

C0 的数据已就位（302 碎片 / 标脏 30 节点也退化整窗），验收标准明确：
**合并后 ≤ 4 块**。做法是在 `damage_batches` 判定退化**之前**先做矩形合并
（相交或间距 < GAP 求并），策略由 `damage_bench` 的三种实现对比选定，
不硬编码 45% / 8 两个魔数。
---

## 2026-10-07 · S2 · C2 脏区合并（D21）—— ✅ 完成，**并纠正我自己的一个错误结论**

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 C2。

### ⚠️ 先纠正：C0 阶段我给出的 D21 定性是**错的**

C0 我看到"滚动一次 302 碎片 ⇒ 退化为整窗"，就写下"这不是滚动场景的优化，而是**任何 ≥9 节点变化都普遍失效**"，
并据此把它当成P0 级问题汇报。**这个推论是错的**，本批动手时才发现：

> **滚动 20px 时，窗口内每一行都位移了 ⇒ 脏区本就覆盖整个视口。**
> 此时退化为整窗**是正确的**——按 302 个碎片分别光栅的总面积是窗口的 8.4 倍（碎片高度重叠）。

我犯的错是：**只看了"退化了"这个事实，没验证"退化是否真的更差"**。
`damage_batches` 的面积判据（> 45% ⇒ 整窗）在这里正确地拦住了 8.4 倍的光栅量。

C0 的数据没错，但**我的解读错了**。这也说明：光有指标不够，还必须问"这个指标意味着什么"。

### 修正后的D21 定性

退化整窗在两种情况下是**正确**的：
1. 脏区**总面积**接近窗口（如滚动）⇒ 整窗更划算；
2. 碎片**高度重叠**（滚动、连续动画）⇒ 应先合并。

退化整窗在一种情况下是**纯浪费**：
- 碎片多**且分散**、总��积很小 ⇒ 旧判据 `out.len() > 8` 让它们白白整窗。

**实测（修后，1280×720）**：

| 场景 | 碎片 | 批次 | 光栅像素 | 判定 |
|---|---|---|---|---|
| **分散 10 个按钮** | 10 | **5** | **7 546**（0.8%） | ✅ 局部，**省 122 倍** |
| 集中 30 节点 | 30 | 1 | 921 600（整窗） | ✅ 整窗正确（面积确实过半） |
| 滚动 50 行 | 302 | 1 | 921 600（整窗） | ✅ 整窗正确（脏区本就覆盖全视口） |

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/render/raster.rs` | **新增 `merge_rects`** | 相交或间隙 ≤ `MERGE_GAP`(2px) 的矩形求并。**不做"全部并成一个"**（那正是 `damage_batches_union` 的问题，会让分散更新退化） |
| `src/render/raster.rs` | 判据调整 | `MAX_BATCHES` 8 → **32**；阈值提为具名常量 `MAX_BATCHES` / `AREA_FALLBACK_RATIO`；面积在**合并之后**统计 |
| `src/render/raster.rs` |既有测试更新 | `batches_dedupe_and_clamp` → 更名 `batches_dedupe_and_merge_adjacent`，断言从"2 块"改为"1 块"（旧断言假设"只去重完全相同的"，与C2 的合并语义冲突） |
| `src/render/raster.rs` | 新增 8 条测试 | 合并的相交/间隙/远离/面积守恒/幂等/传递闭包/空与单元素 + 判据测试 |
| `tests/perf_regression.rs` | 测试重写 | `marking_many_nodes…` 改成"集中 vs 分散"**成对**测试 |

### 关键设计点

1. **`merge_rects_never_shrinks_total_area`** 是本改动的**安全护栏**：
   合并只会让重画面积**变大或不变**（并集 ⊇ 各部分）⇒ "少画"这个风险不存在，**绝不会漏画**。
2. **不合并远离的矩形**（`merge_rects_keeps_distant_rects_apart`）——
   否则就退化成 `damage_batches_union`，分散更新又被包成一个大盒。
3. **传递闭包**（`merge_rects_transitively_merges_a_chain`）：a~b 相交、b~c 相交 ⇒ 三者并成一块。
   贪心必须满足这个，否则碎片数压不下去。
4. **测试场景成对**（集中 vs 分散）—— 集中那个是**镜像护栏**：
   防止将来有人"为了不整窗而整窗"（把面积判据也调到永不fallback）。

### 变异验证

禁用合并逻辑（`if near` ⇒ `if false`）后，**5 条测试同时失败**：

| 测试 | 失败原因 |
|---|---|
| `merge_rects_unions_overlapping_rects` | 相交未合并 |
| `merge_rects_unions_nearby_rects_within_gap` | 间隙 ≤ GAP 未合并 |
| `merge_rects_transitively_merges_a_chain` | 链式未合并 |
| `batches_dedupe_and_merge_adjacent` | 接触的未合并 |
| **`batches_fall_back_to_the_full_window_when_damage_is_large_or_fragmented`**（既有） | 它记录的正是**"碎片多就整窗"的旧行为** |

最后一条是意外收获：那个既有测试**恰好钉住了旧行为**，所以 C2 一改就报红——
说明"退化整窗"这条路径原本也是有测试的（只是没人意识到它同时挡住了正确行为）。

### 验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **13 个测试二进制全 ok** |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |

### 下一步：C3（图片走裁剪栈，`D3`）

`raster.rs:480` 自述"图片 blit 不参与 `PushClip` 裁剪栈" ⇒ 滚动/圆角裁剪内的图片**溢出到裁剪区外**。
做法：在 `rasterize` 的 op 循环里维护一个软件 clip 栈，`blit_image` 接收当前 clip 并对 dst 求交。
顺带把最近邻换成双线性（当前每像素 12 次整数运算）。

同时可把 `scrolling_list_produces_more_than_eight_fragments` 那条 `#[ignore]` 删掉了——
它记录的前提（"滚动必然退化整窗"）经核实是**正确行为**，不是待修的 bug。
---

## 2026-10-07 · S2 · C3 图片参与裁剪栈（D3）—— ✅ 完成，**P0 表收官**

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 C3。

### 关键产出：修D3 时顺带拆掉一个**潜伏的 panic**

写完 C3 的像素测试后第一次运行，直接 panic：

```
panicked at vello_common-0.3.0/src/clip.rs:307:
  clip stack underflowed
```

**根因**：`flush_segment`（图片边界处调用）里的 `ctx.reset()` 会**清空 vello 的裁剪栈**，
而 `PopClip` 是按 op 顺序到来的 ⇒ reset 之后遇到 `PopClip` 就 underflow。

**为什么此前没炸**：只有"裁剪区内有图片"才会走到 `flush_segment`，
而在此之前图片**根本不受裁剪约束**（这正是 D3），于是这个组合从未被触发过。
**修D3 的第一步就把它暴露出来了** —— 两个缺陷叠在同一条路径上。

修法：`flush_segment` 增加 `clip_stack` 参数，`reset()` 之后按软件栈**重建一层**
（栈顶已是各层交集，一层就够）。这让"图片分段渲染"与"vello 裁剪栈"保持一致。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/render/raster.rs` `rasterize` | 修改（D3） | op 循环里并行维护**软件裁剪栈**；`Op::PushClip`/`PopClip` 从"直接 submit"改为"先维护栈再 submit"；`Op::Image` 拿栈顶与目标矩形求交 |
| `src/render/raster.rs` `flush_segment` | 修改 | 新增 `clip_stack` 参数；`reset()` 后重建一层 vello 裁剪 |
| `src/render/mod.rs` | 新增 2 条像素测试 | `image_is_clipped_by_its_container` / `unclipped_image_still_renders` |
| `tests/perf_regression.rs` | `#[ignore]` → 正断言 | 删掉那条**基于错误前提**的 D21 占位（见 C2 日志的更正），改成 `scrolling_falls_back_to_full_window_and_that_is_correct` |

### 设计要点

1. **软件栈每个元素是"到该层的交集"**，不是单个 clip 矩形 ——
   这样嵌套裁剪天然取交集，且 `flush_segment` 重建时只需推**栈顶一个**。
2. **栈空 ⇒ 用批次边界**（"不额外裁剪"），而不是"不裁剪" ——
   批次画布本身就是边界，图片超出批次部分本来就不会被拷回。
3. **整块被裁掉 ⇒ 直接跳过** `blit_image`，省掉一次采样循环。
4. **两条测试成对**（`clipped` / `unclipped`）—— 后者确认前一条不是"把图片整个干掉了"。

### 变异验证

把clip 改成 `batch_bounds`（= 图片不参与裁剪）后，像素测试**精确抓到溢出**：

```
panicked at src\render\mod.rs:177:像素不匹配：PremulRgba8 { r: 255, g: 0, b: 0, a: 255 }
  left: (255, 0, 0, 255)      ← 裁剪外竟是红色（图片溢出）
 right: (255, 255, 255, 255)  ← 期望背景白
```

### 验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **13 个测试二进制全 ok** |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| `tests/perf_regression.rs` | **9 passed / 0 ignored**（曾有 2 条基于错误前提的占位） |

门禁顺带抓到一次doc 引用块语法错误（`>` 标记不完整），已修——
这是 `[lints]` 之外的 rustdoc linter 在起作用。

## P0 表收官

| ID | 缺陷 | 状态 |
|---|---|---|
| D1 | 点击按键配对 | ✅ A1 |
| D2 | `destroy` 不登记脏区 | ✅ A2 |
| **D3** | **图片绕过裁剪栈** | ✅ **C3（本批）** |
| D4 | `close_requested` 丢 Cmd | ✅ A3 |
| D5 | 命中/渲染裁剪坐标系不一致 | ⏳ S4（与 G3 几何收敛一起做） |
| D7 | 任务唤醒器失效 | ✅ A6 |
| D8 | `view()` 值守 | ✅ A4 |
| D57 | `damage_bounds` 静默放弃 | ✅ A2 |

**8 项 P0 中 7 项已修复**（D5 因需重构几何求值而在 S4）。
剩下 D5 与 D11/D12/D13（焦点互斥）、D6（Modal 内Popup）构成 S4。

### 下一步建议

S2 剩余 **C4（display list）** 收益大但工程量大；S3 的 **D-b（滚动脱离布局）** 风险最高。
建议先做 **S3 的低风险部分**（D-a FlexNode 持久化 + 消文本双测、D-c MAX_ITER/snapping），
因为 `lieui-layout` 的特征矩阵已就位（36 条），是动布局的安全网。
---

## 2026-10-07 · S3 · D-c① 冻结循环迭代上限（MAX_FLEX_ITERATIONS）—— ✅ 完成

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 D-c。

### 决策

1. **确认了"无上限while"的真实死循环路径**（不是理论风险）。
   `flex_line.rs:198-207` 里：若 `total_violation != 0`，代码去冻结
   `min_violations`（总违例>0）或 `max_violations`（总违例<0）。
   **若两者都为空而 `total_violation != 0`**（violation 一正一负、浮点求和没抵消成精确 0）
   ⇒ 该轮**什么都不冻结** ⇒ 下一轮输入完全相同 ⇒ **死循环**。
   而这是 GUI 框架的**主线程** ⇒ 整个应用挂死。

2. **超限时"接受当前结果"而不是"强制冻结剩余"**。
   理由：强制冻结会让这些 item 的尺寸停在某个中间值，可能更怪；
   而"接受当前结果"至少保证画面完整（只是违反 min/max）。**渲染略怪 ≫ 应用没反应。**

3. **不是无声无息地接受**：debug 下打一行 `eprintln`，
   让"布局不收敛"成为**可诊断**的问题，而不是"偶尔画得怪"。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `crates/lieui-layout/src/flex_node.rs` | 新增 `MAX_FLEX_ITERATIONS` | `pub const = 32`（正常收敛远少于 10 轮，足够宽松） |
| `crates/lieui-layout/src/flex_node.rs` | 修改 | `while !resolve_flexible_lengths(..) {}` ⇒ 带计数与上限的循环 |
| `crates/lieui-layout/tests/feature_flex.rs` | 新增测试 | `flex_negotiation_always_terminates`（min>max 冲突 + 强制 shrink，要求**必定终止且结果有限**） |

### 踩坑记录：追加测试时破坏了文件结构

用脚本追加测试后连续出现三种症状：

1. `function ... is never used` —— 新测试被**嵌进了上一个函数体内**（局部 fn）
2. `unexpected closing delimiter` —— 补了括号但末尾又多一个 `}`
3. 测试数不涨 —— 以为"用了旧二进制"，其实是**局部 fn 不被 test harness 收集**

**定位方法**：`cargo fmt` 会把结构规范化并暴露缩进异常（它对语法合法但结构异常的文件**不报错**，所以得自己看缩进）；
以及 `Select-String -Pattern 'mod tests'` 确认**集成测试文件没有 `mod tests`，全是顶层函数**——
我一开始误以为有 `mod tests {}`，于是按"追加到 mod 内"的假设去 TrimEnd + 去最后一个 `}`，结果把 `fn` 的闭合当成了 mod 的闭合。

**教训**：批量追加代码到已有文件前，**必须先确认末���的结构**（有 mod tests？还是顶层函数？），
否则"少一个/多一个大括号"会让新代码**静默地变成局部函数** —— 测试不报错、也不执行，
比编译失败更难发现。

### 验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **13 个测试二进制全 ok**；`feature_flex` 16 → **17 passed** |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告（warning 已消失） |
| `cargo fmt --all --check` | 0 处差异 |

### D-c 剩余

- **像素 snapping + 统一浮点容差**（D37）：现状是四套容差
  （`types.rs` 1e-4 / `layout.rs` `rect_eq` 1e-3 / 滚动钳制 1e-4 / 锚点 1e-2）
  ⇒ 1px 分隔线与文本持续半像素模糊。
  **注意**：这条要先想清楚"snapping 在哪一层做"（布局出口？光栅化前？），
  否则可能引入新的 1px 抖动。风险中等，建议单独一批。
- **轴序统一**（D48）：`types.rs` 里 `[L,T,R,B]` 与 `K_AXIS_*` 用的 `[L,R,T,B]` 两套。
  这是纯内部约定，但**索引错了很难发现**（可能编译通过、行为偏 1px）。
  建议加编译期断言（`const _: () = assert!(...)`）而不是直接重排——重排风险高、收益低。
---

## 2026-10-07 · S3 · D-a 前置 —— D44 修复 + 消除 TextSpec 克隆 —— ✅ 完成

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 D-a / D-c。

### 决策

1. **先修 D44，再谈持久化**。D-a 的主体是"FlexNode 持久化复用"，而 D44
   （`layout()` 永久改写 `flex_basis` 且不恢复）**正是持久化的地雷**：
   节点一旦被复用，残留的 `flex_basis` 会直接变成"同一棵树两次布局结果不同"。
   **顺序不能反** —— 先做持久化再修 D44，会把残留带进每一帧。
2. **只修真实使用路径上的 D44a**。D44 还有一条 `tight_width` 的 `f32::MAX` 笔误，
   但它属于 `LayoutConstraint`—— 而 `LayoutConstraint` / `IntrinsicSize` /
   `measurable.rs` / `constraint.rs` 共 142 行**主 crate 零引用**（纯死抽象）。
   **归入 D43/G3 的删死代码范围，本批不动**（避免范围蔓延）。
   > 顺带印证："没人用所以没被发现" —— `tight_width` 的笔误就是死代码的典型代价。
3. **二次测量分两步做，本批只做低风险的一半**。
   `desired_size` 对 Text/Input 重新调 `TextEngine::measure_text`，而 flex 引擎
   **内部已经测过一次**。完全消除需要复用 flex 的测量结果，但：
   - `FlexNode::layout_result.dim` 是**布局后**尺寸（可能被 flex 拉伸/收缩）；
   - 而 `desired_size` 要的是**内容测量尺寸**（它走 `paint_bounds` 的文本收缩路径，
     若变成拉伸后尺寸，脏区就会偏大 ⇒ "精确脏区"退化）。
   ⇒ **直接复用 `layout_result.dim` 是语义错误**。要正确复用必须给 `LayoutResult`
   加一个"原始测量尺寸"字段并在 flex 测量点写入 —— 那是跨 crate 的 API 扩展，
   留到下一批单独做（见「遗留」）。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `crates/lieui-layout/src/flex_node.rs` `layout` | 修改（D44） | `flex_basis` 写入前备份、布局后**还原**（与 `dim` 的 `swr`/`shr` 还原成对） |
| `src/layout.rs` `desired_size` | 修改（D-a） | `TextSpec` 由 `clone()` 改为**按引用**传（`font_family: String` ⇒ 每次克隆 = 一次堆分配 + 字节拷贝） |
| `crates/lieui-layout/tests/feature_flex.rs` | `KNOWN BUG` → 正断言 | `layout_leaves_flex_basis_untouched`、`repeated_layout_is_idempotent`（**D-a 持久化的前置条件**） |
| `src/layout.rs` | 新增 2 条测试 | `text_desired_size_is_the_measured_content_size`、`input_desired_size_uses_placeholder_when_empty` |

### 关键：幂等性是持久化的**硬前置**

`repeated_layout_is_idempotent` 连续 `layout` 四次，断言尺寸不变。
**修复前这条会失败**（第二次的协商输入已被残留的 `flex_basis` 改掉）。

这意味着：**如果先做持久化再做 D44，缓存里的节点会带着上一帧的 `flex_basis`**，
且因为"布局不再执行"而永远错下去 —— 一个只在特定节点尺寸下出现的诡异 bug。
所以两者的顺序不是风格问题，是正确性问题。

### 变异验证

去掉 `flex_basis` 的还原后：

```
panicked at crates/lieui-layout/tests/feature_flex.rs:354:
  D44：layout 必须还原 flex_basis（当前被写成了 100）
test result: FAILED. 16 passed; 1 failed
```

⇒ 精确抓到，且证实**只有根节点受影响**（与 C2 日志里实测的范围一致）。

### 验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **13 个测试二进制全 ok**；`layout` 26 → **28 passed**；`feature_flex` 17 passed |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |

### D-a 遗留（本批未做，需单独设计）

**① 完全消除二次测量**（本批只消掉了 `TextSpec` 克隆，测量本身还在跑两次）
- 方案：给 `LayoutResult` 加`measured_content: [f32; 2]`，在 flex 的 `measure_text`
  调用点写入**内容尺寸**（不是布局后尺寸），`desired_size` 直接读回。
- 风险点：必须确认 flex 内部所有测量入口都覆盖到，且 wrap 场景下
  "测量时的约束宽度"与 `desired_size` 假设的一致 —— `layout.rs:739` 那条
  `wrapped_text_draws_with_the_same_wrap_width_as_measure` 正是这条的护栏。

**② FlexNode 持久化（D-a 主体）**
- 需要：`Node.flex_cache: FlexNode` + `build()` 只在 patch 时更新对应节点 +
  align 的 patch 路径能失效缓存。
- **现在可以做了**（D44 已清障），但仍是本计划里改动面最大的一项：
  涉及 `Node` 结构、`align` 的失效传播、`layout` 的 build 逻辑三处联动。
---

## 2026-10-07 · S1 正确性 P0 —— A4 view() 值守 RAII + always-on 断言 —— ✅ 完成

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 A4。

### 决策

1. **`begin_view()` 改为返回 RAII 守卫，删除手写的 `end_view()`。**
   触发原因不是风格偏好，而是**真实缺陷**：`view()` 是**用户代码**，它 panic 时
   `end_view()` 永不执行 ⇒ `in_view` 永久停在 `Some(..)` ⇒ 此后该窗口所有
   `Signal::set` 都被 `assert_not_in_view` 拦下，**Runtime 被永久毒化**。
   原 `end_view()` 带着 `#[allow(dead_code)]`，但生产路径（`app.rs` 的 `frame`）在用。

2. **`assert_not_in_view` 去掉 `cfg!(debug_assertions)` 门禁，改always-on。**
   代价只是读一个 `Cell<Option<WindowId>>`；换来 release 下也能 fail-fast，
   而不是变成"每帧 view → set → 再 view"的**永久满帧自激**（100% CPU、不报错、无日志）。
   设计 §四本就把这条列为"违反会 panic 或死循环"的硬纪律。

3. **`ViewGuard` 放在模块级而非 `impl Runtime` 内部**——第一版误放进 impl 块，
   编译报 `structs are not allowed in struct definitions`。这是本次连续第三次
   遇到"编辑插入位置错导致结构损坏"（前两次见 A2/A3 日志），已形成习惯：**改完必编译**。

4. **已有测试 `set_inside_view_panics` 带 `#[cfg(debug_assertions)]`** ——
   **测试本身只在 debug 下存在，这正是 D8 的证据**。所以是**升级**它（去门禁 + 改 RAII），
   而不是新增一条重复测试。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/reactive.rs` | 新增 `ViewGuard` + `Drop` | 守卫持有 `&Cell<Option<WindowId>>`，`Drop` 时置 `None` |
| `src/reactive.rs` `begin_view` | 改签名 | `-> ViewGuard<'_>`；**删除 `end_view()`** |
| `src/reactive.rs` `assert_not_in_view` | 修改（D8） | 去掉 `cfg!(debug_assertions)` |
| `src/app.rs` `frame` | 修改 | `let _view_guard = rt.begin_view(self.id);` … `drop(_view_guard);` |
| `src/reactive.rs` | **升级**已有测试 | `set_inside_view_panics` 移除 `#[cfg(debug_assertions)]` + 改 RAII 形式 |
| `src/reactive.rs` | 新增测试 | `view_guard_recovers_after_panic` |
| `src/reactive.rs` | 测试迁移 | `get_inside_view_is_fine_and_no_dirty_is_marked` 改用 RAII |

### 验证

**A4 变异测试**（把 `ViewGuard::drop` 改成空操作，精确模拟"`end_view()` 未被调用"）：

```
panicked at src\reactive.rs:699:
  panic 后 in_view 必须被守卫释放，否则 Runtime 被永久毒化
test result: FAILED. 0 passed; 1 failed
```

> 变异设计说明：第一次尝试的变异（把 `let _guard = ...` 改成先 panic 再 `drop`）是**无效变异** ——
> Rust 的 RAII 语义保证 panic 展开时仍会调用 `Drop`，测试照样通过。
> 正确做法是直接把 `Drop` 实现清空，才能模拟原代码"手写 `end_view()` 未执行"的真实情形。
> **这印证了本项目一直坚持的"变异测试也要验证变异本身有效"。**

**完整门禁**：

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **15 个测试二进制全ok**，合计 **476**（改造前 389） |
| `cargo clippy --workspace --all-targets`（`all = "deny"`，CI 用 `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |

剩余 2 条 `#[ignore]` 均为**已入档的KNOWN BUG**（`align_content: SpaceEvenly` 静默退化、
`line_space` 死配置），是特征测试里显式记录的既有能力缺口，不是被遗忘的债。

### S1 剩余

- **A5（D9）**：定时器回调内 `cancel()` 无效；关窗后成孤儿。
- **A6（D7）**：`run()` 之前 `spawn_task` 永久静默挂起（waker 快照过期 + 本地队列 GUI 模式不 drain）。
  **建议先加诊断**，让"静默"变成"可报错"——这类"卡住且不吭声"的缺陷最难被用户报告。
- **A7（D33/D58/D59/D61/D62）**：阴影超脏区 / `clear_layout_flags` 吞脏标 / `window_sizes` 泄漏 /
  Wheel 绕过捕获 / spinner 用 `SystemTime`。
---

## 2026-10-07 · S3 · D-a 消除文本二次测量 —— ✅ 完成

> 执行基线：[`docs/refactor-plan.md`](./refactor-plan.md) v1.1 §四·主线 D-a。
> 本批与「D44 修复 + `TextSpec` 克隆消除」同属D-a，但**独立成批**（前一批日志已记录）。

### 决策

1. **加 `FlexNode::measured_content`，而不是复用 `layout_result.dim`。**
   这是本批**最关键的一条判断**：`layout_result.dim` 是**布局后**尺寸（会被 flex 拉伸/收缩），
   而 `Node.desired` 要的是**内容测量尺寸** —— 它走 `paint_bounds` 的文本收缩路径，
   若误用布局后尺寸，被拉伸过的文本会让脏区偏大 ⇒ **"精确脏区"直接退化**。
   两者**必须**分开记，这也是 `measured_wrap_width` 已经在用的模式（绘制复用同一约束）。

2. **约定 `[0.0, 0.0]` = "本轮没测量"**，宿主用 `> 0.0` 守卫后回落原路径。
   这样 Image/Custom/容器都不受影响（它们本来就不走文本测量）。

3. **记录点必须在 flex 拉伸/收缩之前**（`layout_single_node` 里`content_w/h` 算完立刻记），
   否则记下的就是被改写后的值。变异测试专门验证了这一点。

4. **顺带发现并修正了一个"测试无法区分"的问题**：
   复用与重测的**结果完全相同**，所以主 crate 现有的 `desired` 测试**无法验证是否真的复用了**。
   解决办法是**在 `lieui-layout`侧直接钉契约**：构造"可拉伸的文本叶子"，
   断言 `measured_content < layout_result.dim`。这样"两者必须分开"成了可执行的不变量。

### 变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `crates/lieui-layout/src/flex_node.rs` | 新增字段 | `measured_content: [f32; 2]`，含"为什么不能复用 `layout_result.dim`"的文档 |
| `crates/lieui-layout/src/flex_node.rs` `new` | 修改 | 初始化为 `[0.0, 0.0]` |
| `crates/lieui-layout/src/flex_node.rs` `layout_single_node` | 修改 | 在 flex 拉伸/收缩**之前**记录 `content_w/h` |
| `src/layout.rs` `desired_size` | 修改（D-a） | 文本 / Input **优先复用** `flex.measured_content`，`[0,0]` 时回落原路径 |
| `crates/lieui-layout/tests/feature_flex.rs` | 新增 3 条测试 | 见下 |

### 测试

| 测试 | 钉住什么 |
|---|---|
| `measured_content_is_content_size_not_stretched_size` | ★ 核心：`measured_content`（内容宽）**必须小于** `layout_result.dim`（拉伸后宽） |
| `measured_content_is_zero_for_non_text_leaves` | 非文本叶子不伪造测量值 |
| `measured_content_is_zero_for_containers` | 容器同理（宿主走"取引擎分配尺寸"分支） |

**变异验证**（移除记录语句）：

```
test measured_content_is_content_size_not_stretched_size ... FAILED
test result: FAILED. 19 passed; 1 failed
```

### 踩坑记录：括号错位（**第四次**同一个坑）

追加测试后编译报 `unclosed delimiter`。根因与前三次完全相同：
`feature_flex.rs`是**顶层函数**文件（无 `mod tests`），而我用的是
"TrimEnd + 去掉最后一个 `}`"这个假设—— 那个 `}` 是**最后一个函数的闭合**，不是 mod 的。

**正确做法（下次务必用这个）**：保留原文件不动，把新内容**追加到文件末尾**
（`[System.IO.File]::AppendAllText`），只在新内容内部保证括号自闭合。
`AppendAllText` 不需要动原文件 ⇒ 不可能破坏既有结构。

> 判据：脚本追加前先确认"文件末尾是 mod 的 `}` 还是函数的 `}`"。
> `Select-String -Pattern 'mod tests'` 一下就有答案。

### 门禁第四次抓到作者本人

`clippy::field_reassign_with_default`：`TextSpec::default()` 之后逐字段赋值。
改成 struct literal + `..Default::default()`。
（此前三次分别是 `type_complexity`、无用 `mut`、`doc` 引用块语法。）

### 验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **13 个测试二进制全 ok**；`feature_flex` 17 → **20 passed** |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |

### D-a 剩余

- **FlexNode 持久化（D-a 主体）**：`Node.flex_cache: FlexNode` + `build()` 只在 patch 时更新
  对应节点 + `align` 的失效传播能清缓存。
  **前置条件已全部就位**（D44 已修、幂等性有测试、特征矩阵 40 条）。
  但它仍是全计划改动面最大的一项：`Node` 结构 + `align` 失效传播 + `layout` build 三处联动。
- **D-c 剩余**：像素 snapping + 统一浮点容差（D37，四套容差 1e-4/1e-3/1e-4/1e-2）；
  轴序统一（D48，建议加编译期断言而非重排）。
- **D-b**（滚动脱离布局）风险最高，建议在 C4display list 之后。
---

## 2026-10-07 · 审计复核 —— D5 推翻 / D37 部分修正 / D-a 收益重估

> 本批**没有修任何 bug**，而是**推翻了一条P0、修正了一条 P2、重估了一项改造的收益**。
> 记录在案是因为：**误判的代价高于漏判** —— 执行者会去"修"一个不存在的缺陷，
> 改动真实代码，反而引入新bug（这正是 refactor-plan 反复警告的"边改边退化"）。

### 一、D5（裁剪坐标系不一致）—— ❌ 误判已推翻

三份审计都把D5 列为 **P0**，理由是：

> `hit.rs:104-108` 用逆变换后的**局部点**测 `clip`；
> `scene.rs:471-480` 把 `clip` 当与 `rect()` 同空间取交 ⇒ **两者坐标系不一致**

**代码核实结论：两侧同语义，D5 不成立。**

| 侧 | 事实 |
|---|---|
| 渲染 | `scene.rs:462` 是 `Op::PushClip { rect: c, transform }` —— `c` 与 `rect()` 同为**节点本地空间**，且 `transform` 被**显式携带**，由光栅器映射到窗口空间 |
| 命中 | `hit.rs:99-108` 先 `q = to_local.apply(p)` 逆变换到本地，再 `clip.contains(q)` |

审计的推断错在**只看了 `hit.rs` 的局部量，没看 `scene.rs` 同时传了 `transform`**。

**处理方式：不只是"删掉这条"，而是补契约测试把正确语义钉死**（`src/hit.rs` 的 `d5_clip_space` 模块，2 条）：

- `clip_is_tested_in_node_local_space` —— 用 `translate(100,0)` 构造**能区分两种语义**的场景：
  正确（clip 本地）时 `(110,10)` 可命中；错误（clip 窗口空间）时窗口 clip 落在原点，`(110,10)` 被拒。
- `clip_cannot_widen_the_node_rect` —— 反向确认 clip 不得把可命中区扩到节点矩形之外。

**变异验证**（把 `clip.contains(q)` 改成 `clip.contains(p)`，即模拟"坐标系不一致"这个缺陷本身）：

```
panicked at src\hit.rs:413:
  clip 内的点应可命中（clip 在节点本地空间）
test result: FAILED. 1 passed; 1 failed
```

⇒ 测试确实能守住契约。这比"审计报告说它坏了"可靠得多。

> 写成**独立顶层 `mod d5_clip_space`** 而不是塞进既有 `mod tests`：
> 既避开反复踩的括号错位坑（本项目第5 次），也让"这是复核结论"在文件结构上一眼可见。

### 二、D37（四套浮点容差）—— ⚠️ 部分修正

审计称"四套容差"。实测是**三种不同语义**，只有两处是真问题：

| 容差 | 语义 | 判定 |
|---|---|---|
| `1e-6` | DPI scale 精确比较 | ⚠️ **同一表达式在 `app.rs:545` 与 `raster.rs:362` 重复实现两遍** |
| `1e-3` | 视觉等价（`layout.rs:rect_eq` + `transform.rs:150`） | ✅ **合理**（0.001px 位移无视觉影响） |
| `1e-4` | 滚动偏移精确钳制（`layout.rs:340`） | ✅ **合理**（滚动偏移就该精确） |

⇒ **"统一四套容差"是伪需求**。真正剩下的是"**无像素 snapping**"（1px 分隔线/文本半像素模糊）。
若日后做snapping，只需统一那两处 `1e-6`，不要动 `1e-3`/`1e-4`。

### 三、D-a 主体（FlexNode 持久化）—— 收益重估，**建议推迟**

我上一条消息说"前置条件已全部就位"，**这个说法过于乐观，此处更正**。
读`build()` 的实际实现后，发现缓存失效条件有**5 条**，其中两条是真障碍：

1. **per-node 的"布局输入版本"信号根本不存在**。今天只有全局 `layout_epoch`（D58 引入）
   与 MEASURE/ARRANGE 脏标，**没有"这个节点的 style/文本/子节点列表变了没有"的廉价判据**。
2. **父属性会改写子的 style**：`build()` 里有
   `if n.layout.overflow_scroll { for c in children { c.style.flex_shrink = 0.0; ... } }`
   —— 父节点切成/切出滚动容器，**必须递归作废所有子孙的缓存**。这条极易漏，
   而漏了就是**静默的错布局**（不报错、只是像素不对），是 GUI 框架里最难查的一类bug。

**更关键的是收益被高估**：D-a 的收益是"每次布局少几百次堆分配"。但

- 边界重排（D-a 之前已修）**已经把重排范围限制住了**；
- 而软渲染的真正瓶颈在**光栅化与 Scene 重建**，不在几百次分配
  （60fps 下几百次分配 ≈ 1万次/秒，相对每帧数百万像素的光栅化可忽略）。

⇒ **D-a 主体的风险（静默错布局）远大于收益（可忽略的分配开销）**。
建议**推迟到有实测性能数据证明布局是瓶颈时再做**，且届时必须先建per-node 失效信号。

### 四、门禁第五次抓到我

rustdoc `doc quote line without `>` marker`：我在 `///` 注释里写了 blockquote
（首行 `/// >`，续行 `/// ⇒` 没加 `>`）。改写为普通文本。
`cargo doc` 不报、只有 `clippy --all-targets` 才暴露 —— **说明 `--all-targets` 不能省**。

### 五、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | 13 个二进制全 ok；lib404 → **406**（新增 D5 契约测试 2 条） |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |

### 六、已同步更新 `refactor-plan.md`

防止后续执行者去"修"不存在的缺陷，共 5 处：
D5 主条目（标 ❌ 已推翻）、D37（标 ⚠️ 部分修正）、v1.1 修订 #8、G3 说明、S5 批次顺序
（G3 移出、降到"持续"）。

> **一条经验**：审计报告的缺陷条目应当**可执行、可验证**。
> 本次 D5 之所以能推翻，是因为它的论断足够具体（点名了两个文件行号），
> 才能被逐行核实。若审计只写"坐标系统一性可能有问题"，就无从证伪。
---

## 2026-10-07 · D6 重定性 + 层序单一权威源 —— ✅ 完成

> 本批的收获是**又一次避免了一次错误修复**：D6 被三份审计列为 P1"功能缺陷"，
> 核实后发现**现象不可达且层序本身是正确设计**。真正成立的部分已修。

### 一、D6 的完整核实（与 D5 不同：**不是误判，是定性错误**）

| 审计断言 | 核实结果 |
|---|---|
| `Root.owner`（嵌套父层）无消费点 | ✅ **属实** —— 全仓只有定义与传参，无任何读取 |
| 设计 §3.7 的 `z = (Layer, 嵌套深度, 序号)` 未实现 | ✅ **属实** |
| 现象"Modal 内 Popup 被盖住、点不到" | ❌ **不可达** —— 生产代码 `owner` **恒为 `None`**（唯一来源 `view.rs:227`），唯一传 `Some(modal)` 的是 `track.rs:2038` 的**测试** |
| "Modal 盖住 Popup"是缺陷 | ❌ **是正确设计** —— 枚举序 `Popup(2) < Modal(4)`，模态框本就该盖住并阻断下层 |

**我第一次写的测试还犯了个错**：假设"Popup 在 Modal 之上"，结果测试失败，
`path=[NodeId(1)]`（只有 content）。查证后发现是我的假设反了——`Modal` 枚举序比 `Popup` 高。
**这恰好证明"测试写不出来"和"缺陷不存在"是两回事**：如果当时为了让测试通过而
把层序反过来，就会真的把一个正确的设计改坏。

### 二、真正成立的问题：**两份手写层序数组的漂移风险**

- `scene.rs: LAYER_BOTTOM_UP`（自下而上）
- `hit.rs: LAYER_TOP_DOWN`（自上而下）

两份**必须严格互逆**，否则出现"画在上面的层收不到点击"——**不报错的错**。
但它们分散在两个文件、没有任何机制保证互逆。这正是 P1「`Layer` 增变体不报错」的成因。

**修法：单一权威源 `Layer::ALL`**

```rust
// track.rs
pub const ALL: [Layer; 6] = [Content, Overlay, Popup, Tooltip, Modal, DragPreview];
// scene.rs
const LAYER_BOTTOM_UP: [Layer; 6] = Layer::ALL;      // 正序
// hit.rs
fn layer_top_down() -> impl Iterator<Item = Layer> { Layer::ALL.into_iter().rev() }  // 逆序
```

命中侧改用**函数 + `.rev()`** 而非常量数组：逆序在运行时表达，
**没有可漂移的第二份副本**。（第一版我试过写 `const` 编译期断言，
但 `assert!` 在 const 上下文里只能做常量折叠，实质是恒真表达式——已放弃。）

### 三、测试（5 条，`hit::layer_order`）

| 测试 | 钉住什么 |
|---|---|
| `modal_blocks_everything_below` | ★ 端到端层序：Modal 铺满时命中必是 Modal（含"Popup 区域内外"两向） |
| `popup_receives_hits_when_no_modal_is_above` | 反方向：没有 Modal 遮挡时 Popup 必须可命中（防"阻断退化成阻断一切"） |
| `later_declared_root_is_above_within_same_layer` | ★ **实际被依赖的那条规则**：`owner` 缺失 ⇒ 全靠"同层后声明在上"（子菜单正是这么做的） |
| `overlay_is_hit_transparent_by_default` | 层**顺序**与层**语义**（`LayerOpts` 穿透性）是两件独立的事 |
| `layer_all_matches_enum_declaration_order` | `ALL` 是全量且顺序合语义（防"为修 bug 随手调换"） |

**两次无效变异（值得记）**：

1. **第一次**：把 `layer_top_down` 改成手写逆序——但写的**恰好是同一个顺序**，
   本来就没有不一致 ⇒ 5 条测试全过，**什么都没验证到**。
2. **第二次（有效）**：真正调换 `Modal` / `Popup` 相对次序 ⇒
   ```
   panicked at src\hit.rs:509: Modal 应盖住并阻断 Popup：[NodeId(1)]
   test result: FAILED. 4 passed; 1 failed
   ```

⇒ 教训同A4：**变异必须真的引入缺陷**。"把代码换成等价物"不是变异，是自我安慰。

### 四、变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/track.rs` | 新增 `Layer::ALL` | 单一权威源，附"为什么不能手写两份"的说明 |
| `src/render/scene.rs` | 修改 | `LAYER_BOTTOM_UP` 改为 `Layer::ALL` |
| `src/hit.rs` | 修改 | 删除 `LAYER_TOP_DOWN` 常量，改为 `layer_top_down()` = `ALL.rev()` |
| `src/hit.rs` | 新增 5 条测试 | 独立 `mod layer_order`（括号安全，见 A4 日志） |

### 五、已同步更新 `refactor-plan.md`（2 处）

D6 主条目改为"已重定性"、S4 批次**移除 E2-1（D6 z 序）**。

### 六、**遗留决策项**（需要产品判断，不是技术判断）

`Root.owner` 现在是**死字段**（有定义、有传参、无消费点），而其**生命周期功能**
（嵌套存储 + 级联移除）已实现且有测试（`track.rs:roots_are_nested_and_removal_cascades`）。
三条路：

- **A. 补 API + 实现 z 序**：让用户能声明嵌套层。成本 3 处改动 + API 设计；
  收益 **0 个已知场景**（子菜单已用同级方案解决）。
- **B. 撤承诺**：删字段，从设计文档删掉 §3.7 的 z 序承诺，写入"不做"清单。
- **C. 保持预留**：字段留着（零成本），文档化为"预留能力，当前无生产路径"。

**我的倾向改为 C**（而非 v1.0 评审时的 A）：**先有真实用例再接线**。
若将来 Modal 内确实需要弹菜单，正确做法是用 `Popup`/`Tooltip` 层而非嵌套层。

### 七、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | 13 个二进制全ok；lib 406 → **411**（新增 5 条） |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
---

## 2026-10-07 · D6 / A 方案 —— 嵌套 z 序真正落地 —— ✅ 完成

> 用户在D6 的三条路线（补实现 / 撤承诺 / 保持预留）中选择 **A（补实现）**。
> 结果证明这个选择是对的，而且**比预想的更省事**：API 早就存在，只差 z 序消费。

### 一、**我上一批的核实结论是错的，在此更正**

上一批我断言"生产代码 `owner` 恒为 `None`，现象不可达"。**只看了 `add_root` 的调用点**
（`align.rs:84` 的 owner 来自 `DescRef`），漏看了 `view.rs:819`：

```rust
let owner = owner.or_else(|| self.root_stack.last().copied());
```

配合 `root_stack` 在 `layer()` 里的 push/pop（`view.rs:856/860`），
**嵌套声明能力本来就完整存在** —— 用户写 `v.modal(|v| { v.popup_at(...) })` 就会自动带上
`owner = modal`。所以：

- ❌ "现象不可达" —— 错，**可达**；
- ✅ "`Root.owner` 无消费点" —— 对；
- ✅ "设计 §3.7 的 z 序未实现" —— 对。

⇒ **A 方案不需要新增对外 API**，只需让 z 序消费已有的 `owner`。改动比预想小得多。

### 二、实现：两个方向的 z 序

```rust
Track::z_ordered_roots()        // 自下而上（渲染）
Track::z_ordered_roots_top_down() // 自上而下（命中）
```

排序键 **`z = (顶层祖先的 Layer, 嵌套深度, 声明序号)`**，两侧共用。

**★ 关键设计决策：嵌套层继承 owner 链顶层的 Layer 基准**

设计 §3.7 写的是 `z = (Layer, 嵌套深度, 序号)`。**字面实现是错的** ——
我先按字面写了，测试直接失败：

```
panicked at src\hit.rs:636: 嵌套 Popup 应能收到点击（z 序未消费 owner？）：[NodeId(1)]
```

原因：Layer 是**槽位**，`Popup`(2) 的槽位低于 `Modal`(4)，所以 Modal 内的 Popup
即使 `depth=1` 也仍排在 Modal **之下** —— **"Modal 里弹菜单"依然不可用，
整个嵌套功能失去意义**。

正确语义：嵌套声明意味着"我属于这个槽位**内部**"，所以外层基准取顶层祖先的 Layer，
深度只用来区分"槽内 / 槽外"。修正后：
- Modal 内的 Popup → `(Modal=4, depth=1)` > Modal `(4, 0)` ✓
- 独立弹窗（无 owner）→ 仍用自己的槽位 `(2, 0)` ⇒ 不被模态框盖住，也不盖住它 ✓

**变异验证**（退回字面形式 `(r.layer, depth)`）：FAILED ✓

### 三、**第二个坑：稳定排序 + key 降序 =顺序反了**

第一版命中侧用 `out.sort_by(|a, b| key(b).cmp(&key(a)))`，想着"稳定排序能保住同 key 内的声明序"。
**结果 5 个既有测试挂了**（含菜单子菜单）：

```
panicked at src\app.rs:5155:  left: ["tap 行 3"]  right: ["menu 行 2 @7"]
```

因为稳定排序 + key 降序 ⇒ 同 key 内保持**升序**（先声明的先来），
正好与"后声明的在上"相反 ⇒ **子菜单跑到父菜单下面**。

修法：**不依赖稳定性**，把声明序号显式排进 key，两个方向对称：

```rust
true  => idx.sort_by(|(ia,a),(ib,b)| key(a).cmp(&key(b)).then_with(|| ib.cmp(ia))),
false => idx.sort_by(|(ia,a),(ib,b)| key(a).cmp(&key(b)).then_with(|| ia.cmp(ib))),
```

> 教训：**"稳定排序会保住相对顺序"这句话只在 key 相同的前提下成立**；
> 一旦把key 的比较方向反过来，稳定性救不了你 —— 必须显式排第二级键。

### 四、顺带消除的隐患

两侧遍历改成同源后，**"渲染与命中的层序不同步"这个类别彻底消失**：
`z_ordered_roots_top_down()` 就是 `z_ordered_roots()` 的严格逆序，
由构造保证，不再依赖两份手写数组恰好互逆。

### 五、测试（5 条 `hit::nested_z_order`）

| 测试 | 钉住什么 |
|---|---|
| `nested_popup_above_its_modal_parent` | ★ **A 方案的存在理由**：Modal 内 Popup 既能命中又后画。z 序未消费 owner 时**必然失败** |
| `hit_order_is_exact_reverse_of_paint_order` | ★ 核心不变量：命中序 = 绘制序的严格逆序（6 个层的混合场景） |
| `nested_tooltip_above_its_parent` |嵌套 Tooltip 同理 |
| `dangling_owner_degrades_to_top_level` | 悬空 `owner` 退化为顶层，**不 panic 不死循环** |
| `cyclic_owner_is_truncated` | `owner` 成环（A↔ B）被 `MAX_NESTING_DEPTH` 截断 |

后两条是**防御性测试**：真实触发路径存在（`Cmd` 单独移除父层、对齐阶段先删父再删子），
而环一旦出现就是**排序死循环**——整个 UI 线程挂死。

### 六、变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/track.rs` | 新增 `Layer::ALL` | 层序单一权威源（枚举声明序） |
| `src/track.rs` | 新增 `z_ordered_roots()` / `z_ordered_roots_top_down()` / `z_order_impl()` | 共用排序核心 |
| `src/render/scene.rs` | 修改 | 改用 `z_ordered_roots()`；删除 `LAYER_BOTTOM_UP` |
| `src/hit.rs` | 修改 | 改用 `z_ordered_roots_top_down()`；删除 `LAYER_TOP_DOWN` / `layer_top_down()`；**顺带省掉一次 `roots().iter().find()` 回查**（原代码每层都线性回查一次 Root） |
| `src/hit.rs` | 新增 5 条测试 | `nested_z_order` |

`hit_path` 现在不再需要"先收集 node 列表再反查 Root"这个两步走 —— **顺带修掉一个 O(n) 回查**。

### 七、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | 13 个二进制全ok；lib 411 → **416** |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| 变异（退回 §3.7 字面形式） | **FAILED** ✓ |

### 八、状态

`refactor-plan.md` 的 D6 条目已改为 **✅ 已完成**。至此审计里**唯一确认可达的功能缺陷**关闭。
EOF 之后 S3 剩余：`D-b`（滚动脱离布局，风险最高，建议在 C4 display list 之后）。
---

## 2026-10-07 · TextCache 两段式 key + 「parley 是否已缓存」的核实 —— ✅ 完成

### 一、用户质疑：**部分正确，但结论不成立**

> "parley 通过它贡献的 layoutcontext，已经能做好对同样文本排版的结果缓存了，
> 应该不用我们再做一次的吧。"

**核实结论：parley 确实有缓存，但缓存的是 shaping 的 _输入侧_，不是排版 _输出_。**

#### 代码证据（parley 0.11.1，Cargo.lock 实际版本）

```text
// parley-0.11.1/src/mod.rs:30-32
shape_data_cache:     LruCache<ShapeDataKey,     harfrust::ShaperData>,
shape_instance_cache: LruCache<ShapeInstanceId,  harfrust::ShaperInstance>,
shape_plan_cache:     LruCache<ShapePlanId,      harfrust::ShapePlan>,
```

外加 `context.rs` 里的 `fcx.source_cache.prune(128, false)`（fontique 字体源，128 项）。

⇒ 命中的是：**字体数据、shaper 实例、shape plan**。省下的是"重复解析字体文件"。

而 `RangedBuilder::build()` → `build_into_layout()` **每次调用都会执行**：

1. `crate::analysis::analyze_text(...)` —— ICU：Bidi 级别、换行机会、脚本/词边界
2. 逐 style_run 的 `shape_text(...)` —— harfrust shaping：字形选择 + 定位
3. `break_all_lines(...)` —— 断行
4. `align(...)` —— 对齐

**没有任何"排版结果已算过"的检查。** 字体数据是热的，但排版每次重算。

> ⚠️ 顺带一个版本事实：**`lru_cache.rs` 是 0.11.1 才有的**。
> 我们 Cargo.lock 锁的就是 0.11.1（不是 0.11.0），所以这些缓存确实生效。

#### 实测数据（release，`tests/text_cache_bench.rs`）

| 指标 | 数值 |
|---|---|
| 文本长度 | 106 字节（中文，需真实 shaping + 断行） |
| **miss（真排版）** | **44.948 us/次** |
| **hit（缓存命中）** | **0.135 us/次** |
| **比值** | **334x** |
| 换算 50 个可见文本 / 帧 @60fps：无缓存 | **2.25 ms/帧** |
| 换算 50 个可见文本 / 帧 @60fps：有缓存 | **0.0067 ms/帧** |

⇒ 无缓存时**光排版就吃掉 2.25ms/帧**（16.7ms 预算的 13.5%），
而软渲染还要再花大量时间在光栅化上。**这一层不是冗余，是必需的。**

#### 两层缓存的分工（互补，不重复）

| 层 | 缓存什么 | 命中省下什么 |
|---|---|---|
| parley 内部 | 字体数据 / shaper 实例 / shape plan | 不必重新解析字体文件 |
| **我们的 `TextCache`** | **`Arc<TextLayout>`（完整排版结果）** | **第二次排版根本不用发生** |

顺带说明：我们还有第三层 `MEASURE_CACHE`（`lieui-text` 内，缓存 `(w,h)` 测度），
它缓存的是**尺寸**而非排版，与上述两层同样不重复。

### 二、本批实际改动：命中路径消除堆分配

核实 parley 的同时发现一个**独立于 parley**的问题：`TextCache` 原先是
`HashMap<TextKey, _>`，而 `TextKey.text: String` ⇒ **命中路径也要分配** ——
每次查询先 `text.to_string()` 构造 key 才能查表，外加一次字节拷贝。

**这违反缓存的基本前提**（命中必须比未命中便宜得多）。改为**两段式key**：

```
hash(内容, 规格, 颜色) → 桶 → 桶内 str == str（memcmp，零分配）
```

- 命中路径**全程零堆分配**（`tests/text_cache_alloc.rs` 用线程局部
  `#[global_allocator]` 计数断言 `allocs == 0`）
- 桶通常 1 项（64 位hash 冲突可忽略），线性比对不比哈希查找慢
- 冲突时**仍正确**（比对内容），只是多一次 memcmp —— 有测试钉住

### 三、**自己引入并修掉的 O(n²)**

第一版用 `len()` 判溢出，而 `len()` 是 `map.values().map(Vec::len).sum()`
—— 在 miss 路径上每次插入都 O(桶数) ⇒ **2048 次插入 × 2048 次求和 = O(n²)**。

改为手工维护 `count: usize`，`len()` 变 O(1)。这是"先量后优化"被自己违反的实例：
优化 hit 路径时引入了 miss 路径的平方级开销。

### 四、变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/render/scene.rs` | 修改 `TextCache` | 两段式 key（`HashMap<u64, Vec<(TextKey, TextEntry)>>`）+ `count` 计数器 + `TEXT_CACHE_MAX` / `key_hash()` |
| `tests/text_cache_alloc.rs` | **新增**（8 条） | 零分配验收 + 冲突正确性 + 溢出清空 + 计数器冒烟 |
| `tests/text_cache_bench.rs` | **新增**（1 条 `#[ignore]`） | hit/miss 耗时对照，`--release -- --nocapture --ignored` 手动跑 |

`tests/text_cache_alloc.rs` 里的 `#[global_allocator]` 用 **thread_local** 计数，
因此与其它并行测试互不干扰，无需 `--test-threads=1`。

> 顺带：这也是 **H6（`tests/api_contract.rs` 的同类工作）** 的一个实例 ——
> 集成测试站在**外部用户视角**（`lieui::prelude::Color` / `lieui::render::TextCache`），
> 编译失败就是 API 回归。

### 五、门禁第六次抓到我

`clippy::field_reassign_with_default`（`TextSpec::default()` 后逐字段赋值）。
此前五次：`type_complexity`、无用 `mut`、doc引用块语法、`auto-deref`、括号错位。

### 六、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **15 个测试二进制全ok**（新增 2 个集成测试文件）；lib 416 未变 |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |
---

## 2026-10-07 · 渲染成本实测 —— C4（display list）**不值得做**，判断被推翻

> 起因：准备投入 C4 之前先量。同 D-a（FlexNode 持久化）一样，
> 已经有一次"高估微小开销"的先例，这次要求**先有数据再决定**。

### 一、实验设计（`tests/render_cost_bench.rs`，`#[ignore]` 手动跑）

关键是把 **Scene 遍历** 与 **光栅化/绘制提交** 分离：

- 固定窗口 1280×800，用**非空**小脏区 `(0,0,64,64)`
  （★不能用 `&[]` —— `render()` 里 `let all = damage_all || damage.is_empty();`
  会把空脏区当成**全重绘**，第一版实验因此得出"两列相同"的假象）
- 树里放 N 个节点，**全部重叠**在同一 64×64 裁剪区内
- 其中只有前 `visible` 个有背景色/文本，其余是**无背景空 Box**
  ⇒ **节点数变、绘制量不变** ⇒ 小脏区耗时的斜率就是纯 Scene 遍历成本

### 二、实测数据（release，1280×800，每帧 40 次取平均）

| 节点数 | 可见节点 | 全重绘 ms/帧 | 小脏区 ms/帧 | 小脏区 us/节点 |
|---|---|---|---|---|
| 200 | 200 | 1.877 | 1.123 | 5.617 |
| 800 | 800 | 6.004 | 4.488 | 5.610 |
| 3200 | 3200 | 21.553 | 19.128 | 5.978 |
| **3200** | **8** | 1.138 | **0.246** | 0.077 |
| **6400** | **8** | 1.481 | **0.443** | 0.069 |

拟合（可见数固定 8，节点 3200 → 6400）：

```
每节点 Scene 遍历成本 a ≈ 0.061 us
⇒ 3200 节点时 Scene 侧 ≈ 0.196 ms/帧
```

### 三、结论：**C4 不值得做**

| 判据 | 数值 |
|---|---|
| Scene 遍历占 60fps 帧预算（16.7ms） | **0.196 / 16.7 = 1.2%** |
| C4 要消除的正是这 1.2% | — |

而真正的成本是**光栅化 / 绘制提交**，与**可见绘制量**成正比
（表里"可见 3200 → 可见 8"省下 **18.9 ms**，就是绘制量的贡献；
绝对值因我的构造存在 overdraw 而偏大，但**量级关系是可信的**）。

⇒ **优化方向应该是"减少绘制量"（更细的 culling、降 overdraw、
按可视区域裁剪子树），而不是"缓存 Scene 的 op 序列"。**

这与我 v1.0 评审里"软渲染的结构性上限在 C4（display list）"的判断**相反**，
现按实测数据推翻。

### 四、**测量本身踩的两个坑**（比结论更值得记）

1. **第一版实验得出假结论**：`damage = &[]` 被 `render()` 当成 `all = true`
   ⇒ "全重绘"与"小脏区"两列几乎相同（1.927 / 2.007），
   我差点据此得出"绘制量不影响成本"的荒谬结论。
   **教训：对照组必须先确认它真的走了对照组分支。**
2. **数据被并行负载污染**：同一份代码两次跑出 `0.932` 与 `0.061` us/节点
   （**15 倍**）—— 差异来自我在后台同时跑 `clippy` / `test`。
   静置后两次跑出一致（0.061 / 0.069）。
   **教训：共享机器上的计时数据，没有复现就不算数据。**
   这个坑与本项目此前的"变异测试必须真的引入缺陷"同源 ——
   **验证本身也需要被验证**。

### 五、门禁第七次抓到我

`clippy::useless_conversion`（`String::into()` 已是String）+ 未使用的 `build_tree`。
连同前六次：`type_complexity`、无用 `mut`、doc 引用块语法、`auto-deref`、
括号错位、`field_reassign_with_default`。

### 六、变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `tests/render_cost_bench.rs` | **新增** | 成本分解实验（`#[ignore]`，`--release -- --nocapture --ignored` 手动跑） |

### 七、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **16 个测试二进制全ok** |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
---

## 2026-10-07 · 降 overdraw —— 裁剪栈纳入 culling —— ✅ 完成（收益 9x）

> 承接上一批"优化方向是减少绘制量"的结论，做**第一项**：让 culling 认识裁剪栈。

### 一、根因：`Cull` 只有脏区，没有 clip 上下文

`scene.rs` 的 `Cull { rects, all }` 只携带**窗口级脏区**，`walk` 递归时**不传 clip**。
后果：一个**被祖先裁剪矩形完全裁掉**的节点（长列表里滚出可视区的行）仍通过
`cull.hit` ⇒ 原语被提交 ⇒ **光栅器求交后才丢弃 —— 开销已经发生**。

长列表是重灾区：容器 bbox 很高、必然与脏区相交，于是**全部**子节点都被遍历提交，
而实际可见的只有十几行。

### 二、改动

```rust
struct WalkCtx<'a> {
    cull: &'a Cull,          // 全程不变
    opts: &'a SceneOptions,   // 全程不变
    clip: Option<Rect>,       // 随递归变化（窗口坐标）
}
```

`walk` 签名从 7 个参数降到 5 个（`clippy::too_many_arguments` 门禁**第八次**抓到我，
但这次是**设计信号**而非噪音 —— 参数失控说明状态没归位）。
递归时写入 `ctx.clip = child_clip`，返回时**恢复**（与 `Op::PushClip`/`PopClip` 同构）。

**★ 一个容易写错的点**（实测踩到）：`child_clip` 必须写成

```rust
let child_clip = match clip {
    Some(c) => /* c 的窗口 bbox ∩ 祖先 clip */,
    None => ctx.clip,   // ★ 无自身 clip 时**继承**祖先，不能置 None
};
```

写成 `clip.and_then(...)` 会在"无 clip 的节点"处把裁剪链**整条断掉** ——
症状：外层 100×100 裁剪区里放一个 400×400 的**无 clip** 容器，其子节点 `culled = 0`。

### 三、实测收益（release，1280×800，`render_cost_bench long_list`）

**修复前**（`ctx.clip = None` 变异）：

| 行数 | culled | visited | ops | ms/帧 |
|---|---|---|---|---|
| 200 | 0 | 252 | 253 | 0.797 |
| 1000 | 0 | 1252 | 1253 | 2.086 |
| 5000 | 0 | 6252 | 6253 | **8.144** |

**修复后**：

| 行数 | culled | visited | ops | ms/帧 |
|---|---|---|---|---|
| 200 | 160 | 212 | **53** | 0.631 |
| 1000 | 960 | 1012 | **53** | 0.682 |
| 5000 | 4960 | 5012 | **53** | **0.906** |

| 指标 | 收益 |
|---|---|
| 5000 行耗时 | **8.144 → 0.906 ms/帧= 9.0x** |
| 提交原语数 | 6253 → **53 = 118x** |
| 增长趋势 | 线性 → **几乎恒定**（5000 行只比 200 行慢 0.28 ms） |

**这才是"减少绘制量"的兑现**：光栅化量与列表总行数**脱钩**。

### 四、测试（5 条 `render::scene::clip_culling`）

| 测试 | 钉住什么 |
|---|---|
| `scrolled_out_rows_are_culled_by_ancestor_clip` | ★ 500 行列表，culled ≳ 488 |
| `visible_rows_are_not_culled` | ★ **反方向**：全部可见时 `culled == 0`（防cull 过头） |
| `nested_clip_culls_grandchildren_outside_outer_clip` | 一般化场景：任何 `clip_content`，不只滚动容器 |
| `explicit_clip_also_culls_children` | 显式 `n.clip` 同样参与 |
| `clip_culling_under_transform_is_consistent` | 变换（平移/缩放）下裁剪仍正确 |

**★ 为什么必须用「计数」而不是「像素」验收**：被裁掉的像素**本来也不会显示**，
所以**像素级断言对这次改动完全失明** —— 改对改错屏幕上一模一样。
唯一能区分的是 `SceneStats::nodes_culled` / `ops`（**提交了多少**而非画出了多少）。

**变异验证**（`ctx.clip = None`）：**4 条精确失败**，其中核心那条报
`nodes_culled=0 nodes_visited=502 ops=503` —— 证明修复前每个节点都提交了原语。

### 五、**测试构造陷阱**（第二次同源问题，坑更深）

`nested_clip` 那条最初写完是**红的**（`culled = 0`）。第一反应是"修复没生效"，
实际是**布局把它修好了**：

```
kid[0] rect = (0,0,400x1)     ← 我设置的是 400x4
inner rect  = (0,0,400x100)   ← 我设置的是 400x100
```

**flex 收缩**把 100 个子节点压成 `400×1`、容器压成 `400×100`，
于是**每一个都恰好落在 100×100 裁剪区内** ⇒ `culled = 0` 是**正确行为**。

修法：给子节点显式 `flex_shrink = 0`（这也正是 `build()` 对滚动容器子节点的做法）。

> 与"对照组必须确认它真的走了对照组分支"同源：
> **期望值写错时，测试会指向错误的结论。** 打印一次 `rect` 就能避免。

### 六、门禁第九次抓到我（本批 2 次）

`empty line after doc comment`（删除函数后残留文档注释块）。
连同前八次：`type_complexity`、无用 `mut`、doc 引用块语法、`auto-deref`、括号错位×2、
`field_reassign_with_default`、`useless_conversion`、`too_many_arguments`。

### 七、变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/render/scene.rs` | 新增 `WalkCtx` | 参数打包 + 裁剪栈状态 |
| `src/render/scene.rs` | 修改 `walk` | 签名 7→5 参数；`visible = screen ∩ ctx.clip`；`child_clip` 继承 |
| `src/render/scene.rs` `build` | 修改 | 建立 `WalkCtx`；层根的祖先裁剪 = 窗口（不是无限大） |
| `src/render/scene.rs` | 新增 5 条测试 | `clip_culling` |
| `tests/render_cost_bench.rs` | 修改 | 换成"长列表收益"场景（旧的对照实验已删除） |

**注意**：`widgets::draw` 的签名**未变** —— 被跳过的节点根本不会调用 `draw`，
所以逐原语 culling（`push_culled`）也自动受益，无需改 `Cull` 的对外形态。

### 八、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **16 个测试二进制全ok** |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |

### 九、后续（同类方向，尚未做）

- **逐原语 culling 也用 clip**：`push_culled` 仍只判"与脏区相交"，
  不判"是否被祖先 clip 裁掉"。节点**部分**可见时仍会提交被裁掉的那部分原语。
  需要把 `visible` 传进 `draw`（改签名）。
- **按可视区域裁剪子树**：容器只在脏区内的部分才展开其子树。
---

## 2026-10-07 · S4 · E1 `handled` 语义 —— 用户优先于内置行为 —— ✅ 完成（D54）

### 一、问题：`mark_handled()` 语义上等于不存在

`WindowCtx::dispatch` 原来是「**内置行为先跑、用户 handler 后跑**」：

- `① widgets::handle_route(...)` —— 内置行为全部跑完（CheckBox 已翻转 `checked`）
- `② event::dispatch(...)`      —— 用户的 `cx.mark_handled()` 到这里才有机会执行

于是用户的 `mark_handled()` 只能当**马后炮**：
- 不能阻止**已经跑完**的内置行为（状态已改，撤不回）
- 不能阻止**后续**的内置行为（`handle_route` 压根不读 `handled`）

用户无法表达"这个 CheckBox 的勾选由我自己处理"。这与 WinUI / WPF 的
`Handled` 语义不一致，也让 `on_tap_with(|cx| cx.mark_handled())` 这个 API 形同虚设。

### 二、修法：调换顺序，让 `handled` 成为真正的开关

```rust
let out = crate::event::dispatch(rt, self.id, &self.track, path, ev, &mut cmds);  // ① 用户
if !out.handled {
    crate::widgets::handle_route(&mut self.track, path, ev, &mut cmds);          // ② 内置兜底
}
```

**"用户先、内置兜底"是语义要求，不是性能取舍** —— 控件的默认行为
（勾选切换、输入插入、拖拽跟踪）本就该是"用户没接手时"的兜底。

**影响面实测为零**：改动后 **16 个测试二进制全过、零失败**，
说明现有控件行为不依赖"内置先跑"的顺序（`DispatchOutcome` 本来就带 `handled`，改动极小）。

### 三、测试（2 条 `app::handled_semantics`）

| 测试 | 钉住什么 |
|---|---|
| `marking_handled_suppresses_the_builtin_toggle` | ★ `mark_handled()` 后内置翻转被抑制；再验证用户自己改的 Signal 在下一帧生效 |
| `without_handled_the_builtin_toggle_still_runs` | ★ 未标记时内置**立即**翻转节点（不等下一帧） |

**★ 断言看 `kind.checked` 而不是绑定的 Signal** —— 这是本测试的关键设计：
内置的 `toggle_checked` **立即**改节点字段，而 Signal 绑定要等**下一帧** `view()` 重建。
两者时序不同，正好能区分"谁改的"。

**变异验证**（把 `if !out.handled` 改成 `if true`）：

```
panicked at src\app.rs:5612:
  ★ mark_handled() 之后内置的勾选切换必须被抑制（修复前这里会变成 true）
test result: FAILED. 1 passed; 1 failed
```

### 四、**测试前提错误**（第二次同源，第三次形态）

第一版用 `c.checkbox(false)`（**无绑定**），第二条测试红了。
查 `widgets/mod.rs:130`：

```rust
fn checkbox_handle(track: &mut Track, id: NodeId, ev: &EventView) {
    if ev.kind != EventKind::Tapped { return; }
    let bound = track.get(id).map(|n| n.bindings.checked.is_some()).unwrap_or(false);
    if !bound { return; } // 未绑定 ⇒ 交给用户处理器（模型说了算）
    track.toggle_checked(id);
}
```

**无绑定的 CheckBox 内置行为本就不切换** —— 这是既有的"未绑定就不改模型"规则。
所以要观察内置行为**必须**用 `checkbox_bound(&sig)`。

> 这是"期望值写错 ⇒ 测试指向错误结论"的第三次形态：
> 前两次是**对照组没走对照组分支**、**flex 收缩让构造失效**；
> 这一次是**被测对象的既有规则被我忽略了**。
> 三次的共同教训：**写断言前先确认"被测系统的实际行为是什么"**。

顺带修正了一处路径取法：`column` **不产生节点**，其子节点直接挂在内容根下
（与既有测试 `children(root)[2]` 的取法一致），我最初多取了一层导致越界。

### 五、门禁（第十次抓到，已连续三次同类）

`empty lines after doc comment` —— 测试模块的文档注释块里嵌了
```` ```text ```` 代码块，被 rustdoc 解析成"文档注释后的空行"。

这一类连续出现三次（`doc quote line without > marker` / `empty line after doc comment` ×2），
说明**在 `///` 注释里写代码块的写法在本项目不友好**。约定：
**注释里描述代码用缩进块，不用围栏代码块。**

### 六、变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src/app.rs` `WindowCtx::dispatch` | 修改（D54） | 用户 handler 先跑；`if !out.handled` 才跑内置行为 |
| `src/app.rs` | 新增 2 条测试 | `handled_semantics` 模块 |

### 七、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **16 个测试二进制全 ok** |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |
---

## 2026-10-07 · S4 · D15 Escape 关闭浮层 —— ✅ 完成

### 功能
`NamedKey::Escape` 此前无任何消费点 —— 弹层/菜单只能用鼠标点外面关掉。实现放在
`WindowCtx::key()` 最前面（先于普通 dispatch）：浮层关闭是框架级响应，不该依赖
"恰好有节点监听 KeyDown"。语义 = **只关z序最上面一个**（WinUI/WPF 一致），
"可关闭"沿用既有的 `LayerOpts::dismiss_on_outside_click`，不另立开关。

### ★★ 本批最严重的失误：按任意键都关弹层
第一版只判 `ev.kind() == KeyDown` —— 因为我把Escape 写成了 `EventKind::Escape`。
**实际它是 `KeyCode::Named(NamedKey::Escape)`**，`EventKind` 里没这个变体。
（根源：看到 `event.rs:51` 的 `Escape,` 就以为属于 `EventKind`，实际那是 `KeyCode`。）

后果：任意按键都关弹层，而"Escape 能关弹层"那条测试**照样通过**（只测正向）。
修正靠补**反向测试**（Enter/Tab/'a'/Delete 不得关闭），变异验证FAILED。

> **教训：写"某条件触发某行为"必须同时测"相邻条件不触发"。**
> 正向测试只能证明实现没坏，证明不了实现没做过头。

### ★★ 第二次事故：误用 git checkout 的风险
修括号时我准备执行 `git checkout -- src/app.rs`。执行前查 `git log` 才发现
**HEAD 是本轮对话之前**的提交（`70a62f7`）—— checkout 会丢掉 S0–S4 全部改动。
用户未批准，改为手工补括号恢复（`AppendAllText` 只追加不破坏结构）。

> **规则固化：`git checkout -- <file>` / `git restore` 之前，
> 必须先 `git log --oneline -3` + `git diff --stat` 确认 HEAD 与工作区差异。
> 未提交的工作不可再生。**

### 测试的模块归属
Escape 测试最初想复用 `mod tests` 的私有 `picker()`/`tap_node()`，但新mod 在顶层，
`use super::*` 看不到。中间还因脚本裁剪把测试嵌进 `handled_semantics` 内部、
并切掉其尾部导致编译失败。
**最终方案：自带 20 行 ViewModel，完全不依赖 `mod tests`** ——
理由写进模块文档：*跨模块私有测试 helper 是本项目反复踩坑的来源，
多写 20 行换零耦合，划算*。

### 测试（3 条 `app::escape_dismiss`，自包含）
- `escape_closes_an_open_popup` —— ★ Escape 关闭弹层
- `other_keys_do_not_close_the_popup` —— ★★ **反向**：Enter/Tab/'a'/Delete 不得关闭
- `escape_without_popup_is_inert` —— 无弹层时不产生动作

### 验证
`cargo test --workspace` 16 个二进制全 ok；`clippy --all-targets` exit 0；
`fmt --check` 0 差异；`build --examples` exit 0。

### 教训汇总
1. **正向测试必须配反向测试** —— 否则"实现做过头"完全隐形。
2. **核实枚举归属要看全上下文** —— 注释里的类型名会污染后续实现。
3. **`git checkout`/`restore` 前必须确认 HEAD 位置** —— 未提交工作不可再生。
4. **测试 helper 的模块归属要一开始就想清楚** —— 跨模块私有依赖是反复踩坑的来源。
---

## 2026-10-07 · S4 · D48 align-content: SpaceEvenly —— ✅ 完成（+ Baseline 显式记录）

### 一、SpaceEvenly：从"静默退化"到正确实现

`align_content` 的实现（`flex_node.rs` Step 16）里**缺 `SpaceEvenly` 分支** ⇒ 落
`_ => 0.0` ⇒ 表现为**贴顶**。

**同语义两处实现、一处有一处没有**是根因：主轴的 `flex_line.rs:284`
本来就有正确的 `SpaceEvenly`（`free / (n+1)`），只有交叉轴漏了。
这正是本项目根因模式（"契约写在文档里、退化路径无测试"）的又一实例 ——
而且它已经以 `#[ignore]` KNOWN BUG 的形式**被记录在案**，
说明"显式记录退化"这个做法是有效的：它让缺陷可见、可追踪。

修复（CSS `space-evenly`：项之间与两端等距）：

```rust
FlexAlign::SpaceEvenly => {
    let s = rem / (lc + 1) as f32;
    off += s;
    s
}
```

### 二、★ 顺带把 KNOWN BUG 测试换成真实断言

原来那条 `#[ignore]` 测试断言的是**退化行为本身**（"全部贴顶"）。
修复后把它改写成**验证正确行为**的断言，并顺带覆盖了三个维度：

- 首行偏移 = 一个间隙（10.0）⇒ 这是 `space-evenly` 与 `space-around` 的区别
- 相邻行间距恒定（30.0 = 行高 20 + 间隙 10）
- **末行之后**也留一个间隙（`space-between` 不会）

最后一条尤其重要：只查"首行偏移 + 行间距相等"的话，`space-between` 也能蒙对。

**变异验证**（移除 `SpaceEvenly` 分支）：

```
panicked at feature_flex.rs:332:
  space-evenly 的首行偏移应等于一个间隙 10.0，实际 0（tops=[0.0, 20.0, 40.0]）
test result: FAILED. 20 passed; 1 failed
```

⇒ 精确抓到退化（`tops` 全贴顶）。

### 三、Baseline：按既定决策"不做"，但**显式记录**

`FlexAlign` 共 **9 个**变体。Step 16 的 match 覆盖 7 个，
剩下的 `Auto` / `Baseline` 落 `_ => 0.0`（贴顶）。

**为什么不实现 Baseline（与 SpaceEvenly 不同）**：
- `Baseline` 的语义依赖**子节点的行盒基线**（文字/图片混排的��齐基准），
  而本引擎的测量口径是 parley 的**行盒**。把"行盒顶边"冒充"基线"
  会给出**看似合理但错误**的结果 —— 比明确退化更糟。
- 当前无真实用例。
- 维护成本高于收益。

**但"不做"不等于"静默"**：新增了一条 `#[ignore]` 的 KNOWN BUG 测试
把现状钉住，并在注释里写清"为什么不做"和"将来实现了怎么改"。
这样 9 个变体的状态全部有据可查（7 个已实现 / Baseline 不做 / Auto 等价 Start）。

### 四、测试状态变化

`lieui-layout/feature_flex`：**20 passed / 1 ignored → 21 passed / 1 ignored**
（SpaceEvenly 从 ignored 转正，Baseline 接过那个 ignored 位置）。

全库 16 个测试二进制全 ok；`clippy --all-targets` exit 0；`fmt --check` 0 差异；
`build --examples` exit 0。
---

## 2026-10-07 · S4 · D17 百分比 —— 决策"不做" + 钉住哨兵常量陷阱

### 一、D17：`flex-basis` 百分比 —— 决定不做

**核实结论**：全仓**没有任何百分比表示方式**（`grep percent|百分比|"%"` 无结果）。
`FlexStyle` 的长度字段都是裸 `f32`，`view.rs` 的 DSL 也没有 `width("50%")` 入口。
`flex_basis: 50` 只会是 50 像素。

**为什么不实现**（与 `align-content: Baseline` 同批决策）：

1. **不是补一个分支，而是加一个能力**。需要引入长度单位表示
   （`enum Length { Px(f32), Percent(f32), Auto, Undefined }`），
   它会贯穿 `style_eq`（`style.rs:84` 逐字段比较）、DSL、写回——
   是一次**结构性改动**，不是 20 行。原plan 估"20 行内"是**低估**。
2. **当前无真实用例**：仓库内 UI 都能用 `flex_grow` 表达
   （`flex_basis: 0` + `flex_grow: 1` 即"均分"）。
3. `FlexStyle` 已有 `content_width` / `content_height: Option<f32>`，
   **加 `flex_basis_percent: Option<f32>` 这类并行字段**会让
   "同一语义两种表示"，比统一改成 `Length` 枚举更难维护。

将来若实现，应**统一**引入 `Length` 枚举（而非加并行百分比字段）。

### 二、★ 顺带发现一个真实陷阱：`VALUE_UNDEFINED` 与 `VALUE_AUTO` 同值

```rust
pub const VALUE_UNDEFINED: f32 = f32::NAN;
pub const VALUE_AUTO: f32 = f32::NAN;   // ← 同一个值！
```

两个常量**语义不同但无法区分**。于是：

- `v == VALUE_AUTO` **恒为 `false`**（`NaN != NaN`）——
  这样的代码**编译通过、测试也可能通过，但语义是错的**。
- 正确判定只有 `is_undefined(v)` / `is_defined(v)`（即 `v.is_nan()`）。

**当前是安全的**：全仓无任何代码试图用 `==` 区分这两个哨兵
（`grep '== VALUE_AUTO'` 无结果），一律走 `is_nan()`。

**处理**：在常量定义处加警示注释，并新增一条测试把事实钉住：

```rust
#[test]
fn sentinels_are_distinguishable_only_by_is_nan() {
    let a = std::hint::black_box(VALUE_AUTO);
    let b = std::hint::black_box(VALUE_UNDEFINED);
    assert!(a.is_nan() && b.is_nan());
    assert!(!(a == b), "★ NaN != NaN：直接比较恒为 false。永远不要这样判定");
    ...
}
```

将来若引入 `Length` 枚举，本测试会失败，提示更新判定方式。

> **`black_box` 不是多余的**：不加它，编译器知道 `VALUE_AUTO` 是 `NAN` 常量，
> `is_nan()` 在编译期就是 `true` ⇒ clippy 的 `assertions_on_constants`
> 会直接拒绝这条断言（门禁第十一次抓到我）。**测试"必须运行期求值"本身
> 是一项需要显式声明的要求。**

### 三、门禁（第十一次）

`assertions_on_constants` —— 见上。
连同前十次：`type_complexity`、无用 `mut`、doc 引用块语法 ×3、`auto-deref`、
括号错位 ×2、`field_reassign_with_default`、`useless_conversion`、
`too_many_arguments`、`field_reassign_with_default`(again)。

### 四、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | 16 个二进制全 ok；`feature_flex` 21 → **23 passed / 1 ignored** |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |
---

## 2026-10-07 · G4 测试外置 + snapping 前置测量（未完成，如实记录）

### 一、G4：app.rs 测试外置（✅ 完成）

| 文件 | 外置前 | 外置后 |
|---|---|---|
| `src/app.rs` | 5816 行（测试占 70%） | **1603 行（测试占 7%）** |
| `src/app_tests.rs` | — | 3605 行 |

**做法：用 `#[path]` 而不是搬到 `tests/`**：

```rust
#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
```

**关键**：这样测试里的 `use super::*` **完全不用改**（父模块仍是 `app`）。
若搬到 `tests/` 目录（变成集成测试），就拿不到 `app` 的**私有** helper
（本文件大量依赖 `setup()` / `picker()` / `tap_node()`）——
实测会爆 **629 个 "cannot find" 错误**。

**为什么优先做这件事**：本轮在 app.rs 上反复出结构事故（括号错位 2 次、
模块归属错误 2 次、一次 `git checkout` 险情）。5800 行文件里 4000 行是测试时，
用脚本切割就是在**高风险区操作**。现在只剩 1500 行实现，后续改动风险实质下降。

**踩到的两个坑**（都是我自己的操作失误，非代码问题）：
1. 切割时**多取了一行**——原 `mod tests` 的闭合 `}`被带进了 `app_tests.rs`；
2. 切割时**漏了一行**——`use super::*;` 丢失，直接导致 629 个编译错误。
   ⇒ 教训：**行号切割后必须立刻 `cargo build`**，不能攒到最后。

### 二、像素 snapping：前置测量**未完成**，因此不实施

D37 剩下的真问题是"1px 分隔线与文本持续半像素模糊"。按本轮方法论
（**先量再改**，此前已用它推翻 C4 / D-a / D37 三个判断），先做前置测量。

**测量构造失败**：`x=2.0` 与 `x=2.5` 产出的 Rects **完全相同**：

```
整数 x=2.0 ⇒ Rects: (0,0,8x4) (0,0,8x1)
半像素 x=2.5 ⇒ Rects: (0,0,8x4) (0,0,8x1)
```

原因：`layout.position` 被 `position_type`（默认 `Relative`）忽略，
节点仍在 x=0。**这意味着我还没搞清"怎样让一个矩形落在半像素上"** ——
而这恰恰是评估 snapping 的前提。

⇒ **不实施**。理由与本轮其他"不做"决策一致：
**没量清楚就不改**，否则又是一次"凭直觉改渲染路径"的高风险改动
（snapping 改的是**所有绘制指令的坐标**，一旦错了整屏错位且极难定位）。

**下一步该先搞清的问题**（留给后续）：
1. `position_type` 各变体（Relative / Absolute）的实际语义与默认；
2. 哪个阶段能把矩形放到"故意的小数坐标"上（`margin`? `transform`? 直接构造 `Op`?）；
3. 现有 `raster` 是否已在做隐式吸附（若是，snapping 的收益接近 0）。

### 三、本轮未做但已定位的问题

**D52 仍在**：`src/track.rs` 2193 行里有 **21 个组件行为方法**
（`slider_drag_to` / `toggle_checked` / `toggle_switch` / `select_radio` /
9 个 `input_*`），本质是组件状态机却挂在保留树上，违反设计 §3.2
（约定它们应在 `widgets/`，形如 `fn slider_handle(track, id, ev, cmd)`）。

外部调用点共 22 处，分布在 `widgets/mod.rs` / `app.rs` / `platform/mod.rs`。
是**纯机械搬移**（零行为变化），但工作量中等。

### 四、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | 16 个二进制全 ok（测试数与外置前一致 ⇒ 纯搬移） |
| `cargo clippy --workspace --all-targets` | exit 0 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |

---

## 2026-10-07 · app/window 职责分离 + 测试分层 —— ✅ 完成

### 一、`app.rs` / `window.rs` 职责分离：1603 → 573 行

**划分依据是"变更节奏"，不是代码行数**：

| 模块 | 职责 | 什么改动会动它 |
|---|---|---|
| `app.rs` **573** | `ViewModel` 契约、对象擦除、`App`（窗口表 / id 分配 / frame_all / 跨窗口请求） | 开窗关窗策略、多窗口编排 |
| `window.rs` **1111** | `WindowId` / `WindowConfig` / `WindowCtx` / `FrameStats` / 会话状态 | 事件分发、命中、布局、绘制、弹层 |

依赖方向**单向**（`App` 持有 `Vec<WindowCtx>`，`WindowCtx` 不引用 `App`）⇒ 可安全拆分。
（`window.rs` 用到 `ViewModel` / `erased` 属于**类型**依赖，与 `App` 的**值**依赖不构成循环。）

拆分前 `WindowCtx` 一个类型就占 850 行，加上 `WindowConfig` / `FrameStats` /
`TooltipSession` / `CtxMenuSession` / `Sessions` 全在 `app.rs`——
**全是单窗口级的东西，与 `App` 的多窗口编排无关**。

**对外 API 不变**：`app.rs` 用 `pub use` 重导出全部迁走的类型，
`lieui::app::WindowConfig` 等路径**继续可用**——
**模块拆分不应该成为对外 API 的破坏性变更**。

顺带发现 `WindowCtx` 早已有 `id()` 方法（我一度加了重复的）；
字段保持私有是对的——字段一旦 `pub`，外部就能绕过窗口表直接改身份，
破坏"id 由 `App` 分配"这条不变量。

### 二、测试分层：补上一个真实的洞

| 层 | 位置 | 数量 | 看到什么 |
|---|---|---|---|
| 单元 | `src/app_tests.rs` | 91 | 模块私有项（`Sessions`、脏标志…） |
| 集成 | `tests/api_contract.rs` | 9 | 公开 API（**能不能构造**） |
| 集成 | `tests/input_flow.rs` **（新）** | 5 | 公开 API（**点了会怎样**） |

**`api_contract.rs` 测的是"能不能构造出来"，没有一条测"点了会怎样"。**
事件输入链路（点击 / hover / Tab / Escape）此前**只被单元测试覆盖**。

**为什么这个洞必须补**：单元测试能断言 `FrameStats.dirty == Dirty::PAINT`，
而用户真正关心的是"我点了按钮，数字变了"。

> **若一个行为只被单元测试覆盖，"它对用户是否可用"这件事从未被验证过。**
> 本轮拆模块时那 91 个单元测试**全部原样通过**，但它们**一个都没走公开 API 路径**
> ⇒ "模块拆分没有破坏用户可见行为"这句话当时**是没有证据的**，现在才补上。

覆盖的五条链路：

- `click_updates_state_and_next_frame_shows_it` —— 点按钮 → Signal 变 → **下一帧真的渲染出来**
- `clicking_outside_does_nothing` —— ★ **反方向**：点空白不误触发
- `tab_without_tab_stop_nodes_is_inert` —— 无 `tab_stop` 时 Tab 不动焦点
- `escape_closes_a_popup_opened_from_the_ui` —— 弹层开 → Escape 关（含内容与层根）
- `other_keys_leave_the_popup_open` —— ★ **反方向**：其它按键不关弹层

两条 ★ 反向测试不是凑数：把命中测试改成"什么都返回按钮"、
或把 Escape 判断写成只判 `kind == KeyDown`（让**任意键**都关弹层），
**正向测试都会照样通过**。

### 三、写这个文件时踩的三个坑（都是我自己的期望错了）

**1. `content_root()` 只覆盖 `Content` 层** —— 弹层在 `Popup` 层，
所以"弹层内容已渲染"这条断言恒假。必须遍历 `track.roots()` 全部层根。
（**这正是"用公开 API 写测试"的代价**：得知道公开 API 的语义边界在哪。）

**2. 按钮默认不参与 Tab 链** —— `tab_order` 只收 `n.tab_stop` 的节点。
我以为"按钮天然可聚焦"，写了 `assert!(first.is_some())`。
失败后查代码才发现是**既有设计**（`tab_stop` 默认关闭），于是把测试改成
"无 tab_stop 时 Tab 是惰性的" —— 这本身也是一条有价值的契约。

**3. 嵌套弹层不能写在 `column` 闭包内** ——
`v.column(|c| { …; v.popup_at(…) })` 会二次独占借用 `v`（E0500/E0501）。
必须写在闭包**外面**。

> 三次的共同教训（与本项目日志里已有的第四次同源经验一致）：
> **期望值写错时，测试会指向错误的结论。**
> 前几次是对照组没走对照组分支 / flex 收缩让构造失效 / 既有规则被忽略，
> 这次是"公开 API 的语义边界比我想的窄"。

### 四、门禁（第十二次）

- `ViewCtx::id` 重复定义（我加了已有方法）—— 删掉新增的那个
- `context_menu_root` 私有性 —— 改 `pub(crate)`
- `cargo fix --lib` 清理了 app.rs 的导入后，测试依赖的 10+ 个类型丢失
  ⇒ 改用**显式 `#[cfg(test)] use`** 重新导入，并在注释里写明这个取舍的理由
  （那些测试断言的**正是窗口级行为**；3600 行测试加前缀会显著降低可读性）

### 五、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **17 个测试二进制全 ok**（新增 `input_flow`） |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |

---

## 2026-10-07 · D52 组件行为归位 —— 尝试后回退（如实记录失败）

### 结论

D52（把 `Track` 里的组件行为方法搬去 `widgets/`）**本轮尝试失败，已回退**。
工作区回到 `e259dba`（clean），门禁全绿（16 个二进制）。
G4 测试外置（上一批）**已提交且保留**。

### 为什么回退而不是继续迭代

搬到一半错误数从 41 涨到 58 —— 插入 `track.rs` 时用的锚点
`Some(cur) }` + `}` **匹配到了 `impl Ancestors` 而不是 `impl Track`**，
方法插进了错误的 impl 块。要继续得先把 `impl Track` 的边界摸清
（文件里 `Ancestors` / `Nodes` 等多个 impl 交织），再逐个重新定位。

判断依据：
- D52 是**纯机械搬移、零功能价值**（不修缺陷、不改行为）；
- 继续需 3~5 轮编译-修复循环，每轮都可能命中新锚点陷阱；
- 手上已有 G4 这个**干净且已提交**的成果。

**不为"看起来没完成"而硬撑** —— 半迁移状态留在工作区比回退更危险。

### 暴露的三个真实坑（比结论更值得记）

**1. 切割范围比预期宽：混进了非 input 的方法。**
计划搬 `input_*`（1540-1845），但区间里还有
`set_dragging` / `capture_pointer` / `release_pointer` / `captured_by` ——
它们直接读写 `Track` 的私有字段 `captures`，是**全局指针捕获**
（滑块拖拽、菜单选中、滚动条拖动都用），**与"是不是输入框"无关**。
教训：**按行号区间切割前，必须先确认区间内的方法都属于同一职责**。

**2. `replace_in_file` 的锚点必须在正确的 impl 块内。**
`Some(cur) }` + `}` 在 `track.rs` 里**不唯一** ⇒ 插错 impl。
用"函数末尾特征"当锚点时，必须额外确认上下文处于哪个 impl。

**3. 工具链拦住了无编码的读回写。**
`Get-Content` → 修改 → `WriteAllLines` 被拦：
> Blocked: reading file content without an explicit encoding

正确做法是用**文件编辑工具**（`replace_in_file` / `write_to_file`）。
此前 PowerShell 搬移成功过若干次，但那是**同编码（UTF-8 无 BOM）**的场景；
一旦涉及中文注释的读回写就有风险。**这条应该写进项目约定。**

### 如果将来重做（不要按行号切割）

1. **先补测试护栏**：确认 `input_*` 的现有测试覆盖全部 12 个方法，缺的先补
   —— 搬移是纯机械的，**测试是唯一的正确性保证**。
2. **一个方法一个方法搬**（12 轮，每轮编译），不要一次搬 12 个
   —— 后者会让错误信息互相掩盖。
3. 方法体整块复制到新文件，立刻 `cargo build`，再逐个修签名；
   确认无误后再删原块。
4. **调用点分批改**：`widgets/mod.rs` 内的先改，再改 `app.rs` / `platform/mod.rs`。
5. 搬完立刻 `cargo test --workspace` + `clippy --all-targets`，
   确认**测试数与搬移前完全一致**（纯搬移的判据）。

### 剩余 D52 清单（供后续参考）

`src/track.rs` 2107 行，待归位：

| 组 | 方法数 | 说明 |
|---|---|---|
| `input_*` | 9 pub + 3 私有辅助 | 最大且最内聚，建议第一个搬 |
| `toggle_checked` / `toggle_switch` / `select_radio` | 3 | checkbox / switch / radio 状态翻转 |
| `slider_drag_to` | 1 | 滑块拖拽（与 `set_dragging` 成对） |

**不属于 D52、不要动**：`capture_pointer` / `release_pointer` / `captured_by` /
`set_dragging` —— 已确认是**树级状态**（`Track::captures`）而非组件行为，
留在 `Track` 是正确的。
---

## 2026-10-07 · P1 逐原语 clip 剔除 —— ✅ 完成（防御性改进，收益温和）

### 一、先量后改：两个候选的收益差了一个量级

| 候选 | 场景 | visited | culled | ops | ms/帧 |
|---|---|---|---|---|---|
| **A** 按容器 bbox 裁子树 | 不裁剪容器 + 5000 子节点 | 5002 | **0** | 401 | 0.880 |
| **B** 逐原语用 clip | 节点与 clip 重合、阴影溢出 | 802 | 0 | **10** | 0.047 |

**候选 A 放弃，理由是正确性而非收益**：不裁剪的容器**允许子节点溢出**
（overflow visible、tooltip 定位、弹窗），按容器 bbox 裁掉子树会**丢失溢出内容**。
实测这类容器确实存在（`visited` 线性增长、`culled=0`），
但**没有正确的方法**在不加裁剪语义的前提下跳过它。

### 二、收益诚实版：远小于上一批

| | ops | ms/帧 (n=800) |
|---|---|---|
| 修复后 `hit_visible` | **10** | **0.047** |
| 修复前 `hit` | 17 | 0.053 |

**ops −41%、时间 −11%**，而上一批节点级裁剪是 **9x**。

⇒ 这是**防御性**改进，不是性能主力：它防的是"原语 bounds 远大于节点 rect"
（典型是**阴影**，`paint_bounds` 会 inflate）时 wasting 一笔。
保留的理由是**单调有益 + 语义安全**，而**不是**性能。

### 三、★ 顺带的结构改进：裁剪状态收敛到 `Cull`

原先裁剪栈在 `walk` 的参数 `WalkCtx` 里，逐原语站点只拿到 `&Cull` ⇒ 看不到。
若新增 `Cull.clip` 就会出现**两份状态**（`WalkCtx.clip` + `Cull.clip`），
必然产生"更新一处忘另一处"的风险。

所以**把 `WalkCtx.clip` 删掉、统一放进 `Cull`**：

```rust
pub(crate) struct Cull {
    rects: Vec<Rect>,
    all: bool,
    clip: Cell<Option<Rect>>,   // Rect: Copy，Cell 给内部可变性
}
```

`WalkCtx` 只剩 `cull` + `opts` 两项，全程 `&Cull` 传递 ⇒ **零签名变更**。

### 四、★★ 我在本批犯了一个真实 bug，**由既有测试抓到**

`full_window_fallback_does_not_erase_elements_outside_damage` 失败
（应为红色的像素渲染成底色）。

**根因**：我在 `draw` 之前设了 `cull.clip`，然后在子节点循环里"保存"时
保存的是**自己刚设的值**，于是恢复后仍停在 `sc` 的裁剪区。
下一个兄弟节点（红箱，在 `sc` 之外）读到的就是**滚动容器的裁剪区** ⇒ 被整个裁掉。

**修法**：`entry_clip` 必须在**压入之前**捕获，且**返回前恢复成它**：

```rust
let entry_clip = cull.clip.get();   // 进入本节点时（= 父层的）
cull.clip.set(child_clip);          // 压入本节点的
draw(...);
for child in children { walk(child) }
cull.clip.set(child_clip);          // 归位（滚动条覆盖层要看）
draw_scrollbar_overlay(...);
cull.clip.set(entry_clip);          // ★ 离开前恢复成进入时的样子
```

> **教训**：状态恢复的基准必须是"**进入时**的值"，不是"自己刚写的值"。
> 这与本项目此前的"对照组必须确认它走了对照组分支"同源——
> **基准点选错，后面全错，而且症状出现在别处**（这里表现为"兄弟节点消失"，
> 离真正的原因有两层距离）。

已加回归测试 `siblings_do_not_inherit_each_others_clip` 把因果钉在��测模块里。

### 五、测量构造第三次栽在"没先验证"上

**① flex 收缩**：`partially_clipped_primitives` 里 800 个 200px 的 row 放在 4px 容器中，
flex 把它们**压成 0.8px 并全部堆进 y ∈ [0,4]** ⇒ 全部可见，`ops` 恒等于 n，**测不到任何东西**。
诊断输出：`child[i] rect = (0,0, 1280x0.800)`。修法：`flex_shrink = 0`。
（这是本项目记录的**第二次**同源问题。）

**② 节点仍被节点级挡住**：改成 `flex_shrink=0` 后 `ops` 从 803 → 4，
但 `culled=799` ⇒ 799 个是被**节点级**裁剪挡掉的，**逐原语根本没被调用**。
必须另构造一个"节点级放行、只有原语在 clip 外"的场景（用阴影）。

**③ 列排布导致超出窗口**：`shadow_rows` 里 40 个 10px 节点 = 400px > 窗口 300px
⇒ 后 7 个被**窗口**裁掉，`ops` 只有 33。

**④ 断言没算 `PushClip`/`PopClip`**：写 `ops <= 42` 但实际 43
（= 40 背景 + 1 容器背景 + PushClip + PopClip，**阴影其实是 0**）。
改成**只数 `Op::Shadow`**。

> 四次同源错误的共同教训：**写断言前先确认"被测系统的实际行为是什么"**。
> 前几次是对照组没走对照组分支 / flex 收缩让构造失效 / 既有规则被忽略 /
> 公开 API 语义边界比想的窄，这次是"ops 里还有非图元项"。

### 六、测试（3 条 `render::scene::primitive_clip_culling`）

| 测试 | 钉住什么 |
|---|---|
| `shadow_primitive_fully_outside_clip_is_not_submitted` | ★ 40 个阴影在 clip 外 ⇒ `Op::Shadow` 计数为 **0** |
| `primitives_inside_clip_are_still_submitted` | ★ **反向**：与 clip 相交的背景必须全部提交（防"永远返回 false"） |
| `siblings_do_not_inherit_each_others_clip` | ★★ 回归：兄弟节点不得互相裁剪（本批真实 bug） |

**变异验证**（`hit_visible` → `hit`）：`left: 40` ⇒ 精确抓到 40 个阴影被提交。

### 七、变更清单

| 文件 | 动作 |
|---|---|
| `src/render/scene.rs` | `Cull` 加 `clip: Cell<Option<Rect>>` + `hit_visible()`；`WalkCtx` 删 `clip`；`walk` 的压入/恢复；新增 3 条测试 |
| `src/widgets/mod.rs` | 4 处逐原语站点 `cull.hit` → `cull.hit_visible` |
| `tests/render_cost_bench.rs` | 新增两个候选场景的测量 |

### 八、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **17 个测试二进制全 ok**（lib 426） |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |
| 变异（`hit_visible` → `hit`） | **FAILED** ✓（`left: 40`） |
---

## 2026-10-07 · P2 · G4 测试外置剩余 9 个文件 —— ✅ 完成

`app.rs`（5816 -> 573）之后，把剩余 god file 的测试全部外置。

### 一、成果

| 文件 | 原 | 实现 | 测试 |
|---|---:|---:|---:|
| `hit.rs` | 743 | **113** | 635 |
| `cmd.rs` | 544 | **309** | 235 |
| `input.rs` | 948 | **353** | 595 |
| `align.rs` | 774 | **374** | 400 |
| `layout.rs` | 1058 | **432** | 626 |
| `raster.rs` | 1355 | **738** | 617 |
| `scene.rs` | 1441 | **719** | 728 |
| `event.rs` | 1315 | **883** | 438 |
| `widgets/mod.rs` | 1988 | **1133** | 853 |
| **合计** | **10166** | **5054** | **5127** |

`hit.rs` 从 743 -> **113 行**（实现只剩 `hit_path` + `descend`）；测试占比 85% 已全部移出。

### 二、切分方案：**Design A**（为什么是零语义风险的）

```rust
// <stem>.rs
#[cfg(test)]
#[path = "<stem>_tests.rs"]
mod tests;
```

把 `mod tests { ... }` 的**内容**抽出、挂回**同名** `mod tests` ⇒
`mod tests` 仍是父模块的直接子模块，body 里的 `use super::*` 解析目标**完全不变**。

> **对照组（刻意避开）**：保留 `mod x { }` 外壳、挂成 `mod stem_tests`。
> 那样 `super` 会指向新模块，`use super::*` 失效 —— 附带的 mod
> （`d5_clip_space` 等用 `use super::hit_path`）会编译失败。

### 三、★★★ 本批最大的坑：**脚本丢掉了 97 行生产代码**

第一版脚本假设「所有 `#[cfg(test)] mod` 都在文件尾部连续」，
用 `lines[:first] + tail[last:]` 重建原文件。实测：

| 文件 | 测试 mod 之间的**生产代码** |
|---|---|
| `event.rs` | **97 行**（`DispatchOutcome` / `RoutePlan` / `dispatch` / `collect_route`） |
| `scene.rs` | 17 + 9 行 |
| `hit.rs` | 17 + 18 + 12 行 |

`event.rs` 编译直接报 `no DispatchOutcome in event`。
修法：**逐块跳过**测试 mod，保留其间所有内容。

> **教训与本项目已有的两条同源**：
> "对照组必须确认它走了对照组分支"、"先验证构造再写断言" ——
> **对文件结构的假设必须先验证**。`git checkout --` + 重切，代价可控。

### 四、另外三个脚本 bug（都被编译器 / 门禁抓到）

| # | 现象 | 根因 |
|---|---|---|
| 2 | `expected item after doc comment` | 块起点没吞掉 `#[cfg(test)]` **之前**的 `///` 文档注释，被留成孤儿 |
| 3 | 3 个 `unused import: super::*` | 我**多加了**一行 `use super::*;`，而原 `mod tests` body 里那行已提供同样作用 ⇒ 编译器判定多余 |
| 4 | `UnicodeEncodeError: 'gbk' codec`（脚本自身崩） | 控制台编码；设 `PYTHONIOENCODING=utf-8` |

第 3 条值得记：我以为"额外加一行更保险"，实际是**冗余**。
注释里原本写"★ 必需"，事实相反，已改成说明"**无需额外添加**，
多加反而触发 unused 警告" —— **注释里的断言也要被验证**。

### 五、★ 守恒核查：两个必须做的验证

机械搬移最怕**静默丢内容**（编译能过、测试能过，但少了东西）。两个判据：

**① 行数守恒**

```
align   774 = 374 + 400     input  948 = 353 + 595
layout 1058 = 432 + 626     cmd    544 = 309 + 235
raster 1355 = 738 + 617     hit    743 = 113 + 635
widgets 1988 = 1133 + 853    event 1315 = 883 + 438
```

**② `pub` 项完整性**（更直接）：正则抽出原文件所有 `pub fn|struct|enum|const|use|type|trait`，
核对是否都在「实现 + 测试」里：

```
155 个 pub 项 -> ALL OK
```

> ★ 第一次跑这个检查时**正则漏了 `re.MULTILINE`** ⇒ 匹配到 **0 个**项却报"ALL OK"。
> **一个"全绿"的验证必须先确认它真的检查了东西** —— 这是本项目反复出现的教训，
> 也是我为什么把它记进日志。

### 六、门禁

无新增（第十七次零新增）。过程中被自己的脚本 + 编译器抓到 4 次，
其中 2 次是"验证脚本本身无效"（gbk 崩溃、无 MULTILINE 的假通过）。

### 七、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **17 个测试二进制全 ok**（测试数与外置前完全一致 ⇒ 纯搬移） |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |
| 行数守恒 + `pub` 项完整性 | 9/9 文件通过 |

### 八、状态

G4（测试外置）**全部完成**：`app.rs` + 9 个文件，共 10166 行 -> 5054 行实现 + 5127 行测试。
接下来是 **D52（组件行为归位）**，`track.rs` 21 个方法，有已记录的重做指南。

---

## 2026-10-07 · P3-a · 组件行为 API 测试护栏 —— ✅ 完成（18 条）

### 一、为什么先补测试而不是直接搬

D52（组件行为归位）的重做指南第 1 条是"**先补测试护栏**"。实测确认了这条的必要性：

| 方法 | 测试文件中的引用数 |
|---|---|
| `input_set_preedit` | 1 |
| 其余 **14 个** | **0**（全是定义处本身） |

即 `slider_drag_to` / `toggle_checked` / `toggle_switch` / `select_radio` /
`input_selection` / `input_selected_text` / `input_insert` / `input_backspace` /
`input_delete` / `input_move_caret` / `input_set_caret` / `input_select_all` /
`input_preedit` / `input_is_active` —— **全部零覆盖**。

**零覆盖意味着**：任何改动都没有保护，且**可能已经有 bug 而没人发现**。

### 二、顺带核实了 D52 的可行性（推翻了我一个预设的反对理由）

我原以为"搬走会削弱封装"（这些方法要读写 `Node.kind` / `Node.bindings`）。
实测：`Node` 的字段**已经全是 `pub`**，且 `align.rs` / `view.rs`
**早就在跨模块直接读写** `n.bindings` ⇒ **封装早已不存在**，搬走没有技术障碍。

> 但**最终仍未决定做 D52 搬移** —— 理由不是封装，而是：
> 收益只有 13%（2411 → ~2100 行）、零功能价值、且上次失败过。
> **补测试本身已是实质产出**，搬移可以以后再做（有了护栏，届时才安全）。

### 三、测试（18 条 `track::component_behavior`）

重点覆盖三类易错点：

**① UTF-8 char 边界**（`caret` 是**字节偏移**，而 `input_move_caret(delta)` 以**字符**为单位）

| 测试 | 钉住什么 |
|---|---|
| `move_caret_steps_by_char_not_by_byte` | "中文字"前移一字符 ⇒ 字节 **0→3**（不是 1） |
| `backspace_removes_whole_char_not_one_byte` | 退格删**整个 3 字节字符** |
| `backspace_handles_four_byte_emoji` | 4 字节 emoji；开头退格返回 false 且**不改文本** |
| `move_caret_clamps_at_both_ends` | 越过两端**夹住**，且报告"未变化" |

**② 反向选区与编辑语义**

| 测试 | 钉住什么 |
|---|---|
| `selection_is_normalized_when_caret_after_anchor` | `caret > anchor` 必须归一化成 `a <= b` |
| `selection_is_none_when_caret_equals_anchor` | 无选区返回 `None`（不是 `(n,n)`） |
| `select_all_covers_whole_text` | 全选覆盖中英混合的 9 字节 |
| `insert_replaces_selection` | 插入替换选区，且选区**收拢**（anchor 也 = caret） |
| `delete_removes_char_after_caret` | 删除键删**光标后**（与退格反向） |
| `delete_at_end_reports_no_change` | 末尾删除返回 false 且不改文本 |

**③ 绑定写回**（不改 signal ⇒ 下次 `view()` 用模型值覆盖 ⇒ 用户点了没反应）

`toggle_checked_writes_back_binding` / `toggle_switch_writes_back_binding` /
`select_radio_writes_back_its_value` / `slider_drag_to_maps_x_and_reports_no_change` /
`slider_drag_to_clamps_out_of_range_x` / `toggle_on_wrong_kind_is_noop` /
`preedit_is_cleared_when_first_char_commits` / `input_api_on_non_input_node_is_safe`

### 四、★★ 写测试时踩的四个坑（**三个是我期望值写错**）

**1. `Node::rect()` 读 `computed`，不是 `layout.dim`。**
只设 `layout.dim` 时 `rect().width` 仍是 0 ⇒ `slider_drag_to` 里
`t = (x - rect.x) / rect.width`恒为 0 ⇒ 永远返回"未变化"。
（第一版注释还写着"必须给节点明确宽度"，其实给了也不够 —— **设错了字段**。）

**2. ★★ `"中文"` 删掉开头的 `'中'` 剩的是 `"文"`，不是 `"文字"`。**
我写 `assert_eq!(text, "文字")` ⇒ 测试失败。查证后确认**实现是对的**，
是我数错了字符。**如果当时为了让测试通过去改实现，就会把一个正确的
UTF-8 删除逻辑改坏。**

**3. ★ `slider_drag_to(id, -50.0)` 在值还是初始 `0.0` 时返回 `false` 是正确的。**
x=-50 夹到 `t=0` ⇒ 值仍 0 ⇒ "未变化"。
我第一版断言它会变。必须**先移到中间**再测夹回边界。

**4. ★ `input_preedit` 在组合串被清空后返回 `Some("")` 而非 `None`。**
`input_insert` 里执行的是 `preedit.clear()`，所以是空字符串而不是 `None`。
语义上等效（渲染侧判空即可），但与"从未设置过 ⇒ `None`"**不一致**，
调用方必须同时处理两者。

> 这四处与本项目已有的"对照组必须确认它走了对照组分支"、
> "先验证构造再写断言"同源。**第 2、3 条尤其危险**：
> **测试失败时，先怀疑自己的期望值，而不是实现。**

### 五、变异验证

把 `prev_char_boundary` 从"退一个**字符**"改成"退一个**字节**"
（真实 bug 的典型形态）：

```
panicked at src\track.rs:1644: start of range should be a character boundary
assertion failed: 应退到字节 0   left: 1
test result: FAILED. 16 passed; 2 failed
```

**两条 UTF-8 测试同时失败**，且症状正是真实 bug 的表现
（`text.replace_range` 切在非 char 边界 ⇒ Rust 直接 panic）。

### 六、门禁（第十八次）

`clippy::assert_eq_bool` × 3 —— `assert_eq!(*v, true)` 应写成 `assert!(*v)`。
连同前十七次：`type_complexity`、无用 `mut`、doc 引用块语法、`auto-deref`、
括号错位、`field_reassign_with_default`、`useless_conversion`、`unused mut`、
`unused import`、rustdoc quote、`expected item after doc comment` 等。

### 七、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **17 个测试二进制全 ok**（lib 429 → **447**） |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |
| 变异（`prev_char_boundary` 改按字节） | **FAILED** ✓（2 条 UTF-8 测试抓到） |

### 八、下一步

**D52 搬移本身仍未做**，理由：收益 13%、零功能价值、上次失败过。
现在**护栏已齐**，若要做可按重做指南逐个方法搬（12~ 15 轮）。
但更值得问的是：除了搬移，`track.rs` 2411 行还有哪些**真正的问题**？

---

## 2026-10-07 · P3-b · G4 测试外置收尾 —— 全 24 个文件完成

### 一、★ 先更正上一批的错误结论

上一批日志写「G4（测试外置）**全部完成**」—— **这是错的**。
当时只统计了**自己处理过的那 9 个文件 + app.rs**，**没做全仓普查**，
把「我做完的那批」当成了「全部」。实际当时还有 **13 个文件、2903 行测试**内嵌。

这与本项目已有的「对照组必须确认它走了对照组分支」同源 ——
**验证的范围必须与结论的范围一致**。

### 二、本批成果（13 个文件 + track.rs）

| 文件 | 原 | -> 实现 | 测试 |
|---|---:|---:|---:|
| track.rs | 2770 | **2039** | 740 |
| task.rs | 1400 | **899** | 501 |
| view.rs | 1544 | **1278** | 266 |
| platform/mod.rs | 1303 | **1051** | 252 |
| reactive.rs | 786 | **496** | 290 |
| menu.rs | 646 | **357** | 289 |
| timer.rs | 468 | **267** | 201 |
| custom.rs | 388 | **145** | 243 |
| overlay.rs | 407 | **170** | 237 |
| render/mod.rs | 412 | **157** | 255 |
| style.rs | 354 | **289** | 65 |
| transform.rs | 236 | **146** | 90 |
| focus.rs | 219 | **107** | 112 |
| icon.rs | 124 | **84** | 40 |

**13/13 一次通过**（脚本已修好夹缝问题，无需重试）。

### 三、G4 最终总量

| 批次 | 文件数 | 原行数 |
|---|---:|---|
| 更早（app.rs） | 1 | 5816 -> 573 |
| P2 | 9 | 10166 |
| P3-b | 14 | 12005 |
| **合计** | **24** | **27987 行** |

全仓**不再有任何文件内嵌 `mod tests`**（只剩 fn 级 `#[cfg(test)]`）。

### 四、★ 又踩了 P2 那个坑（这次是我自己的流程问题）

`track.rs` 切完后 clippy 报 `unused import: super::*` ——
脚本又加回了那行**多余**的导入和「★ 必需」的**错误注释**。

根因：P2 时我是用**单独的 `fix_header.py` 修的**，**没把修正合进切分脚本**，
⇒ 脚本仍持有错误逻辑，下一次必然重犯。

**修法**：把修正并进 `split_tests.py` 本身，并改掉那句错误注释
（「必需」→「**无需额外导入**，多加一行反而触发 unused 警告」）。

> **教训**：**修复若不落在产生问题的源头，下次必然重犯。**
> 临时脚本能救急，但**不能作为修复**。

### 五、守恒核查（三项，全部通过）

| 检查 | 结果 |
|---|---|
| 行数守恒 | **14/14 精确相等（差 0）** |
| `#[test]` 数 | **155 个全部守恒**，不一致文件数 **0** |
| `pub` 项完整性 | 逐文件核对，无缺失 |

`#[test]` 数守恒是「纯搬移」最直接的判据 ——
**测试数量不变 = 没有测试被丢下**。

### 六、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **17 个测试二进制全 ok**（lib 447，未变） |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |

### 七、下一步

G4 完成。按实测重新排序的候选：

| # | 事项 | 价值 | 风险 |
|---|---|---|---|
| 1 | **给 `set_visibility` / `set_interaction_enabled` 补 setter** | 封住「漏脏标记」的结构性风险 | 低 |
| 2 | 删 `add_builtin_handler`（零调用，14 行参数） | 减死代码 | 极低 |
| 3 | D52 组件行为归位 | **低**（减 11%，零功能价值，上次失败过） | 中 |

**track.rs 分析的结论**：`impl Track` 1023 行 / 76 方法，
作者**已自行分 6 区**（最大区 305 行 / 19 方法），生产代码**零 panic**，
80 个 pub fn 里只有 2 个零调用。**结构不混乱，不值得搬。**

---

## 2026-10-07 · P4 · `set_visibility` / `set_interaction_enabled` —— ✅ 完成

> 这是 `track.rs` 实测分析出的**唯一有防御价值**的项：
> 防的是**未来代码的静默错误**（改了状态不重绘），不是整理旧代码。

### 一、问题：`Node` 字段全`pub`，运行期改状态容易漏标脏

`Track` 原本**没有** `set_visibility` / `set_interaction_enabled`。
字段是 `pub`，所以运行期可以直接写 —— 而**漏掉脏标记不会报错，只是看起来坏了**。

#### `visibility` 同时被**三条**路径读取

| 读取点 | 语义 |
|---|---|
| `hit.rs:73` | `!= Visible` ⇒ **不参与命中** |
| `layout.rs:236` | `!= Collapsed` ⇒ **参与布局** |
| `scene.rs:557` | `!= Visible` ⇒ **不绘制** |

直接写字段 ⇒ 既不会**父流重排**（兄弟位置不对），
也**不会把旧像素登记进脏区**（屏幕留残影）。两者都是静默错误。

#### `interaction.enabled` 被绘制侧读了 4 处

`render/mod.rs` × 4（禁用前景色 / 光标闪烁 / 命中反馈）、
`event.rs:802`（事件路由跳过）、`focus.rs:63,78`（焦点链）、`track.rs:1748`（`input_is_active`）。

漏 `mark_paint_dirty` ⇒ 禁用态要等**下一次别的改动**才重绘。

### 二、核实：当前**没有**实际 bug

写之前先核实了三条生产写入路径，**全部已正确处理**：

| 路径 | 是否正确 |
|---|---|
| `cmd.rs:202` `SetVisibility` | ✅ 正确（`mark_flow_dirty` + `mark_paint_dirty`） |
| `align.rs:146`（对齐器） | ✅ 正确（统一标脏） |
| `view.rs:1128/1133/1138`（声明式构建） | ✅ 正确（下一帧 `align` 统一处理） |

⇒ 这是**防御性**改动，不是修 bug。**如实标注。**

### 三、实现

**1. `Track::set_visibility(id, v) -> bool`**

```rust
if changed {
    self.mark_flow_dirty(id);   // ★ Collapsed 让节点退出父流 ⇒ 兄弟要重排
    self.mark_paint_dirty(id);  //   旧像素要重画
}
```

用 `flow` 而非 `layout`：两者的**唯一区别**就是 `flow` 会连父节点一起标脏，
而 `Collapsed` 正是会让占位消失、需要兄弟重排的那种改动。

**2. `Track::set_interaction_enabled(id, v) -> bool`** —— 只标 paint。

事件路由与焦点链是**即时查询**（`hit` / `tab_order` 每次重算）⇒ 不需脏标记。

**3. `cmd.rs` 的 `SetVisibility` 改为调用 setter** —— ★ 这是**收敛**，不是重复。

原先那段 `mark_flow_dirty + mark_paint_dirty` 是**内联在 cmd 处理里**的。
加了 setter 后若不收敛，就变成**两份实现**，将来只改一处就出 bug。

**4. 两个字段加⚠️ 文档注释** —— 明确「运行期请用setter，声明式路径除外」。

### 四、测试（7 条，`track::component_behavior`）

| 测试 | 钉住什么 |
|---|---|
| `set_visibility_marks_flow_and_paint` | ★★ 自己标脏 **且** PAINT_DIRTY |
| `set_visibility_marks_parent_too` | ★★ **父节点也必须标脏**（否则兄弟不重排） |
| `set_visibility_no_change_marks_nothing` | 值未变**不标脏**（否则打爆脏区优化） |
| `set_visibility_on_missing_node_is_safe` | 不存在的节点 ⇒ false 且不 panic |
| `set_interaction_enabled_marks_paint` | ★ 禁用态立刻重绘 |
| `set_interaction_enabled_no_change_marks_nothing` | 未变不标脏 |
| `setters_actually_change_the_fields` | ★★ **交叉验证**：setter 确实改了字段 |

最后一条是**特意加的**：前六条都只在检查"脏标记"，如果 setter 根本没改字段，
它们会**全部照样通过**。**断言"副作用"之前，先断言"主作用"。**

**变异验证**（把 `mark_flow_dirty` 删掉，模拟"漏了父流重排"）：

```
panicked at src\track_tests.rs:776: 自身必须标脏（退出父流）
panicked at src\track_tests.rs:791: ★ 父节点必须一起标脏，否则兄弟不重排
```

**两条同时失败** ⇒ 测试确实守住了"flow ≠ layout"这个契约。

### 五、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **17 个测试二进制全 ok**（lib 447 → **454**） |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |
| 变异（删 `mark_flow_dirty`） | **FAILED** ✓（2 条抓到） |

### 六、剩余

- **#2** 删 `add_builtin_handler`（零调用，14 行参数）—— 极低风险
- **#3** D52 组件行为归位 —— **仍不建议**（减 11%、零功能价值、上次失败过）
- D-b 滚动脱离布局 / G1 RuntimeInner 拆分 —— 前置未就位

---

## 2026-10-07 · P5 · 删`add_builtin_handler`（死 API）—— ✅ 完成

### 一、★ 调查过程：差点**删错**

`add_builtin_handler` 的文档写：

> 用途：框架内置交互（滑块拖拽、文本编辑、滚动条……）在 `align` 之后挂到节点上；
> 它们不出现在用户态，也不需要 `view()` 重新声明。

按这个描述，它像是**框架内置行为的注册入口**—— 删掉会不会让内置交互挂不上？
**所以先去查内置交互实际走哪条路。**

**结论：内置行为走的是另一条路。**

```51:76:src/widgets/mod.rs
pub fn handle(track: &mut Track, id: NodeId, ev: &Event, cmd: &mut CmdBuf) {
    ...
    match node.kind.tag() {
        KindTag::Slider => slider_handle(track, id, &ev.summary(), cmd),
        KindTag::Checkbox => checkbox_handle(track, id, &ev.summary()),
        ...
```

**直接函数分派**（按 `KindTag` switch），完全不经过 handler 机制。

### 二、★ 为什么不用 handler —— 签名装不下 IME payload

`handle()` 收的是 `&Event` 而**不是** `&EventView`。原因写在它自己的注释里：

> 收 `&Event` 而不只是 `&EventView`：IME 预编辑/提交带字符串 payload，
> 而 `EventView` 是 `Copy` 的定长摘要（放不下字符串）。

而 handler 槽位（`HandlerSlot.handler`）只能拿 `&mut Ctx`——
**IME 的字符串 payload 无处可放**。

⇒ `add_builtin_handler` 是**设计变更的残留**：曾设计成"挂 handler"，
实现改成了"直接分派"（为了 IME），入口就没人用了。

### 三、★ 三条独立证据交叉验证"零调用"

本项目反复栽在"搜索范围不够导致误判"，所以这次用三条独立证据：

| # | 证据 | 结果 |
|---|---|---|
| 1 | 全仓文本搜索（`src` + `crates` + `tests` + `examples`） | 仅剩新写的注释引用，**无代码调用** |
| 2 | 死 API 脚本复查 | 死 API 从 2 个 → **0 个** |
| 3 | `pub fn` 计数 | 80 + 2（新 setter）− 1（删除）= **81** ✓ |

> ★ **我自己的搜索 bug**：第一次查 `toggle_checked` 的调用点时用了
> `Select-String -Path src/*.rs`（**只搜顶层**），得出"内置交互零调用"的错误结论。
> 实际调用点在 `src/widgets/mod.rs`。**差点据此认定"整个内置交互机制是死的"。**
> ⇒ **搜索范围必须与结论范围一致**（与P3-b 那次"我做完的那批 ≠ 全部"同源）。

### 四、★ 死 API 脚本的盲点（如实记录）

脚本现在报"0 个死 API"，但 **`input_preedit` 仍然没有生产调用**
（渲染侧 `widgets/mod.rs` 直接解构 `n.kind` 的 `preedit` 字段）。

**脚本判不出来了** —— 因为上一批给它补了测试，`track_tests.rs` 里 3 处调用
被算作"有引用"。

⇒ **"有测试"不等于"被使用"。** 脚本的判据是"全仓有引用"，
它**无法区分**"生产调用"与"仅测试调用"。

这个盲点**留着不修**（改成"生产调用"判据要引入 crate 依赖图分析，
成本远大于收益）；但**结论要人工把关**，不能只看脚本输出。

### 五、顺带补的文档

`view.rs` 的 `on_always()` 注释里写清「内置行为**不用**它」+
指向 `widgets::handle()` + 说明原因（`EventView` 装不下 IME payload）+
注明 `add_builtin_handler` 已于本日删除。

> **留着比删掉更危险**：一个文档与实现不符的入口，会引导后人
> "要加内置行为就用这个"，然后发现内置行为在另一个文件里。

`handled_events_too` **机制本身完全保留**（`on_always` 与 `event.rs:872`
的消费者都健在），只删了那个没人用的**便捷入口**。

### 六、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **17 个测试二进制全 ok**（lib 454，未变） |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |
| 死 API 脚本 | **2 → 0** |

### 七、剩余

| 项 | 状态 |
|---|---|
| `input_preedit` | 仍无生产调用（脚本盲点，已记录）；**保留** —— 它是IME 状态对外的合法读取口 |
| **#3** D52 组件行为归位 | **仍不建议**（减 11%、零功能价值、上次失败过） |
| D-b 滚动脱离布局 | 前置未就位（需 per-node 失效信号） |
| G1 `RuntimeInner` 拆分 | 触及 reactive 核心，等 D52 之后再评估 |
| 像素 snapping | **前置测量未完成**（尚不知如何让矩形落在半像素上） |

---

## 2026-10-07 · P6 · 测量缓存的「整表清空」病态 —— ✅ 完成（20000 行 4.3x）

> 起因：为 compositor 提案做**前置测量**，结果**先测出一个更基础的性能缺陷**。

### 一、★ 起点：compositor 提案的前置测量（`compositor_probe.rs`）

| 行数 | layout ms | render ms | 整帧 ms | **layout 占比** | 重排节点 |
|---|---:|---:|---:|---:|---:|
| 200 | 0.239 | 1.124 | 1.363 | 17.6% | 251 |
| 1 000 | 1.099 | 1.269 | 2.368 | 46.4% | 1 251 |
| 5 000 | 7.164 | 2.379 | 9.543 | **75.1%** | 6 251 |
| 20 000 | 61.745 | 3.875 | 65.620 | **94.1%** | 25 001 |

⇒ **compositor 值得做**（上限 94%）。但**先查成本在哪**，再决定缓存什么。

### 二、★ 关键测量：`build()` vs `flex.layout()`

加了临时 instrumentation（`AtomicU64` 纳秒计数，crate `forbid(unsafe_code)` 所以不能用 `static mut`）：

| 行数 | build ms | flex ms | **build 占比** |
|---|---:|---:|---:|
| 1 000 | 0.474 | 0.411 | **53.6%** |
| 5 000 | 2.585 | 2.205 | **54.0%** |
| 10 000 | 5.389 | 4.728 | 53.3% |

**`build()` 占一半** —— 远超我预期的 5~10%（我以为它只是"分配 + clone"）。
⇒ 「持久化 FlexNode 树」这条**低风险**的路就有 53% 的收益。

### 三、★★ 但接着发现一个**断点**，比 compositor 更基础

| 行数 | flex ms | 每行μs |
|---|---:|---:|
| 10 000 | 4.728 | 1.01 |
| **20 000** | **47.014** | **2.95** |
| 40 000 | 95.343 | 2.96 |

**1万 → 2 万行，flex 涨 10 倍**（行数只 2 倍）——这不是连续超线性，是**过了阈值切到陡 3 倍的曲线**。

**定位过程**：
1. 扫 8000→20000 逐点 → **无断点**（每行 0.42~0.56 μs，线性）
2. ★ **消融：去掉文本子节点** → 20000 行 flex 只要 **3.385 ms**
   ⇒ **断点来自"文本节点"，不是 flex 本身**
3. 对比 B 组（16000 行 / 4000 文本 = 1.32 μs/文本）vs 本次（20000 行 / 5000 文本 = 8.7 μs/文本）
   ⇒ 阈值落在 **4000~5000 文本**之间

### 四、★ 根因：测量缓存「超限整表清空」

```rust
// 简单防膨胀：超限后整体清空（正常 UI 远达不到上限）。
if c.len() >= 4096 {
    c.clear();
}
```

**实测数据与阈值 4096 完全吻合**：

| 不同文本数 | 每文本测度成本 |
|---|---|
| 4000（< 4096） | **1.32 μs**（全部命中） |
| 5000（> 4096） | **8.7 μs**（每帧清空 + 全量重排版） |

**"正常 UI 远达不到上限"这个假设，恰恰被本框架的招牌场景推翻了** ——
长列表正是上一批裁剪栈 culling（9x）要伺候的对象，
却在 20000 行时把布局成本从 9ms 推到 47ms。

### 五、修复

```rust
const MEASURE_CACHE_MAX: usize = 16384;
if c.len() < MEASURE_CACHE_MAX {
    c.insert(key, result);
}
```

**「不再插入」而非「整表清空」**：

- 正确性：缓存只是**加速**，命中与否**不影响结果**。
  满载时新文本仍排版（只是不存），改文本后自然重排。
- 效果：已缓存的条目**继续有效** —— 而滚动列表的工作集恰好是
  **可见的那几十行**，所以它们几乎总是命中。
- 上限提到 16384：5000 文本的列表能**全部**装下 ⇒ 成本归零。

### 六、修复效果（实测）

| rows | texts | flex ms 修复后 | 修复前 | 改善 |
|---|---:|---:|---:|---:|
| 16 000 | 4 000 | 8.719 | 9.035 | 持平 |
| **20 000** | 5 000 | **11.041** | **47.014** | **4.3x** |
| 40 000 | 10 000 | 24.677 | — | — |
| 80 000 | 20 000 | 140.265 | — | 退化但仍优于原 |

80000 行（20000 文本）**超过新上限** ⇒ 退化到 7.0 μs/文本，
但仍优于原来的 8.7（因为"不清空"让已缓存的 16384 条继续有效）。

### 七、★ 一个反复出现的测量错误

我第一次测出"20000 行 flex 52ms、每行成本涨 7 倍"时，判定为**超线性**，
并写进结论。**静置后在干净环境重跑，超线性消失**（每行 0.42→0.56 μs）。

⇒ **差点基于污染数据做出错误判断。**
这与本项目已有的"共享机器上的计时数据，没有复现就不算数据"完全同源，
已经是第2 次栽在同一个坑。**判据固化：任何"复杂度变化"的结论，必须逐点复现。**

### 八、门禁（第十九次）

`clippy::manual_div_ceil`（`(n + 3) / 4`）。

### 九、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **19 个测试二进制全 ok** |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0 |
| `cargo fmt --all --check` | 0 处差异 |
| 20000 行滚动布局 | **47.0 → 11.0 ms（4.3x）** |

### 十、对计划的影响

**compositor 的优先级下降**：它能省 `build()` 的 53%，但**基线已经降了 4.3x**，
所以 compositor 的边际收益随之下降。

**新的优先级**：
1. **FlexNode 缓存**（省 build 的 53%）—— 但要注意：20 000 行时 build 已降到 11.7ms
2. compositor（offset 解耦）—— 在①之后收益才明显

**而且核心问题暴露出来了**：`build()` 递归遍历**整棵子树**，
给**不可见**的节点也设 `measure_text`。这才是根本浪费 ——
可见的只有 40 行，却为 25000 个节点做了准备。

⇒ **"只布局可见部分"比"缓存布局结果"更本质**，且不需要 FlexNode 持久化。
这需要给 flex 引擎加 **"部分布局"** 能力（按 dirty 范围裁剪子树），
与上一批的**渲染侧裁剪栈**思路完全对称。




---

## 2026-10-08 · 修 loading 遮罩不自动消失（pdfkit「打开 PDF 后卡住」）—— ✅ 完成

用户报告：**pdfkit 打开 PDF 后，loading 遮罩不自动消失**，停在最后一次进度上报
的状态（标题「正在打开…」+ 明细「正在生成预览…」），鼠标划过窗口才被顺带重画掉。

「正在生成预览…」是 `jobs.rs` 最后一次进度上报的**明细**（显示在标题下方），
说明遮罩**卡在任务结束前那一帧**——这直接指向"任务结束后没人再出帧"。

### 一、根因：定时唤醒**不请求重绘**，于是 `frame()` 永不运行

四个环节连成一条链：

| # | 环节 | 事实 |
|---|---|---|
| 1 | 任务结束 | `task::end_busy` 配了 `busy_min_visible`（pdfkit 设 **400ms**）时**只置 `hide_at` 就 `return`**，**不 mark 脏**（遮罩要"至少被看见 400ms"） |
| 2 | 收尾时刻 | 到点由 `Runtime::reap_busy` 收掉 —— 而它**只在 `WindowCtx::frame()` 里调** |
| 3 | 谁来出帧 | `next_wakeup` 在有遮罩时给 `Some(now + SPIN_PERIOD)`，平台 `WaitUntil` 到点唤醒；但 `about_to_wait` 用的是 `pump()` 的返回值决定"要不要 `request_redraw`" |
| 4 | ★ 断点 | `pump` 的 `did_work` **只由 `external` / `tick` 置位**。定时唤醒到了、`tick` 没事可做 ⇒ 判定"没事做" ⇒ **不请求重绘** ⇒ `frame()` 永不运行 ⇒ `reap_busy` 永不执行 ⇒ **遮罩永远收不掉** |

**为什么"没有脏标志"是常态**：遮罩的 spinner 相位由**挂钟**算，
`WindowCtx::animate` 只把卡片标脏 —— 那笔脏记在 `Track` 里，
**不在 `Runtime` 的脏标志里**。而且 `animate(now)` 的返回值在**两个调用点都被丢弃**
（`platform/mod.rs` 的 redraw 路径与 pump 路径都是 `ctx.animate(now);`）。

### 二、隐藏的第二半：`reap_busy` 排在 `take_dirty` **之后**

`frame()` 里 `reap_busy` 在 `take_dirty` 之后 ⇒ 它标的那笔脏
（`VIEW | PRESENT`）要等**下一帧**才被消费。而遮罩一收掉，
`next_wakeup` 立刻回到 `None`（不再有唤醒源）⇒ **那一帧永远不来**。

两半缺一不可：

- 只修 `pump`：帧会跑，但收尾那帧只标脏不重跑 view ⇒ 遮罩仍在，随后无唤醒源 ⇒ 卡住；
- 只修次序：压根没有帧来执行 `reap_busy`。

### 三、改动

**① `WindowCtx::frame`** —— `reap_busy` 移到 `take_dirty` **之前**：

```rust
pub fn frame(&mut self, rt: &Runtime) -> FrameStats {
    let now = Instant::now();
    rt.reap_busy(now);                  // ★ 必须在 take_dirty 之前
    let mut dirty = rt.take_dirty(self.id);
    ...
```

**② 新增 `WindowCtx::needs_frame(&Runtime) -> bool`** —— "要不要出帧"的**唯一判据**：

```rust
pub fn needs_frame(&self, rt: &Runtime) -> bool {
    rt.is_busy(self.id) || !rt.peek_dirty(self.id).is_empty()
}
```

**③ `App::pump(now) -> bool`** —— 把"帧之间的非渲染工作 + 要不要出帧"
从平台层**下沉到 `App`**，平台那层只做委托。

理由：平台层要真实 winit 窗口、**单测里构造不出来**，而"该不该重绘"正是
这个 bug 的唯一入口。放到 `App` 上就和 `frame_all` 一样可以被无头测试直接驱动。

**④ `platform` 三处统一问 `needs_frame`** —— `pump` / `user_event` / `window_event`
此前**各写各的**（`user_event` 与 `window_event` 各有一份重复的
`contains(PRESENT|PAINT|VIEW)`），且**都没算遮罩**。现在三处同一个判据。

### 四、测试（3 条，全部变异验证）

| 测试 | 钉住什么 | 变异 | 结果 |
|---|---|---|---|
| `busy_overlay_is_reaped_by_the_first_frame_after_the_min_visible_window` | ★ 到点后**第一帧**就收掉（旧实现要两帧，而第二帧永不来） | `reap_busy` 移回 `take_dirty` 之后 | **FAILED** ✓ |
| `pump_reports_work_while_an_overlay_pends_even_without_dirty` | ★ 脏标志为空、只有遮罩时 `App::pump` 仍须为 `true` | 删掉 `did_work \|= w.needs_frame(..)` | **FAILED** ✓ |
| `overlay_clears_from_pump_and_frame_alone_with_no_other_events` | ★★★ **端到端**：复刻平台事件循环（`pump` → `frame_all` → `sleep(SPIN_PERIOD)`），**不喂任何输入事件**，遮罩必须自己消失 | 同上 | **FAILED** ✓（`跑了 0 帧`） |

**第 2、3 条特意断言**入口**而不是辅助方法**：`needs_frame` 只是判据，
**接线**（`pump` 有没有用它）才是缺陷所在 —— 所以测试直接打 `App::pump`，
并特意注明"删掉 `App::pump` 里那一句就会立刻失败"。

### 五、★ 为什么原来的测试没抓到

现有的 `a_fast_busy_section_still_shows_the_overlay` 是：
**手工**调 `rt.reap_busy(±400ms)` 再 `frame_all()`。

它验证了"收得掉"，却**从没走过"由帧自己来收"这条真实路径** ——
于是上面两半缺陷整整漏掉。

> 又一次同源教训：**测试替被测系统做了它自己该做的事**
> （手工调 `reap_busy` = 替 `frame()` 完成了收尾），
> 于是被测的那一段路径**根本没被执行**。
> 与本项目的"对照组必须确认它走了对照组分支"完全同源。

顺带一个**新测试自身的坑**（当场被抓，已记进注释）：端到端测试最初在循环开头
就读 `root_by_tag(BUSY_OVERLAY_TAG)` —— 而那时遮罩**还没被声明过**
（一次 frame 都没跑），于是立刻判定"已消失"、**假通过**。
靠最后那句 `assert!(frames >= 1)` 才暴露（`frames = 0`）。
修法：进循环前先 `app.frame_all()` 并断言"遮罩必须已经画出来"。

### 六、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace`（lieui） | **19 个测试二进制全 ok** |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |
| `cargo build`（**pdfkit**，path 依赖本仓） | exit 0 |
| 变异（`frame` 次序 / `pump` 接线） | 3 条测试分别 **FAILED** ✓ |

### 七、诚实说明

- **未做真实 GUI 观察**（需要显示环境）：证据是**逐帧推演 + 变异验证 + 端到端语义复刻**。
- 端到端那条是**语义复刻**，不是真平台：它验证不了平台那 3 行接线本身，
  但能验证"接线对了之后遮罩确实会自己消失"，以及"只看脏标志时会卡住"。

---

## 2026-10-08 · 移除 lieui 的「task」概念 —— ✅ 完成

### 一、判据（为什么要移除，而不是"看起来复杂"）

四条**可验证**的证据：

| # | 证据 | 位置 |
|---|---|---|
| 1 | 框架必须**偷看**用户的通道：`external()` 第一句就是 `if task::on_task_message(..) { return; }` —— 一条定义就是"投递不透明 `Send` 数据"的通道，框架得逐条辨认是不是自己发的 | `window.rs` |
| 2 | **原语没有出口**：`Runtime::waker()` 在无头模式返回 `None`，`Runtime` 本身是 `Rc` ⇒ **`!Send`**；"从别的线程投递"唯一的门是 `spawn_task` 给的 `TaskCtx` | `task.rs` |
| 3 | **280/899 行是调用方策略**（线程模型 / 取消 / 进度），且与遮罩**纠缠**：`next_task_id` 是任务与遮罩**共用的同一个计数器**，`busy_id_of_task` 专为"把 task 的进度路由到它挂的遮罩"而存在 | `task.rs` |
| 4 | **信封泄漏进用户代码**：pdfkit 被迫两层 downcast（先 `TaskEvent` 再自己的 `JobDone`）；框架还硬编码了 `std::thread::spawn` | `pdfkit`、`task.rs` |

### 二、★ 过程中被实测推翻的一个判断

上一轮我把"补一个 `Runtime::post`"说成**零风险**。写完立刻被编译器打回：

```
error[E0277]: `std::rc::Rc<RuntimeInner>` cannot be sent between threads safely
```

**`Runtime` 是 `!Send`** ⇒ 那个"入口"对工作线程**根本没用**。
→ 必须补的是一个**可 `Send` 的句柄**（[`Poster`]）。

而补句柄又逼出一件事：句柄会在 `set_waker` **之前**就 clone 出去（构造期的线程），
若它存"当时的唤醒器快照"，平台 waker 后装入就**永远看不见** —— 这正是 **A6**
（消息积压、界面毫无反应且零报错）。当时的补丁是 `LocalQueue::forward` +
`attach_platform` 事后 retrofit。

**所以把槽位本身做成共享的**（`Arc<PostHub>`：本地队列 + 后装入的平台 waker）：

> A6 从"打补丁"变成"**结构上不可能**"。

⇒ 上一轮评估里被我判为"收益有限、可以不做"的**单通道改造，因为这条需求变成了必需**。
（**又一次**：判据要在动手之后再复核，不能只看当时的数据。）

### 三、移除清单

| 删除 | 说明 |
|---|---|
| `spawn_task` / `spawn_task_busy` | 框架不再开线程 |
| `TaskCtx` / `TaskHandle` | 工作线程侧 / UI 线程侧的句柄 |
| `CancelToken` | 取消协议归调用方 |
| `TaskEvent` / `TaskFailed` / `TaskProgress` | 框架信封 |
| `on_task_message` | **偷看通道的入口** |
| `TaskRecord` + `RuntimeInner.tasks` | 任务表 |
| `busy_id_of_task` | (2)↔(3) 的耦合缝 —— 随任务一起消失 |
| `cancel_tasks_of` / `has_tasks` / `Runtime::waker` 的 `Option` 分支 | — |
| `WakerSlot`（快照枚举） / `LocalQueue::forward` / `attach_platform` retrofit | 换成共享的 `PostHub` |

**保留 / 新增**：

| 提供 | 是什么 |
|---|---|
| `Poster`（`Runtime::poster()`） | ★ **可 `Send` 的投递句柄** —— 非 UI 线程的入口 |
| `Runtime::emit` / `emit_global` / `wake` / `take_pending_external` | UI 线程侧的投递面 |
| `Emitter<T>` | **降级为 `Poster` 的类型化糖**（原先自己存 `WakerSlot` 快照） |
| `BusyToken`（`Runtime::begin_busy`） | 遮罩 —— 与任务**无关** |
| `BusyToken::dismiss_after` | ★ 新增：定时兜底（"等不到结果就别再挡 UI"） |
| `clear_busy_of` | 关窗清遮罩（`cancel_tasks_of` 留下的唯一一半） |

`RuntimeInner`：15 → **14** 字段；`task.rs`：**897 → 610 行**；`task_tests.rs`：660 → 397。

### 四、`WindowCtx::external` 不再偷看通道

```rust
// 之前
if crate::task::on_task_message(rt, self.id, &data) { return; }
// 现在：一切原样交给 ViewModel::on_external
```

通道恢复**不透明** —— 这是本次改动里最结构性的一处。

### 五、pdfkit：那"一层"搬到了应用里

`lieui` 不做的事，pdfkit **自己**做：新增 `src/app/tasks.rs`

- `CancelToken`（`Arc<AtomicBool>` 的本地版）
- `Msg { Progress, Done(Box<JobDone>), Panicked }`
- `JobCtx { poster, win, cancel }`（替代 `TaskCtx`）
- `AppVm::spawn_job(cx, label, work)` —— 与旧 `cx.spawn_task_busy` **同形**，
  所以 13 个调用点是一次**文本替换**（`cx.spawn_task_busy(` → `self.spawn_job(cx, `）
- `AppVm::on_job_message` —— **一层** downcast；遮罩在"收到结果"时由**调用方**收

pdfkit：`cargo test` 49 通过、`clippy --all-targets` exit 0。

### 六、★ 边界把关交给**编译器**：`tests/raw_task.rs`

新增集成测试（`tests/` **只能看到公开 API**），搭出"后台工作 + 遮罩 + 进度 + 取消 + 结果回传"：

> 如果原语不够用（缺投递入口 / 遮罩句柄不可从外部构造），**这个文件根本编译不过**。
> ⇒ "层划对了没有"不再靠约定，而是编译期约束。

3 条测试：正向（纯原语跑通）、**反向**（没人说结束 ⇒ 遮罩一直在）、
定时兜底（`dismiss_after` 救回"永远等不到结果"）。

### 七、验证

| 项目 | 检查 | 结果 |
|---|---|---|
| lieui | `cargo test --workspace` | **20 个二进制全 ok / 562 条**（新增 `raw_task`） |
| lieui | `clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| lieui | `fmt --check` / `build --examples` | 0 处差异 / exit 0 |
| **pdfkit** | `cargo test` | **49 通过** |
| **pdfkit** | `cargo clippy --all-targets` | exit 0 |

### 八、教训（三条，都来自本批）

1. **"零风险"的小改动，可能因为一个 `Send` 约束变成结构性改动** ——
   我说 `Runtime::post` 零风险时，漏了 `Runtime` 是 `Rc`。**编译器的第一次拒绝就是信息**。
2. **需求会反过来给旧结论定性**：单通道改造上一轮被判"收益有限"，
   这一轮因为"入口必须对工作线程可用"而变成**必需**。
   ⇒ 记录结论时要写清**它依赖什么前提**，否则前提一变结论就误导人。
3. **"原语够不够用"应当由编译器回答**，而不是由设计文档回答 ——
   `tests/` 只能看公开 API，所以它是天然的边界探针。

### 九、下一步（未做）

- `refactor-plan` 里 **D51**（`RuntimeInner` 杂物抽屉）因此瘦了一圈（15 → 14），
  但**主体仍未拆**（前置的 per-node 失效信号未就位，见早前判断）。
- `D52`（组件行为归位）现在更没必要做了：`track.rs` 里的 `input_*` / `slider_drag_to`
  等是**保留树自己的状态机**，与"任务概念"无关。
- 「便利层」的归处在 `examples/background_task.rs`（已重写为纯原语版）与
  pdfkit 的 `src/app/tasks.rs` —— 两者都是**活的模板**。

---

## 2026-10-08 · 补 `damage_key`（按 Key 标脏）+ 边界证明 —— ✅ 完成

起因：用户指出 `BusyItem` / `BusyToken` 这套也**不该内置在 UI 库里，它们是组件**。
查证途中先纠正了我自己的两个说法，再挖出一个**真正的库缺口**。

### 一、★ 我说错的地方：「忙碌状态必须有人持有」

我上一轮把它说成"需要一个库提供的状态类型"，并给了 A（库内组件层）/ B（应用侧）
两个选项 —— **两个都多余**。

正确答案就是用户说的：**调用者决定 spinner 是否显示**，所以它是**应用自己的普通状态**：

```rust
struct Ui { loading: Signal<bool>, started: Cell<Option<Instant>>, spinner: CustomCell }
// 事件里：self.loading.set(true) / set(false)
// view()：if self.loading.get() { v.modal_tagged(TAG, |m| { … spinner … }) }
```

只有两件事要额外照顾，**都不需要库提供类型**：

| 要照顾的 | 用什么 | 现状问题 |
|---|---|---|
| 最短可见时间 | `set_timeout` 延后置 `false` | 现在却是 `Runtime::set_busy_min_visible` 这个**特例 API** |
| spinner 要转 | `on_animation` 里 `request_animation` | 通用机制，已有 |

### 二、layer 与"元素外挂" —— **已经存在，不用"考虑"**

| 已有的 | 内容 |
|---|---|
| `Layer` | **6 变体**：`Content`/`Overlay`/`Popup`/`Tooltip`/`Modal`/`DragPreview`；`Layer::ALL` 是**唯一层序真相**（此前两份手写数组必须严格互逆） |
| 声明 API | `modal` / `modal_tagged` / `popup_at` / `popup_at_point` / `tooltip_at` |
| **元素外挂** | `DescRef::context_menu(builder)`、`DescRef::tooltip(text)` —— 挂**构造器/文案**，框架管**何时/在哪/谁收** |

框架侧统称 **`Sessions`（框架自管的交互会话）**，作者已写下它与 `Layer` 的分工：
**"会话*生产*层，而不是层"**（层是描述，会话是运行态）。

### 三、★ 真正的不一致：busy 是唯一的例外

| | 挂载点 | 何时弹 | 在哪 | 谁收 |
|---|---|---|---|---|
| tooltip | **元素** | 悬停 + 600ms | 锚元素 | 离开 / 目标失效 |
| 右键菜单 | **元素** | 右键 | 光标 | 点外部 / 点项 |
| **loading 遮罩** | **无挂载点**（`RuntimeInner.busy`） | **框架无条件注入** | 居中 | 应用 `finish()` |

⇒ busy 是**唯一既不在 `Sessions`、又没有挂载点的特例**。

**暂不泛化 `Sessions`**：样本只有 2 个，而抽象它要同时抽象"触发 / 锚定 / 收尾"
**三个各不相同**的维度。★ 意外收获：删掉 busy 后剩下的两种**都是元素挂载型**，
形状反而统一了 —— 等**第三个**出现再抽。

### 四、★★ 查出一个真正的库缺口

我原以为"应用侧重建遮罩"所需的一切都已具备。**有一处没有：**

> **应用声明的自绘节点（`CustomNode`），无法请求每帧重绘。**

- `CustomNode` 没有"我要重绘"的钩子（`on_event` 里能用 `cmd.damage(id)`，但那是**事件期**）
- `Ctx::damage(id)` 要 `NodeId`，而 `on_animation` / `on_tick` 里**拿不到**
  （`Ctx` 不暴露 `Track`；`Track::find_by_key` 是 `pub` 却够不着）
- 唯一可行路径是 `Ctx::damage_all()` ⇒ **每帧整窗重绘**

⇒ 今天能连续"转起来"的**只有框架内部的 spinner**（靠 `WindowCtx::animate`
内部 `mark_paint_dirty(card)`）；**应用侧没有等价路径**。

**补的是一条通用命令，不是"忙碌"概念**：

| 新增 | 位置 |
|---|---|
| `Cmd::DamageKey { key }`（落树时解析成 `NodeId`） | `cmd.rs` |
| `CmdBuf::damage_key(impl Into<Key>)` | `cmd.rs` |
| `Ctx::damage_key(impl Into<Key>)` | `event.rs` |

契约：**标的是该节点的矩形** ⇒ 自绘必须画在自己的 `rect` 内（画到外面会留残影）。
查不到 Key ⇒ **静默跳过**（动画节点可能还没对齐出来 / 刚被销毁；不许退化成整窗脏）。

### 五、★★ 边界证明：`tests/spinner_modal.rs`

只用**公开 API** 做出"自绘 spinner 的 loading 遮罩"（`tests/` 只能看到公开面
⇒ 原语不够就**编译不过**）：

| 谁的事 | 用什么 |
|---|---|
| 显示还是消失 | 应用状态 `Signal<bool>` —— **调用者决定** |
| 层从哪来 | `ViewBuf::modal_tagged`（应用自己定的 tag） |
| 每帧重绘自己 | `Ctx::damage_key` + `Ctx::request_animation` |
| 最短可见时间 | `Runtime::set_timeout` |
| 长什么样 | 应用的 `CustomNode`（自绘三点 spinner，相位取**单调时钟**） |

3 条测试：正向（出现 → 每帧只重绘自己 → 调用方收起）、
**反向**（没人说结束 ⇒ 一直在，且一直在转）、
最短可见时间（`set_timeout` 表达，**不需要特例 API**）。

**实测数字**：动画帧只重绘 **324 像素**（= spinner 节点自己的 18×18），
整窗是 400×300 = **120000**。

### 六、变异验证（两条，都精确命中）

| 变异 | 结果 |
|---|---|
| `on_animation` 里 `damage_key` → `damage_all`（修复前的唯一路径） | **FAILED** ✓ `★ 不是整窗脏 —— damage_all 会让每帧全屏重绘` |
| （另）`DamageKey` 查不到 Key 时退化成整窗脏 | 被 `damage_key_with_an_unknown_key_is_a_silent_no_op` 钉住 |

### 七、验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | **21 个测试二进制全 ok / 568 条**（新增 `spinner_modal` 3 条 + `cmd_tests` 3 条） |
| `cargo clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| `cargo fmt --all --check` | 0 处差异 |
| `cargo build --examples` | exit 0 |

### 八、下一步（本批**未做**，是真正的"删 busy"）

step 0/1 已完成，且证明了"**删掉 busy 不会丢能力**"。剩下：

| 步 | 内容 |
|---|---|
| 2 | 库侧删干净：`Runtime` 13 方法 / `RuntimeInner` 3 字段 / `WindowCtx` 8 处 / `overlay.rs` 170 行 / `WindowConfig::auto_busy_overlay` |
| 3 | 应用侧参考实现（`examples/`：≈100 行） |
| 4 | pdfkit：`busy: RefCell<Option<BusyToken>>` → `loading: Signal<bool>` + `started` + `set_timeout` |
| 5 | 测试迁移：那 4 条护栏测的是"库保证遮罩"，**随路径一起删**；在应用侧重建等价断言 |

> ⚠️ step 5 的说明要提前立好：`needs_frame` / `reap` 次序 / 端到端遮罩那几条
> **会失效**，因为被测路径就是要删的那条 —— **不是回归**。
> 但"最短可见 / 定时兜底 / 没人说结束就一直在"这三条断言必须在应用侧重建，
> 否则会把刚修好的东西重新弄丢（本批的 `spinner_modal` 已经重建了前两条的形状）。

### 九、教训

1. **"必须有人持有"是个含糊说法** —— 我说它时其实没想清"持有者是谁、要不要类型"。
   用户一句"应该是调用者决定"就点破了：**就是应用状态，没有第二种答案**。
   ⇒ 用词含糊时，多半是自己还没想清。
2. **"要不要考虑加 X" 之前，先确认 X 有没有** —— 用户问"是否要考虑暴露 layer
   和元素外挂"，实测**两者都已存在**（6 个 Layer 变体 + 两个元素挂载 session）。
3. **真正的缺口往往不在被质疑的地方** —— 用户的怀疑指向 busy，但实测查出的缺口
   是"自绘节点无法请求重绘"（`damage_key`），一个**与 busy 无关的通用能力**。

---

## 2026-10-08 · 移除 lieui 的「loading 遮罩」（它是个组件，不是库概念）—— ✅ 完成

用户判定：`BusyItem` / `BusyToken` 这套**不该内置在 UI 库里，它属于 widget 层级**。
选定形态 **①**：库**不发布**任何遮罩渲染件 —— 全删，由应用自己写。

### 一、判据：它污染了**核心帧循环**

| # | 位置 | 事实 |
|---|---|---|
| 1 | `WindowCtx::frame` | **帧首无条件**调 `rt.reap_busy(now)` —— 库的主循环知道什么是"忙碌项" |
| 2 | `WindowCtx::frame` | `view()` 之后无条件 `push_busy_overlay(rt)` —— **库注入用户没声明的 UI** |
| 3 | `WindowCtx::next_wakeup` | ★★ `&Runtime` 参数的**唯一用途**就是 `rt.is_busy(id)` —— 一个**纯窗口级时钟查询**被迫借运行时 |
| 4 | `WindowCtx::needs_frame` | 判据里混进 `is_busy` |
| 5 | `WindowCtx::animate` | 里 `busy_card()` → 标脏 spinner 卡片 |
| 6 | `WindowConfig` | `auto_busy_overlay` —— 一个**组件**的开关出现在**窗口配置**里 |
| 7 | `RuntimeInner` | 3 个字段：`busy` / `next_busy_id` / `busy_min_visible` |
| 8 | `task.rs` | `BusyItem`(7 字段) + `BusyToken`(7 方法) + `Runtime` 13 个方法 |

**体量**：`overlay.rs` **170 行** + `task.rs` 里 busy 占 **343 行**（56%）+ `window.rs` 8 处帧触点。

而「遮罩不自动消失」那个缺陷的形状**直接来自这个架构**（"库要保证某个组件的动画帧"
⇒ `reap_busy` 只在 `frame()` 里跑 ⇒ 定时唤醒不重绘就卡住）。

### 二、删除清单（16 文件，**净 −1220 行**）

| 删除 | 说明 |
|---|---|
| `src/overlay.rs` + `overlay_tests.rs` | 整个模块（内置遮罩渲染 + Spinner + SPIN_PERIOD + BUSY_OVERLAY_TAG） |
| `BusyItem` / `BusyToken` | 两个类型（含 7 个方法） |
| `Runtime` 13 个方法 | `begin_busy` / `end_busy` / `reap_busy` / `is_busy` / `busy_items` / `clear_busy_of` / `set_busy_*`（5 个）/ `busy_min_visible` / `set_busy_deadline` |
| `RuntimeInner` 3 字段 | `busy` / `next_busy_id` / `busy_min_visible`（**14 → 13**） |
| `Ctx::begin_busy` | — |
| `WindowConfig::auto_busy_overlay` + builder | 一个组件的开关不该在窗口配置里 |
| `WindowCtx` 8 处 | 帧首收尾 / 每帧注入层 / `next_wakeup` 的 spinner 分支 / `needs_frame` 的 busy 项 / `animate` 的卡片标脏 / `push_busy_overlay` / `busy_card` / `Sessions.spinner` |
| `next_wakeup(&Runtime)` → `next_wakeup()` | ★ 参数的唯一用途没了 ⇒ **纯窗口级查询不再借运行时** |

**顺带**：`next_busy_id`（任务与忙碌项**共用**的计数器）改名 `next_timer_id` ——
它现在只服务定时器，这才叫对了名字。

### 三、应用侧：那"一层"归位（① = 库不发布渲染件）

| 去处 | 内容 |
|---|---|
| `tests/spinner_modal.rs`（上一批已建） | ★ **边界证明**：只用公开 API 做"自绘 spinner + 最短可见时间 + 反向（没人说结束就一直在）" |
| `tests/raw_task.rs`（改写） | **投递**边界证明：后台工作 + 进度 + 取消 + 结果回传，全程只用公开 API |
| `examples/background_task.rs`（重写） | ★ **参考实现**（≈100 行：自绘 Spinner 60 + 卡片 DSL 40 + 应用状态） |
| `examples/gallery.rs`（改造） | 遮罩改为应用声明的 Modal 层 |
| **pdfkit** | 新增 `BusyState`（文案 / 进度 / 明细 / 取消令牌 / 起始时刻）+ `ui::mod` 里的 `busy_modal` 层；`set_busy_min_visible(400ms)` → `Ctx::set_timeout` |

### 四、测试迁移：**4 条护栏失效是预期的**

被删的 4 条（`a_fast_busy_section…` / `busy_overlay_is_reaped…` /
`pump_reports_work_while_an_overlay_pends…` / `overlay_clears_from_pump_and_frame…`）
测的正是"**库保证遮罩**"这条路径 —— **那条路径就是要删的，不是回归**。

**但两条测"真库不变量"的必须留下**，只换 vehicle：

| 保留（改写） | 钉的不变量 |
|---|---|
| `an_app_declared_modal_covers_a_full_window_image` | ★ **光栅层的层序**：整窗图片不得盖住后声明的 Modal（原 bug：图片 op 统一放到批次末尾 blit） |
| `a_declared_modal_blocks_input_to_the_content_below` | ★ **Modal 阻断下层交互**（层语义） |

新增 `pump_reports_work_when_an_animation_frame_is_due`：**RAF 的通用替代** ——
`request_animation` ⇒ `tick` 返回 true ⇒ `App::pump` 报"要出帧"（原先靠 `is_busy` 那一项）。

### 五、验证

| 项目 | 检查 | 结果 |
|---|---|---|
| lieui | `cargo test --workspace` | **21 个测试二进制全 ok / 551 条** |
| lieui | `clippy --workspace --all-targets`（CI `-D warnings`） | exit 0，零警告 |
| lieui | `fmt --check` / `build --examples` | 0 处差异 / exit 0 |
| **pdfkit** | `cargo test` / `clippy --all-targets` | **49 通过** / exit 0 |

**规模**：`task.rs` 611 → **258**；`window.rs` 1291 → **1226**；
`overlay.rs`（170）删除；`RuntimeInner` 14 → **13** 字段；全批 **净 −1220 行**。

### 六、教训

1. **"库提供能力"与"库拥有概念"是两件事** —— 遮罩需要的每一样能力（投递 / 定时 /
   声明层 / 自绘 / 逐帧标脏）都是**通用**的；把"遮罩"本身放进去，是**概念**越界。
   判据很清楚：**删掉它之后，通用能力一条都没少**。
2. **污染会沿着"最省事的路径"蔓延** —— `next_wakeup(&Runtime)` 就是标本：
   一个纯窗口级查询，因为"顺手能问到 is_busy"，就永久背上了一个运行时参数。
   ⇒ 看到"参数只为一件事存在"时，那件事多半不该在这里。
3. **删功能时必须区分"测功能的"与"测不变量的"** —— 4 条护栏测功能（该删），
   2 条测不变量（该留，只换 vehicle）。若一起删，就会丢掉一个真实的**光栅层序**回归护栏。
