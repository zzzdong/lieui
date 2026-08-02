# LieUI 开发指南（v2）

LieUI 是一个 Rust 声明式即时模式（rebuilt-per-frame）GUI 库，基于 [winit] + [softbuffer] 窗口、`vello_cpu` 软件光栅化渲染，布局使用对 [Taitank] 的忠实移植（Flexbox）。

> 历史文档（`docs/architecture.md`、`docs/widget.md`、`docs/render.md`、`docs/event.md`、`docs/layout.md`、`docs/text.md`、`docs/refactoring_plan.md`）描述的是**旧版 `Widget` 架构**，与当前实现不符，仅供考古。本文档是现行权威。

---

## 1. 30 秒上手

```rust
use lieui::geometry::{Color, Size};
use lieui::layout::flex::{AlignItems, JustifyContent};
use lieui::prelude::*;
use lieui::state::State;
use lieui::view::View;

fn main() {
    let count = State::new(0);

    let app = Application::new(
        move || {
            Row::new()
                .expand(true)
                .justify_content(JustifyContent::Center)
                .align_items(AlignItems::Center)
                .child(
                    Column::new()
                        .spacing(16.0)
                        .expand(true)
                        .justify_content(JustifyContent::Center)
                        .align_items(AlignItems::Center)
                        .child(Text::new("Counter").font_size(48.0))
                        .child(Text::new(format!("{}", count.get())).font_size(72.0).color(Color::RED))
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
                )
                .build()
        },
        Size::new(400.0, 300.0),
    );

    app.run();
}
```

核心三步：
1. 用 `State::new` 持有可变状态。
2. `Application::new(builder, viewport)` 传入一个返回 `ViewNode` 的闭包；闭包里用 `Column`/`Row`/`Button`/`Text` 等组合 UI。
3. `app.run()` 启动事件循环。状态变化时调用 `request_rebuild()`（或 `State::update` 内部自动触发）即可重绘。

---

## 2. 核心概念

### 2.1 `View` trait 与 `ViewNode`
所有 UI 块都实现 `View`：

```rust
pub trait View {
    fn build(&self) -> ViewNode;
}
```

`ViewNode` 是一个枚举（`Text` / `Div` / `Image` / `Canvas`），是 UI 的**唯一**中间表示。`Column`/`Row`/`Button`/`Container`/`Text`/`Image` 这些原语只是 `ViewNode` 的便捷构造器；用户也能为自定义组件实现 `View`：

```rust
struct Label { text: String }
impl View for Label {
    fn build(&self) -> ViewNode {
        Text::new(self.text.clone()).color(Color::WHITE).build()
    }
}
```

闭包 `Fn() -> ViewNode` 本身也实现了 `View`，因此 `Container::child(|| Text::new("hi"))` 也合法。

### 2.2 `Application` 与渲染管线
`Application::new(builder, viewport).run()`：
- 每帧（重建触发时）调用 `builder()` 得到一棵 `ViewNode` 树；
- `Runtime::submit_view_tree` + `reconciler` 与上一次树做 diff，更新 `ElementTree`；
- `perform_layout` 把 `ViewNode` 树（经 `to_flex_style` 转换）喂给 Flex 布局引擎，得到每个节点的 `ComputedLayout`；
- `cv` 把布局 + 状态（hover/pressed）+ 样式展开成 `LayeredElement` 列表（z 序由图层决定）；
- `VelloRenderer` 软件渲染成 pixmap，`blit_to_window` 拷到 softbuffer 表面。

> 仅**鼠标移动**（hover 变化）时走轻量的 `frame_render_only`，不再重建整树；其他变更走完整重建。

### 2.3 状态 `State<T>`
`State<T>` 是一个 `Rc<RefCell<T>>` 包装：

| 操作 | 说明 |
|------|------|
| `State::new(v)` | 创建 |
| `s.get()` | 读（`&T`） |
| `s.set(v)` | 写并触发 `request_rebuild()` |
| `s.update(|v| …)` | 闭包内修改并触发重建 |
| `s.clone()` | 复制句柄，在闭包里捕获 |
| `state::clear_state(id)` | 按 id 清除（当前公开 API 未暴露 id 访问） |

闭包在每次重建时重新执行，因此 `count.get()` 总是读到最新值。

### 2.4 事件与回调
- 命中测试 `hit_test_top` 在三层（Modal → Overlay → Base）中找最顶层、最内层节点。
- `EventManager` 做三阶段分发（捕获 → 目标 → 冒泡，沿 `path` 遍历）。
- 给节点加 `.on_click(closure)` 或 `.on_click_with_ctx(|ctx| …)`：
  - `Simple`：点击即执行，`Simple` 类型会自动 `stop_propagation`。
  - `WithCtx`：回调收到 `&mut EventContext`，可调用 `ctx.stop_propagation()`、读 `ctx.phase()`。
  - ✅ **已修复**：`ctx.request_rebuild()/request_layout()/request_render()` 现在会在事件循环中被应用（触发重建 / 重排 / 重绘）。
- 点击只会在 **Target / Bubble** 阶段触发（捕获阶段不触发 `Click`）。
- 监听器区分**内置行为**（`ListenerKind::BuiltIn`，如 Slider 拖拽、Input 聚焦/输入、Draggable 拖拽接线）与**用户回调**（`ListenerKind::User`，如 `.on_click(...)`）。同一节点上内置回调**先于**用户回调执行，因此用户回调调用 `stop_propagation()` 不会阻止同节点已执行的内置行为，只会停止向其他节点传播。hover/pressed 视觉反馈不依赖回调，由样式（`hover_background` / `hover_color` 等）+ 交互状态驱动。

> ✅ **已修复**：hover/pressed 状态现作用在最深层命中节点向上找到的「最近带 listener 的祖先」（交互节点）上，`Button` 的 hover/pressed 背景反馈正常显示（见 `docs/audit.md` 1.1）。

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
| `.justify_content(JustifyContent::*)` | 主轴对齐 |
| `.align_items(AlignItems::*)` | 交叉轴对齐（默认 `Stretch`） |
| `.center()` | 主轴+交叉轴居中并 `expand(true)` |

`Container` 映射为 `ViewNode::Div` + `DisplayMode::Block`，支持 `.width()/.height()/.background()/.padding()/.border_radius()`。
布局引擎是 Taitank 风格移植，支持 `flex_grow`/`flex_shrink`、换行、padding/border/margin。

> ✅ **已修复**：文本在约束宽度下会重新排版以支持换行（见 `docs/audit.md` 2.2）。注意：处于 `Row`（主轴水平）且宽度不受限时文本仍按单行处理；内部 `FlexStyle` 的 `flex_shrink` 默认 0 保持不变，因此在 `NoWrap` 行内挤占过满时仍可能溢出——需给容器设定宽度或改用 `Column`。

### 2.8 图层（Base / Overlay / Modal）
通过顶层函数弹出覆盖层：

```rust
show_overlay(some_view_node);   // 在 Base 之上显示
show_modal(some_view_node);     // 在最顶层显示，且优先命中
hide_overlay();
hide_modal();
```

命中测试按 `Modal → Overlay → Base` 优先级返回最顶层节点，因此 modal 会拦截其范围内的点击。

### 2.9 Reconciler 与 key
每次重建产生新 `ViewNode` 树，reconciler 与旧树 diff：
- 优先按 `key` 匹配（`ViewExt::key`，见下）；
- 无 key 时按「节点类型 + 位置」匹配，并复用原 `ElementId` 以保留布局/状态/回调。
- 动态列表**务必**使用 key，否则重排可能产生错误视觉顺序（见 `docs/audit.md` 4）。

```rust
use lieui::view::ViewExt;
// 给列表项加稳定 key：
Column::new().child(item_view.key(format!("row-{i}")))
```

---

## 3. 公开 API 速查

预导入：`use lieui::prelude::*;` 包含 `Application`、`State`、`Column`、`Row`、`Text`、`Button`、`Container`、`Draggable`、`Icon`、`IconButton`、`IconName`、`Image`、`Color`、`Size`、`Point`、`Rect`、`JustifyContent`、`AlignItems`、`FlexDirection`、`FlexWrap`、`LayeredElement`、`VisualElement`、`View`。

| 类别 | 符号 |
|------|------|
| 入口 | `Application::new(builder, viewport).run()`（自定义字体可用 `.with_font(path)`） |
| 状态 | `State::new/get/set/update/clone`、`request_rebuild()`、`request_redraw()` |
| 图层 | `show_overlay(ViewNode)`、`show_modal(ViewNode)`、`hide_overlay()`、`hide_modal()` |
| 原语 | `Text::new(s)`、`Image::from_rgba(data,w,h)`、`Container::new()`、`Column::new()`、`Row::new()` |
| 组件 | `Button::new(s)`、`Checkbox::new(...)`、`Divider`、`ListView`、`Draggable::new(w)`、`Icon::new(name, size)`、`IconButton::new(name)` |
| 通用方法 | `.child(v)`、`.expand(bool)`、`.spacing(f32)`、`.justify_content(...)`、`.align_items(...)`、`.center()`、`.background(Color)`、`.padding(f32)`、`.border_radius(f32)`、`.width(f32)`、`.height(f32)`、`.on_click(fn)`、`.on_click_with_ctx(fn)`、`.on_drag_start(fn)`、`.on_drag_move(fn)`、`.on_drag_end(fn)`、`.font_size(f32)`、`.color(Color)` |
| key | `ViewExt::key(s)`（需 `use lieui::view::ViewExt;`） |
| 颜色 | `Color::new(r,g,b)`、`Color::WHITE`、`Color::RED`、…（带 alpha 见 `geometry::Color`） |

---

## 4. 已知问题速览（详见 `docs/audit.md`）
1. ✅ 已修复：Button hover/pressed 状态绑错节点（现作用在最深命中节点的可点击祖先上）。
2. ✅ 已修复：`EventContext::request_rebuild/request_layout/request_render` 现被事件循环应用。
3. ✅ 已修复：事件捕获（root→target）与冒泡（parent→root）遍历顺序已区分。
4. 🟡 部分修复：文本在约束宽度下已换行；但 `flex_shrink` 默认 0，`Row` 内挤占过满仍可能溢出。
5. ⏳ softbuffer 像素字节序待实测验证。
6. ⏳ 裁剪（Clip）未实现。

[winit]: https://crates.io/crates/winit
[softbuffer]: https://crates.io/crates/softbuffer
[Taitank]: https://github.com/Tencent/Taitank
[vello_cpu]: https://crates.io/crates/vello_cpu
