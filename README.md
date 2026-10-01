# lieui

**lieui** is a GUI toolkit in pure Rust: a **retained view tree** aligned against per-frame
descriptions, a tiny reactive layer, and **pure-CPU rendering** (vello_cpu) with damage-rect
tracking.

**lieui** 是一个纯 Rust 的 GUI 库：**保留视图树 + 描述对齐**架构、轻量响应式信号、
**纯 CPU 渲染**（vello_cpu）与脏区局部重绘。

```rust,ignore
use lieui::prelude::*;

struct Counter(i32);
impl ViewModel for Counter {
    fn view(self: &Rc<Self>, v: &mut ViewBuf) {
        v.column(|c| {
            c.gap(8.0);
            c.text(format!("clicked {} times", self.0));
            c.button("click me").on_tap(|cx| { /* 读写模型 */ });
        });
    }
}
```

## Highlights / 特性

- **Retained tree + per-frame align**：每帧产出描述树，与保留树对齐补丁（复用节点、保留交互态）；
- **Reactive signals**：`Signal` 驱动 `view()` 重跑，双向绑定（slider / checkbox / input）零回弹；
- **Pure-CPU rendering**：无 GPU 依赖，脏区行带局部光栅化 + 局部上屏；
- **Batteries included**：布局引擎、文本测度缓存、滚动、弹层锚点定位、自定义绘制节点。

## Crates / 工作区

| crate | 说明 |
|---|---|
| `lieui` | 主 crate：视图树、响应式、渲染、平台层（winit，可关） |
| `lieui-layout` | 零依赖 Flexbox 布局引擎（Taitank 风格） |
| `lieui-text` | 文本测度与排版（parley 封装 + 缓存） |
| `lieui-geom` | 基础几何与颜色类型 |

## License / 许可

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

双许可（MIT OR Apache-2.0），任选其一。
