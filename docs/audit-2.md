我先并行派出三个探索代理，对主 crate、事件/布局层、渲染与子 crate 做深度分析。
# lieui 完整分析报告

## 一、总体评价

这是一套**工程质量高于平均水准**的纯 Rust GUI 工具包。分层干净：描述树（`view.rs`）→ 保留树（`track.rs`）→ diff 对齐（`align.rs`）→ 布局/光栅化 → 上屏，依赖方向健康（`reactive` 不知道 `app`，靠 `RequestQueue` 反转）。渲染侧的脏区同源性约束、vello_cpu 0.3 陷阱处理、IME/滚动条回归测试都带着“症状来源”注释，可见作者对细节有掌控。

主要债务集中在四处：**布局桥接每次重排全量重建**、**保守置脏 + handler 全量换血**、`lieui-layout` 测试几乎为零、若干“容器型类型”的职责膨胀。

---

## 二、架构级问题（按严重度排序）

### A1【最高收益】布局桥接：每次重排全量重建 FlexNode 子树

`src/layout.rs` 的 `build()` 对每个边界子树现场 `FlexNode::new(id, n.layout.clone())`，文本叶子还深拷贝 `String + TextSpec`：

```265:283:src/layout.rs
// build() 每次重排为边界子树全量分配新 FlexNode，
// measure_text = Some((s.clone(), n.text.spec.clone())) 深拷贝
```

Taitank 类引擎的正确姿势是**持久化节点 + 脏标志增量重排**。同时文本被测两次：flex 引擎测一次（`flex_node.rs:960-988`），`write_back → desired_size` 又测一次（`layout.rs:398-410`）。

**改进**：
1. `Node.flex_cache: FlexNode` 持久化子树，`build` 只在 patch 时更新对应节点——收益最大的一项；
2. `desired_size` 直接复用 flex 已测结果，消掉第二次测量；
3. `measure_text` 改 `Option<(Rc<str>, TextSpec)>` 消除 String 拷贝。

### A2【唯一实质 bug】任务唤醒器快照过期 → 消息静默丢失

`spawn_task_inner` 在 spawn 时克隆 waker slot 给 `TaskCtx`（`task.rs:506,542`）。若任务在 `set_waker` 之前创建（如 App 构造期），消息永远落 `LocalQueue`，而 GUI 模式下 `take_pending_external` 恒返回空（`task.rs:443-448`）——**遮罩永久挂起且无任何诊断**。

**改进**：post 时查询当前 slot，或 Platform 模式下也 drain 本地队列。

### A3 lieui-layout 测试裸奔

`tests/smoke.rs` 仅 4 条。shrink 冻结循环、wrap 多行、absolute、RTL、min/max clamp、gap 全部零覆盖——这是整个代码库**算法复杂度最高、测试最少**的模块，任何重构都是裸奔。另有已确认的坏味道：
- `layout()` 永久改写 `style.flex_basis` 不恢复（`flex_node.rs:257-261`）——布局对样式有持久副作用；
- `line_space` 是死配置（`style.rs:50` 定义 never read）；
- `tight_width` 的 `min_width = max_width = f32::MAX` 疑似笔误（`box_model.rs:42-49`）；
- `measurable.rs` 是死抽象——主 crate 零引用，wrap 逻辑在它与 `flex_node.rs` 平行实现两份。

**改进**：先按 taffy/yoga 的测试矩阵补特征测试钉住现状（absolute 优先，因为零覆盖且语义最难），再动手改；`flex_basis` 补 save/restore；`measurable.rs` 删除或定为正式接口，二选一。

### A4 保守置脏 + handler 无条件换血 = 每次交互全树分配

两个因素叠加：
1. `Signal::set` 给**所有**窗口置 `Dirty::VIEW`（`reactive.rs:206-210`）——多窗口下一个 signal 改动导致每个窗口重跑 `view()` + 全树 align；
2. align 对每个节点无条件 `n.handlers = d.handlers.clone()`（`align.rs:248-251`），即使节点完全没变。闭包 Rc、`Vec<HandlerSlot>` 每帧全树换血。

**改进**：handler 替换前先比较（比 `Rc` 指针/长度，不变则跳过——一行守卫消掉最大分配源）；`Signal` 增加窗口级订阅（局部信号只 `mark(id)`），保持默认行为不变做渐进优化。

### A5 命中测试：一次指针事件 2~4 次全树命中

- Move：`update_hover` + `hit_path_for` 同坐标打两次（`input.rs:90-91`）；
- Down：再叠 `sync_context_menu` 和 `hit_inside_context_menu` 各一次 `hit()`（`app.rs:906,928`）；
- `hit::descend` 每节点现场构造并求逆 Affine（`hit.rs:93-97`）。

**改进**：Move 分支合并为一次命中复用结果；布局写回时缓存节点累计变换矩阵，命中时只做一次逆乘；hover 交集用公共前缀算法替掉 O(depth²) 的 `Vec::contains`。

### A6 Overlay 固定 6 层枚举的硬伤

`LAYER_TOP_DOWN: [DragPreview, Modal, Tooltip, Popup, Overlay, Content]`（`hit.rs:20-27`）——**Modal 之下的 Popup/Tooltip 会被 backdrop 盖住**：模态框里开下拉/右键菜单直接不可见。且层选项任何字段变化（包括纯交互的 `dismiss_on_outside_click`）⇒ `damage_whole_window()`（`align.rs:63-69`）。

**改进**：popup 允许声明“owner 所在层之上”或动态 z 值；层 opts 拆“影响绘制”与“纯交互”两类，只有前者整窗脏。

### A7 每帧 O(N) 全表扫描

`has_layout_dirty`（`track.rs:1703-1707`）每帧被调两次、`layout_boundaries`、`clear_layout_flags` 都是全 arena 遍历。**改进**：维护 `dirty_count` 计数器变 O(1)；`layout_boundaries` 改为 mark 时压入 `pending_boundaries: Vec<NodeId>`。

---

## 三、代码实现级改进

### 文本缓存

```207:224:crates/lieui-text/src/lib.rs
// 缓存命中路径每次也要 text.to_owned() + font_family.clone() 构造键
// 缓存满 4096 整体清空，无 LRU
```

**改进**：键改预哈希（u64 + `Arc<str>` 二次校验），命中路径零分配；整体清空换 LRU/分代淘汰。`spec_hash` 手写 FNV，建议给 `TextSpec` 派生 Hash 或加测试钉住字段清单——漏 mix 字段是静默错误排版。

### 拆分 god-file

| 文件 | 现状 | 拆分方案 |
|---|---|---|
| `app.rs` | `WindowCtx` impl ≈810 行、约 15 项职责（帧管线/tooltip 会话/右键菜单会话/事件分发/时钟/渲染器所有权…） | 拆 `app/sessions.rs`（三个会话 ≈400 行）、`app/pipeline.rs`（frame/tick/animate）、`app/input_glue.rs`、`app/view_model.rs` |
| `widgets/mod.rs` | 2024 行：11 widget + 行为 + 滚动条子系统 + 330 行 `draw()` 大 match | 按模块头自己预告的切面拆：`input.rs`、`scrollbar.rs`、`basic.rs`、`text_draw.rs` |
| `track.rs` | impl ≈930 行混六种职责，**完整文本编辑引擎 ≈190 行住在 Track 里**；`Node` 55 字段 | `track/text_edit.rs`（收敛为 `TextInputState`，`Node.input: Option<...>`，顺带解决“spacer 也背编辑态”）、`track/damage.rs`；`slider_drag_to/toggle_*` 搬到 widgets 层 |

### 封装纪律不一致

- `Node` 全字段 pub（`track.rs:729-783`，含 `flags/desired/computed` 等内部布局产物），而 `view.rs` 把 `DescNode` 收成 `pub(crate)`——建议 `Node` 收敛为 getter；
- `WindowCtx` 公开暴露 `vello_cpu::Pixmap`（`app.rs:519`）——渲染后端类型穿透 API，换光栅化即破坏兼容，建议返回自有类型或 `&[u8]`；
- `RequestQueue` 的 `take::<T>` 对类型不匹配载荷静默保留（`reactive.rs:140-152`）——放错类型即永久泄漏且无诊断，至少加 debug 断言。

### 渲染层小项

- `Renderer::render` 与 `Rasterizer::rasterize` 各算一遍批次（`render/mod.rs:125` vs `raster.rs:363`）——`rasterize_batches` API 已存在，直接传参；
- 图片 blit 不参与 clip 栈：滚动容器内图片不被裁剪（`raster.rs:480`）——blit 前按当前 clip 求交 dst；
- `caret_at_x` 逐字符前缀 measure（`widgets/mod.rs:509-524`）——改用 parley 的 hit-test。

### 其他

- `Signal`/`in_view` 的借用安全靠纪律：release 下 `view()` 内 `set` 每帧自我触发且静默，`in_view` 应改 RAII guard；
- 焦点三重表示（`Track.focused` + `Node.focus_state` + `Node.interaction.focused`）需手工同步——收敛为派生值或单一 set API；
- hover/pressed 只有背景/文字两维，hover 变边框色都要重跑 view——考虑轻量 token 间接层（`ColorToken(u16)`），主题切换从“重跑 view”降级为“整窗重绘”；
- `align_keyed_children` 的 keyed 匹配是 O(n²)（`align.rs:330-334`）——改 `HashMap<Key, NodeId>` 建池。

---

## 四、建议执行顺序

1. **先钉测试**：lieui-layout 补特征测试矩阵（absolute/shrink/wrap/min-max），没有这层保护后面都是裸奔；
2. **修 A2**（任务消息丢失）——唯一的行为 bug，改动小；
3. **低成本高收益三连**：handler 变化守卫（A4②）、文本缓存键零分配、`flex_basis` save/restore——各一行到几十行；
4. **A1 FlexNode 持久化 + 消除文本双测**——帧成本最大项；
5. **A5 命中去重 + A7 O(1) 脏计数**——常量项优化；
6. **拆文件**（app/widgets/track）——纯机械重构，任何时刻可做；
7. **A6 层栈语义**——做 Modal 内弹层功能前必须解决。

需要的话我可以先从第 1 或第 2 步开始动手。