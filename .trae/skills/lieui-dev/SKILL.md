---
name: "lieui-dev"
description: "LieUI Rust GUI 库开发助手。在实现 Widget、布局、渲染、事件系统时调用，提供代码生成和架构指导。"
---

# LieUI 开发助手

LieUI 是一个极简 Rust GUI 库，采用**声明式布局** + **两阶段布局计算**架构。

## 核心设计原则

1. **Widget 只声明约束**：通过 `layout()` 方法返回 `LayoutNode`，不直接计算位置
2. **LayoutContext 统一计算**：两阶段布局（约束收集 → 位置计算）
3. **CSS Box 模型**：支持 margin、padding、border
4. **Flex 布局**：支持 direction、justify_content、align_items、gap
5. **固有尺寸**：`IntrinsicSize::Fixed` 或 `Measurable`（用于文本）

## 最新 Widget Trait 定义

```rust
pub trait Widget: Any {
    fn type_name(&self) -> &'static str;

    /// 返回布局节点（仅约束，不计算位置）
    /// 
    /// # 参数
    /// - `id`: Widget 的 ID，必须用于创建 LayoutNode
    fn layout(&self, id: WidgetId) -> LayoutNode;

    /// 根据已计算的布局生成渲染节点
    fn render(&self, layout: &LayoutNode, ctx: &ViewContext) -> RenderNode;

    /// 命中测试（默认使用 layout.computed.content_box）
    fn hit_test(&self, point: Point, layout: &LayoutNode) -> bool;

    /// 处理事件
    fn handle_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Continue
    }

    fn children(&self) -> &[WidgetId] { &[] }
    fn children_mut(&mut self) -> &mut Vec<WidgetId> { unimplemented!() }
    fn can_focus(&self) -> bool { false }
    fn bounds(&self) -> Option<Rect> { None }
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// 为 Widget 实现 as_any 的宏
#[macro_export]
macro_rules! impl_widget_any {
    ($type:ty) => {
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    };
}
```

## 布局系统核心类型

### LayoutNode - 布局约束节点

```rust
pub struct LayoutNode {
    pub widget_id: WidgetId,
    pub box_style: BoxStyle,              // CSS Box 模型
    pub flex_style: Option<FlexStyle>,    // Flex 布局配置
    pub intrinsic_size: IntrinsicSize,    // 固有尺寸
    pub children: Vec<LayoutNode>,
    pub computed: Option<ComputedLayout>, // 计算结果（由 LayoutContext 填充）
    pub flex_grow: f32,
    pub flex_shrink: f32,
}

impl LayoutNode {
    pub fn new(id: WidgetId) -> Self { ... }
    pub fn with_box_style(mut self, style: BoxStyle) -> Self { ... }
    pub fn with_flex(mut self, flex: FlexStyle) -> Self { ... }
    pub fn with_intrinsic_size(mut self, size: IntrinsicSize) -> Self { ... }
    pub fn with_fixed_size(mut self, size: Size) -> Self { ... }
    pub fn with_flex_grow(mut self, grow: f32) -> Self { ... }
    pub fn add_child(mut self, child: LayoutNode) -> Self { ... }
    pub fn bounds(&self) -> Option<Rect> { ... }
}
```

### BoxStyle - CSS Box 模型

```rust
pub struct BoxStyle {
    pub margin: EdgeInsets,
    pub padding: EdgeInsets,
    pub border: EdgeInsets,
    pub min_size: Size,
    pub max_size: Size,
}

pub struct EdgeInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl EdgeInsets {
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self { ... }
    pub const fn all(value: f32) -> Self { ... }
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self { ... }
    pub const fn only(left: f32, top: f32, right: f32, bottom: f32) -> Self { ... }
    pub const fn left(value: f32) -> Self { ... }
    pub const fn right(value: f32) -> Self { ... }
    pub const fn top(value: f32) -> Self { ... }
    pub const fn bottom(value: f32) -> Self { ... }
    pub const fn horizontal(value: f32) -> Self { ... }
    pub const fn vertical(value: f32) -> Self { ... }
}
```

### FlexStyle - Flex 布局

```rust
pub struct FlexStyle {
    pub direction: FlexDirection,      // Row | Column
    pub justify_content: JustifyContent, // Start | Center | End | SpaceBetween | SpaceAround | SpaceEvenly
    pub align_items: AlignItems,       // Start | Center | End | Stretch
    pub gap: f32,
}

impl FlexStyle {
    pub fn row() -> Self { ... }
    pub fn column() -> Self { ... }
    pub fn direction(mut self, direction: FlexDirection) -> Self { ... }
    pub fn justify(mut self, justify: JustifyContent) -> Self { ... }
    pub fn align(mut self, align: AlignItems) -> Self { ... }
    pub fn gap(mut self, gap: f32) -> Self { ... }
}
```

### IntrinsicSize - 固有尺寸

```rust
pub enum IntrinsicSize {
    Fixed(Size),                    // 固定尺寸
    Measurable(Box<dyn Measurable>), // 动态测量（如 Text）
}

impl IntrinsicSize {
    pub fn measure(&self, max_width: Option<f32>) -> Size { ... }
}

/// 可测量 trait
pub trait Measurable: Send + Sync {
    fn measure(&self, max_width: Option<f32>) -> Size;
    fn clone_box(&self) -> Box<dyn Measurable>;
}

/// 文本测量器
pub struct TextMeasure {
    pub content: String,
    pub style: TextStyle,
}

impl Measurable for TextMeasure {
    fn measure(&self, max_width: Option<f32>) -> Size { ... }
    fn clone_box(&self) -> Box<dyn Measurable> { ... }
}
```

## 内置控件

### Text - 文本

```rust
pub struct Text {
    content: String,
    style: TextStyle,
}

impl Widget for Text {
    crate::impl_widget_any!(Text);

    fn type_name(&self) -> &'static str { "Text" }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        let measure = TextMeasure::new(self.content.clone(), self.style.clone());
        LayoutNode::new(id)
            .with_intrinsic_size(IntrinsicSize::Measurable(Box::new(measure)))
    }

    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            let text_layout = self.do_layout(Some(computed.content_box.width));
            RenderNode::text(computed.content_box, text_layout)
        } else {
            RenderNode::view(Rect::zero())
        }
    }
}
```

### Button - 按钮

```rust
pub struct Button {
    text: String,
    bounds: Rect,
}

impl Widget for Button {
    crate::impl_widget_any!(Button);

    fn type_name(&self) -> &'static str { "Button" }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        let text_measure = TextMeasure::new(self.text.clone(), TextStyle::new());
        let text_node = LayoutNode::new(WidgetId::new())
            .with_intrinsic_size(IntrinsicSize::Measurable(Box::new(text_measure)));

        LayoutNode::new(id)
            .with_box_style(BoxStyle {
                padding: EdgeInsets::symmetric(16.0, 8.0),
                ..Default::default()
            })
            .with_flex(FlexStyle::row().align(AlignItems::Center))
            .add_child(text_node)
    }

    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            RenderNode::div(computed.content_box)
                .background("#1976D2")
                .border_radius(4.0)
        } else {
            RenderNode::view(Rect::zero())
        }
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { button: MouseButton::Left, .. } => {
                EventResult::Stop
            }
            _ => EventResult::Continue,
        }
    }
}
```

### Column - 垂直布局

```rust
pub struct Column {
    children: Vec<WidgetId>,
    spacing: f32,
    expand: bool,
    bounds: Rect,
}

impl Widget for Column {
    crate::impl_widget_any!(Column);

    fn type_name(&self) -> &'static str { "Column" }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        use crate::layout::JustifyContent;

        let mut node = LayoutNode::new(id).with_flex(
            FlexStyle::column()
                .align(AlignItems::Center)
                .justify(JustifyContent::Center)  // 垂直居中
                .gap(self.spacing),
        );

        if self.expand {
            node = node.with_flex_grow(1.0);
        }

        node
    }

    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            RenderNode::view(computed.content_box)
        } else {
            RenderNode::view(Rect::zero())
        }
    }

    fn children(&self) -> &[WidgetId] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<WidgetId> { &mut self.children }
    fn bounds(&self) -> Option<Rect> { Some(self.bounds) }
}
```

### Row - 水平布局

```rust
pub struct Row {
    children: Vec<WidgetId>,
    spacing: f32,
    bounds: Rect,
}

impl Widget for Row {
    crate::impl_widget_any!(Row);

    fn type_name(&self) -> &'static str { "Row" }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        LayoutNode::new(id).with_flex(
            FlexStyle::row()
                .align(AlignItems::Center)
                .gap(self.spacing),
        )
    }

    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            RenderNode::view(computed.content_box)
        } else {
            RenderNode::view(Rect::zero())
        }
    }

    fn children(&self) -> &[WidgetId] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<WidgetId> { &mut self.children }
    fn bounds(&self) -> Option<Rect> { Some(self.bounds) }
}
```

## 渲染节点

```rust
pub enum RenderNode {
    View { bounds: Rect },
    Div { bounds: Rect, style: DivStyle },
    Text { bounds: Rect, layout: TextLayout },
}

pub struct DivStyle {
    pub background: Option<Color>,
    pub border_color: Option<Color>,
    pub border_width: f32,
    pub border_radius: f32,
}

impl RenderNode {
    pub fn view(bounds: Rect) -> Self { ... }
    pub fn div(bounds: Rect) -> Self { ... }
    pub fn text(bounds: Rect, layout: TextLayout) -> Self { ... }
    pub fn background(mut self, color: impl Into<Color>) -> Self { ... }
    pub fn border_color(mut self, color: impl Into<Color>) -> Self { ... }
    pub fn border_width(mut self, width: f32) -> Self { ... }
    pub fn border_radius(mut self, radius: f32) -> Self { ... }
}
```

## 事件系统

```rust
pub enum Event {
    MouseMove { x: f32, y: f32 },
    MouseDown { button: MouseButton, x: f32, y: f32 },
    MouseUp { button: MouseButton, x: f32, y: f32 },
    MouseWheel { delta: f32, x: f32, y: f32 },
    KeyDown { key: Key },
    KeyUp { key: Key },
    TextInput { text: String },
    FocusIn,
    FocusOut,
}

pub enum EventResult {
    Continue,       // 继续传播
    Stop,           // 停止传播
    PreventDefault, // 阻止默认行为但继续传播
}
```

## 编程提示

当用户需要:

1. **创建新 Widget** → 生成 Widget 结构体 + trait 实现模板
   - 必须包含 `impl_widget_any!` 宏
   - `layout()` 方法必须接受 `id: WidgetId` 参数
   - 使用 `LayoutNode::new(id)` 创建节点

2. **实现布局** → 提供 LayoutNode 构建代码
   - 使用 `BoxStyle` 声明边距
   - 使用 `FlexStyle` 声明布局方式
   - 使用 `IntrinsicSize` 声明尺寸策略
   - 文本使用 `TextMeasure`

3. **处理事件** → 提供事件处理模式和 EventResult
   - 返回 `EventResult::Stop` 阻止传播
   - 返回 `EventResult::Continue` 继续传播

4. **渲染节点** → 生成 RenderNode 构建代码
   - 使用 `layout.computed.content_box` 获取位置
   - 使用 `RenderNode::div().background()` 链式调用

5. **调试问题** → 提供诊断建议
   - 使用 `view.debug_render_tree = true` 输出渲染树
   - 添加有意义的 `type_name()` 便于调试
   - 检查 `layout.computed` 是否为 Some

## 代码模板

### 新 Widget 模板

```rust
use lieui::prelude::*;

pub struct MyWidget {
    bounds: Rect,
    // 状态字段
}

impl MyWidget {
    pub fn new() -> Self {
        Self {
            bounds: Rect::zero(),
        }
    }
}

impl Widget for MyWidget {
    crate::impl_widget_any!(MyWidget);

    fn type_name(&self) -> &'static str { "MyWidget" }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        LayoutNode::new(id)
            .with_box_style(BoxStyle {
                padding: EdgeInsets::all(10.0),
                ..Default::default()
            })
            .with_intrinsic_size(IntrinsicSize::Fixed(Size::new(100.0, 50.0)))
    }

    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            RenderNode::div(computed.content_box)
                .background("#3366CC")
        } else {
            RenderNode::view(Rect::zero())
        }
    }

    fn handle_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Continue
    }

    fn bounds(&self) -> Option<Rect> {
        Some(self.bounds)
    }
}
```

### 容器 Widget 模板

```rust
pub struct MyContainer {
    children: Vec<WidgetId>,
    bounds: Rect,
}

impl Widget for MyContainer {
    crate::impl_widget_any!(MyContainer);

    fn type_name(&self) -> &'static str { "MyContainer" }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        LayoutNode::new(id)
            .with_flex(FlexStyle::column()
                .align(AlignItems::Center)
                .justify(JustifyContent::Center)
                .gap(10.0))
    }

    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            RenderNode::view(computed.content_box)
        } else {
            RenderNode::view(Rect::zero())
        }
    }

    fn children(&self) -> &[WidgetId] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<WidgetId> { &mut self.children }
    fn bounds(&self) -> Option<Rect> { Some(self.bounds) }
}
```

## 项目结构

```
src/
├── lib.rs              # 库入口
├── app.rs              # App 运行时（winit 集成）
├── core/               # 核心类型
│   ├── id.rs           # WidgetId
│   ├── view_context.rs # ViewContext（协调 WidgetTree/LayoutTree/事件）
│   └── mod.rs
├── widget/             # Widget trait 和 WidgetTree
│   ├── mod.rs
│   └── tree.rs         # WidgetTree（Widget 树管理）
├── widgets/            # 内置控件
│   ├── mod.rs
│   ├── text.rs         # Text
│   ├── button.rs       # Button
│   ├── container.rs    # Container
│   ├── column.rs       # Column
│   └── row.rs          # Row
├── layout/             # 布局系统
│   ├── mod.rs
│   ├── node.rs         # LayoutNode, IntrinsicSize
│   ├── box_model.rs    # BoxStyle, EdgeInsets, ComputedLayout
│   ├── flex.rs         # FlexStyle
│   ├── measurable.rs   # Measurable trait
│   ├── constraint.rs   # LayoutConstraint
│   └── context.rs      # LayoutContext
├── render/             # 渲染系统
│   ├── mod.rs
│   ├── node.rs         # RenderNode
│   └── engine.rs       # VelloRenderer
├── event/              # 事件系统
│   ├── mod.rs
│   ├── types.rs        # Event
│   ├── handler.rs
│   ├── context.rs
│   └── propagation.rs
├── text/               # 文本系统
│   └── mod.rs          # TextStyle, TextEngine
└── geometry/           # 几何类型
    ├── mod.rs
    └── types.rs        # Point, Size, Rect, Color
```

### 三棵树架构

| 树 | 职责 | 类型 |
|:---|:---|:---|
| **WidgetTree** | 管理所有 Widget 实例和树结构 | `widget::tree::WidgetTree` |
| **LayoutTree** | 布局约束收集和位置计算 | `layout::context::LayoutContext` |
| **RenderTree** | 渲染指令描述 | 每帧从 LayoutTree 构建 |
