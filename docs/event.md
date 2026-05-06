# 事件系统文档

## 概述

LieUI 事件系统处理用户输入（鼠标、键盘、IME）和窗口事件，支持事件传播机制。

## 核心概念

### Event 类型

```rust
pub enum Event {
    // 鼠标事件
    MouseDown { x: f32, y: f32, button: MouseButton, modifiers: Modifiers },
    MouseUp { x: f32, y: f32, button: MouseButton, modifiers: Modifiers },
    MouseMove { x: f32, y: f32 },
    MouseEnter,
    MouseLeave,
    MouseWheel { delta_x: f32, delta_y: f32 },

    // 键盘事件
    KeyDown { key: Key, modifiers: Modifiers },
    KeyUp { key: Key, modifiers: Modifiers },

    // 焦点事件
    FocusIn,
    FocusOut,

    // IME 事件
    ImePreedit { text: String, cursor_start: Option<usize>, cursor_end: Option<usize> },
    ImeCommit { text: String },
    ImeDisabled,

    // 窗口事件
    WindowResize { width: f32, height: f32 },
}
```

### EventContext

事件处理上下文，提供对 Widget 树的访问和副作用收集：

```rust
pub struct EventContext<'a> {
    tree: &'a WidgetTree,
    effects: RefCell<EventEffects>,
    propagation: RefCell<Propagation>,
}
```

**Widget 访问方法**：

```rust
impl<'a> EventContext<'a> {
    /// 获取 Widget 的不可变引用
    pub fn get<W: Widget + 'static>(&self, id: WidgetId) -> Option<Ref<'_, W>>;

    /// 获取 Widget 的可变引用
    pub fn get_mut<W: Widget + 'static>(&self, id: WidgetId) -> Option<RefMut<'_, W>>;
}
```

**副作用方法**：

```rust
impl<'a> EventContext<'a> {
    /// 请求重新渲染
    pub fn request_render(&self);

    /// 请求重新布局
    pub fn request_layout(&self);

    /// 请求动画帧
    pub fn request_animate(&self);
}
```

### EventEffects

事件处理的副作用收集：

```rust
#[derive(Debug, Default)]
pub struct EventEffects {
    pub needs_render: bool,
    pub needs_layout: bool,
    pub needs_animate: bool,
}
```

### Propagation

事件传播控制器：

```rust
pub struct Propagation {
    stopped: bool,
}

impl Propagation {
    pub fn stop(&mut self) { self.stopped = true; }
    pub fn is_stopped(&self) -> bool { self.stopped }
}
```

## 事件传播机制

### 传播路径

事件从根节点传播到目标节点，再冒泡回根节点：

```
Root
  └── Parent
       └── Target (事件目标)
```

**捕获阶段**：Root → Parent → Target
**冒泡阶段**：Target → Parent → Root

### 处理流程

```rust
fn dispatch_event(&mut self, event: Event, ctx: &mut ViewContext) {
    // 1. 命中测试找到目标
    let target = self.hit_test(event.position(), ctx);

    // 2. 构建传播路径
    let path = ctx.widget_tree().path_to(target);

    // 3. 捕获阶段
    for widget_id in &path {
        if propagation.is_stopped() { break; }
        self.invoke_widget(*widget_id, &event, &mut propagation, ctx);
    }

    // 4. 冒泡阶段（如果未停止）
    if !propagation.is_stopped() {
        for widget_id in path.iter().rev() {
            if propagation.is_stopped() { break; }
            self.invoke_widget(*widget_id, &event, &mut propagation, ctx);
        }
    }
}
```

## 事件类型详解

### 鼠标事件

#### MouseDown / MouseUp

```rust
Event::MouseDown { x, y, button, modifiers }
```

- `x`, `y`: 鼠标位置（相对于窗口）
- `button`: Left | Right | Middle
- `modifiers`: Ctrl | Shift | Alt | Meta

#### MouseMove

```rust
Event::MouseMove { x, y }
```

触发时机：
- 鼠标在窗口内移动
- 用于实现悬停效果、拖拽等

#### MouseEnter / MouseLeave

触发时机：
- 鼠标进入/离开 Widget 边界
- 用于实现悬停状态

### 键盘事件

#### KeyDown / KeyUp

```rust
Event::KeyDown { key, modifiers }
```

Key 类型：

```rust
pub enum Key {
    Character(char),    // 字符键
    Space,              // 空格
    Enter,              // 回车
    Tab,                // Tab
    Backspace,          // 退格
    Escape,             // Esc
    ArrowLeft,          // 方向键
    ArrowRight,
    ArrowUp,
    ArrowDown,
}
```

### IME 事件

#### ImePreedit

输入法预编辑状态：

```rust
Event::ImePreedit { text, cursor_start, cursor_end }
```

- `text`: 预编辑文本（如拼音）
- `cursor_start`, `cursor_end`: 预编辑区光标位置

#### ImeCommit

输入法提交：

```rust
Event::ImeCommit { text }
```

- `text`: 最终输入的文本（如汉字）

#### ImeDisabled

输入法被禁用：

```rust
Event::ImeDisabled
```

## 使用示例

### 基本事件处理

```rust
impl Widget for MyWidget {
    fn handle_event(&mut self, event: &Event, ctx: &mut EventContext) -> EventResult {
        match event {
            Event::MouseDown { button, .. } => {
                if *button == MouseButton::Left {
                    println!("Left button clicked!");
                    ctx.stop_propagation();
                    return EventResult::Stop;
                }
            }
            Event::KeyDown { key, .. } => {
                match key {
                    Key::Enter => println!("Enter pressed!"),
                    Key::Character('a') => println!("'a' pressed!"),
                    _ => {}
                }
            }
            _ => {}
        }
        EventResult::Continue
    }
}
```

### 使用 EventContext 访问其他 Widget

```rust
impl Widget for CounterButton {
    fn handle_event(&mut self, event: &Event, ctx: &mut EventContext) -> EventResult {
        match event {
            Event::MouseUp { .. } => {
                // 直接通过 EventContext 访问其他 Widget
                if let Some(mut text) = ctx.get_mut::<Text>(self.counter_text_id) {
                    let current = text.text_content().parse::<i32>().unwrap_or(0);
                    text.set_content((current + 1).to_string());
                }
                ctx.request_render();
                ctx.stop_propagation();
                return EventResult::Stop;
            }
            _ => {}
        }
        EventResult::Continue
    }
}
```

### 处理 IME 输入

```rust
impl Widget for TextInput {
    fn handle_event(&mut self, event: &Event, ctx: &mut EventContext) -> EventResult {
        match event {
            Event::ImePreedit { text, cursor_start, cursor_end } => {
                if text.is_empty() {
                    self.clear_compose();
                } else {
                    let cursor = cursor_start.map(|s| (s, cursor_end.unwrap_or(s)));
                    self.set_compose(text, cursor);
                }
                ctx.request_render();
            }
            Event::ImeCommit { text } => {
                if !text.is_empty() {
                    self.finish_compose();
                    self.insert_text(text);
                }
                ctx.request_render();
            }
            Event::ImeDisabled => {
                self.clear_compose();
                ctx.request_render();
            }
            _ => {}
        }
        EventResult::Continue
    }
}
```

### 焦点管理

```rust
impl Widget for TextInput {
    fn can_focus(&self) -> bool { !self.is_disabled }

    fn handle_event(&mut self, event: &Event, ctx: &mut EventContext) -> EventResult {
        match event {
            Event::FocusIn => {
                self.is_focused = true;
                ctx.request_render();
            }
            Event::FocusOut => {
                self.is_focused = false;
                ctx.request_render();
            }
            _ => {}
        }
        EventResult::Continue
    }
}
```

## 事件管理器

### EventManager

```rust
pub struct EventManager {
    callbacks: CallbackMap,
    focus_widget: Option<WidgetId>,
    hover_widget: Option<WidgetId>,
}
```

职责：
- 事件分发
- 焦点管理
- 悬停状态跟踪

### 焦点管理

```rust
impl EventManager {
    /// 设置焦点 Widget
    pub fn set_focus(&mut self, widget_id: Option<WidgetId>, ctx: &mut ViewContext) {
        // 1. 发送 FocusOut 给旧焦点
        if let Some(old) = self.focus_widget {
            self.dispatch_event_to(old, Event::FocusOut, ctx);
        }

        // 2. 更新焦点
        self.focus_widget = widget_id;

        // 3. 发送 FocusIn 给新焦点
        if let Some(new) = widget_id {
            self.dispatch_event_to(new, Event::FocusIn, ctx);
        }
    }
}
```

### 悬停管理

```rust
impl EventManager {
    /// 更新悬停状态
    pub fn update_hover(&mut self, point: Point, ctx: &mut ViewContext) {
        let new_hover = self.hit_test(point, ctx);

        if new_hover != self.hover_widget {
            // 发送 MouseLeave
            if let Some(old) = self.hover_widget {
                self.dispatch_event_to(old, Event::MouseLeave, ctx);
            }

            // 发送 MouseEnter
            if let Some(new) = new_hover {
                self.dispatch_event_to(new, Event::MouseEnter, ctx);
            }

            self.hover_widget = new_hover;
        }
    }
}
```

## 回调系统

### UserCallback

用户回调类型，接收 EventContext：

```rust
pub type UserCallback = Box<dyn Fn(&mut EventContext) + 'static>;
```

### 使用示例

```rust
pub struct Button {
    on_click: Option<UserCallback>,
}

impl Button {
    pub fn on_click<F>(mut self, callback: F) -> Self
    where F: Fn(&mut EventContext) + 'static
    {
        self.on_click = Some(Box::new(callback));
        self
    }
}

impl Widget for Button {
    fn handle_event(&mut self, event: &Event, ctx: &mut EventContext) -> EventResult {
        match event {
            Event::MouseUp { .. } => {
                if let Some(ref callback) = self.on_click {
                    callback(ctx);
                }
                ctx.stop_propagation();
                return EventResult::Stop;
            }
            _ => {}
        }
        EventResult::Continue
    }
}
```

### 完整示例：计数器

```rust
fn main() {
    let mut ctx = ViewContext::new(Size::new(800.0, 600.0));

    // 创建计数显示文本
    let count_text = ctx.create(Text::new("0").font_size(72.0));

    // 加号按钮 - 使用 EventContext 直接访问 Widget
    let btn_plus = ctx.create(Button::new("+").on_click(move |ctx| {
        if let Some(mut text) = ctx.get_mut::<Text>(count_text) {
            let current = text.text_content().parse::<i32>().unwrap_or(0);
            text.set_content((current + 1).to_string());
        }
        ctx.request_render();
    }));
}
```

## 最佳实践

1. **尽早停止传播**：如果处理了事件，调用 `ctx.stop_propagation()`
2. **使用 EventContext**：通过 `ctx.get/get_mut` 访问其他 Widget，避免 Rc<RefCell<>>
3. **请求副作用**：状态变化后调用 `ctx.request_render()` 等
4. **检查 modifiers**：使用 `modifiers.ctrl` 等检查组合键
5. **处理空字符串**：IME 事件可能传递空字符串
6. **焦点管理**：实现 `can_focus()` 控制焦点行为

## 参考

- [架构设计文档](./architecture.md)
- [Widget 系统文档](./widget.md)
- [winit 事件文档](https://docs.rs/winit/latest/winit/event/index.html)
