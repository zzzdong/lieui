# LieUI 架构指南

## 项目概述

**LieUI** 是一个极简 Rust GUI 库，采用**声明式布局** + **两阶段布局计算**架构。

**核心原则**：**简化设计优先**——最少概念、最少代码、最直观 API

**技术栈**：
- **渲染后端**：vello_cpu（CPU 矢量渲染）
- **文本布局**：parley（Linebender 官方文本引擎）
- **窗口/事件**：winit（跨平台窗口与事件循环）

---

## 一、核心架构

### 1.1 三棵树设计

| 树 | 职责 | 生命周期 | 位置 |
|:---|:---|:---|:---|
| **WidgetTree** | 用户创建的组件树，持有状态 | 持久化 | `widget::tree::WidgetTree` |
| **LayoutTree** | 布局约束和计算结果 | 持久化（可增量更新） | `layout::context::LayoutContext` |
| **RenderTree** | 渲染指令描述 | 每帧重建 | 由 `build_render_tree()` 生成 |

### 1.2 核心类型

```rust
// 1. Widget - 用户定义的组件
trait Widget {
    fn layout(&self, id: WidgetId) -> LayoutNode;  // 返回约束
    fn render(&self, layout: &LayoutNode, ctx: &ViewContext) -> RenderNode;
    fn handle_event(&mut self, event: &Event) -> EventResult;
}

// 2. LayoutNode - 布局约束节点
struct LayoutNode {
    widget_id: WidgetId,
    box_style: BoxStyle,        // CSS Box 模型
    flex_style: Option<FlexStyle>, // Flex 布局配置
    intrinsic_size: IntrinsicSize, // 固有尺寸
    children: Vec<LayoutNode>,
    computed: Option<ComputedLayout>, // 计算结果
}

// 3. RenderNode - 渲染节点
enum RenderNode {
    View { bounds: Rect },
    Div { bounds: Rect, style: DivStyle },
    Text { bounds: Rect, layout: TextLayout },
}
```

---

## 二、布局系统

### 2.1 两阶段布局

```
┌─────────────────────────────────────────────────────────────┐
│                     阶段 1: 约束收集                           │
│                                                              │
│   Widget::layout(id) → LayoutNode {                          │
│       box_style: ...,     // margin/padding/border           │
│       flex_style: ...,    // direction/align/justify         │
│       intrinsic_size: ..., // Fixed 或 Measurable            │
│       children: [...],    // 递归收集子节点                   │
│       computed: None,     // 尚未计算                        │
│   }                                                          │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     阶段 2: 位置计算                           │
│                                                              │
│   LayoutContext::compute(node, position, available)          │
│                                                              │
│   1. 计算 content_size（使用 intrinsic_size）                 │
│   2. 应用 Box 模型（margin/border/padding）                   │
│   3. 如果是 Flex 容器，计算子元素位置                          │
│   4. 递归计算子节点                                           │
│                                                              │
│   → 填充 computed: ComputedLayout {                          │
│       content_box: Rect,    // 内容区域                       │
│       padding_box: Rect,    // padding 区域                   │
│       border_box: Rect,     // border 区域                    │
│       margin_box: Rect,     // margin 区域                    │
│   }                                                          │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 CSS Box 模型

```rust
// layout/box_model.rs
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

pub struct ComputedLayout {
    pub content_box: Rect,   // 内容区域（排除 padding/border/margin）
    pub padding_box: Rect,   // 包含 padding
    pub border_box: Rect,    // 包含 border
    pub margin_box: Rect,    // 包含 margin
}
```

### 2.3 Flex 布局

```rust
// layout/flex.rs
pub struct FlexStyle {
    pub direction: FlexDirection,      // Row | Column
    pub justify_content: JustifyContent, // Start | Center | End | SpaceBetween | ...
    pub align_items: AlignItems,       // Start | Center | End | Stretch
    pub gap: f32,
}

pub enum FlexDirection { Row, Column }
pub enum JustifyContent { Start, Center, End, SpaceBetween, SpaceAround, SpaceEvenly }
pub enum AlignItems { Start, Center, End, Stretch }
```

### 2.4 固有尺寸（IntrinsicSize）

```rust
// layout/node.rs
pub enum IntrinsicSize {
    Fixed(Size),                    // 固定尺寸
    Measurable(Box<dyn Measurable>), // 动态测量（如 Text）
}

// layout/measurable.rs
pub trait Measurable: Send + Sync {
    fn measure(&self, max_width: Option<f32>) -> Size;
}

// Text 测量实现
pub struct TextMeasure {
    pub content: String,
    pub style: TextStyle,
}

impl Measurable for TextMeasure {
    fn measure(&self, max_width: Option<f32>) -> Size {
        // 使用 parley 测量文本尺寸
    }
}
```

---

## 三、Widget 开发指南

### 3.1 创建新 Widget

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

    fn type_name(&self) -> &'static str {
        "MyWidget"
    }

    // ========== 布局 ==========
    fn layout(&self, id: WidgetId) -> LayoutNode {
        LayoutNode::new(id)
            .with_box_style(BoxStyle {
                padding: EdgeInsets::all(10.0),
                ..Default::default()
            })
            .with_intrinsic_size(IntrinsicSize::Fixed(Size::new(100.0, 50.0)))
    }

    // ========== 渲染 ==========
    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        if let Some(computed) = &layout.computed {
            RenderNode::div(computed.content_box)
                .background("#3366CC")
        } else {
            RenderNode::view(Rect::zero())
        }
    }

    // ========== 事件 ==========
    fn handle_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Continue
    }

    fn bounds(&self) -> Option<Rect> {
        Some(self.bounds)
    }
}
```

### 3.2 容器 Widget（带 children）

```rust
pub struct MyContainer {
    children: Vec<WidgetId>,
    bounds: Rect,
}

impl Widget for MyContainer {
    crate::impl_widget_any!(MyContainer);

    fn layout(&self, id: WidgetId) -> LayoutNode {
        let mut node = LayoutNode::new(id)
            .with_flex(FlexStyle::column()
                .align(AlignItems::Center)
                .justify(JustifyContent::Center)
                .gap(10.0));

        // 添加子节点
        for &child_id in &self.children {
            // 子节点由 LayoutContext::collect_node 递归收集
        }

        node
    }

    fn children(&self) -> &[WidgetId] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<WidgetId> {
        &mut self.children
    }
}
```

### 3.3 文本 Widget

```rust
pub struct Text {
    content: String,
    style: TextStyle,
}

impl Widget for Text {
    fn layout(&self, id: WidgetId) -> LayoutNode {
        // 使用 TextMeasure 动态测量文本尺寸
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

---

## 四、事件系统

### 4.1 事件类型

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
```

### 4.2 事件处理

```rust
impl Widget for Button {
    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { button: MouseButton::Left, .. } => {
                self.pressed = true;
                EventResult::Stop  // 停止传播
            }
            Event::MouseUp { button: MouseButton::Left, .. } => {
                if self.pressed {
                    self.pressed = false;
                    // 触发点击回调
                }
                EventResult::Stop
            }
            _ => EventResult::Continue,  // 继续传播
        }
    }
}
```

### 4.3 事件传播

```
Capture Phase（捕获）:  Root → Parent → Target
                           ↓
                    Widget::handle_event
                           ↓
Bubble Phase（冒泡）:   Target → Parent → Root
```

---

## 五、项目结构

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
│   ├── column.rs       # Column（垂直布局）
│   └── row.rs          # Row（水平布局）
├── layout/             # 布局系统
│   ├── mod.rs
│   ├── node.rs         # LayoutNode, IntrinsicSize
│   ├── box_model.rs    # BoxStyle, EdgeInsets, ComputedLayout
│   ├── flex.rs         # FlexStyle, FlexDirection, JustifyContent, AlignItems
│   ├── measurable.rs   # Measurable trait, TextMeasure
│   ├── constraint.rs   # LayoutConstraint
│   └── context.rs      # LayoutContext（两阶段布局计算）
├── render/             # 渲染系统
│   ├── mod.rs
│   ├── node.rs         # RenderNode
│   └── engine.rs       # VelloRenderer
├── event/              # 事件系统
│   ├── mod.rs
│   ├── types.rs        # Event, MouseButton, Key
│   ├── handler.rs      # EventHandler
│   ├── context.rs      # EventContext
│   └── propagation.rs  # 事件传播
├── text/               # 文本系统
│   └── mod.rs          # TextStyle, TextEngine
└── geometry/           # 几何类型
    ├── mod.rs
    └── types.rs        # Point, Size, Rect, Color
```

### 关键组件关系

```
ViewContext
├── widget_tree: WidgetTree     # 管理所有 Widget
├── layout_ctx: LayoutContext   # 管理布局计算
└── event_handler: EventHandler # 管理事件处理

WidgetTree
└── widgets: HashMap<WidgetId, Box<dyn Widget>>

LayoutContext
└── root: Option<LayoutNode>    # 布局树根节点
```

---

## 六、调试技巧

### 6.1 输出渲染树

```rust
view.debug_render_tree = true;  // 在 App 中设置
```

输出示例：
```xml
<Div x="0" y="0" w="800" h="600" bg="#F0F0F0">
  <View x="0" y="0" w="800" h="600">
    <Text x="254.17" y="206.40" w="291.66" h="36.80"/>
    <Text x="306.20" y="263.20" w="187.59" h="18.40"/>
    <Div x="367.11" y="309.60" w="97.78" h="36" bg="#1976D2">
      <Text x="383.11" y="318.40" w="65.78" h="18.40"/>
    </Div>
  </View>
</Div>
```

### 6.2 添加 type_name

```rust
impl Widget for MyWidget {
    fn type_name(&self) -> &'static str {
        "MyWidget"  // 用于调试输出
    }
}
```

### 6.3 日志记录

```rust
use log::info;

info!("Widget {} layout: {:?}", self.type_name(), layout);
```

---

## 七、最佳实践

### 7.1 布局原则

1. **Widget 只声明约束，不计算位置**
   - 使用 `BoxStyle` 声明边距
   - 使用 `FlexStyle` 声明布局方式
   - 使用 `IntrinsicSize` 声明尺寸策略

2. **让 LayoutContext 统一计算**
   - 不要手动计算子元素位置
   - 依赖 Flex 布局系统自动排列

3. **合理使用 expand**
   - `expand(true)` 会让组件填满可用空间
   - 配合 `JustifyContent::Center` 实现居中

### 7.2 性能优化

1. **LayoutTree 缓存**
   - 布局结果会自动缓存
   - 只有脏标记的节点会重新计算

2. **RenderTree 轻量**
   - 每帧重建，但只包含渲染指令
   - 不包含业务状态

3. **避免频繁创建 Widget**
   - Widget 是长期存在的
   - 通过修改状态来更新 UI

### 7.3 代码规范

1. **文档注释**
   - 公共 API 使用 `///`
   - 模块使用 `//!`

2. **单元测试**
   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn test_widget() {
           // 测试代码
       }
   }
   ```

3. **提交前检查**
   ```bash
   cargo check
   cargo clippy
   cargo fmt
   cargo test
   ```
