# Layer Stack 实施检查清单

> 在每个阶段完成后勾选对应项目，确保无遗漏

---

## P0：基础设施

### 类型定义

- [ ] `LayerKind` 有 6 种变体（Content/Popup/Overlay/Tooltip/Modal/System）
- [ ] `LayerKind::z_base()` 返回 0/1000/2000/**3000(Modal)**/**4000(Tooltip)**/5000
- [ ] `LayerKind::dispatch_order()` 从高到低返回 [System, **Tooltip, Modal**, Overlay, Popup, Content]
- [ ] `LayerHandle(u64)` 实现 Debug/Copy/Clone/PartialEq/Eq/Hash
- [ ] `Anchor` 枚举 7 种变体：None / Above{anchor,gap} / Below / LeftOf / RightOf / Fixed{x,y} / ScreenCenter
- [ ] `FocusPolicy` 枚举 3 种变体：Transparent / Dismissable / BlockBelow
- [ ] `LayerOptions` 字段：visible(bool), dismiss_on_outside(bool), backdrop(Option<Color>)
- [ ] `LayerEntry` 字段全齐：handle/kind/root_id/anchor/visible(Cell)/focus/seq/dismiss_on_outside_click/backdrop
- [ ] `LayerEntry::z()` = `kind.z_base() + seq`

### LayerStack 核心方法

- [ ] `new()` 初始化：entries 空，kind_seq=[0;6], next_handle=0
- [ ] `push(kind, view, anchor, focus, opts) -> LayerHandle`：
  - [ ] 调用 `tree.create_from_node` 得到 root_id
  - [ ] alloc_seq 单调递增（溢出风险无）
  - [ ] 生成 handle（next_handle 自增）
- [ ] `remove(handle)`：找到 pos，取出 entry，调用 `tree.remove(root_id)`
- [ ] `set_visible(handle, bool)`：找到 entry，Cell::set
- [ ] `update_view(handle, new_view)`：对 root_id 调 `tree.update_node`
- [ ] `reanchor(handle, new_anchor)`：更新 entry.anchor
- [ ] `sorted_entries_for_render()`：按 `z()` 升序
- [ ] `sorted_entries_for_hit()`：按 `z()` 降序
- [ ] `top_blocking_modal()`：返回 `visible && focus==BlockBelow && kind==Modal` 中 z 最大的 handle
- [ ] `by_handle(handle) -> Option<&LayerEntry>`（或 clone）

### 兼容旧 API

- [ ] `show_modal(view)`（旧 API）依然只显示一个：第二次调用自动 remove 第一次
- [ ] `hide_modal()`（旧 API）删除当前默认 modal
- [ ] `show_overlay(view)` / `hide_overlay()` 同理（单实例语义）
- [ ] `set_base_root(id)` 语义保留（映射到 content_handle 对应的 entry）
- [ ] `layer_root(LayerType::Base/Overlay/Modal)` 若兼容期没删除 LayerType，返回对应 entry 的 root_id

### Runtime + state.rs 集成

- [ ] `submit_view_tree`：若 Content 层还没 entry → `push(Content, view, Anchor::None, Transparent, ..)`；已存在则 update_view
- [ ] `perform_layout()`：
  - [ ] Content 走现有完整 Flex 布局
  - [ ] 其他 LayerKind：`FlexNode.layout(UNCONSTRAINED, UNCONSTRAINED)` 拿到本征尺寸 → `apply_anchor(entry, viewport)` 写回 top/left
- [ ] `apply_anchor` 所有 Anchor 变体实现：
  - [ ] None → 不改动
  - [ ] Above → `top = anchor.top - h - gap; left = anchor.left + (w - content_w) / 2`
  - [ ] Below / LeftOf / RightOf 镜像对称
  - [ ] Fixed(x,y) → 直接赋值
  - [ ] ScreenCenter → `(viewport - content) / 2`
  - [ ] 所有变体末尾 clamp：保证 `left ∈ [0, vw-cw]`, `top ∈ [0, vh-ch]`
- [ ] Modal 显示/隐藏：`take_pending_modal()` 中 consume → 对 default_modal_handle update 或新建 push
- [ ] Overlay 显示/隐藏同理
- [ ] `build_render_tree()` 遍历 sorted_entries_for_render：
  - [ ] entry 不可见跳过
  - [ ] backdrop Some → 先 Push FillRect(viewport_rect, backdrop_color) at z=entry.z()-1
  - [ ] 再 `cv(entry.root_id, entry.z())`
- [ ] cv() 签名：`fn cv(id, z: i32, &mut Vec<LayeredElement>)` — 不再依赖 LayerType::z_index

### 事件派发

- [ ] `hit_test_top(point)`：
  - [ ] 先找 cutoff_z（top_blocking_modal z），低于 cutoff 的跳过
  - [ ] 按 sorted_entries_for_hit 遍历，visible 的 entry 单独命中
  - [ ] 命中 → 返回 (Some, entry.handle)
  - [ ] 未命中 + focus=Dismissable + dismiss_on_outside_click → 记录进待 dismiss 列表
  - [ ] 循环结束后，对所有待 dismiss 列表依次 remove
- [ ] `build_hit_result`（app.rs 中）返回的 HitTestResult.target 仍然是 ElementId，不受影响

### P0 验证

- [ ] `cargo check` 通过
- [ ] `cargo test` 全部通过（≥ 69 个，保持旧测试数不变）
- [ ] `cargo clippy` 零新增警告
- [ ] `examples/hello` 运行：
  - [ ] 窗口标题/大小正常 ✅
  - [ ] Button 点击计数正常 ✅
  - [ ] hover 背景色变化正常 ✅
  - [ ] 窗口缩放无黑边 ✅
- [ ] `examples/gallery` 运行：
  - [ ] Tooltip（Button tooltip + IconButton tooltip）正常显示 ✅（虽然还在 Base 层，但文字应横向不竖排——本次修复用 explicit width 已单独完成）
  - [ ] Modal 弹框：内容 + 背景遮罩显示，Esc 关闭 ✅
  - [ ] Toast/Overlay 显示在 Base 之上 ✅

---

## P1：Tooltip 层

### Tooltip Widget 重写

- [ ] `EventContext::target_rect() -> Rect`：对 `target` ElementId 调 `tree.layout(id).rect()`（在事件执行阶段可用）
- [ ] `Widget for Tooltip`：
  - [ ] `on_mouse_enter`：`target_rect()` → `state::push_pending_tooltip(view, Anchor::Above { anchor, gap: 4.0 })`
  - [ ] `on_mouse_leave`：`state::pop_pending_tooltip(h)`，或更直接：use_state 存 Option<LayerHandle> → leave 时 remove
- [ ] state.rs crate-internal：`PENDING_TOOLTIPS` 通道 + Tooltip 层 push/remove 封装

### Anchor 翻转（防止出屏幕）

- [ ] `Anchor::Above`：若 `anchor.top - h - gap < 0` → 自动改为 Below
- [ ] `Anchor::Below`：若 `anchor.bottom + h + gap > vh` → 自动改为 Above
- [ ] LeftOf/RightOf 水平翻转同理

### P1 验证

- [ ] 在 Modal 内放一个 IconButton + tooltip("我是 Tooltip")：
  - [ ] 鼠标悬停：tooltip 出现在按钮上方，不被 Modal 遮挡 ✅
  - [ ] 点击 tooltip 区域：不触发 Modal 内容事件（Tooltip 是上层但 FocusPolicy=Transparent，用户点击穿透回 Modal 顶部条目 ✅ 或 BlockBelow 都可；按设计文档选 Transparent 使点击穿过 Tooltip 到 Modal 本身）
- [ ] IconButton（24px）tooltip："当前页顺时针旋转90°" 宽度 ~ 14*12 + 16 = 184px，文字不换行 ✅
- [ ] ListView overflow_scroll 内按钮的 tooltip：气泡越过 ListView 边界正常显示，未被裁剪 ✅
- [ ] 全窗口 resize 后重新 hover，Tooltip anchor 基于新 rect 计算 ✅

---

## P2：多实例 + 细粒度 API

### API 面

- [ ] `pub fn show_modal_owned(view: ViewNode) -> LayerHandle` — 不覆盖默认
- [ ] `pub fn hide_modal(handle: LayerHandle)`
- [ ] `pub fn hide_top_modal()` — z 最大的那个 Modal（Esc 快捷关闭绑定应用侧）
- [ ] Overlay 同理：`show_toast(view) -> LayerHandle` + `hide_toast(handle)`
- [ ] Popup：`show_popup(view, anchor_rect) -> LayerHandle`（FocusPolicy=Dismissable, dismiss_on_outside=true）+ `hide_popup(handle)`

### 验证

- [ ] 连开两个 Modal：
  - [ ] 第二个覆盖在第一个之上 ✅
  - [ ] 第二个的 BlockBelow 阻止点击穿到第一个 Modal ✅（点击第一个 Modal 背景不生效）
  - [ ] 关掉第二个后，第一个 Modal 仍在并可交互 ✅
- [ ] 两个 Toast 先后弹出：z 大的覆盖在 z 小的之上 ✅
- [ ] Popup 打开后点击 Popup 外任意位置 → Popup 关闭 ✅；点内部不关闭 ✅
- [ ] **验证 P2（Modal 内 Popup 顺序）**：若 `Popup` 在 Modal 内部打开，需要 `Associated(parent_modal_handle)` 定位模式使 Popup.z = parent.z + 段内 offset；否则 `Popup.z_base=1000 < Modal.z_base=3000` 会被 Modal 覆盖。详见 P2 设计补遗

---

## P3：性能与体验

### Tooltip 节流

- [ ] mouse_enter → 启动 200ms 定时器（由 about_to_wait 轮询 + Instant 检查）
- [ ] 200ms 内 mouse_leave → 取消（设置 cancelled 标志）
- [ ] mouse_leave → 100ms 后 hide（进入后 cancel 避免闪烁）

### 避免 rebuild

- [ ] `show_modal / show_popup / show_tooltip / show_tooltip` 不再设置 `request_rebuild`：走独立 pending_* 通道
- [ ] Runtime::about_to_wait 或下次 user event 时消费 pending_* 并执行 push / remove
- [ ] 仅设置 `needs_layout = true, needs_render = true`，builder 闭包不执行 ✅

### Anchor 翻转完整验证

- [ ] 按钮在屏幕最顶行 → Above 自动翻转到 Below ✅
- [ ] 按钮在屏幕底行 → Below 翻转到 Above ✅
- [ ] 按钮在屏幕左边缘 → LeftOf 翻转到 RightOf ✅
- [ ] 按钮在屏幕右边缘 → RightOf 翻转到 LeftOf ✅

---

## 全局回归

- [ ] 每阶段后 `cargo test` 保持通过
- [ ] `cargo fmt --all` 无差异
- [ ] `cargo clippy --all-targets -- -D warnings` 0 警告
- [ ] `examples/gallery` 所有组件视觉对照截图（在 PDFKit 文档的 checklist 对应 SVG 可自动生成作回归）
