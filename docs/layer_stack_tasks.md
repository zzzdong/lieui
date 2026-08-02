# Layer Stack 实施计划

> 对应设计文档：`layer_stack_design.md` | 目标：增量式落地，每个阶段独立可编译可测试

---

## 阶段 P0：基础设施（约 60% 工作量）

**目标**：`core/layers.rs` 从 "三层+单root" 迁移到 `LayerStack`；保持 `Runtime / app / widget` 层现有 API 不变。

### T0.1 定义核心类型（`src/core/layers.rs` 重写）

- [ ] 新增 `LayerKind` 枚举（6 档：Content/Popup/Overlay/Tooltip/Modal/System），实现 `z_base` / `z_range` / `dispatch_order`
- [ ] 新增 `LayerHandle(u64)` 句柄类型
- [ ] 新增 `Anchor` 枚举（None / Above / Below / LeftOf / RightOf / Fixed / ScreenCenter）
- [ ] 新增 `FocusPolicy` 枚举（Transparent / Dismissable / BlockBelow）
- [ ] 新增 `LayerEntry` 结构体（handle, kind, root_id, anchor, visible, focus, seq, dismiss_on_outside_click, backdrop）
- [ ] 新增 `LayerOptions` 结构体（visible, dismiss_on_outside, backdrop）
- [ ] 删除原 `LayerType` 枚举 + `LayerInfo` 单 root 结构
- [ ] 新增 `LayerStack` 结构体（`entries: RefCell<Vec<LayerEntry>>` / `kind_seq` / `next_handle` / `tree: ElementTree` / `event_manager`）
- [ ] 实现 LayerStack 方法：`new` / `push` / `remove` / `set_visible` / `update_view` / `reanchor` / `sorted_entries_for_render` / `sorted_entries_for_hit` / `top_blocking_modal` / `by_handle`
- [ ] `impl Default for LayerStack`

### T0.2 兼容现有 `Layers` 调用点

- [ ] 全局搜索 `LayerType::Base / Overlay / Modal` 的所有匹配，迁移到新 API：
  - `set_base_root(id)` → 改造为 `self.content_root = Some(id)`（Content 仍固定为单 root，在 P1 之前保留）
  - `show_overlay(root_id)` → `self.default_overlay_handle` 指向的 entry 的 `update_view`
  - `hide_overlay()` → remove default_overlay_handle
  - `show_modal(root_id)` → `self.default_modal_handle` → push Modal + ScreenCenter anchor + BlockBelow + backdrop
  - `hide_modal()` → remove default_modal_handle
  - `layer_root(lt)` → 根据 LayerType(兼容期保留一个 shadow 枚举?) 或直接 by_handle
- [ ] 兼容策略：在 `LayerStack` 内部保留 `default_overlay_handle: Cell<Option<LayerHandle>>` / `default_modal_handle` / `content_handle: LayerHandle(0)`，这样 `Runtime / state.rs` 的旧 API 调用点零改动

### T0.3 `Runtime` 层集成

- [ ] `Runtime::new(viewport)` 中把 `layers: Layers::new()` 改为 `layers: LayerStack::new()`
- [ ] 首次 `submit_view_tree` 时自动 `layers.push(Content, view, Anchor::None, Transparent, ..)` 并得到 `content_handle`
- [ ] `perform_layout()` 中：
  - 仍然对 `content_handle` 条目做完整 Flex 布局（现状流程）
  - 其他条目：先 `FlexNode.layout(UNCONSTRAINED, UNCONSTRAINED)` 拿到本征尺寸，再 `apply_anchor(entry, viewport)` 覆写 top/left
- [ ] `build_render_tree()` 改造：按 `sorted_entries_for_render()` 遍历，每个 entry 产出 backdrop（若有）+ `cv(entry.root_id, entry.z(), ..)`
- [ ] `cv()` 函数签名从 `(rid, z_index=LayerType::z_index())` 改为 `(rid, z_index=i32)` — 兼容成本低
- [ ] Modal / Overlay 显示/隐藏请求（`take_pending_modal` / `take_pending_overlay`）→ 改用 LayerStack default_*_handle

### T0.4 `state.rs` 全局通道兼容

- [ ] `show_modal(view)` → 写入 pending_modal = Some(view)
- [ ] Runtime::frame 中消费 pending_modal：若 default_modal_handle 存在则 `update_view`；否则 `push(Modal, view, Anchor::ScreenCenter, BlockBelow, LayerOptions { backdrop: Some(modal_bg), .. })` 并保存 handle
- [ ] `hide_modal()` → pending_modal = None，且在 Runtime::frame 中 remove handle
- [ ] Overlay 对应同理
- [ ] 保持 `request_rebuild` 触发（P0 保留；P1 可优化为无需 rebuild 只改 entries）

### T0.5 事件派发改造（最小化）

- [ ] `Layers::hit_test_top(point)` → 改为 `LayerStack::hit_test_top`，按 `sorted_entries_for_hit()` + `cutoff_z = top_blocking_modal().map(|e| e.z())` 实现
- [ ] 对 hit_test_rec(entry, point)：落在 entry 外且 focus=Dismissable + dismiss_on_outside_click=true → 标记，循环结束后统一 remove entries（避免遍历中修改）
- [ ] EventManager 内部的 hovered/pressed/focused 是 ElementId，不依赖 LayerType → 直接保留

### T0.6 验证 P0

- [ ] `cargo check` 通过
- [ ] `cargo test` 现有 69 测试全部通过
- [ ] `cargo clippy` 零警告
- [ ] 手动跑 `examples/hello` + `examples/gallery`：
  - show_modal / hide_modal 工作
  - Button hover / click 工作
  - 无 visual regression（截图对照）

---

## 阶段 P1：Tooltip 层落地（解决 Tooltip 被 Modal 遮挡问题，约 20% 工作量）

### T1.1 Tooltip Widget 重写

- [ ] 新增 `EventContext::target_rect(&self) -> Rect`：通过 `self.target_element_id` + `tree.layout(id).rect()` 返回触发节点的屏幕矩形
- [ ] `state.rs` 新增（非 pub，仅 crate 内）：`pending_tooltips: ThreadLocal<Vec<(ViewNode, Anchor)>>`
- [ ] `Widget for Tooltip` 重写（见设计文档 §6.2）：
  - `on_mouse_enter`：读取 anchor → push Tooltip 层 → 存 handle 到 use_state
  - `on_mouse_leave`：remove handle
  - 外层 Div 不再内嵌气泡 Div

### T1.2 Runtime 消费 Tooltip

- [ ] Runtime::frame 末尾：消费 `pending_tooltips`，对每个 push Tooltip layer；返回对应 LayerHandle 回写
- [ ] 布局阶段：Tooltip 层条目的 `apply_anchor(Above{..})` + 视口边界 clamp 处理（若溢出视口右/左侧则修正 left，溢出顶则翻到 anchor 下方）

### T1.3 验证 P1

- [ ] 在 Modal 内部放带 tooltip 的按钮，断言 tooltip 覆盖在 Modal 之上 ✅
- [ ] IconButton(24px) + tooltip("当前页顺时针旋转90°")：断言气泡宽度 ≈ 210px，文字单行 ✅
- [ ] ListView 内按钮 + tooltip：断言气泡不被 ListView overflow_scroll 裁剪 ✅

---

## 阶段 P2：多实例化 + 细粒度 API（约 12% 工作量）

### T2.1 多 Modal / 多 Overlay 支持

- [ ] state.rs 公开：
  - `pub fn show_modal_owned(view: ViewNode) -> LayerHandle`（返回句柄，不覆盖默认）
  - `pub fn hide_modal(handle: LayerHandle)`
  - `pub fn hide_top_modal()` — Esc 快捷键使用
  - Overlay 同理（Toast 场景有用：多个 Toast 堆叠，`z 2000+seq` 实现新 toast 在上）

### T2.2 Popup API

- [ ] state.rs 新增：`pub fn show_popup(view: ViewNode, anchor: Rect) -> LayerHandle`（默认 Dismissable，点击外部自动关闭）
- [ ] `pub fn hide_popup(handle: LayerHandle)`

### T2.3 System 层

- [ ] crate-internal API：`show_system_layer(view)` — 调试面板 / Draggable 幽灵节点

### T2.4 验证 P2

- [ ] 打开两个 Modal，关闭上层后下层仍在
- [ ] Popup 点击外部自动关闭，点击内部保留
- [ ] 两个 Toast 先后弹出，z 正确（新在上）

---

## 阶段 P3：性能 + 体验优化（约 8% 工作量）

### T3.1 节流 Tooltip 显示/隐藏

- [ ] 新增 `ToolTipManager`（在 LayerStack 或单独模块）：mouse_enter 设 200ms 定时器；定时器到点前 leave 取消；hide 设 100ms 防抖

### T3.2 减少 rebuild 依赖

- [ ] `show_tooltip` / `show_popup` / `show_modal` 不再触发全局 `request_rebuild()`：只在 Runtime::frame 时由 pending_* 消费 → push 到 LayerStack → perform_layout → render
- [ ] 对 Content 唯一受影响：`needs_layout = true, needs_render = true`，跳过 builder 执行

### T3.3 Anchor 溢出时的翻转

- [ ] 对 `Anchor::Above`，若 `anchor.top - height < 0`（上方不足空间）自动翻到 `Below`；`Below` 同理翻到 Above
- [ ] 水平溢出：LeftOf ↔ RightOf 翻转

### T3.4 Modal backdrop 动画位（可选）

- [ ] `LayerEntry.backdrop_opacity: f32` 渐入

---

## 每阶段退出标准

| 阶段 | 退出标准 |
|------|---------|
| P0 | 无 API 回归；现有测试全通过；examples 肉眼对比无 visual regression；clippy 0 警告 |
| P1 | 三项 Tooltip 场景测试通过；Modal 内 tooltip 可见；不被 clip 裁剪 |
| P2 | 多 Modal、多 Toast 行为正确；Popup 点外关闭 |
| P3 | Tooltip hover 无闪烁；Tooltip/Popup/Modal 打开不再触发全局 rebuild |

---

## 工作量估算

| 阶段 | 代码行数 | 预计工时 | 风险 |
|------|---------|---------|------|
| P0 | ~1200 行（layers.rs 重写 600，runtime/state 改造 400，命中测试 200） | 1.5 天 | 中：兼容期 API 若漏改会打断旧测试 |
| P1 | ~200 行 | 0.5 天 | 低 |
| P2 | ~150 行 | 0.25 天 | 低 |
| P3 | ~250 行 | 0.5 天 | 低 |
| 合计 | ~1800 行 | ≈ 3 天 | |

---

## 推荐实施顺序（严格按 P0→P1→P2→P3）

1. P0 完成后跑完整测试 + gallery 对照截图
2. P1 完成后再把 pdfkit 拿出来验证 Tooltip 在 Modal 内正常显示
3. P2 / P3 可并行做，不影响核心路径
