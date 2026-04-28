# lieui 布局系统设计文档

## 1. 架构概览

布局系统采用 **收集-计算** 两阶段设计：

```
Widget 树  →  collect()   →  LayoutNode 树 (约束)  →  compute()   →  ComputedLayout (位置)
                   Phase 1                                    Phase 2
```

- **Phase 1 (collect)**：遍历 Widget 树，调用每个 Widget 的 `layout()` 方法，收集其布局约束信息（BoxStyle、FlexStyle、IntrinsicSize），构建一棵同形的 `LayoutNode` 树。这棵树是瞬时的——每次 layout 都重新构建。
- **Phase 2 (compute)**：从根节点开始，用 `tight(viewport)` 约束递归计算每个节点的最终位置，写入 `node.computed`。

## 2. 核心类型

### 2.1 盒模型 (`BoxStyle`)

每个 Widget 是一个四层盒子：**Margin → Border → Padding → Content**

```rust
pub struct BoxStyle {
    pub margin: EdgeInsets,
    pub border: EdgeInsets,
    pub padding: EdgeInsets,
    pub min_size: Size,
    pub max_size: Size,
}
```

- `EdgeInsets`：`top/right/bottom/left`，提供 `deflate(size)` 从可用空间中减去自身。
- `BoxStyle::content_available(available)`：依次减去 margin、border、padding，返回内容区可用尺寸。
- `Size::clamp(min, max)`：将尺寸约束在 `[min_size, max_size]` 范围内。

### 2.2 布局约束 (`LayoutConstraint`)

```rust
pub struct LayoutConstraint {
    pub min_width: f32, pub max_width: f32,
    pub min_height: f32, pub max_height: f32,
}
```

关键方法：
- `tight(size)` — `min == max`，强制固定尺寸。
- `loose(size)` — `min=0, max=size`，子元素可在范围内自由决定。
- `UNBOUNDED` — `min=0, max=INFINITY`，无限制。

`LayoutConstraint` 贯穿整个布局流程，父容器通过它向子节点传递尺寸限制。

### 2.3 布局节点 (`LayoutNode`)

```
LayoutNode {
    id: WidgetId,
    box_style: BoxStyle,
    flex_style: Option<FlexStyle>,     // 容器：Flex 配置
    intrinsic_size: IntrinsicSize,      // 叶子：固有尺寸
    flex_grow: f32, flex_shrink: f32, flex_basis: Option<f32>,
    children: Vec<LayoutNode>,
    computed: Option<ComputedLayout>,   // Phase 2 写入
}
```

**节点类型判定**：
- `children.is_empty()` → 叶子节点（使用 `intrinsic_size` 测量）
- `flex_style.is_some()` → Flex 容器
- 其他 → 流式容器（纵向堆叠，参见 `flow_layout`）

#### 固有尺寸 (`IntrinsicSize`)

```rust
pub enum IntrinsicSize {
    Fixed(Size),                              // 固定尺寸
    Measurable(Box<dyn Measurable>),          // 可动态测量（Text 等）
}
```

`Measurable` trait：
```rust
pub trait Measurable: Send + Sync {
    fn measure(&self, max_width: Option<f32>) -> Size;
}
```

内置实现：`TextMeasure`（通过 parley 文本引擎）、`FixedMeasure`（固定尺寸封装）。

#### 计算结果 (`ComputedLayout`)

```rust
pub struct ComputedLayout {
    pub margin_box: Rect,   // 最外层
    pub border_box: Rect,   // margin + border
    pub padding_box: Rect,  // margin + border + padding
    pub content_box: Rect,  // 实际内容区域
}
```

### 2.4 Flex 配置

```rust
pub enum FlexDirection { Row, Column }
pub enum JustifyContent { Start, End, Center, SpaceBetween, SpaceAround, SpaceEvenly }
pub enum AlignItems { Start, End, Center, Stretch }

pub struct FlexStyle {
    pub direction: FlexDirection,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
    pub gap: f32,
}
```

## 3. 布局算法

### 3.1 入口

```rust
pub fn compute(&mut self, viewport: Size) {
    if let Some(root) = &mut self.root {
        let constraint = LayoutConstraint::tight(viewport);
        Self::layout_node(root, constraint, Point::ZERO);
    }
}
```

### 3.2 `layout_node` — 递归布局，写 `computed`

```
layout_node(node, constraint, position) → Size (margin-box)
  1. constraint.max_size() → BoxStyle::content_available() → content_avail
  2. 根据节点类型计算 content_size:
     - 叶子: intrinsic_size.measure(max_width)
     - Flex: flex_layout(node, flex, content_avail, content_origin)
     - 流式: flow_layout(node, content_avail, content_origin)
  3. content_size.clamp(min_size, max_size) → final_content
  4. 从 position + margin + border + padding 计算四层盒子:
     content_box → padding_box → border_box → margin_box
  5. 写入 node.computed = Some(ComputedLayout { ... })
  6. 返回 margin_box.size()
```

关键点：
- `content_avail` 是 content 区域的可用尺寸（已减去 margin/border/padding）。
- `content_origin` 是 content 区域的原点（position + margin + border + padding），传递给子布局。
- `flex_style` 需要 `.clone()` 后使用，以避免 `&node` 和 `&mut node.children` 的借用冲突。

### 3.3 `measure_node` — 只读测量

**无副作用**的递归测量函数，返回 **border-box 尺寸**（content + padding + border）。用于 flex 布局 Phase 1 中子元素的尺寸估算。

```
measure_node(node, constraint) → Size (border-box)
  1. constraint.max_size() → content_avail
  2. 根据节点类型测量 content_size:
     - 叶子: intrinsic_size.measure(max_width)
     - Flex: measure_flex_content(node, flex, content_avail)
     - 流式: measure_flow_content(node, content_avail)
  3. clamp(min, max) → 添加 padding + border → 返回
```

**为什么返回 border-box 而非 content-box？** Flex 布局的空间分配以 border-box 为单位。父容器需要知道子元素占据的总尺寸（含 padding + border），才能正确计算剩余空间。

#### `measure_flex_content` — Flex 容器只读测量

对子元素施加松约束（主轴无限制，交叉轴约束在容器交叉轴范围内），递归测量每个子元素，累加主轴和交叉轴尺寸，返回容器估算的内容尺寸。

#### `measure_flow_content` — 流式容器只读测量

对子元素施加宽度约束 `content_avail.width`、高度无限制，纵向堆叠测量，返回 `(max_width, total_height)`。

### 3.4 `flex_layout` — 5 阶段 Flex 算法

```
Phase 1 — 测量子元素
  - 构建子元素测量约束：主轴 UNBOUNDED，交叉轴 LOOSE(container_cross)
  - 对每个子元素调用 measure_node()，提取 main/cross
  - flex_basis 覆盖主轴尺寸

Phase 2 — 分配弹性空间
  - 计算总主轴消耗 = 子元素主轴和 + gaps
  - free_space = available_main - total_main
  - free_space > 0: flex-grow 按比例分配剩余空间
  - free_space < 0: flex-shrink 按 (main_size × flex_shrink) 加权收缩

Phase 3 — 交叉轴尺寸
  - container_cross = fill_available || Stretch ? available_cross : max(child_cross)

Phase 4 — 子元素定位
  - justify-content 计算主轴起始偏移
  - align-items 计算交叉轴偏移（Start/Center/End/Stretch）
  - 对每个子元素调用 layout_node() 写入 computed

Phase 5 — 返回容器内容尺寸
  - fill_available (flex_grow > 0): 返回 content_avail（填满可用空间）
  - 否则: 返回 (actual_total_main, container_cross)
```

### 3.5 `flow_layout` — 流式布局

纵向堆叠子元素，每个子元素位置递增 y_offset：

```
flow_layout(node, content_avail, content_origin) → Size
  child_constraint = LayoutConstraint::loose(content_avail)
  for child in node.children:
    child_pos = content_origin + (0, y_offset)
    size = layout_node(child, child_constraint, child_pos)
    y_offset += size.height
    max_width = max(max_width, size.width)
  return (max_width, y_offset)
```

### 3.6 轴辅助函数

```rust
fn main_size(size, direction)    → Row: width,  Column: height
fn cross_size(size, direction)   → Row: height, Column: width
fn main_component(point, dir)    → Row: x,      Column: y
fn make_size(main, cross, dir)   → Row: (main, cross), Column: (cross, main)
fn justify_offset(justify, start, free_space, n)  → 主轴起始偏移
fn justify_gap(justify, free_space, n)            → 间距（Space* 系列）
```

## 4. Widget 的布局职责

Widget trait 中与布局相关的核心方法：

```rust
pub trait Widget: Any {
    /// 返回布局约束节点
    fn layout(&self, id: WidgetId) -> LayoutNode;

    /// 根据计算的布局生成渲染节点
    fn render(&self, layout: &LayoutNode, ctx: &ViewContext) -> RenderNode;

    /// 脏标记：状态变化时返回 true，触发自动重布局和渲染
    fn is_dirty(&self) -> bool { false }
    fn clear_dirty(&mut self) {}
}
```

**Widget 与布局的关系**：
- **layout()**：Widget 在此方法中声明自己的布局约束（BoxStyle、FlexStyle、IntrinsicSize），但不做任何位置计算。位置由 `LayoutContext::compute()` 统一计算。
- **render()**：Widget 根据 `layout.computed` 中的位置信息生成渲染树节点。
- **is_dirty()**：Widget 的状态变化后（如 Button 悬停、Text 内容改变），返回 true 触发布局系统在下一帧自动重建。

**常见 Widget 的 layout 实现**：

| Widget | BoxStyle | FlexStyle | IntrinsicSize |
|--------|----------|-----------|---------------|
| Text | 无 | 无 | Measurable(TextMeasure) |
| Container | padding/border | 无 | Fixed(ZERO) |
| Column | 无 | column + AlignItems::Center | Fixed(ZERO) |
| Row | 无 | row + AlignItems::Center | Fixed(ZERO) |
| Button | padding + min_size | 无 | Fixed(文本+内边距) |

## 5. 脏标记系统

Widget 通过 `is_dirty()` 声明自身是否需要重布局和重渲染。框架自动检测：

```
Event → Widget.handle_event() → state changed → self.dirty = true
                                                        ↓
ViewContext::render() → scan_dirty_flags() → 有 dirty  → invalidate_layout()
                                                        ↓
perform_layout() → rebuild layout tree → build_render_tree() → clear_dirty_flags()
```

**规则**：
- 任何 `dirty == true` 的 Widget 都会触发完整的**重布局+重渲染**（不区分 render-only 和 layout-only，保持简单）。
- dirty 标记在渲染完成后由 `clear_dirty_flags()` 统一清除。
- 框架在 `render()` 入口处扫描一次，事件处理器中无需手动触发。

**当前实现了脏标记的 Widget**：

| Widget | 何时设脏 | 原因 |
|--------|---------|------|
| Button | MouseEnter/MouseLeave/MouseDown/MouseUp | 视觉状态变化 |
| Text | set_content() / content() | 内容/尺寸变化 |

## 6. 事件处理与布局的集成

事件处理在 `ViewContext` 中统一管理：

```rust
pub fn handle_mouse_up(&mut self, point: Point, button: MouseButton) {
    if self.needs_layout {
        self.perform_layout();  // 确保 hit test 前布局已更新
    }
    if let Some(layout_root) = self.layout_ctx.root.clone() {
        let mut event_ctx = EventContext::new(&self.widget_tree.widgets);
        self.event_handler.handle_mouse_up(point, button, &layout_root, &mut event_ctx);
        // 事件处理完成，脏标记由下一帧 render() 自动处理
    }
}
```

**流程**：
1. 事件到达 → `handle_mouse_xxx()` → 三阶段传播（捕获 → 目标 → 冒泡）
2. Widget 的 `handle_event()` 修改内部状态并设置 `dirty = true`
3. 事件处理器返回 → **不立即触发布局/渲染**
4. 下一帧 `render()` → `scan_dirty_flags()` → 发现 dirty → `perform_layout()` → `build_render_tree()`

**事件传播三阶段**：
```
捕获: 根 → ... → 目标父节点
目标: 目标节点
冒泡: 目标父节点 → ... → 根
```
任何阶段的 `EventResult::Stop` 会终止传播。

## 7. 命中测试

`LayoutNode::hit_test(point)` 从根节点递归，**从后向前**遍历子元素（后绘制的在上层），返回最先匹配的 `WidgetId`。

匹配规则：
1. 检查 `margin_box` 是否包含 point（不包含则返回 None）
2. 递归子元素（从后向前），优先返回子元素命中结果
3. 如果子元素都没有命中，检查 `content_box` 是否包含 point
4. 都不命中返回 None

## 8. 完整布局流程示例

```
Container (root, 800x600)
  └── Column (expand, align:Center)
        ├── Text "Counter" (font_size:48)
        ├── Text count (font_size:72)
        └── Row (spacing:16)
              ├── Button "-"
              ├── Button "+"
              └── Button "Reset"
```

**执行顺序**：

```
1. collect(root):
   → widget.layout() → LayoutNode(Container, no flex)
     → widget.layout() → LayoutNode(Column, flex, flex_grow=1)
       → widget.layout() → LayoutNode(Text "Counter")
       → widget.layout() → LayoutNode(Text count)
       → widget.layout() → LayoutNode(Row, flex)
         → widget.layout() → LayoutNode(Button "-")
         → widget.layout() → LayoutNode(Button "+")
         → widget.layout() → LayoutNode(Button "Reset")

2. compute(800x600):
   → layout_node(Container, tight(800,600), (0,0))
     → Container has children, no flex → flow_layout
       → layout_node(Column, loose(800,∞), content_origin)
         → Column has flex → flex_layout
           Phase 1: measure_node(ContentColumn, unbounded) → etc
           Phase 2: free_space = ∞ - total → flex-grow fills
           Phase 3: container_cross = available_cross (width=800)
           Phase 4: position children with center alignment
           Phase 5: fill_available → return (800, ∞)
         → clamp(800, ∞) → (800, ∞) → compute boxes
       → flow_layout returns (800, content_height)
     → clamp → compute boxes → root has computed

3. render():
   → scan_dirty_flags → all clean → use existing layout
   → build_render_tree → recursive LayoutNode → RenderNode
```

## 9. 类型系统总览

```
src/layout/
├── mod.rs              # 重新导出所有公开类型
├── context.rs          # LayoutContext (collect + compute)
│   ├── collect()/collect_node()     # Phase 1: 构建 LayoutNode 树
│   ├── compute()                     # Phase 2: 计算布局
│   ├── layout_node()                 # 递归布局，写 computed
│   ├── measure_node()                # 只读测量
│   ├── measure_flex_content()        # Flex 容器只读测量
│   ├── measure_flow_content()        # 流式容器只读测量
│   ├── flex_layout()                 # 5 阶段 Flex 算法
│   ├── flow_layout()                 # 纵向堆叠布局
│   └── 轴辅助函数 (main_size, make_size, justify_offset...)
├── node.rs             # LayoutNode, IntrinsicSize
├── box_model.rs        # BoxStyle, EdgeInsets, ComputedLayout
├── constraint.rs       # LayoutConstraint
├── flex.rs             # FlexDirection, JustifyContent, AlignItems, FlexStyle
└── measurable.rs       # Measurable trait, TextMeasure, FixedMeasure
```

## 10. 与旧版的差异

| 旧版 (重构前) | 新版 (当前) |
|:---|:---|
| `compute_node(node, Size, pos)` — 裸 Size | `layout_node(node, LayoutConstraint, pos)` |
| 无 `LayoutConstraint` 使用 | `tight()` / `loose()` 贯穿约束传递 |
| `resolve_intrinsic_size` + `resolve_content_size` | `measure_node()` 统一只读测量 |
| `estimate_child` 返回 `(f32, f32)` 元组 | `measure_node` 返回 `Size` |
| `compute_default_children` 子元素位置重叠 | `flow_layout` 纵向堆叠 |
| 内联 `padding.deflate(border.deflate(...))` | `BoxStyle::content_available()` |
| `ctx.effects().request_render()` 手动触发 | `is_dirty()` 自动检测 |
| `compute_flex_children` 5 阶段算法 | `flex_layout` 5 阶段算法（功能相同） |
