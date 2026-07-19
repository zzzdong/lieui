# Widget 系统文档

## 概述

Widget 系统是 LieUI 的核心，定义了所有 UI 组件的接口和行为。

## 核心概念

### Widget Trait

所有 UI 组件必须实现 `Widget` trait：

```rust
pub trait Widget: Any {
    /// 返回组件类型名称（用于调试）
    fn type_name(&self) -> &'static str;

    /// 脏标记检查
    fn is_dirty(&self) -> bool { false }
    fn clear_dirty(&mut self) {}

    /// 返回布局节点（约束信息）
    fn layout(&self, id: WidgetId) -> LayoutNode;

    /// 生成渲染节点
    fn render(&self, layout: &LayoutNode, ctx: &ViewContext) -> RenderNode;

    /// 命中测试
    fn hit_test(&self, point: Point, layout: &LayoutNode) -> bool;

    /// 处理事件
    fn handle_event(&mut self, event: &Event, ctx: &mut EventContext) -> EventResult;

    /// 子组件管理
    fn children(&self) -> &[WidgetId];
    fn children_mut(&mut self) -> &mut Vec<WidgetId>;

    /// 是否可获得焦点
    fn can_focus(&self) -> bool { false }

    /// 边界矩形（可选）
    fn bounds(&self) -> Option<Rect> { None }

    /// 类型转换
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
```

### WidgetId

组件的唯一标识符：

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WidgetId(NonZeroU64);
```

特点：
- 使用 `NonZeroU64` 优化内存布局
- 全局唯一，按创建顺序递增
- `WidgetId(1)` 保留为无效 ID

### WidgetTree

存储和管理所有组件：

```rust
pub struct WidgetTree {
    widgets: HashMap<WidgetId, WidgetRef>,
    parent_child: HashMap<WidgetId, Vec<WidgetId>>,
    child_parent: HashMap<WidgetId, WidgetId>,
    root: Option<WidgetId>,
}
```

功能：
- 组件存储（`HashMap<WidgetId, WidgetRef>`）
- 父子关系管理
- 路径查找（`path_to`）

## 实现 Widget

### 基本结构

```rust
use lieui::prelude::*;

pub struct MyWidget {
    // 组件状态
    label: String,
    dirty: bool,
}

impl MyWidget {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            dirty: false,
        }
    }
}

impl Widget for MyWidget {
    lieui::impl_widget_any!(MyWidget);

    fn type_name(&self) -> &'static str { "MyWidget" }

    fn is_dirty(&self) -> bool { self.dirty }
    fn clear_dirty(&mut self) { self.dirty = false; }

    fn layout(&self, id: WidgetId) -> LayoutNode {
        LayoutNode::new(id)
            .with_style(BoxStyle::default())
    }

    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        RenderNode::div(layout.computed_bounds())
            .background(Color::WHITE)
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut EventContext) -> EventResult {
        match event {
            Event::MouseDown { .. } => {
                println!("Clicked!");
                ctx.request_render();
                ctx.stop_propagation();
                EventResult::Stop
            }
            _ => EventResult::Continue,
        }
    }
}
```

### 使用宏简化

`impl_widget_any!` 宏自动实现 `as_any` 和 `as_any_mut`：

```rust
lieui::impl_widget_any!(MyWidget);
```

展开后：

```rust
fn as_any(&self) -> &dyn std::any::Any { self }
fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
```

## 内置 Widgets

### Container

矩形容器，支持背景色、边框、圆角：

```rust
let container = ctx.create(
    Container::new()
        .background(Color::WHITE)
        .border(Color::GRAY, 1.0)
        .border_radius(8.0)
        .padding(EdgeInsets::all(16.0))
);
```

### Column / Row

Flex 布局容器：

```rust
let column = ctx.create(
    Column::new()
        .spacing(10.0)
        .align_items(AlignItems::Center)
);

let row = ctx.create(
    Row::new()
        .spacing(10.0)
        .justify_content(JustifyContent::SpaceBetween)
);
```

### Text

文本显示：

```rust
let text = ctx.create(
    Text::new("Hello, LieUI!")
        .font_size(16.0)
        .text_color(TextColor::from(Color::BLACK))
);
```

### Button

按钮组件，支持点击回调：

```rust
let button = ctx.create(
    Button::new("Click Me")
        .on_click(|ctx| {
            println!("Clicked!");
            ctx.request_render();
        })
        .primary(true)
);
```

### TextInput

文本输入框，支持键盘和 IME：

```rust
let input = ctx.create(
    TextInput::new()
        .placeholder("Enter text...")
        .text("Default value")
        .width(300.0)
);
```

## 组件状态管理

### 脏检查模式

```rust
pub struct MyWidget {
    value: i32,
    dirty: bool,
}

impl MyWidget {
    pub fn set_value(&mut self, value: i32) {
        if self.value != value {
            self.value = value;
            self.dirty = true;
        }
    }
}

impl Widget for MyWidget {
    fn is_dirty(&self) -> bool { self.dirty }
    fn clear_dirty(&mut self) { self.dirty = false; }
}
```

### 回调管理

使用 `UserCallback` 类型存储回调：

```rust
use lieui::event::UserCallback;

pub struct MyWidget {
    on_click: Option<UserCallback>,
}

impl MyWidget {
    pub fn on_click<F>(mut self, callback: F) -> Self
    where F: FnMut(&Event, &EventContext) + 'static
    {
        self.on_click = Some(Box::new(callback));
        self
    }
}

impl Widget for MyWidget {
    fn handle_event(&mut self, event: &Event, ctx: &EventContext) -> EventResult {
        match event {
            Event::MouseUp { .. } => {
                if let Some(ref mut callback) = self.on_click {
                    callback(event, ctx);
                }
                ctx.stop_propagation();
                EventResult::Stop
            }
            _ => EventResult::Continue,
        }
    }
}
```

## 最佳实践

1. **最小化状态**：只存储必要的状态
2. **及时清理脏标记**：在 `clear_dirty` 中重置状态
3. **合理使用事件传播**：大多数情况下使用 `Continue`，只在需要时 `Stop`
4. **实现 `type_name`**：便于调试和日志
5. **使用链式 API**：提供流畅的构造体验

## 参考

- [架构设计文档](./architecture.md)
- [布局系统文档](./layout.md)
- [渲染系统文档](./render.md)
