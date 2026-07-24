> ⚠️ **本文档已过时**（描述 v1 旧 `Widget`/`ViewContext` 架构），与当前 `v2-rewrite` 实现不符。
> 现行架构为 `runtime` 协调器 + `view::ViewNode`（实现 `View` trait）+ `ElementTree`（slotmap）+ 全局 `state` + `core::layers`（Base/Overlay/Modal）。
> **请以 [`../guide.md`](../guide.md) 为最新权威文档**；本文档仅留作历史参考。

# LieUI 架构设计文档

## 1. 概述

LieUI 是一个极简 Rust GUI 库，采用**三棵树架构**（WidgetTree、RenderTree、LayoutTree）和 **ViewContext** 管理它们的架构。

### 核心原则

- **简化设计优先**：最少概念、最少代码、最直观 API
- **立即模式渲染**：与 vello_cpu 的立即模式 API 保持一致
- **外部组合**：Widget 层级关系通过外部设置，而非内部嵌套

### 技术栈

| 组件 | 库 | 用途 |
|------|-----|------|
| 渲染后端 | `vello_cpu` | CPU 矢量渲染 |
| 文本布局 | `parley` | Linebender 官方文本引擎 |
| 窗口/事件 | `winit` | 跨平台窗口与事件循环 |
| 软渲染 | `softbuffer` | 窗口表面呈现 |

## 2. 架构总览

```
┌─────────────────────────────────────────────────────────────┐
│                        Application                          │
│                         (App)                               │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                      ViewContext                            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │  WidgetTree  │  │  LayoutTree  │  │  RenderTree  │      │
│  │  (组件树)     │  │  (布局树)     │  │  (渲染树)     │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│  ┌──────────────┐  ┌──────────────┐                        │
│  │ EventManager │  │LayoutContext │                        │
│  │  (事件管理)   │  │  (布局上下文) │                        │
│  └──────────────┘  └──────────────┘                        │
└─────────────────────────────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                      VelloRenderer                          │
│                     (vello_cpu)                             │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                        Pixmap                               │
│                      (CPU 位图)                              │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                     softbuffer                              │
│                      (窗口表面)                              │
└─────────────────────────────────────────────────────────────┘
```

## 3. 核心子系统

### 3.1 Widget 系统 (`src/widget/`)

**职责**：定义组件接口和管理组件树

**核心组件**：
- `Widget` trait：所有组件必须实现的接口
- `WidgetTree`：组件树的存储和管理
- `WidgetId`：组件的唯一标识符

**Widget 生命周期**：
1. `create()` - 创建组件实例
2. `layout()` - 返回布局约束
3. `render()` - 生成渲染节点
4. `handle_event()` - 处理输入事件

**关键设计**：
```rust
pub trait Widget: Any {
    fn layout(&self, id: WidgetId) -> LayoutNode;
    fn render(&self, layout: &LayoutNode, ctx: &ViewContext) -> RenderNode;
    fn handle_event(&mut self, event: &Event, propagation: &mut Propagation) -> EventResult;
    // ...
}
```

### 3.2 布局系统 (`src/layout/`)

**职责**：计算组件的位置和尺寸

**核心组件**：
- `LayoutContext`：布局计算的上下文
- `LayoutNode`：单个组件的布局信息
- `BoxStyle`：盒模型样式（margin、padding、border）
- `FlexStyle`：Flex 布局属性

**布局流程**（两阶段）：
1. **收集约束** (`collect`)：从 WidgetTree 收集所有布局约束
2. **计算位置** (`compute`)：根据约束计算最终位置和尺寸

**布局节点结构**：
```rust
pub struct LayoutNode {
    pub id: WidgetId,
    pub style: BoxStyle,
    pub flex: Option<FlexStyle>,
    pub intrinsic: IntrinsicSize,
    pub computed: Option<ComputedLayout>,  // 计算后填充
}
```

### 3.3 渲染系统 (`src/render/`)

**职责**：将组件渲染为像素

**核心组件**：
- `VelloRenderer`：渲染引擎
- `RenderNode`：渲染树节点
- `Pixmap`：CPU 位图缓冲区

**渲染流程**：
1. ViewContext 生成 RenderTree
2. VelloRenderer 遍历 RenderTree
3. 使用 vello_cpu 的 RenderContext 绘制
4. 结果写入 Pixmap
5. softbuffer 将 Pixmap 呈现到窗口

**RenderNode 类型**：
- `View`：容器节点
- `Div`：矩形区域（支持背景色、边框、圆角）
- `Text`：文本渲染（使用 parley Layout）
- `Image`：图片渲染
- `Canvas`：自定义绘制回调
- `CanvasWithData`：带数据的自定义绘制
- `Pixmap`：离屏渲染节点

### 3.4 事件系统 (`src/event/`)

**职责**：处理用户输入和事件分发

**核心组件**：
- `EventManager`：事件管理和分发
- `EventContext`：事件处理上下文（Widget 访问 + 副作用收集）
- `Event`：事件类型枚举
- `Propagation`：事件传播控制
- `EventEffects`：副作用收集（渲染、布局、动画请求）

**事件类型**：
- 鼠标事件：`MouseDown`, `MouseUp`, `MouseMove`, `MouseEnter`, `MouseLeave`
- 键盘事件：`KeyDown`, `KeyUp`
- 焦点事件：`FocusIn`, `FocusOut`
- IME 事件：`ImePreedit`, `ImeCommit`, `ImeDisabled`
- 窗口事件：`WindowResize`

**EventContext 功能**：
- **Widget 访问**：`get<T>()` / `get_mut<T>()` 直接访问其他 Widget
- **副作用请求**：`request_render()` / `request_layout()` / `request_animate()`
- **传播控制**：`stop_propagation()` 停止事件传播

**事件传播**：
1. 命中测试确定目标 Widget
2. 构建从根到目标的路径
3. 捕获阶段（根→目标）
4. 冒泡阶段（目标→根）
5. 任一阶段可调用 `ctx.stop_propagation()` 停止传播

### 3.5 文本系统 (`src/text/`)

**职责**：文本布局和字体管理

**核心设计**：
- **全局线程存储**：`FONT_CONTEXT` 和 `LAYOUT_CONTEXT` 使用 `thread_local!`
- **便捷访问函数**：`with_font_context`, `with_layout_context`, `with_text_contexts`

**关键类型**：
- `TextEngine`：文本布局引擎（纯静态方法）
- `TextStyle`：文本样式
- `TextColor`：文本颜色（parley Brush 实现）
- `TextLayout`：布局结果（parley Layout 别名）

**使用示例**：
```rust
// 直接调用静态方法
let layout = TextEngine::layout("Hello", &style, 1.0, Some(100.0));

// 或使用便捷函数访问上下文
with_text_contexts(|font_cx, layout_cx| {
    let driver = editor.driver(font_cx, layout_cx);
    // ...
});
```

### 3.6 几何系统 (`src/geometry/`)

**职责**：基础几何类型定义

**核心类型**：
- `Point`：二维点
- `Size`：尺寸（宽、高）
- `Rect`：矩形区域
- `Color`：颜色（包装 vello_cpu::AlphaColor）

## 4. 核心流程

### 4.1 应用启动流程

```
main()
  └── EventLoop::new()
      └── ViewContext::new()
          └── 创建 WidgetTree、EventManager、LayoutContext
      └── 构建 UI（create, add_child）
      └── App::new(view)
          └── VelloRenderer::new()
      └── app.run(event_loop)
          └── 处理 WindowEvent::Resumed
              └── init_window()
                  └── 创建 Window、Surface、Pixmap
                  └── window.set_ime_allowed(true)
                  └── 首次渲染
```

### 4.2 渲染流程

```
App::render_and_present()
  └── ViewContext::render()
      └── 遍历 WidgetTree
          └── Widget::render() → RenderNode
      └── 构建 RenderTree
  └── VelloRenderer::render()
      └── 遍历 RenderTree
          └── 根据 RenderNode 类型调用 vello_cpu API
              └── fill_rect()、draw_text() 等
      └── 结果写入 Pixmap
  └── Surface::present()
      └── Pixmap 呈现到窗口
```

### 4.3 布局流程

```
ViewContext::perform_layout()
  └── LayoutContext::collect()
      └── 遍历 WidgetTree
          └── Widget::layout() → LayoutNode（仅约束）
  └── LayoutContext::compute()
      └── 计算 Flex 布局
      └── 计算绝对定位
      └── 填充 ComputedLayout
  └── 标记 needs_layout = false
```

### 4.4 事件处理流程

```
App::window_event()
  └── 转换 winit 事件为 LieUI Event
  └── ViewContext::handle_xxx_event()
      └── EventManager::dispatch_xxx()
          └── 命中测试找到目标 Widget
          └── 构建事件路径
          └── 遍历路径分发事件
              └── Widget::handle_event()
                  └── 返回 EventResult::Continue 或 Stop
```

## 5. 设计决策

### 5.1 为什么使用三棵树？

| 树 | 职责 | 更新时机 |
|----|------|---------|
| WidgetTree | 存储组件状态 | 组件创建/销毁时 |
| LayoutTree | 存储布局约束和结果 | 尺寸变化时 |
| RenderTree | 存储渲染指令 | 每帧渲染时 |

**优势**：
- 关注点分离，代码清晰
- 可以独立优化每棵树的性能
- 支持脏检查和增量更新

### 5.2 为什么使用外部组合？

```rust
// LieUI 方式（外部组合）
let button = ctx.create(Button::new("Click"));
ctx.add_child(parent, button);

// 对比内部组合
let button = Button::new(|ctx| {
    ctx.create(Text::new("Click"));
});
```

**优势**：
- API 更简单（只需 `create` 和 `add_child`）
- 动态 UI 更灵活（运行时增删节点）
- 与立即模式渲染风格一致

### 5.3 为什么使用全局线程存储？

```rust
thread_local! {
    pub static FONT_CONTEXT: RefCell<FontContext> = ...;
    pub static LAYOUT_CONTEXT: RefCell<LayoutContext<TextColor>> = ...;
}
```

**优势**：
- 避免在 Widget 中存储字体上下文
- 简化 TextEngine API（纯静态方法）
- 线程安全（每线程独立上下文）

## 6. 扩展指南

### 6.1 添加新 Widget

1. 实现 `Widget` trait
2. 实现 `layout()` 返回约束
3. 实现 `render()` 生成渲染节点
4. 可选：实现 `handle_event()` 处理交互

### 6.2 添加新 RenderNode 类型

1. 在 `render/node.rs` 添加新变体
2. 在 `VelloRenderer` 添加渲染逻辑
3. 更新 `bounds()`、`to_xml()`、`Debug`、`Clone` 实现

### 6.3 添加新事件类型

1. 在 `event/types.rs` 添加新变体
2. 在 `EventManager` 添加分发逻辑
3. 在 `App` 添加事件转换

## 7. 性能考虑

### 7.1 脏检查机制

- `needs_layout`：布局是否需要重新计算
- `needs_render`：是否需要重新渲染
- `Widget::is_dirty()`：组件状态是否变化

### 7.2 布局缓存

- `LayoutContext` 缓存布局约束
- `ComputedLayout` 缓存计算结果
- 仅当 viewport 或 widget 树变化时重新计算

### 7.3 渲染优化

- 使用 Pixmap 作为离屏缓冲区
- 支持局部重绘（未来可扩展）
- vello_cpu 的 CPU 渲染适合中小型应用

## 8. 调试工具

### 8.1 渲染树调试

```rust
ctx.debug_render_tree = true;
```

输出 XML 格式的渲染树：
```xml
<View>
  <Div bounds="..." background="...">
    <Text bounds="...">Hello</Text>
  </Div>
</View>
```

### 8.2 日志记录

使用 `log` crate 进行日志记录：
```rust
log::debug!("Widget created: {:?}", id);
```

## 9. 参考

- [Widget 系统文档](./widget.md)
- [布局系统文档](./layout.md)
- [渲染系统文档](./render.md)
- [事件系统文档](./event.md)
- [文本系统文档](./text.md)
