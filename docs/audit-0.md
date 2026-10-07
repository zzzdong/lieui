我先全面了解项目结构。
当前在 `main` 分支（与记忆中的 `newworld` 不同）。让我做全面探索。
项目约 25k 行。我并行派出探索代理深入各子系统，同时自己读架构文档。
核心事实已收集。我来亲自验证几个关键论断。
验证完毕。以下是完整分析报告。

---

# lieui 实现分析与改进方案

> 分析基线：`main` 分支 `70a62f7`（alpha.4），`src/` 约 19k 行 + `crates/` 约 2.5k 行，389 个测试。
> 设计基线：`docs/architecture-v3.md`（v3.4-draft）。核心结论：**v3 设计已诚实落地，问题不在"方向错"，而在"实现细节与设计意图的三处背离 + 若干 O(n)/O(像素) 热点"。**

## 一、总体判断

v3 重构从"一棵 UI 表示 4 次"收敛到**保留树 `Track` + 描述 `ViewBuf`** 两层，删除 reconciler/`Msg`/`StateMap`/thread_local 全局信号/图层命令队列——这些目标在代码里均已兑现（`align.rs` 的 desc/state 分组、`reactive.rs` 只有脏标志、`reactive.rs` 零 thread_local）。这是**显著优于旧架构**的成果，且 `#![forbid(unsafe_code)]`、零 `transmute`。

但有三处**实现背离设计意图**，我认为是当前最值得修的东西：

1. 设计 §3.8 承诺"帧循环只看 `dirty`"，实际 **`frame()` 每次事件被无条件跑 2–3 遍**；
2. 设计 §3.7 承诺"脏区收益 ∝ 脏区面积"，实际**滚动场景常态退化整窗**；
3. 设计 §四借用纪律要求"`view()` 内禁止 `set`，debug 断言拦下"，实际 **release 下保护完全消失**。

---

## 二、一帧的真实开销（验证过的路径）

四个 `tick()` 调用点都会无条件执行 `ctx.frame()`：

```923:940:src/platform/mod.rs
    fn window_event(&mut self, el: &ActiveEventLoop, os_id: OsWindowId, event: WindowEvent) {
        self.on_window_event(el, os_id, event);
        // 交互产生的脏 ⇒ 请求重绘（winit 只在需要时才会给 RedrawRequested）
        if let Some(id) = self.app_window_id(os_id) {
            let rt = self.app.runtime();
            let d = rt.peek_dirty(id);
            if !d.is_empty()
                && let Some(ws) = self.windows.get(&id)
            {
                ws.window.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // 请求队列可能在事件里被写入（如关闭按钮 ⇒ `ctx.close_window`）
        self.tick(el, Instant::now());
    }
```

`Runner::tick`（`platform/mod.rs:370-407`）对**每个窗口**跑 `animate → tick → frame`，且不看脏标志。调用点：`resumed:891`、`user_event:897/904`、`RedrawRequested:722-723`、`about_to_wait:939`。

于是**一次鼠标移动**的真实序列是：`about_to_wait → frame#1`（消费脏并 present）→ `RedrawRequested → frame#2` → `about_to_wait → frame#3`。设计里"脏了才画"只对 `present` 成立（platform:929-933、404），** `frame` 本身是裸跑的 **。

`frame()` 无条件执行的固定成本（即使 `dirty` 为空）：

| 步骤 | 位置 | 复杂度 |
|---|---|---|
| `take_dirty` / `peek` / `mark` | `reactive.rs:277-295` | `Vec` 线性 `find` |
| `drop_dead_context_menu` | `app.rs:606` | 每帧 |
| `reap_busy` | `app.rs:616` | 全表 + `Instant` |
| 主题比对 + `Borrow` | `app.rs:626-635` | 19 token 比较 |
| `Take_damage` | `app.rs:680` | `mem::take`，OK |
| `place_anchored_layers` | `app.rs:664` | **无条件**遍历所有锚定层 |
| `take_scroll_changes` | `track.rs:1239` | **全 arena 扫描** |
| `has_layout_dirty` | `app.rs:658` 与 `app.rs:679` | 全表，**调两次** |
| `next_deadline` | `timer.rs:209` | 全表 |

---

## 三、P0：必须修的架构问题

### P0-1 帧重入：让 `RedrawRequested` 成为唯一的 `frame()` 入口

**证据**：`platform/mod.rs:370-407` + 上表。
**根因**：`about_to_wait` / `user_event` 里的 `tick` 是"怕漏"的兜底，与 `RedrawRequested` 重复。
**改法**：

```
// platform/mod.rs
fn about_to_wait(&mut self, el) { self.drain_requests(el); self.schedule_next_wakeup(el); }  // 不再 frame
fn user_event(...) { /* 处理后 */ if dirty { ws.window.request_redraw(); } }
fn window_event(...) { /* 处理后 */ if dirty { ws.window.request_redraw(); } }
// RedrawRequested: 唯一 tick+frame 入口
```

`tick()` 里的 timers/animation 需要一个独立闸门：加 `WindowCtx::last_frame: Instant`，`tick` 只在 `now >= next_clock` 或 `dirty != EMPTY` 时推进，避免 timers 被 `frame` 次数放大。
**验收**：`examples/damage_bench.rs` 加一个计数器，单次 `InputEvent::Move` 下 `frame()` 调用从 3 降到 1；业务行为不变（现有 389 测试全绿）。

### P0-2 脏区在滚动场景常态退化为整窗

**证据**：`raster.rs:114-117` 阈值 `area > 45%` 或 `> 8` 块即整窗；碎片来源是 `layout.rs:151-152 / 354-355` 每个移动节点登记"旧∪新"两个矩形，嵌套滚动一次产出 20+ 块（`render/mod.rs:112-116` 注释已承认，`damage_bench` 有回归测试钉着这个坑）。
而且**已实现的更优策略没启用**：`raster.rs:165 damage_batches_bands` 生成互不重叠行带，批次数远低于 8。

同时上屏还是只按行不按列：

```484:498:src/platform/mod.rs
                for r in &batches {
                    let (y, h) = (r.y as usize, r.height.get() as usize);
                    for row in y..(y + h).min(ph as usize) {
                        let start = row * stride;
                        let end = (start + stride).min(src.len()).min(dst.len());
                        ...
```

一个 40×20 的脏区被展开成 20 × 全窗宽。

**改法（按性价比排序）**：
1. **登记侧合并**：`Track::damage_rect` 入库时对重叠/相近矩形做 union（容忍 ≤10% 面积膨胀），把碎片压到 ≤8；
2. **默认策略换 `damage_batches_bands`**（保证互不重叠 → 同一像素不被重复合成，且 batch 与场景剔除同源这一约束天然成立）；
3. **present 按列裁剪**：`start = row*stride + r.x`，`end = start + r.width`，与 `present_with_damage` 的矩形一致。

**验收**：`damage_bench` 中滚动 500 行列表时 `batches` 数 < 8、`rasterized_pixels` 接近真实脏面积而非整窗。

### P0-3 release 下没有任何脏硅酸盐重入保护

```403:411:src/reactive.rs
```
`assert_not_in_view` 被 `cfg!(debug_assertions)` 包裹（且 `begin_view` 还挂着 `#[allow(dead_code)]`，`reactive.rs:333`）。用户在 `view()` 里误调 `Signal::set`，debug 下 panic 拦得住，release 下变成"每帧 view → set → 再 view"的**永久满帧自激**，100% CPU 且不报错。
**改法**：断言改为 always-on（成本只是读一个 `Cell<Option<WindowId>>`）。设计文档 §四把它列为"违反会 panic 或死循环"的硬纪律，就该无条件执行。

### P0-4 滚动会重走完整 flex + 文本重测

`set_scroll_offset` → `mark_layout_dirty`（`track.rs:1232`），下一帧整个滚动子树：① `layout.rs:265-310 build()` **深拷贝**（`FlexStyle` clone + `String` clone + 递归 Vec push）→ ② 跑完整 flex → ③ 每个叶子调 2–3 次文本 measure（`flex_node.rs:486/650/702`，每次可能是 parley `break_all_lines`）。

滚动是一次视图变换，不是一次布局。
**改法**：滚动偏移作为**子树的绘制/命中平移**（已有 `Affine` 逆变换基础设施，`transform.rs`）， limb 层仍用 debounced 的 removed 布局后结果；FlexNode 树改为持久缓存 + 按脏样式 patch。
**风险**：涉及 `write_back`（`layout.rs:360-384`）写回 debounced 子原点的逻辑，需要保留 M2 那批"滚到边界 + text_wrap 一致"（`layout.rs:739` 有像素级回归）测试。**建议单独一个冲刺做**。

---

## 四、P1：性能热点与 API 卫生

### P1-1 文本缓存把颜色编进 key

`scene.rs:275` 的 key 含 `color`，而 `raster.rs:608` 已经用 op 上的颜色重新着色 ⟹ hover 换色必然重建 parley 排版 + 新增缓存条目；`len >= 2048` 时**整表 clear** ⟹ 密集 hover 的界面会出现周期性重排风暴。
**改法**：key 去掉 color，改为容量按 LRU 淘汰而非整表清空。这是投入 10 行、收益明显的一处。

### P1-2 每次 `view()` 的节点级堆分配

`ViewBuf::begin`（`view.rs:184-190`）只复用外层 `Vec` 容量，每个 `DescNode` 内含 `children / child_keys / handlers` 三个 Vec 全是新建即弃；`.on()` 每次 `Rc::new`（`view.rs:1264-1271`）；`align.rs:248-251` **无条件 clone** handlers/bindings/context_menu（即使内容未变）。
**改法**：① `DescNode` 也池化（池里保留内层 Vec 容量）；② handlers 改为 Node 与 desc 之间 **`mem::swap`** 而非 clone（批扑）。

### P1-3 `Kind` 三份平行枚举 + 三处 match 分派

`track.rs:207 KindTag / 256 KindDesc / 409 Kind`，`tag()` 实现重复两遍（276-291 与 459-474），行为分派在 `widgets/mod.rs:53-91`，绘制在 `widgets/mod.rs:602-1028`。新增一个控件要改 4 处以上，且没有任何编译期或单测手段保证三份同步。
**改法**：derive 宏从单一定义生成 `KindTag / KindDesc / Kind + apply_to`（把 desc/state 分组写成属性标注）。预计消 200+ 行样板，并把"加控件"变成一处修改。

### P1-4 巨型模块

`src/app.rs` 5385 行、`src/widgets/mod.rs` 2023 行、`src/render/raster.rs` 1170 行。`app.rs` 已经把 `overlay.rs`/`menu.rs` 分出去了，但 `Ⅷ 交互会话`（busy / context-menu session / tooltip session，见 `app.rs:422` 附近的 Ject structs）仍在内。建议按"会话"和"窗口生命周期"再拆 2–3 个模块；`widgets/mod.rs` 按类别拆（text / button-family / input-family / container / indicator）。这是纯搬移，无行为风险，但要在 P0-1 之后做以免冲突。

### P1-5 R1 的全窗口广播

`reactive.rs:206 mark_all` — 任一 `Signal::set` 给**所有**窗口置 `Dirty::VIEW`。设计 §3.14 已给出升级路径（`Signal.seen_by: SmallVec<WindowId>`，`get()` 时登记，`set()` 时定向），成本低且不引入观察者图：**新增实现 + 未登记过的保守回退到广播即可**。3 窗口以上或高频 set（滑块/IME）场景收益明显。

### P1-6 task.rs 的线程与背压

每任务一个 OS 线程（`task.rs:546`）、`TaskCtx::progress` 每次上报一个 OS 事件（180-190）、本地队列无界（68-87）、`TaskHandle` 无 `Drop`（237-242，丢句柄不取消任务）、`CancelToken` 用 SeqCst（126-141，Relaxed 语义已足够）。
**改法**：worker 线程 + 任务队列；progress 上报合并到"每帧至多一次"（和 P0-1 的帧闸门配合天然成立）。

---

## 五、P2：能力缺口（按重要性）

**浮层 / 焦点**
- `tab_order`（`focus.rs:71-88`）遍历**全部层根**并全局排序 ⟹ Popup/Modal 打开时 Content 仍在同一条 Tab 链；`FocusPolicy`（`track.rs:601`）与 `LayerOpts.focus`（`track.rs:643`）定义了**零读取点**。需要 FocusScope：进出浮层保存/恢复焦点，Tab 限定当前 scope。
- 无显式 z-index，同层只能靠声明序 ⟹ `menu.rs:57` / `app.rs:916` 被迫用 `CTX_MENU_TAG` 认领层根，这是口头协议而非类型约束。建议给 `LayerOpts` 加 `z_index: i32`。
- Escape 关闭完全缺失；轻关闭有两套实现且规则不同步（`input.rs:254-272` 与 `app.rs:944-1006`，后者有"点击禁用项不关"特判，前者没有）。

**事件 / 输入**
- 两条并行路由遍历：`widgets::handle_route`（`widgets/mod.rs:29-51`）与 `event::collect_route`（`event.rs:901-924`）各自解释 `Routing`，是漂移源。应合并为单次 collect。
- 死事件：`PreviewKeyDown`（`event.rs:158`）、`GettingFocus / LosingFocus`（162-164）、`TextCompositionStarted/Ended` **全仓无生产者**，`focus.rs:9-10` 承诺的"可取消焦点迁移"是死文档。要么实现，要么从 `EventKind` 删除（留着是误导 API）。
- `Cmd::BringIntoView` 只标脏不滚动（`cmd.rs:234-249`，注释自认 M2 TODO），API 已暴露但行为缺失。
- 文本编辑：键映射仅 6 条（Backspace/Delete/←/→/Home/End + Ctrl+A，`widgets/mod.rs:438-462`），缺词跳、上下行、Cmd(macOS)；剪贴板 Ctrl+C/X/V 硬编码在平台层（`platform/mod.rs:590-625`）而 `Modifiers::META`（`event.rs:114`）已定义却无读取点；IME `ImePreedit` 的 `cursor` 一路丢（`widgets/mod.rs:247-249` 只取 `text`），且无 `set_ime_area`（候选窗不跟随光标）——中文输入体验这是必须补的。

**布局**
- 缺百分比（与 Yoga/Taffy 最大断层）；`line_space`（`style.rs:50`）声明后从未读取⟹交叉轴 gap 不存在；`align-content: SpaceEvenly` 静默退化为 0（`flex_node.rs:800-816`）；`Baseline` 在三条对齐路径全被吞掉；默认 `flex_direction = Column`（`style.rs:123`，CSS 是 row）；min/max 负值被静默丢弃。
- `flex_node.rs:603` `while !resolve_flexible_lengths(...) {}` **无迭代上限** — NaN/震荡会挂死 UI 主线程，须加 `MAX_ITER` + 越界回落。
- `types.rs` 里 `[L,T,R,B]` 与 `K_AXIS_*`（`[L,R,T,B]`）两套轴序混用。
- 无像素 snapping，且浮点容差有四套（1e-4 / 1e-3 / 1e-4 / 1e-2）⟹ 1px 分隔线与文本持续半像素模糊（CPU 渲染下尤其难看）。应统一常量并在布局出口 snapping。
- `hit.rs` 无空间索引（可接受的取舍），但**没有用 `bounding_box` 做 AABB 早退**，且 `hit.rs:36` 在已拿到 root 的情况下再用 `roots().iter().find()` 线性回查。

**死代码**
- `crates/lieui-layout/src/measurable.rs`（138 行）+ `constraint.rs`（4 行）**全仓无任何外部使用点**；`box_model::IntrinsicSize` 同样。void 抽象层是维护负担：要么删，要么真正让 `FlexNode::measure_text` 走 `Measurable`（我倾向删——正如 `layout.rs` 适配器里已经实践的做法，具体类型反而更简单）。

**渲染抽象**
- `render/` 直连 `vello_cpu` + `softbuffer`，没有 `RenderBackend` trait，将来接 wgpu 要重写呼叫点；`vello_cpu 0.3` 未启用多线程（`Cargo.lock` 无 rayon 依赖），大脏区/整窗翻新是纯单核。
- `raster.rs:397-420` 每个 batch 都重放**整份场景** ⟹ 成本是 O(op × batches)，而非 O(命中脏区的 op)。应在 rasterize 内按 batch（已是物理矩形）先筛 op 再重放。

---

## 六、建议的实施路线

| 冲刺 | 内容 | 预期收益 | 风险 |
|---|---|---|---|
| **S1（低风险高收益）** | P0-1 帧闸门、P0-3 always-on 断言、P1-1 文本缓存 key、P1-2 handler swap、P1-6 task 合并上报、P2 删 `measurable/constraint`、补齐 `flex_node.rs:603` 迭代上限 | 单次交互的 CPU 开销降 ~3×；消除 release 自激；消除 hover 重排风暴 | 低：均为局部改动，有现有测试钉住 |
| **S2** | P0-2 脏区（登记侧合并 + bands 策略 + present 按列）+ P1-5 `seen_by` 定向置脏 | 滚动/局部刷新真正吃到脏区红利（现在是"实现了但拿不到") | 中：脏区正确性要用 `damage_bench` + 现有 render 回归测试守 |
| **S3** | P0-4 滚动脱离布局 + FlexNode 树持久化 | 滚动从"重排 + 重测文本"降为"平移 + 局部重绘"，量级差异 | **高**：触碰 `write_back` 与 `text_wrap` 一致性，建议先写失败测试再动实现 |
| **S4** | P1-3 Kind derive 宏、P1-4 模块拆分、P2 焦点/浮层/IME/布局能力补齐 | 可持续性与功能完备度 | 中宏 Expand ndtyñ 系统下的宏会改编译时间，建议先量一遍 maketime |

每条都要求：`cargo test` 全绿 + `clippy` 零新增警告，并在 `docs/operation-log.md` 按既有约定记录决策与遗留。

---

## 七、需要你决定的三个取舍

1. **R1 响应式是否升级到窗口粒度**：`seen_by` 只需 ~40 行，但它让 `Signal` 长大一大一小设计一个自然只能 listicle — 是否接受"值与指针状态耦合"的代价换取多窗口下的确定性收益？
2. **滚动是否真的脱离布局**：这是最大的单点收益，也是唯一触碰布局核心且已通过测试验证过的改动。要不要把它作为下一个主线？
3. **`Cmd::BringIntoView` 与 Escape 关闭**：这两个"API 已暴露、行为缺失"的洞，是补实现还是先把它们从公开 API 里撤下来（避免用户踩到半实现的语义）？

如果你确认方向，我建议从 **S1** 开始——它改动集中、风险最低，且每一项都能用现有测试直接验证。要我直接动手，还是先把某一项展开成详细实施方案？