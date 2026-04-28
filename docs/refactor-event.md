## LieUI 事件系统简化重构文档

### 1. 重构目标
- **极简回调签名**：用户回调类型简化为 `Box<dyn FnMut()>`，无需传递 `ViewContext` 或 `EventContext`。
- **Widget 内部持有回调**：每个 Widget 直接存储用户注册的回调，移除全局回调管理器。
- **消除 `ViewContext` 依赖**：事件系统只依赖 `WidgetTree`（`Rc<RefCell<Box<dyn Widget>>>`）和 `LayoutNode`，不再需要 `ViewContext` 参与。
- **统一副作用收集**：Widget 的 `handle_event` 接收 `&mut EventContext`，通过它请求重绘/布局/动画等。
- **保留三阶段传播**：捕获、目标、冒泡机制保留，但简化路径构建和事件分发接口。

### 2. 当前系统问题
- `EventCallback` 签名包含 `&mut ViewContext`，在引入 `WidgetTree` 后，`ViewContext` 已无必要，且引发借用复杂性。
- `EventHandler` 与 `EventDispatcher` 职责重叠，一部分逻辑在 `EventHandler` 中生成事件列表，另一部分在 `EventContext` 中再次分发，造成割裂。
- 回调存储分散在 `ViewContext` 中，需要 `take_callbacks` 来避免借用冲突，实现繁琐。
- Widget 内部行为与用户回调调用逻辑混杂在同一个 `handle_event` 中，但当前实现中尚未清晰分离。

### 3. 新设计概览
**核心类型**：
- `WidgetRef` = `Rc<RefCell<Box<dyn Widget>>>`
- `EventCallback` = `Box<dyn FnMut()>`
- `EventContext` 持有 `&WidgetTree` 和 `EventEffects`
- `Event` / `EventType` 保持不变

**存储方式**：
- Widget 内部持有 `callbacks: HashMap<EventType, Vec<EventCallback>>`
- 通过 Builder 风格注册：`button.on_click(|| { … })`

**事件传播流程**：
1. `EventHandler` 将原始输入转为标准 `Event`（如 `Click`），并更新交互状态（hovered/pressed/focused）。
2. `EventContext::dispatch(event, point, layout_root)` 执行命中测试、构建路径、三阶段传播，依次调用 `Widget::handle_event(&mut self, event, &mut EventContext)`。
3. Widget 内部 `handle_event` 先处理自身视觉状态（如 hover 变色），然后根据事件类型触发对应回调。
4. 所有副作用通过 `ctx.effects.request_render()` 标记，最后返回给 App 层统一处理。

### 4. 详细实现
#### 4.1 Widget trait 简化
```rust
pub trait Widget: 'static {
    fn handle_event(&mut self, event: &Event, ctx: &mut EventContext) -> EventResult;
    // 其他方法：layout, render, children...
}
```
不再需要 `ViewContext` 或 `WidgetTree` 参数，Widget 可通过 `ctx.tree()` 访问其他 Widget，但用户回调直接用闭包捕获 `WidgetRef`。

#### 4.2 EventContext 调整
```rust
pub struct EventContext<'a> {
    tree: &'a WidgetTree,
    pub effects: EventEffects,
}

impl<'a> EventContext<'a> {
    pub fn new(tree: &'a WidgetTree) -> Self { … }
    pub fn tree(&self) -> &WidgetTree { self.tree }

    pub fn dispatch(&mut self, event: &Event, point: Point, layout_root: &LayoutNode) {
        let Some(target) = layout_root.hit_test(point) else { return };
        let path = build_path(target, layout_root);
        // 捕获、目标、冒泡
        for &id in path.iter().take(path.len()-1) { if self.invoke(id, event) == Stop { return; } }
        if self.invoke(target, event) == Stop { return; }
        for &id in path.iter().rev().skip(1) { if self.invoke(id, event) == Stop { return; } }
    }

    fn invoke(&mut self, id: WidgetId, event: &Event) -> EventResult {
        self.tree.widgets.get(&id)
            .and_then(|w| w.try_borrow_mut().ok())
            .map(|mut widget| widget.handle_event(event, self))
            .unwrap_or(Continue)
    }
}
```
`build_path` 和 `find_path` 函数保持与现有实现一致。

#### 4.3 EventHandler 精简
- 移除所有 `handle_xxx` 返回事件列表的方法，改为直接调用 `EventContext::dispatch`。
- `EventHandler` 仅负责：
  - 记录交互状态（`hovered`, `pressed`, `focused`）。
  - 合成复杂事件（如根据 `MouseDown`/`MouseUp` 生成 `Click`）。
  - 调用 `EventContext::dispatch(event, point, layout_root)`。

#### 4.4 Widget 内部回调存储（以 Button 为例）
```rust
pub struct Button {
    // 状态字段 ...
    callbacks: HashMap<EventType, Vec<EventCallback>>,
}

impl Button {
    pub fn on_click(mut self, f: impl FnMut() + 'static) -> Self {
        self.callbacks.entry(EventType::Click).or_default().push(Box::new(f));
        self
    }
}

impl Widget for Button {
    fn handle_event(&mut self, event: &Event, ctx: &mut EventContext) -> EventResult {
        match event {
            Event::MouseEnter => { self.is_hovered = true; ctx.effects.request_render(); }
            Event::MouseLeave => { self.is_hovered = false; ctx.effects.request_render(); }
            Event::Click => {
                self.is_pressed = false;
                for cb in self.callbacks.entry(EventType::Click).or_default() {
                    (cb)();
                }
                ctx.effects.request_render();
            }
            _ => {}
        }
        EventResult::Continue
    }
}
```
回调闭包捕获外部 `WidgetRef` 修改其他 Widget，或通过 `ctx.tree()` 访问（如需访问非捕获的 Widget）。

### 5. 回调注册示例
```rust
let label_ref = tree.add(label_id, Text::new("0"));
let button = Button::new("+")
    .on_click({
        let label = label_ref.clone();
        move || {
            label.borrow_mut().set_text("updated");
        }
    });
tree.add(btn_id, button);
```
无需 `ctx`，闭包自然捕获所需引用。

### 6. 事件处理流程整合（App 层）
```rust
// 在 winit 事件循环中
let mut ctx = EventContext::new(&tree);
match window_event {
    CursorMoved { position, .. } => {
        // EventHandler 内部转换并分发
        event_handler.handle_mouse_move(point, &layout_root, &mut ctx);
    }
    MouseInput { state, button, .. } => {
        event_handler.handle_mouse_input(state, button, point, &layout_root, &mut ctx);
    }
    _ => {}
}
// 事件处理完毕后，检查副作用
if ctx.effects.needs_render() { window.request_redraw(); }
if ctx.effects.needs_layout() { /* 重新布局 */ }
```

### 7. 迁移步骤
1. **修改 `EventCallback` 类型**：移除 `&mut ViewContext` 参数，改为 `Box<dyn FnMut()>`。同时删除 `EventCallbackManager` trait。
2. **移除 `EventCallbackManager` 实现**：从 `ViewContext`（或曾有的全局存储）中移走回调注册相关代码。
3. **在各 Widget 中添加回调存储字段**：如在 `Button`、`Text`、`Container` 等 Widget 结构体中增加 `callbacks: HashMap<EventType, Vec<EventCallback>>`。
4. **重构 `Widget::handle_event` 签名**：改为 `fn handle_event(&mut self, event: &Event, ctx: &mut EventContext) -> EventResult`，内部按需触发存储的回调。
5. **简化 `EventHandler`**：删除返回事件列表的方法，改为接收 `&mut EventContext` 并直接调用其 `dispatch`。原始事件转化逻辑保留，状态更新保留。
6. **调整 `EventContext`**：确保其持有 `&WidgetTree` 和 `EventEffects`，`dispatch` 方法签名不变，但 `invoke` 内部改为调用新的 `handle_event` 签名。
7. **更新 App 层事件循环**：使用新的 `EventHandler` 和 `EventContext`，不再需要中间事件列表。
8. **测试**：验证 Button 点击、文本更新、布局树命中、事件冒泡等。

### 8. 优势总结
- **API 极简**：`button.on_click(|| { … })`，零额外参数。
- **生命周期安全**：彻底消除 `ViewContext` 借用冲突，`Rc<RefCell<>>` + 闭包捕获提供自然所有权语义。
- **职责清晰**：`EventHandler` 管状态合成，`EventContext` 管传播，Widget 管自身行为和用户回调触发。
- **易于扩展**：需要新副作用时只需修改 `EventEffects` 和 Widget 内部调用，无需更改回调签名。
- **代码量减少**：删除 `EventCallbackManager`、`take_callbacks` 等复杂装置，Widget 实现更加内聚。

按照此文档重构后，LieUI 的事件系统将完全切合我们讨论的“简约化”理念，同时保持强大的功能和清晰的架构。