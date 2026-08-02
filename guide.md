# LieUI 开发指南（v2）

LieUI 是一个 Rust 声明式、按帧重建（rebuilt-per-frame）的 GUI 库，基于 [winit] + [softbuffer] 窗口、[vello_cpu] 软件光栅化渲染，布局使用对 [Taitank] 的忠实移植（Flexbox）。

---

## 1. 30 秒上手

```rust
use lieui::geometry::{Color, Size};
use lieui::layout::{FlexAlign, FlexDirection};
use lieui::prelude::*;
use lieui::state::State;
use lieui::widget::{Widget, BuildContext};

fn main() {
    let count = State::new(0);

    let app = Application::new(
        WindowConfig::new().title("Counter").size(400.0, 300.0),
        move |_ctx: &mut BuildContext| {
            Box::new(
                Row::new()
                    .justify_content(FlexAlign::Center)
                    .align_items(FlexAlign::Center)
                    .child(
                        Column::new()
                            .spacing(16.0)
                            .justify_content(FlexAlign::Center)
                            .align_items(FlexAlign::Center)
                            .child(Text::new("Counter").font_size(48.0))
                            .child(
                                Text::new(format!("{}", count.get()))
                                    .font_size(72.0)
                                    .color(Color::RED),
                            )
                            .child(
                                Row::new()
                                    .spacing(12.0)
                                    .child(Button::new("-1").on_click({
                                        let c = count.clone();
                                        move || c.update(|v| *v -= 1)
                                    }))
                                    .child(Button::new("+1").on_click({
                                        let c = count.clone();
                                        move || c.update(|v| *v += 1)
                                    })),
                            ),
                    ),
            ) as Box<dyn Widget>
        },
    );

    app.run();
}
```

核心三步：
1. 用 `State::new` 持有可变状态。
2. `Application::new(config, builder)` 传入窗口配置与一个返回 `Box<dyn Widget>` 的闭包；闭包里用 `Column`/`Row`/`Button`/`Text` 等组合 UI。多窗口用 `.window(config, builder)` 链式追加。
3. `app.run()` 启动事件循环。状态变化时调用 `request_rebuild()`（或 `State::update` 内部自动触发）即可重绘。

> 自定义窗口级选项：`.inspector(true)`（feature `inspector`，开启 Chrome DevTools 看 Widget 树）、`.close_guard(...)`（关闭前守卫，用于未保存确认）、`.with_font(path)`（自定义字体）。

---

## 2. 核心概念

### 2.1 `Widget` trait 与 `ViewNode`

所有 UI 块都实现 `Widget` trait：

```rust
pub trait Widget {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode;
    // 另有 key() / inspect_name() / inspect_text() 等默认方法
}
```

- `Widget` 是可复用的 UI 配置对象，每次 rebuild 都会重新生成，但状态通过 `BuildContext::use_state` 持久化。
- `Widget::build` 产出 **immutable 的 `ViewNode`**。`ViewNode` 是 UI 的**唯一**纯数据中间表示，枚举只有三个原语：`Text` / `Image` / `Div`（`node.rs` 已移除旧版的 `Canvas`）。
- `Column`/`Row`/`Button`/`Container`/`Text`/`Image` 这些原语与组件都是 `Widget`；用户也能为自定义组件实现 `Widget`：

```rust
use lieui::widget::{Widget, BuildContext};
use lieui::view::ViewNode;

struct Label { text: String }
impl Widget for Label {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        Text::new(self.text.clone()).color(Color::WHITE).build_node()
    }
}
```

闭包 `Fn(&mut BuildContext) -> Box<dyn Widget>` 作为 `Application` 的 builder 直接产出根 Widget；组合使用 `Widget` 的 `.child(...)`（接受 `impl Widget` 或 `Box<dyn Widget>`）。

> 注意：`Widget` trait 不在 `prelude` 中；实现自定义组件需 `use lieui::widget::Widget;`。`prelude` 导出的是具体 widget 类型（`Button`、`Column`…）。

### 2.2 `Application` 与渲染管线

`Application::new(config, builder)` 构造单窗口应用；`.window(config, builder)` 追加窗口；`.run()` 启动事件循环。

每个窗口（`WindowContext`）持有**独立的** `Runtime`、`VelloRenderer`、`StateMap` 与重建/重绘/关闭标志（**多窗口状态与信号完全隔离**，互不串味）。单窗口的渲染管线：

- 当重建触发时，调用 `builder(&mut BuildContext)` 得到根 `Widget`，再 `build` 成 `ViewNode` 树；
- `Runtime::submit_view_tree` + `reconciler` 与上一次树做 diff（按 `key` 或「类型 + 位置」匹配），更新 `ElementTree`；
- `perform_layout` 把 `ViewNode` 树（经 `to_flex_style` 转换）喂给 Flex 布局引擎，得到每个节点的 `ComputedLayout`；
- 渲染层（`render/engine.rs` + `render/visual.rs`）把布局 + 交互状态（hover/pressed）+ 样式展开成 `Vec<LayeredElement>`（z 序由图层决定）；
- `VelloRenderer` 软件渲染成 pixmap，`blit_to_window` 拷到 softbuffer 表面。

> 仅**鼠标移动**（hover 变化）时走轻量的「仅重绘」路径，不再重建整树；其他变更走完整重建。

### 2.3 状态 `State<T>`

`State<T>` 是一个 `Rc<RefCell<T>>` 包装，并通过 `BuildContext::use_state` / 全局句柄共享：

| 操作 | 说明 |
|------|------|
| `State::new(v)` | 创建 |
| `s.get()` | 读（`&T`） |
| `s.set(v)` | 写并触发 `request_rebuild()` |
| `s.update(\|v\| …)` | 闭包内修改并触发重建 |
| `s.clone()` | 复制句柄，在闭包里捕获 |

闭包在每次重建时重新执行，因此 `count.get()` 总是读到最新值。**每个窗口拥有独立的 `StateMap`**，因此多窗口下 `use_state` 的状态互不影响。

`request_rebuild()` / `request_redraw()` 是**按「当前窗口」路由**的（per-window 标志）：在 `window_event` / `build_and_render` 流程内调用时只标记该窗口；若在窗口上下文之外调用（如后台 controller 回调、动画全局 tick），则广播到所有已注册窗口——单窗口场景下二者等价。

### 2.4 事件与回调

- 命中测试 `hit_test_top` 在三层（Modal → Overlay → Base）中找最顶层、最内层节点。
- `EventManager` 做三阶段分发（捕获 → 目标 → 冒泡，沿 `path` 遍历）。
- 给节点加 `.on_click(closure)` 或 `.on_click_with_ctx(|ctx| …)`：
  - `Simple`：点击即执行，`Simple` 类型会自动 `stop_propagation`。
  - `WithCtx`：回调收到 `&mut EventContext`，可调用 `ctx.stop_propagation()`、读 `ctx.phase()`。
  - `EventContext::request_rebuild()/request_layout()/request_render()` 会在事件循环中被应用（触发重建 / 重排 / 重绘）。
- 点击只会在 **Target / Bubble** 阶段触发（捕获阶段不触发 `Click`）。
- 监听器区分**内置行为**（`ListenerKind::BuiltIn`，如 Slider 拖拽、Input 聚焦/输入、Draggable 拖拽接线）与**用户回调**（`ListenerKind::User`，如 `.on_click(...)`）。同一节点上内置回调**先于**用户回调执行，因此用户回调调用 `stop_propagation()` 不会阻止同节点已执行的内置行为，只会停止向其他节点传播。hover/pressed 视觉反馈不依赖回调，由样式（`hover_background` / `hover_color` 等）+ 交互状态驱动。

> hover/pressed 状态作用在最深层命中节点向上找到的「最近带 listener 的祖先」（交互节点）上，`Button` 的 hover/pressed 背景反馈正常显示。

### 2.5 拖拽（Drag）

事件系统内建通用的「按下 → 移动 → 释放」拖拽语义，由 `EventManager` 合成三种事件：

| 事件 | 触发时机 | 数据 |
|------|---------|------|
| `DragStart { x, y, offset_x, offset_y, button, modifiers }` | 按下后移动超过阈值（默认 3px） | 触发位置与修饰键（偏移始终为 0） |
| `DragMove { x, y, dx, dy, offset_x, offset_y, button, modifiers }` | 每次鼠标移动 | 相对上次的增量 + 相对按下点的累计偏移 |
| `DragEnd { x, y, dx, dy, offset_x, offset_y, button, modifiers }` | 鼠标释放 | 最终偏移 |

使用方式：在 `MouseDown` 回调里调用 `ctx.begin_drag()`（或 `begin_drag_with_threshold(px)`）请求拖拽；拖拽开始后自动捕获鼠标，指针移出组件仍持续收到 `DragMove`；超过阈值的真实拖拽结束时会抑制 `Click`，避免「拖完又触发点击」。

推荐直接用 `Draggable` 组件包装任意内容：

```rust
use lieui::prelude::*;
use lieui::event::Event;

let pos = State::new((0.0f32, 0.0f32));

Draggable::new(
    Container::new()
        .width(120.0)
        .height(40.0)
        .child(Text::new("拖我")),
)
.on_drag_move({
    let pos = pos.clone();
    move |ctx| {
        if let Some(Event::DragMove { offset_x, offset_y, .. }) = ctx.event() {
            pos.set((*offset_x, *offset_y));
        }
    }
})
```

注意：拖拽开始后该节点会接管后续所有鼠标事件，因此不要用 `Draggable` 包裹 Button/Slider 等自身需要鼠标交互的组件；需要时用 `.enabled(false)` 关闭拖拽。

### 2.6 图标（Icon / IconButton）

项目随附 Google Material Icons（OFL 许可）字体，通过 `include_bytes!` 在**编译期内嵌**到二进制中，首次构建图标时自动注册到排版引擎——无需任何手动注册或额外文件。图标以文本字形渲染，可缩放、可着色。

- `Icon::new(IconName::Search, 18.0)`：纯图标字形，支持 `.color()` / `.hover_color()` / `.pressed_color()`。
- `IconButton::new(IconName::Add).on_click(...)`：工具栏风格的方形图标按钮（默认 28px、图标 18px），hover/pressed 时背景与图标自动变色；`IconButtonVariant::Plain / Outline / Primary` 控制风格。
- 常用图标列表见 `IconName` 枚举（增删改查、方向箭头、收藏、设置、用户、文件夹等 100+）；码点与 `assets/MaterialIcons-Regular.codepoints` 一一对应，可自行扩展。

```rust
Row::new().spacing(4.0)
    .child(IconButton::new(IconName::Add).on_click(|| add()))
    .child(IconButton::new(IconName::Close).variant(IconButtonVariant::Outline))
    .child(IconButton::new(IconName::Check).variant(IconButtonVariant::Primary));
```

### 2.7 布局（Flexbox）

`Column`/`Row` 映射为 `ViewNode::Div` + `DisplayMode::Flex`：

| 方法 | 作用 |
|------|------|
| `.expand(true)` | 在主轴方向填满父容器（等价于 `flex_grow=1`） |
| `.spacing(f32)` | 子项间距 |
| `.justify_content(FlexAlign::*)` | 主轴对齐 |
| `.align_items(FlexAlign::*)` | 交叉轴对齐（默认 `Stretch`） |
| `.center()` | 主轴+交叉轴居中并 `expand(true)` |

`Container` 映射为 `ViewNode::Div` + `DisplayMode::Block`，支持 `.width()/.height()/.background()/.padding()/.border_radius()`。布局引擎是 Taitank 风格移植，支持 `flex_grow`/`flex_shrink`、换行、padding/border/margin。

> 文本在约束宽度下会重新排版以支持换行。注意：处于 `Row`（主轴水平）且宽度不受限时文本仍按单行处理；内部 `FlexStyle` 的 `flex_shrink` 默认 0 保持不变，因此在 `NoWrap` 行内挤占过满时仍可能溢出——需给容器设定宽度或改用 `Column`。

### 2.8 图层（Base / Overlay / Modal）

`LayerStack`（Modal / Overlay / Base / Popup / Tooltip / System 等多层 z-index 栈）是**每个窗口 `Runtime` 的独立字段**，因此多窗口下各窗口的浮层互不串扰。弹出覆盖层使用全局入口函数：它们把「以 widget tree 声明层内容」的命令压入 per-window 待处理队列（`PENDING_LAYER` thread-local），并由 `request_rebuild()` 路由到「当前窗口」；下一帧该窗口重建时，命令被消费并合并进该窗口自己的 `LayerStack`。

```rust
// 接受 FnOnce(&mut BuildContext) -> Box<dyn Widget>，与主内容 builder 形态一致
show_overlay(|ctx: &mut BuildContext| {
    Box::new(Text::new("我是浮层").font_size(16.0))
});
show_modal(|ctx: &mut BuildContext| {
    Box::new(Container::new()
        .padding(24.0)
        .child(Text::new("确认退出？")))
});
hide_overlay();
hide_modal();
```

> 层内容的 builder 每次调用都会新建独立 `BuildContext`，因此基于 `use_state` 的 hook 状态**不会**跨层会话持久；需要持久状态时应放在外层（如 `AppState`），每次重建时重新 `show_layer`。

命中测试按 `Modal → Overlay → Base` 优先级返回最顶层节点，因此 modal 会拦截其范围内的点击。亦可用底层 `show_layer(LayerKind, LayerSpec, builder)` 自定义层类型、锚点与焦点策略（`Anchor`/`FocusPolicy`/`LayerSpec` 见 `core::layers`）。

### 2.9 Reconciler 与 key

每次重建产生新 `ViewNode` 树，reconciler 与旧树 diff：
- 优先按 `key` 匹配（`Widget::key()`，见下）；
- 无 key 时按「节点类型 + 位置」匹配，并复用原 `ElementId` 以保留布局/状态/回调。
- 动态列表**务必**使用 key，否则重排可能产生错误视觉顺序。

```rust
use lieui::widget::Widget;
// 给列表项加稳定 key（Widget trait 自带方法，无需额外 trait import）：
Column::new().child(item_widget.key(format!("row-{i}")))
```

---

## 3. 公开 API 速查

预导入：`use lieui::prelude::*;` 包含：

| 类别 | 符号 |
|------|------|
| 入口 | `Application::new(WindowConfig, builder).window(...).inspector(bool).close_guard(...).run()` |
| 窗口配置 | `WindowConfig::new().title().size().min_size().max_size().resizable().decorations().icon().always_on_top().position()` |
| 构建上下文 | `BuildContext`（配合 `use lieui::widget::Widget` 实现自定义组件） |
| 状态 | `State::new/get/set/update/clone`、`request_rebuild()`、`request_redraw()`、`request_window_close()` |
| 图层 | `show_overlay(builder)`、`show_modal(builder)`、`hide_overlay()`、`hide_modal()`、`show_layer(LayerKind, LayerSpec, builder)`、`LayerSpec` |
| 原语 | `Text::new(s)`、`Image::from_rgba(data,w,h)`、`Container::new()`、`Column::new()`、`Row::new()` |
| 组件 | `Button`、`Checkbox`、`Radio`、`Switch`、`Slider`、`Progress`、`Tab`、`Card`、`Divider`、`ListView`、`VirtualList`、`ScrollView`、`ScrollBar`、`Draggable`、`Tooltip`、`Icon`、`IconButton`、`Input` |
| 通用方法 | `.child(v)`、`.expand(bool)`、`.spacing(f32)`、`.justify_content(FlexAlign)`、`.align_items(FlexAlign)`、`.center()`、`.background(Color)`、`.padding(f32)`、`.border_radius(f32)`、`.width(f32)`、`.height(f32)`、`.on_click(fn)`、`.on_click_with_ctx(fn)`、`.on_drag_start(fn)`、`.on_drag_move(fn)`、`.on_drag_end(fn)`、`.font_size(f32)`、`.color(Color)`、`.key(s)` |
| 颜色 | `Color::new(r,g,b)`、`Color::WHITE`、`Color::RED`、…（带 alpha 见 `geometry::Color`） |
| 布局 | `FlexAlign`、`FlexDirection`、`FlexWrap` |
| 主题 | `Theme`、`current()`、`set(theme)` |
| 渲染 | `Runtime`、`VelloRenderer`、`Renderer`、`LayeredElement`、`VisualElement` |

> `Widget` trait 本身不在 prelude，需用 `use lieui::widget::Widget;`。`State` 的 `clear_state(id)` 接口**未公开**，按路径/key 的清理由 reconciler 自动完成，无需手动调用。

---

## 4. 已知问题速览

1. ✅ 已修复：Button hover/pressed 状态绑错节点（现作用在最深命中节点的可点击祖先上）。
2. ✅ 已修复：`EventContext::request_rebuild/request_layout/request_render` 现被事件循环应用。
3. ✅ 已修复：事件捕获（root→target）与冒泡（parent→root）遍历顺序已区分。
4. 🟡 部分修复：文本在约束宽度下已换行；但 `flex_shrink` 默认 0，`Row` 内挤占过满仍可能溢出。
5. ✅ 已验证：softbuffer 像素字节序与 `VelloRenderer` 的 pixmap `blit_to_window` 路径一致，跨平台渲染结果正确（有像素级单测覆盖）。
6. ✅ 裁剪（Clip）已实现：基于 `vello_cpu` 的 `RenderContext::push_clip_path` / `pop_clip_path` 非隔离路径裁剪。当 `ViewNode` 标记 `paint.clip_content`（如 `Container::clip(true)`）或 `overflow_scroll`（如 `ScrollView` / `VirtualList`）时，reconciler 会把子节点收进带 `clip_rect` 的 `Group`，引擎层执行精确路径裁剪；Image 走自定义 blit 路径时单独做矩形+圆角相交裁剪。见测试 `clip_content_generates_group_with_clip_rect` 与 `scroll_view_clips_overflowing_content`。

---

## 5. 架构设计总结

LieUI 采用「声明式 Widget 树 → 纯数据 ViewNode → 布局 → 渲染图元 → 软件光栅化」的分层管线，单线程、按帧重建。核心分层如下：

### 5.1 分层结构

```
┌──────────────────────────────────────────────────────────────┐
│ 应用层（examples / 集成方）                                     │
│   Application（多窗口编排） + 自定义 Widget + State<T> 句柄      │
└───────────────────────────────┬──────────────────────────────┘
                                 │ builder(&mut BuildContext) -> Box<dyn Widget>
┌───────────────────────────────▼──────────────────────────────┐
│ Widget 层（src/widget）                                         │
│   Column/Row/Button/Container/Text/Image/ScrollView/…          │
│   Widget::build(ctx) -> ViewNode；用 BuildContext::use_state    │
│   持久化组件内状态                                              │
└───────────────────────────────┬──────────────────────────────┘
                                 │ 产出
┌───────────────────────────────▼──────────────────────────────┐
│ ViewNode 层（src/view） — UI 唯一纯数据中间表示                  │
│   enum ViewNode { Text, Image, Div }  + PaintStyle/FlexStyle    │
│   不含任何组件逻辑，仅内联样式（layout + paint）                 │
└───────┬───────────────────────────────────┬───────────────────┘
        │ diff                               │ 消费
┌───────▼────────────────┐      ┌────────────▼──────────────────┐
│ Reconciler（runtime）   │      │ Layout（Taitank 风格移植）      │
│ 按 key/位置 复用 Element │      │ FlexNode 引擎 → ComputedLayout │
└───────┬────────────────┘      └────────────┬──────────────────┘
        │ ElementTree                          │ 布局结果
        └───────────────┬──────────────────────┘
                        ▼
┌──────────────────────────────────────────────────────────────┐
│ Runtime（每窗口独立，src/runtime）                              │
│   submit_view_tree → reconciler diff → perform_layout          │
│   → 展开 LayeredElement + 交互状态（hover/pressed）            │
│   ElementManager / EventManager（三层 Layer 命中 + 事件分发）  │
└───────────────────────────────┬──────────────────────────────┘
                                 │ Vec<LayeredElement>
┌───────────────────────────────▼──────────────────────────────┐
│ 渲染层（src/render）                                            │
│   engine.rs：VelloRenderer 用 vello_cpu 软件光栅化             │
│   - 普通图元经 RenderContext（含 push/pop_clip_path 裁剪）      │
│   - Image 走自定义 blit（矩形+圆角相交裁剪）                    │
│   - 视口剔除优化                                                │
│   visual.rs：LayeredElement / VisualElement / Group(clip)       │
└───────────────────────────────┬──────────────────────────────┘
                                 │ pixmap
┌───────────────────────────────▼──────────────────────────────┐
│ 窗口层（src/window + winit + softbuffer）                       │
│   blit_to_window：pixmap → softbuffer 表面 → 屏幕              │
└──────────────────────────────────────────────────────────────┘
```

### 5.2 关键设计决策

- **按帧重建（rebuilt-per-frame）而非增量 diff 式声明 UI**：builder 每次重建整棵 `Widget` 树，但 `Widget` 实例是廉价的配置对象；真正的「增量」发生在 `reconciler` 对 `ViewNode` 的 diff（复用 `ElementId` 以保留布局/状态/回调）。换取实现简单与可预测性。
- **Widget 与 ViewNode 分离**：`Widget` 是面向用户的、可带状态的逻辑组件；`ViewNode` 是扁平的、无逻辑的纯数据原语（仅 Text/Image/Div）。Widget 把"类 CSS 的高级语义"编译成 ViewNode 的内联样式，下游布局/渲染对 Widget 完全不可知。
- **每窗口完全隔离**：`Application` 持有 `HashMap<WindowId, WindowContext>`；每个 `WindowContext` 拥有独立的 `Runtime`、`VelloRenderer`、`StateMap` 与重建/重绘/关闭标志。所有 `request_*` 通过「当前窗口」路由（`state::set_current_window` 在进入 `window_event` 时设置），多窗口信号互不吞噬；`StateMap` 不再跨窗口共享，状态不串味。
- **单线程 + thread-local 路由**：winit 事件循环单线程驱动，重建/重绘/关闭标志以 `thread_local CURRENT_WID` + `HashMap<WindowId, Flags>` 实现 per-window 路由；在窗口上下文外调用（后台 controller、动画 tick）时广播到所有窗口，保证单窗口语义不变。
- **每窗口图层栈（Base/Overlay/Modal/…）**：`LayerStack` 是 `Runtime` 的独立字段，浮层命令经全局 `show_*` 入口压入 per-window 待处理队列（`PENDING_LAYER` thread-local），由 `request_rebuild` 路由到当前窗口，下一帧重建时合并进该窗口的 `LayerStack`——多窗口浮层互不串扰。`EventManager` 命中测试按 `Modal → Overlay → Base` 优先级返回最顶层节点，Modal 自动拦截其范围内点击。
- **裁剪**：`vello_cpu` 的 `RenderContext::push_clip_path` / `pop_clip_path` 非隔离路径裁剪实现 Group 级精确裁剪；Image 因走自定义 blit 而单独做矩形+圆角相交裁剪。
- **Inspector（feature `inspector`）**：每个窗口独立起一个非阻塞轮询的 WebSocket/CPD server，把 `Widget` 树（含 `inspect_name` 真实类型名）推给 Chrome DevTools；server 线程用 `set_nonblocking` + 短轮询，Drop 时仅置 stop 标志后 join，避免阻塞主线程退出。

### 5.3 模块地图

| 模块 | 职责 |
|------|------|
| `src/widget` | Widget 抽象、BuildContext、所有内置组件 |
| `src/view` | ViewNode 数据 + PaintStyle/FlexStyle 样式 |
| `src/runtime` | Runtime、Reconciler、ElementTree、事件/命中分发 |
| `src/layout` | Flexbox（Taitank 移植）布局引擎 |
| `src/render` | VelloRenderer（vello_cpu 软光栅）、LayeredElement |
| `src/core` | ElementId、图层栈（LayerKind）、交互状态 |
| `src/event` | 事件类型、EventManager、EventContext、拖拽合成 |
| `src/state` | State<T>、per-window 重建/重绘/关闭标志、图层函数 |
| `src/app` | Application 多窗口编排、winit 事件循环接入 |
| `src/window` | WindowConfig、softbuffer 表面与 blit |
| `src/inspector` | （feature）DevTools CDP server |
| `src/text` | 文本排版/度量（含内嵌图标字体） |
| `src/theme` | 主题（颜色/字体默认值） |
| `src/geometry` | Color / Size / Point / Rect 等基础几何 |

[winit]: https://crates.io/crates/winit
[softbuffer]: https://crates.io/crates/softbuffer
[Taitank]: https://github.com/Tencent/Taitank
[vello_cpu]: https://crates.io/crates/vello_cpu
