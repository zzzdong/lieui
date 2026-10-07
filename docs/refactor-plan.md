# lieui 改造分析（四份审计整合版）

> 版本：v1.1 | 日期：2026-10-07 | 状态：**待评审**
> 基线：`main` 分支 `70a62f7`，`lieui 0.1.0-alpha.4`
> 设计基线：[`docs/architecture-v3.md`](./architecture-v3.md)（v3.4-draft）
>
> **输入**（四份独立审计 + 一轮交叉评审）：
> - [`docs/audit-0.md`](./audit-0.md) —— **帧调度 / 布局引擎 / 渲染管线**视角
> - [`docs/audit-1.md`](./audit-1.md) —— **正确性**视角，聚焦"文档承诺 vs 实现"的落差与退化路径
> - [`docs/audit-2.md`](./audit-2.md) —— **性能与模块结构**视角，聚焦布局桥接、置脏粒度、god-file
> - [`docs/audit-3.md`](./audit-3.md) —— **模块边界回潮**视角，聚焦职责漂移（`RuntimeInner` / `Track` / `app.rs`）
> - **交叉评审** —— 对四份报告做事实复核与排序校准（发现 1 处功能性漏洞、5 处事实偏差、6 项遗漏）
>
> **整合方法**：不同子代理、不同切分维度的结论做交叉比对；所有引用行号与统计数字均已实测复核。
> 标注规则：`✔已核验` = 逐行读过源码确认；`✔✔多源` = 至少两份独立指出且已核验；
> `△待验` = 单源提出、整合时未逐行确认（动手前需先复现）。

---

## 〇、v1.1 相对 v1.0 的修订

| # | 修订 | 原因 |
|---|---|---|
| 1 | **主线 B 补上 `request_redraw` 闭环** ⚠️ | v1.0 的帧调度改法**会导致定时器/动画停帧**，是全计划唯一的功能性风险 |
| 2 | **D6 降级 P1、移出 S1**，结论修正为"绘制与命中**两侧**都不消费 `Root.owner`" | 与 §3.7 + §3.15 两条设计承诺冲突，是功能缺陷而非数据破坏；修复跨 3 文件 |
| 3 | **基线数字全部复核**（v1.0 的 5385/2023/1170 有误） | 见 §附录 A；偏差 10%+，影响 D50/G1 的估算 |
| 4 | **`△待验` 标记统一**（6 项其实已核验，其中 D3 是代码注释自述） | 避免执行者重复劳动 |
| 5 | **C2 验收标准量化** | "接近真实脏面积"不可验收 |
| 6 | **H1/H2 先行 + clippy 清零**后才允许大规模机械重构 | 当前 clippy 不干净，机械搬移会淹没行为变更 |
| 7 | **新增 §四·G「模块边界收敛」主线**（G1–G8） | 吸收 audit-3 的高价值发现：`RuntimeInner` 杂物抽屉、组件行为回归 `Track`、几何求值分散 4 处 |
| 8 | ~~**D5 修法升级**：从"改 `hit.rs` 判据"改为"收敛出 `Track::visual_xform`"~~ **⇒ 2026-10-07 后续推翻**：D5 本身是误判（见 §三），但 G3「几何求值收敛」作为可维护性论点仍成立，只是**不再有 P0 撑腰**，应按"持续"批次处理 | audit-3 §8 指出四个求值点各自实现 |
| 9 | **补 6 项遗漏**（D51–D56） | 四份审计各自的盲区 |

---

## 一、结论速览

四份分析在**方向判断上完全一致**：

> **架构骨架不需要推翻重来。** retained 树 + `desc/state` 分组 + `Cmd` 缓冲 + 脏区光栅化 + 单线程类型约束，
> 这些决策都站得住，已经达到可长期演化的水平。

问题集中在**实现层**，且四份分析独立地指向同一个根因：

> **契约写在文档里、代码只实现主路径、退化路径既没实现也没测试。**
> 于是"最常见的场景"恰好走退化路径，而退化路径恰好没有断言。

三条被点名的"承诺 vs 实现"落差（`✔✔多源`）：

| 设计承诺 | 实现现状 | 后果 |
|---|---|---|
| `Cmd` 是唯一延迟写入通道（`cmd.rs:1-12`） | 内置行为（`widgets`/`input`/`overlay`）直接持 `&mut Track` 写树，同一次分发内两套语义混用 | 顺序不可推断，脏标记语义不可预测 |
| 精确脏区，避免残影（§3.7） | `destroy` 不登记旧矩形；`damage_bounds` 遇祖先缺失静默放弃；`mark_layout_dirty` 不产生矩形；阴影超出 `paint_bounds`；图片绕过裁剪栈 | 视觉残影 / 溢出 |
| 局部光栅化，开销 ∝ 脏区面积（§3.7） | 碎片 >8 或面积 >45% 退化整窗（**滚动必然超限**）；上屏只按整行拷 | 滚动每帧全窗口光栅 + 全树 Scene 重建 |

外加两条 v1.1 新增的系统性问题：

> **帧调度没有单一入口**：`frame()` 在单次输入事件下被无条件执行 2–3 次，且每次都跑若干 O(N) 全表扫描。 [1][3]

> **模块边界在执行中回潮**：`RuntimeInner`（14 字段跨 5 域）、`Track`（27 个组件行为方法）、`app.rs`（`WindowCtx` ≈810 行 / 15 项职责）三个点承载了超出其职责的内容。 [4]
> 这是**结构性问题，不需要动核心设计即可收敛**，但它是前四条主线的"阻力来源"——不先收敛，G1–G4 的收益会被持续抵消。

---

## 二、应当固化的资产（不要动）

分析中反复确认的**优秀决策**，比问题更值得记录——改造时不得破坏：

1. **`KindDesc` / `Kind` 双枚举 + desc/state 字段分组**（`track.rs:254-508`）——全仓最干净的设计。
   声明数据在 desc 组、视图态在 state 组，对齐时 `apply_to` 只覆盖 desc 组 ⇒ `dragging`/`caret`/`scroll` 天然保留。
   这是"hover 不丢、滚动不跳、输入框不闪回"的根本保证（`✔✔多源`）。
2. **单线程由类型系统强制**：`Rc<RuntimeInner>` + `RefCell`，零 `unsafe impl Send/Sync`，零 `unsafe`（`#![forbid(unsafe_code)]`）。
3. **`Cmd` 缓冲解决借用冲突**：`collect_route`（只读克隆）→ 释放借用 → 执行。比 `Rc<RefCell>` 互逃优雅。
4. **`HandlerSlot.handled_events_too`**（`event.rs:481-488`）——用数据表达"已处理也要响应"，取代旧 `.builtin()` 排序 hack。
5. **边界重排**（`track.rs:1737-1804`）：脏标记冒泡到"尺寸确定"的节点即停，只重排最上层脏子树。
6. **`next_wakeup` 统一时钟**（`app.rs:766-777`）：spinner/blink/tooltip/timer/动画帧汇总成单一唤醒源，空闲零功耗。软渲染 GUI 的决定性优势。
7. **两段式文本排版**（`lieui-text`）：`measure` 缓存 + `layout` 只跑一次，`TextEngine` 与绘制层彻底分离。
8. **`Root.owner` 嵌套层**（`track.rs:701-714`）：父层消失 ⇒ 嵌套子层级联销毁，删掉了旧的父子句柄图与 `PENDING_POPUP_HIDE` 队列。
   **数据层是对的，只是命中与绘制都没消费它**（见 D6）。
9. **声明式层**：Modal/Popup/Tooltip 都是 `view()` 里声明的根，删掉了旧的层命令队列与父子句柄图。
10. **`#![forbid(unsafe_code)]` 且渲染路径真的做到了**（`raster.rs:612-621` 用 `glyph_errors` 计数替代 `unwrap`）。
11. **像素级回归测试 tradition**（`render/mod.rs:171-178` 的 `px()` 助手、`layout.rs:739` 的 scroll/text_wrap 断言）——
    这是本仓最有价值的工程资产，**残影类缺陷只能靠它发现，逻辑断言无效**。

---

## 三、缺陷总表（统一编号 D1–D56）

严重度定义：**P0** = 用户可见错误（数据破坏级 / 视觉错误级）；**P1** = 挂起 / 状态泄漏 / 语义错乱；
**P2** = 性能（软渲染帧率杀手）；**P3** = 工程化、结构与 API 债务。

### P0 — 用户可见错误

| ID | 问题 | 证据 | 来源 | 核验 |
|---|---|---|---|---|
| **D1** | `Tapped` 不校验按下/抬起的按键配对 | `input.rs:149-162` 只用 `button` 决定合成 `Tapped`/`RightTapped`，不比对按下时记录的按键 | [1][2] | ✔✔多源 |
| **D2** | `destroy` 不登记脏区 | `track.rs:1044-1067`，全程无 `damage_rect`/`damage_bounds`；仅靠"脏区恰好为空⇒整窗"兜底 | [1][2][3] | ✔✔多源 |
| **D3** | 图片绕过裁剪栈 | `raster.rs:480-481` **代码注释自述**"不走 vello ⇒ 不参与 `PushClip` 裁剪栈（滚动容器里的图片不会被裁）" | [1][2][3][4] | ✔已核验（**自述缺陷，非待验**） |
| **D4** | `close_requested` 丢弃全部 Cmd | `app.rs:1273-1276` 建了 `cx`、调了回调，从不 `take_cmds()` | [1][2] | ✔✔多源 |
| ~~**D5**~~ | ~~`hit` 与 `render` 的裁剪坐标系不一致~~ **⇒ 误判已推翻（2026-10-07）** | **实测两侧同语义**：`scene.rs:462` 的 `Op::PushClip { rect: c, transform }` 里 `c` 与 `rect()` 同为**节点本地空间**，且 `transform` 被显式携带；`hit.rs:104-108` 也先把点逆变换到本地再 `clip.contains(q)`。已加契约测试 `hit::d5_clip_space`（2 条，含变异验证）钉住该语义 | ~~[1][2][4]~~ | ❌**已推翻**（原定P0 不成立） |
| **D6** | ~~**Modal 内嵌 Popup / Tooltip 不可见且不可命中**~~ **⇒ 2026-10-07 修正定性** | 核实：`Root.owner` **确实无消费点**（设计 §3.7 的 `z = (Layer, 嵌套深度, 序号)` 未实现）—— 属实；但**现象不可达**（生产代码 `owner` 恒为 `None`，唯一传 `Some(modal)` 的是测试），且**"Modal 盖住 Popup"本身是正确设计**（枚举序 `Popup(2) < Modal(4)`，模态框本就该阻断下层）。**真正成立的是"两份手写层序数组漂移风险"，已修**（`Layer::ALL` 单一权威源） | ~~[2][3][4]~~ | ✅**已完成**（2026-10-07，A 方案：z 序已实现嵌套深度） |
| **D7** | `run()` 之前 spawn 的任务永久静默挂起 | `task.rs:506` 克隆 waker 快照 + `platform/mod.rs:955` 的 `set_waker` 在 `run()` 内 + 本地队列 GUI 模式不 drain（`task.rs:443-448`） | [1][2][3] | ✔✔多源 |
| **D8** | release 下 `view()` 内 `set` 静默自激；`view()` panic 后 `in_view` 永久污染 | `reactive.rs:403-411` 断言被 `cfg!(debug_assertions)` 包裹；`app.rs:639/650` 的 `begin/end_view` 非 RAII | [1][2][3][4] | ✔✔多源 |

> **D6 的严重性高于 v1.0 的判断**：它不只是命中问题 —— **绘制侧同样不消费 `owner`**，所以现象是"模态框里开的下拉/右键菜单**既画不出来也点不到**"。这是 §3.7 与 §3.15 两条设计承诺同时未落地。

### P1 — 泄漏 / 语义错乱

| ID | 问题 | 证据 | 来源 | 核验 |
|---|---|---|---|---|
| **D9** | 定时器回调内 `cancel()` 无效；关窗后成孤儿 | `app.rs:1230-1236`：先出表 → 执行 → **无条件** `reschedule_timer` | [1][2] | ✔✔多源 |
| **D10** | 按下态泄漏 | `input.rs:100-104` 二次 `Down` 覆盖 `pressed_path` 但不清旧链 | [1][2] | ✔已核验（v1.1 由待验转已核验） |
| **D11** | 焦点与浮层零互斥 | `FocusPolicy`（`track.rs:601`）/ `LayerOpts.focus`（`:643`）**全仓零消费点**；`focus.rs:71-88` Tab 链含所有层根；`Cmd::SetFocus`（`cmd.rs:199-216`）不发事件、不校验 `enabled`，与 `WindowCtx::focus` 双语义 | [1][2][3] | ✔✔多源 |
| **D12** | `Layer` 增变体不报错 | `hit.rs:20` 写死 `[Layer; 6]` | [1][2] | ✔✔多源 |
| **D13** | 焦点三重表示需手工同步 | `Track.focused` + `Node.focus_state` + `Node.interaction.focused` | [2][4] | ✔已核验 |
| **D14** | 事件路由双遍历 + 死事件 | `widgets::handle_route`（`widgets/mod.rs:29-51`）与 `event::collect_route`（`event.rs:901-924`）各自解释 `Routing`；`PreviewKeyDown`/`GettingFocus`/`LosingFocus`/`TextCompositionStarted|Ended` **全仓无生产者** | [1][3] | ✔已核验 |
| **D15** | Escape 关闭缺失；轻关闭双实现不同步 | `input.rs:254-272` vs `app.rs:944-1006`（后者有"点击禁用项不关"特判，前者没有） | [3][4] | ✔已核验 |
| **D16** | IME 缺陷组 | `ImePreedit.cursor` 一路丢弃（`widgets/mod.rs:247-249` 只取 `text`）；无 `set_ime_area`（候选窗不跟随光标）；剪贴板 Ctrl+C/X/V 硬编码（`platform/mod.rs:590-625`）而 `Modifiers::META`（`event.rs:114`）已定义零消费 ⇒ Mac 上应为 Cmd | [3] | ✔已核验 |
| **D17** | `Cmd::BringIntoView` 只标脏不滚动 | `cmd.rs:234-249`，注释自认 TODO；API 已暴露、行为缺失 | [3] | ✔已核验 |
| **D18** | `RequestQueue::take::<T>` 类型不匹配静默保留 | `reactive.rs:140-152`，放错类型即永久泄漏且零诊断 | [2][4] | ✔已核验 |
| **D19** | 文本输入能力薄弱 | 键映射仅 6 条（`widgets/mod.rs:438-462`），缺词跳 / 上下行 / PageUp；`caret`/`anchor` 只到字节偏移 | [3] | ✔已核验 |
| **D54** | **`handled` 语义缺口**：内置行为永远先于用户 handler 跑完，且**不读 `handled`** ⇒ 用户 `mark_handled()` 既无法阻止已跑完的内置行为，也无法阻止后续内置行为 | `app.rs:814-849` 的 `WindowCtx::dispatch` 串起两遍；`widgets/mod.rs:29-51` 签名不接收 `handled` | [4] | ✔已核验 · 与分歧 1 同源 |

### P2 — 性能

| ID | 问题 | 证据 | 量级 / 说明 | 来源 | 核验 |
|---|---|---|---|---|---|
| **D20** | **帧重入 2–3 次 / 事件** | `platform/mod.rs:370-407` 的 `tick()` 对每窗口**无条件** `frame()`；四个调用点：`resumed:891`、`user_event:897/904`、`RedrawRequested:722`、`about_to_wait:939`；`window_event:932` 又 `request_redraw` | 一次 Move ⇒ `about_to_wait(#1)` → `Redraw(#2)` → `about_to_wait(#3)` | [1][3] | ✔已核验 |
| **D21** | 脏区退化整窗 | `raster.rs:114-117`：>8 块 或 >45% 面积 | ⚠️ **实测比预估严重得多**（见下方"实测基线"）：50 行滚动一次产生 **302 个碎片**，标脏 30 个节点也退化 ⇒ **不是"滚动场景的问题"，而是任何 ≥9 节点变化的普遍问题** | [1][2][3][5] | ✔✔多源 + **实测** |
| **D22** | 上屏只按整行拷 | `platform/mod.rs:485-497`：`start = row*stride; end = start+stride`，忽略 `r.x`/`r.width` | 40×20 的脏区被展开成 20 × 全窗宽 | [1][2][3] | ✔✔多源 |
| **D23** | 滚动 → 整棵子树重排 + 文本重测 | `track.rs:1232` `set_scroll_offset` → `mark_layout_dirty` → 下一帧重跑 flex；`flex_node.rs:486/650/702` 每叶子测 2–3 次 | 滚动是视图变换，不该触发布局 | [1][3] | ✔已核验 |
| **D24** | FlexNode 每次全量重建 + 字符串深拷贝 | `layout.rs:265-310`（`layout.clone()` + `String`/`TextSpec` clone + 递归 push）；`layout.rs:398-410` 二次测量 | 边界重排限制了**范围**，没限制**成本** | [1][2][3] | ✔✔多源 |
| **D25** | 每帧 O(N) 全表扫描 | `has_layout_dirty`（`app.rs:658` 与 `:679`，**调两次**）、`layout_boundaries`、`clear_layout_flags`、`take_scroll_changes`（`track.rs:1239`）、`next_deadline`（`timer.rs:209`） | 均为全 arena / 全表遍历 | [1][2][3] | ✔✔多源 |
| **D26** | Scene 每帧从零重建 + 文本缓存隐患 | `render/mod.rs:137` / `scene.rs:407`；`TextCache` key **含 color**（`scene.rs:275`）而 `raster.rs:608` 已按 op 颜色着色 ⇒ hover 换色重建排版；满 2048 **整表 clear** | 密集 hover 界面周期性重排风暴 | [1][2][3] | ✔✔多源 |
| **D27** | `mark_all` 全窗口广播 + handler 无条件换血 | `reactive.rs:206-210` 任一 `set` 给所有窗口置 `VIEW`；`align.rs:248-251` **无条件** `handlers.clone()` | 每次交互全树 `Rc`/`Vec` 换血 | [1][2][3] | ✔✔多源 |
| **D28** | `Signal::set` 无同值短路 | `reactive.rs:390-394` | `on_tick` 里无条件 set 同值 ⇒ 永动重建 | [1][2] | ✔已核验（v1.1 转已核验） |
| **D29** | 命中测试 2–4 次全树遍历 / 事件 | Move：`update_hover` + `hit_path_for` 同坐标打两次（`input.rs:90-91`）；Down 再叠 `app.rs:906/928`；`hit::descend` 每节点现场构造并求逆 `Affine`（`hit.rs:93-97`）；`hit.rs:36` 拿到 root 后又线性 `find` 回查；**无 AABB 早退** | | [2][3][4] | ✔✔多源 |
| **D30** | 每个事件多次堆分配 | `InputStep.events`（`input.rs:77`）、每次命中新建 Vec、`collect_route` 新建 `Vec<HandlerSlot>`、每事件新建 `CmdBuf`；IME `Commit` **每字符**一次完整 dispatch（`platform/mod.rs:838-843`） | 中文长句提交放大 N 倍 | [3] | ✔已核验 |
| **D31** | 每节点 `TextSpec.font_family: String` | `track.rs:747` + `lieui-text/src/spec.rs:37`；`lieui-text/src/lib.rs:207-224` 缓存命中路径也要 `to_owned()` 构造键 | | [1][2][4] | ✔已核验 |
| **D32** | 脏区算三遍、批次算两遍 | `render/mod.rs:121-125`、`raster.rs:363`、`platform/mod.rs:182-193` | 每帧 ≥4 个临时 Vec | [1][2][4] | ✔已核验（v1.1 转已核验） |
| **D33** | 阴影模糊超出脏区 | `widgets/mod.rs:633-636` vs `track.rs:1717-1730` | 光晕外圈不在脏区 ⇒ 残影 | [1][2] | ✔已核验（v1.1 转已核验） |
| **D34** | 任务线程与背压 | `task.rs:546` 每任务一条 OS 线程无池；`progress` 每次一个 OS 事件（`:180-190`）；`TaskHandle` 无 `Drop`（`:237-242`）；`CancelToken` SeqCst | | [3] | ✔已核验 |
| **D35** | `align_keyed_children` O(n²) | `align.rs:330-334` `pool.iter().position(...)` | | [2] | ✔已核验 |
| **D36** | 层 opts 任何变化 ⇒ 整窗脏 | `align.rs:63-69` 含纯交互的 `dismiss_on_outside_click` | | [2] | ✔已核验 |
| **D37** | 无像素 snapping + 浮点容差分散 | `float_is_equal` 1e-4（`types.rs:227`）、`rect_eq` 1e-3（`layout.rs:424`）、滚动钳制 1e-4、锚点 1e-2 | 1px 分隔线与文本持续半像素模糊 | [3] | ⚠️**部分修正**（2026-10-07）：四套容差其实是**三种不同语义**——1e-6 精确比较（DPI scale，两处重复实现）、1e-3 视觉等价（`rect_eq`/`transform.rs`，**合理**）、1e-4 滚动偏移精确钳制。**真正的问题只有"无 snapping"与"1e-6 重复两处"，统一容差常量属可读性而非正确性** |
| **D38** | 渲染后端无抽象 + 无多线程 | `render/` 直连 `vello_cpu`+`softbuffer`，无 `RenderBackend` trait；`vello_cpu 0.3` 无 rayon 依赖 | 换 wgpu 要重写呼叫点 | [3] | ✔已核验 |

### P3 — 工程化、结构与 API 债务

| ID | 问题 | 证据 | 来源 |
|---|---|---|---|
| **D39** | 零 CI、零 lint 门禁 | `Cargo.toml:46-48` 无 `[lints]`；无 `.github/`、`rust-toolchain.toml`、`rustfmt.toml`、`deny.toml` | [1][3] |
| **D40** | doc 示例 100% `ignore` | 全仓非 ignore 块为 0 ⇒ API 签名漂移不会被 `cargo test` 发现 | [1] |
| **D41** | 无集成测试 | 主 crate 无 `tests/`，没有"外部用户视角"的 API 契约测试 | [1][4] |
| **D42** | **`lieui-layout` 测试裸奔** | `tests/smoke.rs` 仅 4 条；shrink 冻结循环 / wrap 多行 / absolute / RTL / min-max clamp / gap 零覆盖 —— **算法最复杂、测试最少**的模块 | [1][2][3] |
| **D43** | 死代码 / 死 API | `measurable.rs`(138) + `constraint.rs`(4) 主 crate 零引用；`ImageStyle`/`ImageFit`（`style.rs:226-285`，绘制不读，`gallery.rs:460` 注释"contain 缩放"与实现矛盾）；`line_space` 死配置（`style.rs:50`）；`RoutePlan.target`；8 个不可达 `Cmd` 变体 | [1][2][3] |
| **D44** | 布局对样式有持久副作用 | `flex_node.rs:257-261` 改写 `style.flex_basis` 不 restore；`box_model.rs:42-49` `tight_width` 的 `min_width = max_width = f32::MAX` 疑似笔误 | [2][3] |
| **D45** | `DescRef` 一次性借用 + 容器返回 `()` | `view.rs:262` vs `:282`；用户被迫写 4 行样板（`gallery.rs:151-165`），并放大 `view.rs:1331` 的 `panic!("icon 应是 Text")` | [1][4] |
| **D46** | 内置行为分支几乎零测试 | `widgets/mod.rs` 的 `handle` 6 分支中 4 个（Slider/Checkbox/Switch/Radio）无测试，"未绑定就不改模型"这条反直觉规则没钉住 | [1][2] |
| **D47** | 封装纪律不一致 | `Node` 全字段 pub（`track.rs:729-783`，含 `flags/desired/computed` 内部产物），而 `DescNode` 是 `pub(crate)`；`WindowCtx` 公开暴露 `vello_cpu::Pixmap`（`app.rs:519`） | [2][4] |
| **D48** | 布局能力断层 | 无百分比；交叉轴 gap 不存在；`align-content: SpaceEvenly` 静默退化 0（`flex_node.rs:800-816`）；`Baseline` 三条对齐路径全被吞；默认 `flex_direction = Column`（CSS 是 row）；min/max 负值丢弃；`types.rs` 里 `[L,T,R,B]` 与 `K_AXIS_*`（`[L,R,T,B]`）两套轴序混用；**`flex_node.rs:603` 的 `while !resolve_flexible_lengths(){}` 无迭代上限**（NaN/震荡挂死主线程） | [1][2][3] |
| **D49** | `Kind` 三份平行枚举 + 三处分派 | `track.rs:207 KindTag` / `:256 KindDesc` / `:409 Kind`，`tag()` 重复两遍（`:276-291` 与 `:459-474`）；行为分派 `widgets/mod.rs:53-91`，绘制 `:602-1028` | [1][2][3][4] |
| **D50** | God file | `app.rs` **4816** 行（实现 1448 + 测试 3368）；`widgets/mod.rs` 1886；`raster.rs` 1074；`track.rs` 1912。`WindowCtx` ≈810 行 / ≈15 项职责 | [1][2][3][4] |
| **D51** | **`RuntimeInner` 是"运行时杂物抽屉"** | `reactive.rs:87-118`：**14 个字段跨 5 个域** —— 脏标志(`windows`/`in_view`) / 请求(`requests`) / 主题(`theme`/`theme_mode`/`system_dark`) / 任务(`waker`/`tasks`/`busy`/`next_task_id`/`busy_min_visible`) / 时钟(`timers`/`animating`) / 窗口尺寸(`window_sizes`)。`Signal` 只需要脏标志表，却被迫持有整个 `Rc<RuntimeInner>` | [4] |
| **D52** | **组件行为方法回归 `Track`** | `track.rs` 里 **27 个** `pub fn` 行为方法（`slider_drag_to`/`toggle_checked`/`input_*`/`scroll_*` 等），本质是组件状态机却挂在保留树上；设计 §3.2 原本约定它们应在 `widgets/`（`fn slider_handle(track, id, ev, cmd)`）。`track.rs` 1850 行起是测试 ⇒ 实现部分约一半是 Input 编辑与控件逻辑 | [4] |
| **D53** | **几何语义分散 4 处，一致性靠约定** | 同一份 `transform`+`clip` 语义分别在 `hit.rs:descend`（逆矩阵+contains）、`track.rs:damage_bounds`（`bounding_box`）、`scene.rs`（组合变换生成 op）、`raster.rs`（`shift*kaffine`+`push_clip`）实现。任一处对 `origin`/`scale=0`/`clip` 判据不一致 ⇒ "命中对、绘制错"或反之。**这是 D5 的根因** | [4] |
| **D55** | **测试与实现同文件导致单文件爆炸** | `#[cfg(test)]` 起点：`app.rs:1450`（测试占 70%）、`view.rs:1314`（占 92%）、`track.rs:1850`、`raster.rs:653`、`layout.rs:441` | [4] |
| **D56** | `ViewBuf` 转发糖已到该抽宏的点 | `view.rs:908-960` 十余个转发方法，注释自陈"若继续膨胀再抽 macro" | [4] |

### 补充遗漏（v1.1 新增，四份审计各自的盲区）

| ID | 问题 | 证据 | 后果 |
|---|---|---|---|
| **D57** | **`damage_bounds` 祖先链缺失时 `?` 静默放弃整个脏区登记** | `track.rs:1720-1730` 的 `?` 运算符 | 脏区丢失 ⇒ 残影。**D2 的同类问题，应合并处理** |
| **D58** | **`clear_layout_flags` 无条件清全树 MEASURE/ARRANGE** | `track.rs:1777-1782`，`layout.rs:105/243` 调用 | 布局过程中新提的脏标被吞 ⇒ 漏失效 |
| **D59** | **`window_sizes` 永不清理** | `reactive.rs:117/309-315` 只有 push/find 无 remove；`unregister_window` 不删 | 每次开关窗泄漏一条 |
| **D60** | 宿主层唯一 panic，与 `run() -> Result` 矛盾 | `platform/mod.rs:330` `.expect("softbuffer context")` | 无显示环境/驱动异常时整个应用 abort |
| **D61** | Wheel 绕过指针捕获 | `input.rs:186` `let _ = pointer;`（Wheel 走 `hit_path` 而非 `hit_path_for`） | 拖拽中滚轮行为与其它事件语义不一致 |
| **D62** | spinner 相位用 `SystemTime`（非单调时钟） | `overlay.rs:146-149` | NTP 回拨时 spinner 跳帧 |

---

## 四、主线改造（A–I）

### 主线 A：把"退化路径"钉死（P0，约 150 行）

全部是"小改动 + 真实错误"，且都有现成测试助手可写断言（`render/mod.rs:171-178` 的 `px()`）。

> **执行顺序要求**：`A2` 必须**第一个做**。它会改变所有后续改动的测试基线 —— 在它之前写的任何像素测试，都可能把"残影"误判为"预期结果"。

**A1 —— 输入状态机（`D1` `D10`）**：`pressed` 从 `Option<NodeId>` 升级为完整按下态：

```rust
pub struct PressState {
    pub node: NodeId,
    pub pointer: PointerId,
    pub button: PointerButton,   // ★ 抬起时校验配对
    pub pos: Point,
    pub at: Instant,             // ★ 长按阈值（见下方"分两步"）
}
```

> ⚠️ **工作量修正**：`input.rs:85` 的 `step(&mut Track, InputEvent) -> InputStep` 是**纯函数、无时钟来源**，加时间阈值要改签名并穿透全部调用点。
> **建议分两步**：① 先只做**按键配对**（不需要时钟，堵住数据破坏级缺陷）；② 位移/时长阈值留到有测试支撑时再做。
> 顺带修 `D10`：二次 `Down` 时若已有 `pressed`，先把旧链按 `Up` 的方式清干净。

**A2 —— `destroy` 登记旧矩形（`D2` + `D57`）**：破坏前先登记绘制范围；并把 `damage_bounds` 的 `?` 改为逐段累加（祖先缺失时不应放弃整条链）：

```rust
pub fn destroy(&mut self, id: NodeId) -> usize {
    let ids = self.descendants(id);
    // ★ 破坏前登记旧绘制范围
    for i in &ids {
        if let Some(r) = self.damage_bounds(*i) { self.damage_rect(r); }
    }
    let parent = self.parent_of(id);
    self.detach(id);
    // …（其余不变）
}
```

**A3 —— `close_requested` 补 `apply_cmds`（`D4`）**：`app.rs:1275` 之后三行即可。

**A4 —— 响应式断言 RAII + always-on（`D8`）**：

```rust
pub(crate) struct ViewGuard<'a>(&'a Cell<Option<WindowId>>);
impl Drop for ViewGuard<'_> { fn drop(&mut self) { self.0.set(None) } }
```

`begin_view/end_view` 改为返回/持有守卫，`?`/panic 都能正确恢复；断言去掉 `cfg!(debug_assertions)`（成本只是读一个 `Cell`，
换来 release 下也能 fail-fast 而不是静默满帧自激）。

**A5 —— 定时器取消（`D9`）**：`reschedule_timer` 前检查取消标记；关窗时清理该窗口定时器。

**A6 —— 任务唤醒器（`D7`）**：post 时查询**当前** waker slot（而非 spawn 时的快照），或 Platform 模式下也 drain 本地队列。
至少加一条诊断：无 waker 时返回错误而非静默入队。

**A7 —— 杂项 correctness（`D33` `D58` `D59` `D61` `D62`）**：
阴影模糊半径纳入 `paint_bounds`；`clear_layout_flags` 改为"清除本轮消费的标记"（或在布局后重新扫描补标）；
`unregister_window` 清理 `window_sizes`；Wheel 走 `hit_path_for`；spinner 相位改用 `Instant` 基准。

> **配套要求**：A2 / D3 / D33 / A7 各加一个**像素级回归测试** —— 断言删除/patch/图片/阴影后目标区域像素等于底色。
> **残影类缺陷只能靠像素测试发现，逻辑断言无效。**

### 主线 B：帧调度收敛到单一入口（P0，`D20`）

**根因**：`about_to_wait` / `user_event` 里的 `tick` 是"怕漏"的兜底，与 `RedrawRequested` 重复；`tick()` 不看脏标志。

**⚠️ v1.1 关键补充：必须同时补上"重绘请求闭环"，否则改完比改前更糟。**

当前 `tick()` 在跑 `frame` 的过程中顺带执行定时器回调。若只把 `frame` 移到 `RedrawRequested`，
则回调**照样执行但没人请求重绘** —— `mark` 只置脏标志，触发点被移走 ⇒
**定时器回调的效果要等到下一次真实输入才可见（动画/loading 进度/spinner 全部停在旧帧）**。

正确改法（两个半部分缺一不可）：

```rust
// platform/mod.rs
fn about_to_wait(&mut self, el) {
    self.drain_requests(el);          // 不再 frame
    self.schedule_next_wakeup(el);
}
fn user_event(..) { /* 处理后 */ if dirty { ws.window.request_redraw(); } }
fn window_event(..) { /* 处理后 */ if dirty { ws.window.request_redraw(); } }
// RedrawRequested：唯一的 tick + frame 入口
```

```rust
// ★ 闭环：tick() 内消费到定时器/动画时，必须主动请求重绘
fn tick(&mut self, el, now) {
    for w in 0..self.n {
        let due = self.app.window_ctx(w).map(|c| c.tick(&rt, now));  
        if due.timers_fired || due.animation_consumed {
            self.windows[w].window.request_redraw();   // ← 缺这一环就会停帧
        }
    }
}
```

`WindowCtx::tick` 应返回"本轮是否消费了定时器/动画帧"的标记（替代现在的 `()`），平台层据此决定是否 redraw。
这一改动也顺带解决"timers 被 frame 次数放大"的问题（不再需要额外的 `last_frame` 闸门）。

**验收**：`examples/damage_bench.rs` 加计数器，单次 `InputEvent::Move` 下 `frame()` 调用次数 **3 → 1**；
且**新增一条断言：设一个 50ms 定时器，在无输入的情况下验证画面确实更新**（防停帧回归）。现有 389 测试全绿。

### 主线 C：让"局部"真的局部（P2，性能杠杆最大）

**C0（先做，量化决策）** —— 把 `damage_bench.rs:194-195` 的 `fragments`/`union`/`bands` 三策略从"打印数字"
改成**断言式基准**：给定固定滚动场景（N=500 行、滚动 dy=40），输出 `(batch 数, 覆盖像素/窗口像素, 重叠像素数)`。
**用数据选定默认策略，再写 `damage_batches` 的改动。** 顺序不能反。

**C1（5 行，收益立竿见影，`D22`）** —— 修 `platform/mod.rs:485-497` 为真正的矩形拷贝：

```rust
for r in &batches {
    let x0 = (r.x.max(0.0) as usize).min(pw);
    let w  = ((r.width.get() as usize).min(pw - x0)).max(1);
    let y0 = r.y.max(0.0) as usize;
    let h  = (r.height.get() as usize).min((ph as usize).saturating_sub(y0));
    for row in y0..y0 + h {
        let s = row * stride + x0;
        let (a, b) = (s, (s + w).min(src.len()).min(dst.len()));
        if a < b { pack_xrgb(&src[a..b], &mut dst[a..b]); }
    }
}
```

**C2（算法级，`D21`）** —— 在 `damage_batches`（`raster.rs:83-119`）判定退化**之前**先做矩形合并：
相交或间距 < GAP 的矩形求并。不硬编码 45% / 8 两个魔数。

> **✅ C0 已完成，实测基线如下**（`tests/perf_regression.rs`，1280×720，`--nocapture` 可复现）：
>
> | 场景 | 脏区碎片 | 批次 | 光栅像素 |
> |---|---|---|---|
> | 50 行滚动一次（dy=20） | **302** | 1 | 921600 = **整窗** |
> | 标脏 30 个节点 | **30** | 1 | 921600 = **整窗** |
> | 标脏 1 个按钮（深埋） | 1 | 1 | 远小于 2% 窗口 ✅ |
> | 空闲帧 | 0 | 0 | 0，`rasterized == false` ✅ |
>
> **这把 D21 从"滚动场景优化"重新定性为"普遍失效"**：碎片阈值 8 意味着
> **任何 ≥9 节点变化都退化成整窗**，而"标脏 30 个节点"这种极常见的批量状态变更就已经踩中。
> 另：`damage_bench.rs` 的 `Page` 用的是 `v.column` 而**没有滚动容器**，
> 所以那份基准其实从未测过"滚动脏区"这条最关键的路径 —— 新测试已补上。
>
> C2 的策略选择应以"302 个碎片合并后剩几个"为准；两条基线测试里留了 `println!`，改完直接对比数字。

**C3（`D3`，约 40 行）** —— 图片走 clip 栈：op 流里已有 `PushClip`/`PopClip`，在 `rasterize` 的 op 循环
（`raster.rs:396-420`）维护一个软件 clip 栈，`blit_image` 接收当前 clip 并对 dst 求交。
建议同时把最近邻换为双线性（当前每像素 12 次整数运算，缩放图片质量明显）。

**C4（结构性，`D26`）** —— display list 缓存：按 `(NodeId, 内容版本)` 缓存每节点的 op 段，节点未变则整段复用；
失效条件为"节点自身或其任一祖先重排"。
> **排序说明**：C4 放在主线 D 之后**更便宜** —— `D-b`（滚动脱离布局）落地后，display list 的失效判定会从
> "祖先 layout dirty"退化为"子树整体平移"，**反而更简单**。
> ⚠️ 但**先做 D26 的独立小修**：`TextCache` key 去掉 color（`raster.rs:608` 已按 op 颜色着色）+ 容量改 LRU
> 替代整表 `clear`。这是 10 行、两个数量级便宜的收益，应排在 C4 之前。

### 主线 D：布局与滚动（P2，改动最大的一项）

**D-a（`D24`，风险低，先做）** —— FlexNode 持久化：`Node.flex_cache` 持有持久子树，`build()` 只在 patch 时更新对应节点；
`desired_size` 直接复用 flex 已测结果（消掉 `layout.rs:398-410` 的第二次测量）；`measure_text` 改 `Option<(Rc<str>, TextSpec)>` 消除 String 拷贝。
同步修 `D44`：`flex_basis` 补 save/restore，`tight_width` 的 `f32::MAX` 笔误。

**D-b（`D23`，风险高，单独冲刺）** —— 滚动脱离布局：偏移改为**子树的绘制/命中平移**（`transform.rs` 已有 `Affine` 逆变换基础设施），
不再 `mark_layout_dirty`；FlexNode 树持久缓存 + 按脏样式 patch。

> **风险**：
> ① 触碰 `layout.rs:360-384` 的 `write_back` 与 `text_wrap` 一致性（`layout.rs:739` 有像素级回归测试钉着）。
> ② **[v1.1 新增]** `virtual_list`（`view.rs:626-695`）的窗口起点依赖 `ScrollChanged` 信号，而该信号是
> **layout 之后**补派的（`app.rs:668-675`）。滚动脱离布局后，"滚动位置 → 哪几行可见"的时序要重新设计。
>
> **建议**：先写失败测试再动实现；验收里加一条「`virtual_list` 滚动到中部后，行内容与纯滚动方案逐像素比对」。

**D-c（`D48` `D37`，可与 D-a 并行，互不影响）** —— `flex_node.rs:603` 加 `MAX_ITER` + 越界回落
（**最高优先，会挂死 UI 主线程**）；统一浮点容差为单一常量并在布局出口做 pixel snapping；统一轴序；
补百分比与交叉轴 gap 需先定语义（见 §六决策 3）。

### 主线 E：事件与浮层语义（P0/P1）

**E1 —— `handled` 语义显式化（`D54`）** ★ *v1.1 采用 audit-3 的折中，替代 v1.0 的"三级写入契约 + 文档固化"*：

不追求"Cmd 是唯一通道"（设计 §3.5 已诚实记录过为什么滑块拖拽/文本编辑绕道加 `Cmd` 很别扭），
而是**让内置行为共享同一个 `handled` 状态**：

```rust
// widgets/mod.rs
pub fn handle_route(track: &mut Track, plan: &RoutePlan, handled: &mut bool) { ... }
```

内置行为改完视图态后**可选置 `handled`**，用户 handler 的 `mark_handled()` 也能阻止后续内置行为。
零额外分配、零架构改动，同时把"内置行为无视 handled"这个隐式语义**显式化**。

> **建议加一条测试钉住**："用户 handler 不拿 `&mut Track`"（守住借用纪律这条不变量）。

**E2 —— 浮层与焦点（`D6` `D11` `D12` `D13` `D15`）**：
1. **修 D6**（跨 `hit.rs` + `scene.rs` + `track.rs` 三处）：命中与绘制**两侧同时**改为 `z = (Layer, 嵌套深度, 序号)`，
   对齐 §3.7 / §3.15。**两侧必须同改，否则出现"画对了点不对"**。若判定成本过高，退化方案是给 `LayerOpts` 加显式 `z_index`。
2. **`FocusScope`**：弹层挂载时按 policy 保存旧焦点 / 夺取焦点 / 阻断下层 Tab；`Cmd::SetFocus` 与 `WindowCtx::focus`
   合并为单一入口（当前一个发事件一个不发，是最难查的一类不一致）；焦点收敛为单一 set API（`D13`）。
3. **`Layer` 增变体的编译期保护**（`D12`）：`LAYER_TOP_DOWN` 改为由 `Layer` 的常量数组派生，或 `debug_assert` 覆盖全部变体。
4. **Escape 关闭 + 轻关闭规则统一**（`D15`）：把 `input.rs:254-272` 与 `app.rs:944-1006` 合并为一套判定。
5. **层 opts 拆"影响绘制"与"纯交互"两类**（`D36`），只有前者整窗脏。

**E3 —— 事件路由合并（`D14`）**：`widgets::handle_route` 与 `event::collect_route` 合并为单次 collect；
死事件（`PreviewKeyDown`/`GettingFocus`/`LosingFocus`/`TextCompositionStarted|Ended`）**要么实现要么从 `EventKind` 删除** ——
留着是误导 API。

### 主线 F：响应式"防抖"而非"追踪"（P2）

文档 §七 明确不做细粒度响应式 —— 这是**正确取舍，不推翻**。三个低成本改进压住最坏情况：

**F1（`D28`）** —— 同值短路（需 `T: PartialEq`）：

```rust
impl<T: PartialEq + 'static> Signal<T> {
    pub fn set(&self, value: T) {
        self.assert_not_in_view();
        {
            let mut slot = self.inner.value.borrow_mut();
            if *slot == value { return; }
            *slot = value;
        }
        self.rt.mark_all(Dirty::VIEW);
    }
}
```

> **代价评估**：`Signal<T>` 的 `get()` 本就要求 `Clone`，实际用途（count/bool/String/enum/Vec）几乎都有 `PartialEq`。
> **接受 impl 面收窄**。原 `impl<T: Clone>` 块保留给 `get()`，新增的 `impl<T: PartialEq + Clone>` 块承载 `set`/`update`/`take`。

**F2（`D27`）** —— 窗口级订阅：`Signal.seen_by: SmallVec<WindowId>`，`get()` 时登记 `rt.in_view`，
`set()` 时定向 `mark(id)`；未登记过的保守回退到广播。设计 §3.14 已给出这条路径，**不引入观察者图**。

> ⚠️ **成本修正**：v1.0 估"40 行"偏低。实际要覆盖 `get`/`with`/`take`/`set`/`update` 五个入口 + `Clone`/`Debug` impl
> + `seen_by` 清理（窗口关闭时），约 80–100 行。
> **风险很低**：`set` 在 `view()` 期间被断言禁止（`reactive.rs:403-411`），不会出现两个窗口同时登记同一 signal 的并发写。
> 排在 F1 之后（零语义变化、收益立刻可见）。

**F3（`D27`）** —— handler 换血守卫：`align.rs:248-251` 替换前先比 `Rc` 指针/长度，不变则跳过（一行守卫消掉最大分配源）。

### 主线 G：模块边界收敛（P3，v1.1 新增主线，吸收 audit-3）

> 这条主线**不改行为**，但它是 D25/D27/D29/D31 的"阻力来源" —— 不先收敛，前面那些优化的收益会被持续抵消。
> 前提：**H1/H2 已完成且 clippy 清零**（见主线 I）。

**G1（`D51`）—— `RuntimeInner` 按域拆分**：

```rust
// Signal 只需要脏标志表，就只持它
pub struct Signal<T> { inner: Rc<SignalInner<T>>, dirty: Rc<DirtyTable> }
// App 侧组合
pub struct Runtime { dirty: Rc<DirtyTable>, theme: ThemeState,
                     tasks: TaskState, timers: TimerState, requests: RequestQueue }
```

按域拆成 `ThemeState` / `TaskState` / `TimerState`，`Signal` 只持 `Rc<DirtyTable>`（而非 `Rc<RuntimeInner>` 全家桶）。
**收益**：`Signal` 的语义耦合从"整个运行时"缩到"一张脏标志表"；各域可独立单测；`Signal` 克隆从"全家桶 +1"变"一张表 +1"。
**注意**：`Signal` 仍需访问 `in_view`（F2 的登记点），`DirtyTable` 应把 `windows` + `in_view` 一起装下。

**G2（`D52`）—— 组件行为方法归位 `widgets/`**：把 27 个 `pub fn` 行为方法
（`slider_drag_to`/`toggle_checked`/`select_radio`/`input_*`/`scroll_*`）从 `impl Track` 移到
`widgets/{input,controls,scrollbar}.rs` 的自由函数（接收 `&mut Track`）。
`Track` 只保留树 / 脏区 / 视图态三类原语。**这是回归设计 §3.2 的原始约定**，比"文件太大"更本质。
顺带把 `Input` 的编辑态收敛为 `TextInputState`，解决"spacer 节点也背编辑态字段"的问题。

**G3（`D53` + `D5`）—— 几何求值收敛为单一纯函数**：

```rust
// Track 上的单一真源，hit / damage / scene / raster 四处共用
pub fn visual_xform(&self, id: NodeId) -> VisualXform;  // { affine: Affine, clip: Option<Rect> }
```

任一处对 `origin`/`scale=0`/`clip` 的判据不一致，就会产生"命中对、绘制错"或反之（**D5 就是这个根因**）。
收敛后 `D29` 的"每节点现场构造并求逆 Affine"也随之消失（可缓存 `visual_xform`）。

> **2026-10-07 修正**：本条原写"收敛后 D5 一并修复" —— 实测 **D5 是误判**（`hit` 与 `render`
> 的 clip 本就在同一空间，见 §三 D5 行与 `hit::d5_clip_space` 契约测试）。
> G3 的价值因此从"修 P0"降为"消除 4 处重复实现 + 保住 D29 的优化空间"，**不再是阻塞项**。

**G4（`D50` `D55`）—— 拆 god file + 测试外置**：
- `app.rs` → `app/{pipeline.rs, sessions/{tooltip,ctx_menu,busy}.rs, input_glue.rs, view_model.rs}`。
  依据：`Sessions`（`app.rs:444-462`）已把"框架自管会话"聚合成 struct，**方向正确，只是没拆到模块边界**。
  **测试随模块走**（`#[cfg(test)] mod tests;` + `app/tests.rs`），否则拆完文件仍然不可读。
- `widgets/mod.rs` → `widgets/{basic, input, scrollbar, text_draw}.rs`。
- `track.rs` → `track/{text_edit, damage}.rs`。
- 顺带：`layout.rs` 测试占 49%、`raster.rs` 占 61%、`view.rs` 占 92%，一并外置。

**G5（`D49`）** —— `KindTag`/`KindDesc`/`Kind`/`apply_to`/`from_desc`/`tag()` 用 derive 宏从单一定义生成；
desc/state 分组写成属性标注，**把"desc/state 分组"这个核心约定编码进宏**而不是靠注释纪律。预计消 200+ 行样板。

**G6（`D56`）** —— `ViewBuf` 的 10+ 个转发糖（`view.rs:908-960`）抽 `macro_rules!`。

**G7（`D43`）** —— 删死代码：`measurable.rs` + `constraint.rs`（零引用）、`ImageStyle`/`ImageFit`
（或补齐实现并修正 `gallery.rs:460` 的错误注释）、8 个不可达 `Cmd` 变体、`line_space`、`RoutePlan.target`。
`damage_batches_union/bands` 移到 `examples/` 或 bench 目录，避免在 `src/` 里腐烂（[4]§9）。

**G8（`D47`）** —— 封装收敛：`Node` 字段改 getter（`flags`/`desired`/`computed` 等内部产物不该全 pub）；
`WindowCtx::pixmap()` 返回自有类型或 `&[u8]`，不让 `vello_cpu::Pixmap` 穿透 API。

### 主线 H：能力补齐（P2，可与主线 A–G 并行）

`D16`（IME 光标 + `set_ime_area` + META 修饰键）、`D17`（`BringIntoView` 实现）、
`D19`（键映射扩展、caret 超出字节偏移）、`D15`（Escape，E2 已含）、`D37`/`D48`（布局能力，D-c 已含）。

---

## 五、推进路线

| 批次 | 内容 | 预估 | 验收标准 | 风险 |
|---|---|---|---|---|
| **S0 · 门禁与测试地基**（必须先于 S3/S4） | H1 lint 门禁 + H2 toolchain → **单独跑一次全量 clippy 并清零**；主线 I 全量；`lieui-layout` 特征矩阵；像素回归助手扩到"删除/阴影/图片"三例；`tests/api_contract.rs` | 1~2 天 | CI 绿；`lieui-layout` 测试 4 → ≥20；clippy 零警告 | 低 |
| **S1 · 正确性（P0）** | **A2 先做**（重建测试基线）→ A1（按键配对）→ A3 → A4 → A5 → A6 → A7 | ~150 行 | 每项一个新回归测试；其余 389 测试全绿 | 低：均为局部改动 |
| **S2 · 帧与脏区** | 主线 B（**含 redraw 闭环**）→ C0 基准 → C1 → C2 → C3 → F1/F3 | ~200 行 | 单次 Move 的 `frame()` 3→1；**定时器在无输入下确实更新画面**；滚动时 `batches` 满足 C0 基准的量化目标 | 中：帧调度是唯一会引入新故障的改动，**建议单独 spike** |
| **S3 · 布局与滚动** | D-a（FlexNode 持久化 + 消双测）→ D-c（MAX_ITER / snapping / 轴序）→ D-b（滚动脱离布局，单独冲刺） | 大 | `damage_bench` 与 `layout.rs:739` 像素回归全绿；**滚动不触发 `mark_layout_dirty`**；`virtual_list` 滚动中部逐像素比对通过 | **高**（仅 D-b） |
| **S4 · 语义与能力** | E1（`handled`）→ E2-2/3/4/5 → E3 → 主线 H | 中 | 各补测试 | 中 |
| **S5 · 模块边界** | G1（`RuntimeInner` 拆分）→ G2（行为归位）→ G4 → G5/G6/G7/G8 → G3（几何收敛，**已降级为"持续"**，见 §三 D5 修正） | 大 | `cargo test` + `clippy` 绿 | 低（机械重构） |
| **持续** | C4 display list（**必须在 D-b 之后**） | 大 | 稳态帧 `scene.ops` 显著下降 | 中 |

> **批次间的硬约束**：
> ① **S0 必须先于 S3/S4/S5** —— 没有 clippy 门禁 + 测试矩阵，4 个 god file 的搬移 diff 会淹没真正的行为变更。
> ② **A2 必须先于其它任何像素测试** —— 它改变残影的基线。
> ③ **主线 B 建议先做 spike** —— 它是唯一"改法不完整就会引入新故障"的主线（停帧）。
> ④ **G3 必须先于（或同批于）任何依赖几何一致性的改动** —— 否则"命中对、绘制错"的漂移会持续复发。

---

## 六、分歧与待决策点

**分歧 1：`Cmd` 是否应为唯一写入通道？**（v1.1 已给出折中方案）
- audit-1 主张：把 `widgets::handle_route`、`input::step`、overlay 的 `remove_root` 改为向 `Ctx` 提交 Cmd，
  换来"分发期树只读"这条可推理的不变量。
- **反对依据**（设计 §3.5 的 M5 实现修正已解释过）：`Handler = Rc<dyn Fn(&mut Ctx)>` 拿不到保留树，
  而滑块拖拽 / 文本编辑必须算几何、改视图态，绕道加 `Cmd` 变体很别扭。
- **v1.0 折中**：不改架构，用文档 + 断言固化三级写入契约（L0 立即 / L1 `CmdBuf` / L2 `RequestQueue`）。
- **v1.1 折中（采纳 audit-3 §6 的具体化）**：让内置行为**共享 `handled` 状态**（E1）。
  这同样消除了"顺序不可推断"的核心痛点，但**不付 Vec 分配的代价、也不要求内置行为改写架构**。
- **裁决建议**：采纳 E1。原来的三级契约仍写进文档，但降级为"说明"而非"改造项"。

**分歧 2：同值短路 vs 窗口级订阅的顺序** — 已解决：两者可叠加。先做 F1（零语义变化、收益立刻可见），再做 F2。

**分歧 3："加一个控件要改几处"** — 统一口径为 **≥4 处**（`KindTag` / `KindDesc` / `Kind` / 行为分派 / 绘制分派），
改造目标为 1 处（G5）。

**分歧 4：滚动问题 —— 持久化 FlexNode 还是脱离布局？** — 两者不冲突且互补：
持久化是低风险先手（D-a），平移是高风险高收益（D-b）。按 S3 顺序都做。

**分歧 5：测试基线数字** — 已复核并统一（见附录 A）。`#[test]` = **389**，以实测为准。

**分歧 6：`Signal` 收窄 vs 拆分 `RuntimeInner`？**（v1.1 新增，audit-3 §3 提出）
- audit-3 主张 `RuntimeInner` 14 字段跨 5 域是"运行时杂物抽屉"，应按域拆，`Signal` 只持 `Rc<DirtyTable>`。
- **代价评估**：改动面比看起来大 —— `Ctx`、`ViewModel`、`TaskCtx`、各窗口 ctx 都持有 `Rc<Runtime>`，
  拆成 4 个 `Rc` 后 `App` 侧要组合，且 `Ctx` 上现有的 `set_theme` / `spawn_task` / `set_timeout` 等转发方法需重新指向。
  估约 300 行改动 + 一次全量回归。
- **裁决建议**：**G1 排在中优先级（S5）而非 P0/P1**。理由：它是纯结构收敛，不改行为，
  而主线 A/B/C 才是当前帧率与正确性的瓶颈。若 S2 完成后帧率达标，G1 可降级为可选清理。

**还需你决策的三件事**：
1. **D6 的修法定向**：让命中 + 绘制两侧都消费 `Root.owner` 嵌套深度（对齐 §3.7/§3.15），
   还是加显式 `z_index`（更简单但放弃嵌套语义），还是反过来收缩设计文档承诺（"Modal 内不放 Popup"）？
   **倾向前者**，因为两条设计承诺都已写明且有测试钉住了 `owner` 的父子关系。
2. **D17 `BringIntoView` 与 D15 Escape**：补实现（倾向）还是先从公开 API 撤下？
   倾向补实现 —— Escape 是菜单/弹层的基线能力，两者都在 20 行内。
3. **D48 的百分比 / Baseline / SpaceEvenly**：补齐（提升 CSS 兼容度）还是写入 §七"不做什么"清单？
   倾向：百分比补齐（高频需求），Baseline/SpaceEvenly 写入"不做"（语义微妙、当前无真实需求）。

---

## 附录 A：实测基线（可复核）

```
统计命令：Get-ChildItem -Recurse -Include *.rs src,crates
          | ForEach-Object { (Get-Content $_ | Measure-Object -Line).Lines }
```

| 文件 | 总行数 | `#[cfg(test)]` 起点 | 实现占比 | 备注 |
|---|---|---|---|---|
| `src/app.rs` | **4816** | 1450 | 30% | ⚠️ 三份审计均误记为 5385/5386 |
| `src/track.rs` | 1912 | 1850 | 3% | ⚠️ 误记为 2114；实现部分约 57% 是组件行为（`D52`） |
| `src/view.rs` | 1425 | 1314 | 8% | 测试占比最高 |
| `src/widgets/mod.rs` | 1886 | — | — | ⚠️ 误记为 2023 |
| `src/render/raster.rs` | 1074 | 653 | 39% | ⚠️ 误记为 1170 |
| `src/layout.rs` | 903 | 441 | 51% | |
| `src/` 合计 | ≈ 2.42 万 | | | |
| `crates/` 合计 | ≈ 0.44 万 | | | |
| **总计** | **≈ 2.86 万** | | | |
| `#[test]` 总数 | **389** | | | 四份审计一致 ✔ |

**关于行数偏差的说明**：`app.rs` 的"实现 ≈1448 行"在 audit-3 中是准确的（与实测测试起点 1450 吻合），
但**总数**四份报告均偏高约 570 行，偏差模式一致（`track.rs` +202、`widgets` +137、`raster` +96）。
推测为统计工具口径差异（CRLF 计数 / 编辑器总行数）。**建议以本附录数字为准**，
并在 CI 里加一条 `wc -l` 校验或统一用 `tokei` 作为单一来源。

---

## 七、一句话总结

lieui 的架构骨架已达到可长期演化的水平，**不需要推翻重来**。它在实现层的系统性问题是同源的：
**契约写在文档里、代码只实现主路径、退化路径无测试。**

四份审计共同指向的改造重点，按投入产出排序是：

1. **先建安全网**（S0：lint 门禁 + `lieui-layout` 测试矩阵 + 像素回归）—— 没有这层，后面全是裸奔；
2. **钉死退化路径**（S1 约 150 行 + S2 帧调度闭环）—— 消除残影、数据破坏级缺陷与停帧风险；
3. **让"局部"真的局部**（S2/S3）—— 兑现"纯 CPU 渲染 + retained 树"的性能承诺；
4. **收敛模块边界**（S5）—— `RuntimeInner` / `Track` / `app.rs` 三个回潮点的结构性清理，不改行为但决定长期可维护性。

**唯一需要立刻警惕的是主线 B**：它是全计划唯一一个"设计意图正确但改法不完整就会引入新 bug"的地方 ——
帧调度收敛到单一入口是对的方向，但漏掉"谁来请求重绘"这一环，就会把"帧跑 3 次"换成"定时器和动画停帧"。
建议这一条先单独做一个 spike 验证，再铺开 S2。
---

## v1.2 增补（2026-10-07）：两条"高估收益"的判断被实测推翻

本批连续三次用**测量**替代推理，否决了三条看似合理的优化。
记录在此，避免后续执行者重做。

| 原判断 | 实测结论 | 依据 |
|---|---|---|
| **C4（display list）** 是软渲染的结构性上限，值得投入 | ❌ **不值得**。Scene 遍历 = **0.061 us/节点**，3200 节点 = **0.196 ms/帧 = 帧预算的 1.2%** | `tests/render_cost_bench.rs`（`--release -- --nocapture --ignored`） |
| **D-a 主体**（FlexNode 持久化）收益可观 | ❌ **收益被高估**。省的是几百次堆分配；边界重排已限制重排范围，而瓶颈在光栅化 | 同上；且缓存失效需 per-node 版本信号 +父属性改写子 style（易漏 ⇒ 静默错布局） |
| **D37** 的"四套浮点容差"需统一 |⚠️ **部分是伪需求**。实测是三种不同语义（1e-6 精确 / 1e-3 视觉等价 / 1e-4滚动钳制），只有"无像素 snapping"是真问题 | `layout.rs` / `app.rs` / `raster.rs` / `transform.rs` 逐处核实 |

### 真正的优化方向

**减少绘制量**（更细的 culling、降 overdraw、按可视区域裁剪子树），
而不是缓存 Scene 的 op 序列。前者与成本成正比，后者只占1.2%。

### 测量方法的两条教训

1. **对照组必须先确认它真的走了对照组分支** ——
   第一版用 `damage = &[]` 当"小脏区"，但 `render()` 里
   `let all = damage_all || damage.is_empty();` 会把它当成**全重绘**，
   于是两列耗时几乎相同，差点据此得出"绘制量不影响成本"的荒谬结论。
2. **共享机器上的计时数据，没有复现就不算数据** ——
   同一份代码两次跑出 `0.932` 与 `0.061` us/节点（**15 倍**），
   差异来自后台同时跑 `clippy`；静置后两次一致。

> 这两条与本项目此前的"变异测试必须真的引入缺陷"同源 ——
> **验证本身也需要被验证**。