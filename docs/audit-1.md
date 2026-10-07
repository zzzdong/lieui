我先摸清仓库现状，然后分模块深入分析。
-
代码约 2.7 万行。先读架构文档与入口，再并行分模块深挖。
架构文档 `architecture.md` 是 v2 旧版，实际代码是 v3（retained tree）。我读 v3 基线，同时并行派子代理分模块深挖。
子代理已带回详实发现。我核实几个最关键的问题点，避免误判。
核心缺陷已确认。再核实渲染与跨线程两处。
核实完毕。最后确认设计基线文档的关键约定。
最后确认脏区登记与失效入口的确切签名，以便给出可落地的改法。
分析完成。我读了架构基线、核实了子代理的关键发现，并抽查了 8 处代码。

---

# lieui 代码库完整分析报告

**规模**：主 crate `lieui`（v3 retained架构）+ 3 个基础 crate（geom/text/layout），约 2.7 万行 Rust，350 个单测，无 CI。分支 `main`，`0.1.0-alpha.4`。

## 一、结论速览

这套架构的**方向是对的**，核心决策（retained 树 + desc/state 分离 + Cmd 缓冲 + 脏区光栅化）都站得住。真正的问题不是"架构错了"，而是：

> **设计文档承诺的三条核心性质，在实现里都只做了一半，且所有退化路径恰好都没有测试。**

| 文档承诺 | 实现现状 |
|---|---|
| `Cmd` 是唯一延迟写入通道（`cmd.rs:1-12`） | 内置行为（widgets/input/overlay）直接持 `&mut Track` 写树，同一次分发内两套语义混用 |
| 精确脏区，避免残影（`architecture-v3.md:693-694`） | `destroy` 不登记旧矩形；`mark_layout_dirty` 不产生矩形；阴影超出 `paint_bounds` |
| 局部光栅化，开销 ∝ 脏区面积（§3.7） | 碎片 >8 或面积 >45% 退化为整窗 —— 而**滚动和虚拟列表必然超过8 碎片** |

一句话：**稳态是对的，边界是对的，但"最常见的场景"恰好走了退化路径，而退化路径没有断言。**

---

## 二、真正做对的设计（应当固化的资产）

分析中发现的优秀决策，比问题更值得记录：

1. **`KindDesc` / `Kind` 双枚举分离**（`track.rs:254-508`）—— 这是全仓最干净的设计。声明数据（`checked`/`value`/`text`）在 desc 组，运行时视图态（`dragging`/`caret`/`preedit`）在 state 组；对齐时 `apply_to` 只覆盖 desc 组，**state 组天然保留**。这正是 WinUI 的做法，比"reconciler 更新整个节点"高明得多。
2. **单线程由类型系统强制**：`Runtime` 是 `Rc<RuntimeInner>` + 全 `RefCell`，无任何 `unsafe impl Send/Sync`。不需要运行时检查来保证 UI 单线程。
3. **Cmd 缓冲解决借用冲突**：`collect_route`（只读克隆 `HandlerSlot`）→ `dispatch`（可写）是"既要改树又要发事件"的唯一解，这个设计比 `Rc<RefCell>` 互逃优雅。
4. **`HandlerSlot` 的 `handled_events_too`**（`event.rs:481-488`）—— 用数据表达"已处理也要响应"，取代了旧 `.builtin()` 排序 hack。
5. **边界重排**（`track.rs:1737-1804`）：脏标记冒泡到"自身尺寸确定"的节点即停，`layout_boundaries` 只取最上层脏节点。比全树重排有量级优势。
6. **两段式文本排版**（`lieui-text`）：`measure` 缓存 + `layout` 只跑一次，`TextEngine` 与绘制层彻底分离。
7. **`next_wakeup` 统一时钟**（`app.rs:766-777`）：spinner/blink/tooltip/timer/动画帧汇总成单一唤醒源，无事件不重绘，空闲零功耗。软渲染 GUI 里这是决定性优势。
8. **`#![forbid(unsafe_code)]`** 且渲染路径真的做到了（`raster.rs:612-621` 用 `glyph_errors` 计数替代 `unwrap`）。

---

## 三、缺陷分级清单

### P0 — 会产生用户可见的错误（数据破坏级 / 视觉错误级）

| # | 问题 | 证据 | 后果 |
|---|---|---|---|
| 1 | **`Tapped` 不校验按下/抬起是否同一按键** | `input.rs:149-162` | 右键按下 → 左键抬起落在同一节点 ⇒ 合成 `Tapped` ⇒ 触发勾选/提交/删除。**数据破坏级** |
| 2 | **`destroy` 不登记脏区** | `track.rs:1044-1067` | 删除节点不 push 旧矩形；仅靠"脏区恰好为空⇒整窗"兜底。同帧有其它脏区时**留残影** |
| 3 | **图片绕过裁剪栈** | `raster.rs:480` 自述 | 滚动容器/圆角裁剪内的图片**溢出到裁剪区外**。这是可见渲染错误 |
| 4 | **`close_requested` 丢弃全部 Cmd** | `app.rs:1273-1276` | `cx` 建了、`on_close_request` 调了，`take_cmds()` 从不调用 ⇒ 关闭回调里的 `focus`/`damage`/`scroll_to` 全部静默失效 |
| 5 | **命中裁剪与渲染裁剪坐标系不一致** | `hit.rs:104-108` vs `scene.rs:471-480` | hit 用逆变换后的**局部点**测 `clip`，render 把 `clip` 当与 `rect()` 同空间取交 ⇒ 带 `transform+clip` 的节点"能点到的区域 ≠ 画出来的区域" |

### P1 — 挂起 / 状态泄漏

| # | 问题 | 证据 | 后果 |
|---|---|---|---|
| 6 | **定时器回调内 `cancel()` 无效；关窗后成孤儿** | `app.rs:1230-1236` | 定时器先出表 → 执行回调 → **无条件** `reschedule_timer`。取消失效 + 永久泄漏（持闭包捕获） |
| 7 | **`run()` 之前 `spawn_task` 永久静默挂起** | `task.rs:506`（克隆旧 waker）+ `platform/mod.rs:955`（`set_waker` 在 run 内）+ `app.rs:1350`（只有 `frame_all` 消费本地队列，平台层不调） | 表现为"任务永不回调、界面无反应"，无任何报错 |
| 8 | **按下态泄漏** | `input.rs:100-104` | 二次 `Down` 覆盖 `pressed_path` 但不清旧链 ⇒ 旧节点**永久残留 pressed 视觉** |
| 9 | **`view()` panic 会毒化 Runtime** | `reactive.rs:403-411` + `app.rs:639/650` 非 RAII | `in_view` 永久为 `Some`，此后所有 `Signal::set` 在 debug 下 panic，release 下静默死循环 |
| 10 | **焦点与浮层零互斥** | `LayerOpts.focus: FocusPolicy`（`track.rs:643`）**全仓无消费点**；`focus.rs:71-88` Tab 链含所有层根 | 弹层打开时 Tab 会走到被遮住的节点后面；`Cmd::SetFocus`（`cmd.rs:199-216`）不发焦点事件、不校验 `enabled`，与 `WindowCtx::focus` 双语义 |
| 11 | `Layer` 增变体不报错 | `hit.rs:20-27` 硬编码 6 层 | 新增层类型**静默漏遍历**，无编译期保护 |

### P2 — 性能（软渲染下帧率杀手）

| # | 问题 | 证据 | 量级 |
|---|---|---|---|
| 12 | **局部上屏只按整行拷，水平零节省** | `platform/mod.rs:485-497` — `start = row*stride; end = start+stride` 完全忽略 `r.x`/`r.width` | 呈现带宽**白白翻倍**，改动成本极低 |
| 13 | **碎片阈值把滚动打成整窗** | `raster.rs:115` — `out.len() > 8` 即退化 | 滚动/虚拟列表必然 >8 碎片 ⇒ 每帧全窗口光栅 + 全树 Scene 重建 |
| 14 | **Scene 每帧从零重建** | `render/mod.rs:137` + `scene.rs:407` | 稳态也有 O(n) 遍历 + Vec 分配；`TextCache` 满 2048 即**整表 clear**（`scene.rs:261`）有抖动风险 |
| 15 | **FlexNode 临时树每次布局全量重建 + 字符串克隆** | `layout.rs:94/265-310` | 边界重排限制了范围但没限制成本，收益被分配与 clone 抵消 |
| 16 | **`Signal::set` 无同值短路** | `reactive.rs:390-394` | `on_tick` 里无条件 set 同值 ⇒ **永动重建循环** |
| 17 | **每节点内联 `TextSpec.font_family: String`** | `track.rs:747` + `lieui-text/src/spec.rs:37` | 每节点一次堆分配 + Node 深拷贝；align 阶段逐节点拷贝 |
| 18 | **脏区算三遍、批次算两遍** | `render/mod.rs:121-125`、`raster.rs:349-364`、`platform/mod.rs:182-193` | 每帧 ≥4 个临时 Vec |
| 19 | 阴影模糊超出脏区 | `widgets/mod.rs:633-636` vs `track.rs:1717-1730` | 光晕外圈不在脏区内 ⇒ 残影 |

### P3 — API / 工程化债务

- **零 CI、零 lint 门禁**：`Cargo.toml:46-48` 无 `[lints]` 段，无 `.github/`、`rust-toolchain.toml`、`rustfmt.toml`、`deny.toml`。唯一门禁是 `forbid(unsafe_code)`。
- **文档示例 100% `ignore`**：所有 doc 代码块都是 ```` ```ignore ````，非 ignore 块搜索结果为 0 ⇒ **API 签名漂移不会被 `cargo test` 发现**。
- **无集成测试**：主 crate 无 `tests/` 目录，控件层没有"外部用户视角"的 API 契约测试。
- **死 API**：`ImageStyle`/`ImageFit`（`style.rs:226-285`）—— 每节点存一份、无 builder 写入、绘制不读、`raster.rs:399-413` 直接拉伸 blit，而 `gallery.rs:460` 注释宣称"contain 缩放"（**文档与实现矛盾**）。`Cmd` 15 个变体中 8 个生产不可达。
- **`DescRef<'a>` 一次性借用 + 容器 API 返回 `()`**（`view.rs:262` vs `:282`，函数体 3 行完全相同）⇒ 用户被迫写4 行样板（`gallery.rs:151-165`），并放大 `view.rs:1331` 的 `panic!("icon 应是 Text")`。
- **内置行为分支几乎零测试**：`widgets/mod.rs` 的 `handle` 6 个分支中 4 个（Slider/Checkbox/Switch/Radio）无任何测试，"未绑定就不改模型"这条反直觉规则完全没钉住。

---

## 四、架构改进建议（按投入产出排序）

### 主线 A：统一写入通道 —— 让 `Cmd` 名副其实（P0/P1）

**根因**：`Cmd` 只约束了用户 handler，而框架自己的内置行为为了性能直接写 `&mut Track`。两套语义在同一次分发内混用，顺序不可推断。

**方案**：引入**分层写入**，明确三级优先级并在文档固化：

```rust
/// 写入分级（解决"Cmd 是唯一通道"名存实亡）
/// L0 立即：纯框架内部、不产生事件、不影响分发结果（如 capture 的记录）
/// L1 本帧：CmdBuf 落树，当前设计/// L2 跨帧：RequestQueue（开窗/关窗）
```

把 `widgets::handle_route`（`app.rs:824`）、`input::step`（`app.rs:869`）、overlay 的 `track.remove_root`（`app.rs:1069/1133`）改为**直接向 `Ctx` 提交 Cmd**，而非拿 `&mut Track`。代价是每帧一次小 Vec 分配（可复用缓冲），换来"分发期树只读"这条**可推理的不变量**——这是整个架构最值钱的东西，不该被内部代码绕过。

同时修 P0#4：`close_requested`补上 `apply_cmds`（`app.rs:1275`之后三行）。

### 主线 B：脏区正确性 —— 把"绘制影响范围"变成契约（P0）

`mark_paint_dirty`（`track.rs:1710-1715`）已经做对了（登记矩形），但另外两条路径没跟上。三处对齐：

1. **`destroy` 登记旧矩形**（P0#2）：

```rust
pub fn destroy(&mut self, id: NodeId) -> usize {
    let ids = self.descendants(id);
    // ★ 破坏前登记旧绘制范围：否则同帧存在其它脏区时，删掉的节点留下残影
    for i in &ids {
        if let Some(r) = self.damage_bounds(*i) {
            self.damage_rect(r);
        }
    }
    let parent = self.parent_of(id);
    self.detach(id);
    // …（其余不变）
}
```

2. **`mark_layout_dirty` 与 `mark_paint_dirty` 对称**（P2#19）：布局变化必然改变绘制，当前只置 flag 不登记矩形（`track.rs:1752`）。让 `mark_layout_dirty` 末尾也调 `damage_bounds`。

3. **`damage_bounds` 的祖先缺失静默放弃**（`track.rs:1720-1730` 的 `?`）：改成 `continue` 式逐段累加，并对"根节点无祖先"给出显式分支。

**关键配套**：这三条各加一个像素级回归测试 —— 用现有 `render/mod.rs:171-178` 的 `px()` 助手，断言删除/patch 后目标区域像素等于底色。**残影类缺陷只能靠像素测试发现，逻辑断言无效。**

### 主线 C：渲染的"局部"要真的局部（P2，性能杠杆最大）

三步独立可做，风险递增：

**C1（改动5 行，收益立竿见影）**修 `platform/mod.rs:485-497` 为真正的矩形拷贝：

```rust
for r in &batches {
    let x0 = (r.x.max(0.0) as usize).min(pw);
    let w = ((r.width.get().max(0.0) as usize).min(pw - x0)).max(1);
    let y0 = r.y.max(0.0) as usize;
    let h = (r.height.get() as usize).min((ph as usize).saturating_sub(y0));
    for row in y0..y0 + h {
        let s = row * stride + x0;
        let (a, b) = (s, (s + w).min(src.len()).min(dst.len()));
        if a < b {
            pack_xrgb(&src[a..b], &mut dst[a..b]);
        }
    }
}
```

**C2（算法级）** 碎片阈值太激进。`damage_batches`（`raster.rs:83-119`）在判定退化前**先做矩形合并**：相交或间距< GAP 的矩形求并。滚动场景的"旧∪新"矩形高度重叠，合并后通常 ≤4 块。`examples/damage_bench.rs:194-195` 已有 `union`/`bands` 两种策略的实现和对比数据 —— 应当**用基准数据选定默认策略**，而不是硬编码 45%/8 两个魔数。

**C3（图片走 clip 栈）** 修 P0#3。最小改动方案：`Op` 流里已有 `PushClip`/`PopClip`，在 `rasterize` 的 op 循环里维护一个软件 clip 栈（`raster.rs:396-420`），`blit_image` 接收当前 clip 并求交。改动约 40 行，无需图片缓存与 vello 集成。

**C4（display list）** 消除 P2#14。按 `(NodeId, 内容版本)` 缓存每节点的 op 段，节点未变则整段复用；失效条件为"节点自身或其任一祖先重排"。工程量较大（需 per-node 缓存 + 布局失效传播），但这是软渲染框架的**结构性上限所在** —— 建议排在主线 A/B 之后。

### 主线 D：输入状态机的正确性（P0/P1）

**修 P0#1**：`pressed` 从 `Option<NodeId>` 升级为完整按下态：

```rust
pub struct PressState {
    pub node: NodeId,
    pub pointer: PointerId,
    pub button: PointerButton,   // ★ 新增：抬起时校验配对
    pub pos: Point,
    pub at: Instant, // ★ 新增：长按阈值
}
```

`Tapped` 合成条件改为：`pressed.button == up.button` **且** `pressed.node ∈ up_path` **且** 位移 < 阈值 **且** 时长 < 长按阈值。这是 15 行改动，堵住数据破坏级缺陷。

顺带修 P1#8：`Down` 时若已有 `pressed`，先把旧链按 `Up` 的方式清干净。

**修 P1#10（焦点互斥）**：`LayerOpts.focus: FocusPolicy` 已定义未消费。补上消费点：弹层挂载时按 policy 保存旧焦点 / 夺取焦点 / 阻断下层 Tab；`Cmd::SetFocus` 与 `WindowCtx::focus` 合并为单一入口（当前一个发事件一个不发，是最难查的一类不一致）。

### 主线 E：响应式的"防抖"而非"追踪"（P2）

文档 §7 明确"不做细粒度响应式"，这是**正确的取舍**，不需要推翻。但两个低成本改进能把最坏情况压住：

```rust
impl<T: PartialEq + 'static> Signal<T> {
    /// 同值写入不置脏（消除 on_tick/动画回调里的重建风暴）
    pub fn set(&self, value: T) {
        self.assert_not_in_view();
        let mut slot = self.inner.value.borrow_mut();
        if *slot == value { return; }
        *slot = value;
        drop(slot);
        self.rt.mark_all(Dirty::VIEW);
    }
}
```

`Signal<T>` 已有 `impl<T: Clone> Clone`，把 `PartialEq` 加到 `Clone` 那个 impl 块上即可（几乎所有实用类型都有）。另加 RAII 守卫修 P1#9：

```rust
pub(crate) struct ViewGuard<'a>(&'a Cell<Option<WindowId>>);
impl Drop for ViewGuard<'_> { fn drop(&mut self) { self.0.set(None) } }
```

`begin_view/end_view` 改为返回这个守卫，`?`/panic 都能正确恢复。

---

## 五、工程化补齐（投入极低、防止回归累积）

按性价比排序，建议一并做：

1. `Cargo.toml` 加 `[lints.rust] unexpected_cfgs = "deny"` + `[lints.clippy] all = "deny"` —— 现有 `menu.rs` 3 条既有警告会立刻暴露，正好清理。
2. `rust-toolchain.toml`（`channel = "stable"`）+ `rustfmt.toml`。
3. `.github/workflows/ci.yml`：`fmt --check` / `clippy -D warnings` / `test --workspace` / MSRV 1.88 检查 / `cargo build --examples`。
4. 新建 `tests/api_contract.rs`：以**外部用户视角**测 prelude API（这段代码的编译失败就是最好的 API 回归检测）。
5. doc test 去 `ignore`：至少 `lib.rs`、`view.rs`、`custom.rs`、`theme.rs` 四处改成可编译 + `no_run`。
6. 删死代码：8 个不可达 `Cmd` 变体、`ImageStyle`/`ImageFit`（或补齐实现并修正 `gallery.rs:460` 的错误注释）、`RoutePlan.target`、`FocusPolicy`（或补消费点）、`damage_batches_union/bands`（或改为 bench 内部函数）。
7. `deny.toml` 锁依赖（当前 vello/parley/skrifa 版本对齐是靠人工记忆 —— 上次 `9b00f0a` 就是为这个专门修的）。

---

## 六、建议的推进顺序

| 批次 | 内容 | 理由 |
|---|---|---|
| **P0立即**（约 150 行） | 主线 B脏区三处 + 像素回归测试；主线 D 的 PressState；`close_requested` 补 `apply_cmds`；主线 C1矩形上屏 | 全是"小改动 + 真实错误"，且都有现成测试助手可写断言 |
| **P1 本迭代** | 主线 A 统一写入通道；主线 D 焦点互斥；`run()` 前任务挂起；定时器取消；Layer 变体编译期保护 | 需要设计讨论，但不改对外 API |
| **P2 下迭代** | 主线 C2/C3（脏区合并、图片 clip）；主线 E 防抖；`TextSpec.font_family` 改 `Arc<str>`；主线 C4 display list | 性能，需要基准数据驱动 |
| **持续** | 第五节工程化；`app.rs` 的 `WindowCtx` 按已有的 `Sessions`（`app.rs:444-462`）思路拆成 `FrameDriver`/`InputRouter`/`SessionHost`；`KindTag`/`from_desc`/`apply_to` 三元组用 `macro_rules!` 生成（现在加一个控件要改 6 处） | 结构重构，不阻塞功能 |

---

**一句话总结**：lieui 的架构骨架（retained + desc/state 分离 + 单线程类型约束 + 事件驱动调度）已经达到可长期演化的水平，**不需要推翻重来**。它在实现层的三个系统性问题是同源的——**"契约写在文档里、代码只实现主路径、退化路径无测试"**。优先把三条主路径（脏区、写入、局部渲染）钉死并补上像素级断言，中期做 display list，中期做 display list，这套设计就能真正兑现"纯 CPU 渲染 + retained 树"的性能承诺。