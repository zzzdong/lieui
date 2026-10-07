The design doc is thorough but describes intent. Let me examine the actual implementation in depth.
我已经通读了核心源码（`reactive` / `cmd` / `track` / `align` / `view` / `event` / `layout` / `hit` / `render` / `app` 主体），并核对了设计文档 `docs/architecture-v3.md`。下面给出完整分析。

---

# lieui 实现分析

## 一、架构全景

v3 相对旧实现（4 重 UI 表示、3 轨状态、3 个帧入口、全局 `thread_local` + `Mutex` 广播）做了一次成功的简化，落地后的核心结构是：

```
ViewModel（用户态：Signal 字段 + view(&Rc<Self>, &mut ViewBuf)）
        │ Signal::set/update → Runtime.mark_all(Dirty::VIEW)（保守标脏，批处理免费）
        ▼
ViewBuf（描述 arena，begin() 只重置游标，跨帧复用）
        │ align(描述 ↔ 保留树)：位置 + 一层 key 对齐，逐字段比 → patch → 精确脏区
        ▼
Track（唯一所有者：arena + NodeId，含视图态 + 布局结果 + 层栈 roots）
        │ layout（脏边界重排）/ hit（纯函数）/ render（只读展开 Scene → 脏区光栅化）
        ▼
持久 Pixmap → present_with_damage → softbuffer（winit 可选）
```

**三棵树分工清晰**：`Track` 是唯一所有者，`ViewBuf` 是"某时刻的描述"（用完即弃），`Scene` 是只读扁平 draw list。用户态**拿不到 `NodeId`**，也**不出现 patch**。

**几个设计决策值得肯定**（这些是真正做对的地方）：

1. **desc/state 分组**（`KindDesc` vs `Kind` 的 state 字段）——这是"hover 不丢、滚动不跳、输入框不闪回"的根本保证，`apply_to` 只写 desc 组。
2. **0 thread_local / 0 全局单例**——`Signal` 自带 `Rc<Runtime>`，`Ctx` 自带 `WindowId`，`App` 用 `Rc<dyn WindowView>` 擦除去泛型。
3. **精确脏区而非签名猜测**——patch 发生那一刻就登记脏矩形，配 `paint_bounds`（文本只算内容范围）与 `damage_bounds`（含祖先变换）。
4. **借用纪律**——两段式分发（只读收集 `Rc` → 可写调用）+ `Cmd` 延迟写入通道，让 `Kind::Custom` 能安全持有 trait 对象。
5. **声明式层**——Modal/Popup/Tooltip 都是 `view()` 里声明的根，父层消失级联销毁，删掉了旧的父子句柄图 + 层命令队列。

测试覆盖极其扎实（每个模块内嵌单测，且大量像素级/几何级断言），这是质量上的亮点。

---

## 二、问题与改进建议

### 1. `app.rs` 重新长成了 God Object（5386 行）

设计文档 §1.6 自己诊断过"`app.rs` 1020 行、`WindowContext` 承担 7 种职责"，但重构后 `app.rs` 反而涨到 **5386 行**（约 1448 行实现 + 3900 行测试）。`WindowCtx` 现在承担：帧驱动、tooltip 会话、右键菜单会话、光标闪烁、loading spinner、键盘/焦点、指针输入、背景/DPI、请求队列 drain。

`Sessions`（`app.rs:445`）是一个很好的方向——把"框架替你记的临时交互态"聚合成一个 struct——但聚合**没有继续拆到模块边界**，它们仍然住在 `app.rs` 里，靠 `sync_tooltip` / `open_tooltip_layer` / `sync_context_menu` / `drop_dead_context_menu` / `push_busy_overlay` / `busy_card` 等十几个人肉方法伺候。

**建议**：把这三类"框架自管会话"抽成独立模块（`sessions/tooltip.rs`、`sessions/ctx_menu.rs`、`sessions/busy.rs`），每个会话持有自己的状态机 + 对 `Track`/`Runtime` 的操作；`WindowCtx` 只留帧管线编排。测试也随模块走（下一条）。

### 2. 测试与实现同文件 → 单文件爆炸

`app.rs` 从 1450 行起全是 `#[cfg(test)]`。测试量大是优点，但同文件导致文件不可读。**建议**：把测试拆到 `app/tests.rs`（`#[path]` 或子模块目录），或集成测试。`track.rs`（2114 行）、`layout.rs`（1014 行）、`raster.rs`（1170 行）同理。

### 3. `RuntimeInner` 是"运行时杂物抽屉"（13 个字段跨 4 个域）

`reactive.rs:87-118` 的 `RuntimeInner` 同时持有：脏标志表、`in_view`、`requests`、`theme`/`theme_mode`/`system_dark`、`waker`、`tasks`/`busy`/`next_task_id`/`busy_min_visible`、`timers`、`animating`、`window_sizes`。

`Runtime` 名义上是"响应式运行时"，实际是主题 + 任务 + 定时器 + 窗口尺寸的宿主。`Signal::set` 需要的只是"脏标志表"，却被迫带上整个 Runtime。

**建议**：拆成组合——`Runtime` 只保留响应式核心（`windows` 脏标志 + `in_view`），其余按域拆成 `ThemeState` / `TaskState` / `TimerState`，由 `App` 组合持有；`Signal` 只持有"脏标志表"的句柄（一个 `Rc<DirtyTable>` 而非 `Rc<RuntimeInner>` 全家桶）。这既缩小 `Signal` 的语义耦合，也让各域可独立单测。

### 4. `Kind` 与 `KindDesc` 是镜像枚举，新增组件要写 5 处

每加一个组件，要改：`KindDesc` 定义（`track.rs:256`）、`Kind` 定义（`track.rs:409`）、`KindDesc::apply_to`（`track.rs:296`）、`Kind::from_desc`（`track.rs:477`）、`tag()` × 2。这是一条很容易漏改、且靠 `match` 穷尽性才拦住的维护负担。

**建议**：用 `macro_rules!` 生成枚举 + `tag` + `from_desc` + `apply_to` 的 desc 组（state 组由宏显式标注），把"desc/state 分组"这个核心约定**编码进宏**而不是靠注释纪律。

### 5. 组件行为 API 挂在 `Track` 上，`Track` 成了行为宿主

`track.rs:1261-1649` 有 20+ 个组件行为方法：`slider_drag_to` / `toggle_checked` / `toggle_switch` / `select_radio` / `input_insert` / `input_backspace` / `input_delete` / `input_move_caret` / `input_set_preedit` / `input_ensure_caret_visible`…… 这些本质是"组件的状态机"，却都在 `impl Track` 里。

设计文档 §3.2 明确说组件行为应是 `fn slider_handle(track, id, ev, cmd)`（放在 `widgets/`），但**底层几何操作**全沉淀回了 `Track`。`track.rs` 2114 行里约一半是 Input 编辑和绑定组件逻辑。

**建议**：把这些方法移到 `widgets/`（如 `widgets/input.rs`、`widgets/controls.rs`），作为接收 `&mut Track` 的自由函数；`Track` 只保留纯粹的树/脏区/视图态原语。这样 `widgets::handle` 与这些底层操作同域，且 `track.rs` 回归"保留树"单一职责。

### 6. 事件分发是"两套独立路径"，`handled` 语义有微妙缺口

`widgets::handle_route`（内置行为，拿 `&mut Track`）与 `dispatch`（用户处理器，拿 `&mut Ctx`）是**完全独立的两遍**（`app.rs:814` 的 `WindowCtx::dispatch` 串起两者）。设计文档 §3.5 已诚实记录了这个修正。

后果：内置行为**永远先于**用户处理器跑完，用户处理器的 `mark_handled` 无法阻止**已跑完的**内置行为，也无法阻止**后续**内置行为（内置行为不读 `handled`）。这与 WinUI 的"同一路由序列内 `handled` + `handledEventsToo`"语义有偏差。

**建议**：至少让内置行为共享同一个 `handled` 状态（把 `handled` 作为可写字段传给 `handle_route`，内置行为在改完视图态后可选置 `handled`）。是否需要"用户处理器先拦截内置行为"取决于产品语义，但当前"内置行为无视 handled"应当显式化或修正。

### 7. 图片 blit 绕开 vello，裁剪栈失效（真实缺口）

`raster.rs:480-481` 自述：手动 `blit_image` "**不走 vello ⇒ 不参与 `PushClip` 裁剪栈**（滚动容器里的图片不会被裁），采样是最近邻"。这意味着滚动容器里的图片会溢出裁剪边界。这是渲染层的已知正确性缺口，建议列入计划用 `fill_image` 或 clip 传递修复。

### 8. 几何语义分散在 4 处，一致性靠约定

同一份 `transform` + `clip` 语义在：`hit.rs:descend`（逆矩阵 + 矩形 contains）、`track.rs:damage_bounds`（`bounding_box`）、`scene.rs`（组合变换生成 op）、`raster.rs`（`shift * kaffine` + push_clip）分别实现。任何一处对 `origin`/`scale=0`/`clip` 的判据不一致就会产生"命中对、绘制错"或反之。

**建议**：把"节点 → 窗口空间变换/裁剪"的求值收敛到一个纯函数（如 `Track::visual_bounds(id)` 或一个 `VisualTransform` 类型），hit / damage / scene 共用，消除四处漂移。

### 9. 低优先级但值得做的

- **`Signal::set` 无条件 `mark_all`**：设计文档 §3.14 已给出"窗口粒度粗追踪（`get()` 登记 `seen_by`）"的升级路径，成本低、向后兼容，多窗口场景值得提前做。
- **`ViewBuf` 的 10+ 个转发糖**（`view.rs:908-960`）注释里自己说"若继续膨胀再抽 macro"——已经到该抽的点。
- **`damage_batches` / `bands` / `union` 三策略并存**：`Renderer::render` 只用 `damage_batches`，另两个是基准用的，注意别让它们腐烂。

---

## 三、优先级建议

| 优先级 | 事项 | 理由 |
|---|---|---|
| P0 | 拆 `app.rs`（会话模块化 + 测试外置） | 可读性/可维护性的最大单一风险点 |
| P0 | 修图片裁剪缺口（§7） | 正确性 bug |
| P1 | `RuntimeInner` 按域拆分 | 响应式核心与主题/任务/定时器解耦 |
| P1 | 组件行为从 `Track` 移到 `widgets/` | 让 `track.rs` 回归"保留树"单一职责 |
| P1 | 事件 `handled` 语义显式化 | 修正内置/用户处理器路由语义 |
| P2 | `Kind`/`KindDesc` 用宏消除镜像 | 降低新增组件成本 |
| P2 | 收敛变换/裁剪几何求值 | 消除四处漂移 |
| P3 | `seen_by` 窗口粒度追踪、样式糖抽宏 | 优化与消噪 |

整体而言，v3 的**架构决策是对的**——retained tree + 描述对齐 + R1 响应式 + 精确脏区这套路线，比旧实现（每帧重建 + reconciler + 签名猜测脏区）干净一个数量级，且被大量像素级测试钉住了正确性。剩下的问题主要是**模块边界在执行中回潮**（`app.rs`、`Track`、`Runtime` 三个点承载了超出其职责的内容），属于"结构性问题"，不需要动核心设计即可收敛。

需要的话，我可以直接从 P0（拆 `app.rs`）开始动手，给出具体的拆分方案和迁移步骤。