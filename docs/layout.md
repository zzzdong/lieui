> ⚠️ **本文档已过时**（描述旧 `Widget` 布局架构）。布局引擎本身（Flexbox / Taitank 移植）仍可参考，但其上层的 `ViewContext`/`Widget` 集成已不存在。
> 现行集成入口为 `layout::context::LayoutContext` + `view::node::to_flex_style` + `layout::flex_node`。
> **请以 [`../guide.md`](../guide.md) 为最新权威文档**；本文档仅留作历史参考。

# 布局系统文档

## 概述

LieUI 采用**两阶段布局系统**：
1. **收集约束**（Collect）：从 WidgetTree 收集布局约束
2. **计算位置**（Compute）：根据约束计算最终布局

## 核心概念

### LayoutNode

单个组件的布局信息：

```rust
pub struct LayoutNode {
    pub id: WidgetId,
    pub style: BoxStyle,           // 盒模型样式
    pub flex: Option<FlexStyle>,   // Flex 属性
    pub intrinsic: IntrinsicSize,  // 固有尺寸
    pub computed: Option<ComputedLayout>, // 计算结果
}
```

### BoxStyle

盒模型样式：

```rust
pub struct BoxStyle {
    pub margin: EdgeInsets,      // 外边距
    pub padding: EdgeInsets,     // 内边距
    pub border: EdgeInsets,      // 边框宽度
    pub border_color: Option<Color>,
    pub background: Option<Color>,
    pub border_radius: Option<f32>,
    pub width: Option<f32>,      // 固定宽度
    pub height: Option<f32>,    // 固定高度
}
```

### FlexStyle

Flex 布局属性：

```rust
pub struct FlexStyle {
    pub direction: FlexDirection,      // 主轴方向
    pub justify: JustifyContent,       // 主轴对齐
    pub align_items: AlignItems,       // 交叉轴对齐
    pub wrap: bool,                    // 是否换行
    pub gap: f32,                      // 子项间距
    pub grow: f32,                     // 伸展因子
    pub shrink: f32,                   // 收缩因子
    pub basis: Option<f32>,            // 基础尺寸
}
```

### ComputedLayout

计算后的布局结果：

```rust
pub struct ComputedLayout {
    pub x: f32,                    // 左上角 X
    pub y: f32,                    // 左上角 Y
    pub width: f32,               // 宽度
    pub height: f32,              // 高度
    pub content_box: Rect,        // 内容区域
    pub padding_box: Rect,        // 内边距区域
    pub border_box: Rect,         // 边框区域
    pub margin_box: Rect,         // 外边距区域
}
```

## 布局流程

### 1. 收集约束

```rust
// ViewContext::perform_layout()
layout_ctx.collect(root_id, self);
```

遍历 WidgetTree，收集每个 Widget 的 `LayoutNode`：

```rust
// LayoutContext::collect()
fn collect(&mut self, widget_id: WidgetId, ctx: &ViewContext) {
    if let Some(widget) = ctx.get_widget(widget_id) {
        let layout_node = widget.layout(widget_id);
        self.nodes.insert(widget_id, layout_node);

        // 递归收集子节点
        for child_id in widget.children() {
            self.collect(*child_id, ctx);
        }
    }
}
```

### 2. 计算位置

```rust
// ViewContext::perform_layout()
layout_ctx.compute(self.viewport);
```

根据约束计算每个节点的位置和尺寸：

```rust
// LayoutContext::compute()
fn compute(&mut self, viewport: Size) {
    // 1. 计算 Flex 布局
    self.compute_flex_layout(viewport);

    // 2. 计算绝对定位
    self.compute_absolute_layout();
}
```

## Flex 布局

### 主轴方向

```rust
pub enum FlexDirection {
    Row,         // 水平排列
    RowReverse,  // 水平反向
    Column,      // 垂直排列
    ColumnReverse, // 垂直反向
}
```

### 主轴对齐

```rust
pub enum JustifyContent {
    Start,       // 起点对齐
    End,         // 终点对齐
    Center,      // 居中对齐
    SpaceBetween, // 两端对齐
    SpaceAround,  // 均匀分布（含两端间距）
    SpaceEvenly,  // 均匀分布（等间距）
}
```

### 交叉轴对齐

```rust
pub enum AlignItems {
    Start,       // 起点对齐
    End,         // 终点对齐
    Center,      // 居中对齐
    Stretch,     // 拉伸填充
    Baseline,    // 基线对齐
}
```

## 使用示例

### 固定尺寸

```rust
impl Widget for MyWidget {
    fn layout(&self, id: WidgetId) -> LayoutNode {
        LayoutNode::new(id)
            .with_style(BoxStyle {
                width: Some(100.0),
                height: Some(50.0),
                ..Default::default()
            })
    }
}
```

### Flex 容器

```rust
impl Widget for Column {
    fn layout(&self, id: WidgetId) -> LayoutNode {
        LayoutNode::new(id)
            .with_style(BoxStyle::default())
            .with_flex(FlexStyle {
                direction: FlexDirection::Column,
                justify: JustifyContent::Start,
                align_items: AlignItems::Stretch,
                gap: self.spacing,
                ..Default::default()
            })
    }
}
```

### 固有尺寸

```rust
impl Widget for Text {
    fn layout(&self, id: WidgetId) -> LayoutNode {
        LayoutNode::new(id)
            .with_intrinsic_size(IntrinsicSize::Measurable(
                TextMeasure::new(self.content.clone(), self.style.clone())
            ))
    }
}
```

## 盒模型

### 尺寸计算

```
┌─────────────────────────────────────┐
│              Margin                 │
│  ┌─────────────────────────────┐   │
│  │           Border            │   │
│  │  ┌─────────────────────┐   │   │
│  │  │       Padding       │   │   │
│  │  │  ┌───────────────┐  │   │   │
│  │  │  │    Content    │  │   │   │
│  │  │  │   (width x    │  │   │   │
│  │  │  │    height)    │  │   │   │
│  │  │  └───────────────┘  │   │   │
│  │  └─────────────────────┘   │   │
│  └─────────────────────────────┘   │
└─────────────────────────────────────┘

总宽度 = margin.left + border.left + padding.left + width + padding.right + border.right + margin.right
```

### EdgeInsets

```rust
pub struct EdgeInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl EdgeInsets {
    pub fn all(value: f32) -> Self { ... }
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self { ... }
    pub fn only(left: f32, top: f32, right: f32, bottom: f32) -> Self { ... }
}
```

## 约束系统

### LayoutConstraint

父组件对子组件的尺寸约束：

```rust
pub struct LayoutConstraint {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}
```

### 约束应用

```rust
impl LayoutConstraint {
    /// 将尺寸限制在约束范围内
    pub fn clamp(&self, size: Size) -> Size {
        Size::new(
            size.width.clamp(self.min_width, self.max_width),
            size.height.clamp(self.min_height, self.max_height),
        )
    }
}
```

## 可测量尺寸

### Measurable Trait

用于需要动态计算尺寸的 Widget（如 Text）：

```rust
pub trait Measurable: Send + Sync {
    fn measure(&self, max_width: Option<f32>) -> Size;
    fn clone_box(&self) -> Box<dyn Measurable>;
}
```

### TextMeasure

```rust
pub struct TextMeasure {
    pub content: String,
    pub style: TextStyle,
}

impl Measurable for TextMeasure {
    fn measure(&self, max_width: Option<f32>) -> Size {
        let layout = TextEngine::layout(&self.content, &self.style, 1.0, max_width);
        Size::new(layout.width(), layout.height())
    }
}
```

## 最佳实践

1. **优先使用固有尺寸**：让内容决定尺寸，而非固定值
2. **合理使用 Flex**：利用 `grow` 和 `shrink` 实现弹性布局
3. **最小化约束**：只提供必要的约束信息
4. **缓存测量结果**：避免重复计算文本布局

## 参考

- [架构设计文档](./architecture.md)
- [Widget 系统文档](./widget.md)
- [渲染系统文档](./render.md)
