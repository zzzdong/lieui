# LieUI 代码审计报告

> 审计日期：2026-07-24
> 分支：`v2-rewrite`
> 范围：`src/` 全量 + `examples/`
> 结论：`cargo check` 干净通过（无 warning）。架构完整可编译，但存在若干**功能性 bug**与**文档漂移**。

## 0.1 修复状态（2026-07-24 起实施）

| 编号 | 问题 | 状态 |
|------|------|------|
| 1.1 | Button hover/pressed 状态绑错节点 | ✅ 已重构为 HTML 式事件模型：移除 `interactive`，视觉状态直接作用在命中目标，并沿 hit.path 上所有带 listener 的祖先传播；渲染时子节点继承最近有 listener 的祖先状态 |
| 1.2 | `EventEffects` 被丢弃 | ✅ 已修复（`app.rs` 应用 effects + `Runtime::request_layout/request_render`/`frame_visual_update`） |
| 1.3 | 事件捕获/冒泡遍历顺序相同 | ✅ 已修复（`event/manager.rs::dispatch_three_phase`） |
| 1.4 | mouse_up 清错 pressed 节点 | ✅ 已修复（`EventManager::pressed_node` 跟踪真正按下的节点） |
| 2.2 | 文本不换行 + flex_shrink 默认 0 | 🟡 部分修复（`layout/flex_node.rs` 按约束宽度重新测量文本以支持换行；`flex_shrink` 默认 0 未改，Row 内挤占仍可能溢出） |
| 2.3 | 文本布局缓存每帧重复创建 | ✅ 已修复（`runtime/mod.rs::cv` 命中复用 + 未命中仅创建一次） |
| 2.7 | `frame()` 增量复用注释不符 | ✅ 已修正注释 |
| 3.1 | `ViewNode::render()` 死代码 | ✅ 已删除（`view/node.rs`） |
| 3.2 | `state::request_redraw()` 死代码 | ✅ 已接线（`app.rs::about_to_wait` 消费 redraw 标记，函数真正生效） |

剩余未处理项：2.2 的 `Row` 内 flex_shrink 溢出、3.3/3.5（两套 FlexStyle、alpha 预乘）、4（Reconciler 无 key 重排）、5（softbuffer 字节序）、6（Clip 未实现）——见各节。

---

## 0. 当前实际架构（与文档对照）

实际实现（v2-rewrite）与 `guide.md` 及 `docs/*.md` 描述的旧架构**完全不一致**：

| 维度 | 文档描述（已过时） | 实际实现 |
|------|--------------------|----------|
| 根 trait | `Widget` trait | `View` trait（`fn build(&self) -> ViewNode`） |
| 视图描述 | `Widget` / `ViewContext` | `view::ViewNode` 枚举（Text / Div / Image / Canvas） |
| UI 树 | `WidgetTree` + `WidgetEntry` | `runtime::element::ElementTree`（slotmap） |
| 协调 | （无 / 整树重建） | `runtime::reconciler`（`Patch`：Create/Update/Remove + key 匹配） |
| 状态 | `PropMap` | 全局线程局部：`state::State<T>`、`CALLBACKS`、`OVERLAY_VIEW`/`MODAL_VIEW` |
| 入口 | `Application::new(ctx)` / `app.run(|ctx| …)` | `Application::new(builder: Fn() -> ViewNode, viewport).run()` |
| 图层 | （无） | `core::layers`：`Base` / `Overlay` / `Modal` 三层 |

详细改进见 `guide.md`（已重写）。

---

## 1. 严重 / 高优先级 Bug

### 1.1 Hover / Pressed 视觉状态绑错节点 —— Button 高亮永远不显示
- 文件：`src/event/manager.rs`（`handle_mouse_down` / `handle_mouse_up` / `handle_mouse_move`）、`src/runtime/mod.rs`（`cv`）
- 现象：`hit_test` 返回**最深层**命中节点。当鼠标悬停在 `Button` 上时，命中的是 Button 内层的 `content` Div（或 `Text`），二者都没有 `listener`。`EventManager` 只在 `hit.target` 上设置 `hovered` / `pressed`，而 `cv` 里带 `listener` 的 Button 外层 Div 读取自己的 `tree.state(id)` —— 该状态从未被设置，所以背景色永远不变。
- 影响：点击回调**能正常工作**（dispatch 会遍历整条 path，Button 祖先在路径上），但 hover / 按下**背景反馈完全失效**，交互体验破损。
- 修复：移除 `interactive` 字段，改为 HTML 式事件模型。`EventManager` 将 `hovered` / `pressed` 直接设置在命中目标，同时沿 `hit.path` 上所有带 `listener` 的祖先传播；`cv` 渲染时，子节点继承「最近有 listener 的祖先」的交互状态。

### 1.2 `EventEffects` 被丢弃 —— `EventContext::request_rebuild/request_layout/request_render` 是空操作
- 文件：`src/app.rs`（`CursorMoved` / `MouseInput` 等分支均写 `let _effects = em.handle_…(…)`）、`src/event/manager.rs`（`take_effects()`）
- 现象：`EventManager` 内部用 `take_effects()` 收集 `EventEffects`，但 `app.rs` 调用方一律丢弃返回值。因此在 `on_click_with_ctx(|ctx| …)` 回调中调用 `ctx.request_rebuild()` 不会有任何效果。
- 影响：API 自相矛盾。用户只能通过全局 `request_rebuild()`（或 `State::set`/`update` 内部触发）来请求重建。
- 修复：在事件循环里消费 `effects`，对 `needs_rebuild` / `needs_layout` / `needs_render` 分支触发对应流程。

### 1.3 事件捕获 / 冒泡阶段遍历顺序相同（均为 target→root）
- 文件：`src/event/manager.rs`（`dispatch_three_phase`）
- 现象：两个循环都是 `hit.path.iter().rev().skip(1)`。`path` 为 `root→target`，`rev().skip(1)` 得到 `parent→…→root`（内层到外层），这其实是**冒泡**顺序。捕获阶段应为 `root→parent`（外层到内层）。
- 影响：捕获与冒泡方向无区别，`ctx.phase()` 在捕获期返回的方向错误，`ctx.stop_propagation()` 在捕获期会停错位置。
- 修复：捕获用 `path[..len-1]` 正向遍历，冒泡用反向遍历。

---

## 2. 中优先级 Bug

### 2.1 mouse_up 清错 pressed 节点
- 文件：`src/event/manager.rs`（`handle_mouse_up`）
- 现象：`set_pressed_state(tree, Some(hit.target), false)` 在**释放位置**的节点上清除 pressed。`EventManager` 只记录了 `mouse_down: bool`，未记录被按下的目标 id。
- 影响：若「按下 A → 移到 B → 在 B 释放」，A 会一直保持 pressed 高亮。
- 修复：增加 `mouse_down_target: Option<ElementId>`，按下时记录、抬起时清除该目标。

### 2.2 文本不换行 + `flex_shrink` 默认 0 → 长文本溢出
- 文件：`src/runtime/mod.rs`（`cv` 中 `create_text_layout(…, max_width: None)`）、`src/view/node.rs`（`measure` 用无约束 `LayoutConstraint::default()`）、`src/layout/context.rs`（`to_flex_style` 内部 `FlexStyle` 的 `flex_shrink` 来自 `..Default::default()` = 0）
- 影响：Text 节点尺寸等于整行宽度；放进定宽容器会**溢出且不收缩，也不换行**。

### 2.3 文本布局缓存每帧创建两次
- 文件：`src/runtime/mod.rs`（`cv` 的 Text 分支）
- 现象：先 `text_layout_cache(id)`（take 出旧布局）push 进元素，再无条件 `set_text_layout_cache(Box::new(create_text_layout(…)))` 重新创建一份。每个文本节点每帧深拷贝 + 重建两份 `Box<TextLayout>`。
- 影响：功能无错，但每帧每文本节点 2 倍文本排版开销。建议命中缓存时直接复用、仅 miss 时创建。

### 2.4 `frame()` 的「增量复用」并未实现
- 文件：`src/layout/context.rs`（`collect` 接收 `prev.root` 但完全忽略）
- 影响：`LayoutContext::collect` 每次全量重建，`perform_layout` 接收 `prev.root` 却未使用。注释「增量复用」与实现不符（非正确性 bug，但误导）。

---

## 3. 死代码 / 文档漂移

### 3.1 `ViewNode::render()` 是死代码
- 文件：`src/view/node.rs`
- 现象：该方法（含独立文本排版、不读缓存）在 `src/` 内无任何调用，实际渲染由 `runtime/mod.rs` 的 `cv` 内联完成。可删除或统一。

### 3.2 `state::request_redraw()` 是死代码
- 文件：`src/state.rs`（定义但从未被调用）；`take_redraw_requested` 标 `#[allow(dead_code)]` 且从未读取；`app.rs` 用的是 `w.request_redraw()`（winit 原生）。

### 3.3 `guide.md` 与实现严重不符
- 描述 `Widget` trait、三棵树、`builder.rs`/`widgets/`、`WWEvent` 等，均不存在于当前代码。已重写（见 `guide.md`）。

### 3.4 `docs/*.md`（architecture / render / event / widget / layout / text / refactoring_plan）描述旧 `Widget`/`ViewContext` 架构
- 与当前 `runtime`/`view`/`ElementTree` 模型矛盾，已全部加「过时」横幅并指向 `guide.md`。

### 3.5 两套 `FlexStyle`
- `layout::flex::FlexStyle`（对外，默认 `flex_shrink=1`）与 `layout::style::FlexStyle`（内部，默认 `flex_shrink=0`）命名易混；转换时未显式设置 `flex_shrink`，收缩行为实际依赖 `expand` 标志。

### 3.6 图像未做 alpha 预乘 / 混合
- 文件：`src/render/visual.rs`（`blit_image`）
- 现象：直接把原始 RGBA 写入（预乘的）vello pixmap；半透明图像不会与背景正确混合，且未预乘。

### 3.7 softbuffer 像素字节序需实测
- 文件：`src/app.rs`（`blit_to_window`）：`(p.b) | (p.g<<8) | (p.r<<16) | (p.a<<24)`。
- 当前 UI 多为灰 / 纯色，可能掩盖 RGBA/BGRA 顺序问题；彩色像素若错序会蓝黄翻转。建议在真实窗口下用彩色图验证。

---

## 4. 设计层限制（非 bug，建议文档化）

- **Reconciler 无 key 时重排不可靠**：靠「类型 + 位置」匹配，不会真正移动子节点顺序；同类型内容可正常更新，但**不同类型混排重排会产生错误视觉顺序**。对动态列表，请使用 `.key()`（来自 `ViewExt`）。
- **每次 `State` 变更重建整棵树**：full rebuild + reconcile，实现正确但对大 UI 有性能开销。
- **裁剪（Clip）不支持**：`VisualElement::Group` 存在但 `cv` 不产生 Group；`vello_cpu` 路径不裁剪（仅预留给 GPU 渲染器）。

---

## 5. 修复优先级建议
1. **1.1**（交互反馈失效，最影响可用性）
2. **1.2**（事件 API 自相矛盾）
3. **1.3**（事件传播语义）
4. **2.1**（拖拽抬起残留 pressed）
5. **3.3 / 3.4**（更新文档，移除过时内容）
