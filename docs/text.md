# 文本系统文档

## 概述

LieUI 使用 **parley** 作为文本布局引擎，支持复杂的文本排版和 IME 输入。

## 核心概念

### 全局线程存储

文本上下文使用 `thread_local!` 实现全局访问：

```rust
thread_local! {
    /// 全局字体上下文
    pub static FONT_CONTEXT: RefCell<FontContext> = RefCell::new(FontContext::default());

    /// 全局布局上下文
    pub static LAYOUT_CONTEXT: RefCell<LayoutContext<TextColor>> = RefCell::new(LayoutContext::default());
}
```

### 便捷访问函数

```rust
/// 访问字体上下文
pub fn with_font_context<R, F: FnOnce(&mut FontContext) -> R>(f: F) -> R {
    FONT_CONTEXT.with(|cx| f(&mut cx.borrow_mut()))
}

/// 访问布局上下文
pub fn with_layout_context<R, F: FnOnce(&mut LayoutContext<TextColor>) -> R>(f: F) -> R {
    LAYOUT_CONTEXT.with(|cx| f(&mut cx.borrow_mut()))
}

/// 同时访问两个上下文
pub fn with_text_contexts<R, F: FnOnce(&mut FontContext, &mut LayoutContext<TextColor>) -> R>(f: F) -> R {
    FONT_CONTEXT.with(|font_cx| {
        LAYOUT_CONTEXT.with(|layout_cx| {
            f(&mut font_cx.borrow_mut(), &mut layout_cx.borrow_mut())
        })
    })
}
```

## 核心类型

### TextEngine

文本布局引擎（纯静态方法）：

```rust
pub struct TextEngine;

impl TextEngine {
    /// 创建布局
    pub fn layout(
        text: &str,
        style: &TextStyle,
        scale: f32,
        max_width: Option<f32>,
    ) -> TextLayout {
        with_text_contexts(|font_cx, layout_cx| {
            let mut builder = layout_cx.ranged_builder(font_cx, text, scale, true);
            style.apply(&mut builder);

            let mut layout = builder.build(text);
            layout.break_all_lines(max_width);
            layout.align(parley::Alignment::Start, parley::AlignmentOptions::default());

            layout
        })
    }
}
```

### TextStyle

文本样式：

```rust
#[derive(Clone)]
pub struct TextStyle(pub parley::Style<'static, TextColor>);

impl Default for TextStyle {
    fn default() -> Self {
        let mut style = parley::Style::default();
        style.brush = TextColor::default();
        style.font_size = 14.0;
        Self(style)
    }
}

impl TextStyle {
    pub fn font_size(mut self, size: f32) -> Self {
        self.0.font_size = size;
        self
    }

    pub fn text_color(mut self, color: TextColor) -> Self {
        self.0.brush = color;
        self
    }
}
```

### TextColor

文本颜色（实现 parley 的 Brush trait）：

```rust
#[derive(Clone, Copy, Debug, Default)]
pub struct TextColor(pub AlphaColor<Srgb>);

impl TextColor {
    pub fn from(color: Color) -> Self {
        Self(color.0)
    }
}

// 实现 parley 的 Brush trait
impl parley::Brush for TextColor {
    // ...
}
```

### TextLayout

布局结果（parley Layout 的别名）：

```rust
pub type TextLayout = parley::Layout<TextColor>;
```

## 使用示例

### 基本文本布局

```rust
use lieui::text::{TextEngine, TextStyle, TextColor};
use lieui::geometry::Color;

// 创建样式
let style = TextStyle::default()
    .font_size(16.0)
    .text_color(TextColor::from(Color::BLACK));

// 创建布局
let layout = TextEngine::layout("Hello, LieUI!", &style, 1.0, Some(200.0));

// 获取尺寸
let width = layout.width();
let height = layout.height();
```

### 直接使用上下文

```rust
use lieui::text::with_text_contexts;
use parley::{FontContext, LayoutContext};

with_text_contexts(|font_cx: &mut FontContext, layout_cx: &mut LayoutContext<TextColor>| {
    // 使用上下文进行复杂操作
    let mut builder = layout_cx.ranged_builder(font_cx, "Text", 1.0, true);
    // ... 配置 builder ...
    let layout = builder.build("Text");
});
```

### 在 Widget 中使用

```rust
use lieui::prelude::*;
use lieui::text::{TextEngine, TextStyle};

pub struct Text {
    content: String,
    style: TextStyle,
}

impl Widget for Text {
    fn layout(&self, id: WidgetId) -> LayoutNode {
        LayoutNode::new(id)
            .with_intrinsic_size(IntrinsicSize::Measurable(
                TextMeasure::new(self.content.clone(), self.style.clone())
            ))
    }

    fn render(&self, layout: &LayoutNode, _ctx: &ViewContext) -> RenderNode {
        let bounds = layout.computed_bounds();
        let text_layout = TextEngine::layout(&self.content, &self.style, 1.0, Some(bounds.width));
        RenderNode::text(bounds, text_layout)
    }
}
```

## PlainEditor 集成

### 文本编辑器

parley 的 `PlainEditor` 提供文本编辑功能：

```rust
use parley::{PlainEditor, PlainEditorDriver};

pub struct TextInput {
    editor: PlainEditor<TextColor>,
}

impl TextInput {
    pub fn new() -> Self {
        Self {
            editor: PlainEditor::new(14.0),  // 默认字体大小 14.0
        }
    }

    /// 使用 Driver 执行编辑操作
    fn with_driver<F, R>(&mut self, f: F) -> R
    where F: FnOnce(&mut PlainEditorDriver<TextColor>) -> R
    {
        with_text_contexts(|font_cx, layout_cx| {
            let mut driver = self.editor.driver(font_cx, layout_cx);
            f(&mut driver)
        })
    }
}
```

### 编辑操作

```rust
impl TextInput {
    /// 插入文本
    pub fn insert(&mut self, text: &str) {
        self.with_driver(|driver| {
            driver.insert_or_replace_selection(text);
        });
    }

    /// 删除选区或向后删除
    pub fn backdelete(&mut self) {
        self.with_driver(|driver| {
            driver.backdelete();
        });
    }

    /// 移动光标
    pub fn move_left(&mut self) {
        self.with_driver(|driver| {
            driver.move_left();
        });
    }

    /// 选区操作
    pub fn select_all(&mut self) {
        self.with_driver(|driver| {
            driver.select_all();
        });
    }
}
```

### IME 支持

```rust
impl TextInput {
    /// 设置预编辑文本
    pub fn set_compose(&mut self, text: &str, cursor: Option<(usize, usize)>) {
        self.with_driver(|driver| {
            driver.set_compose(text, cursor);
        });
    }

    /// 完成预编辑
    pub fn finish_compose(&mut self) {
        self.with_driver(|driver| {
            driver.finish_compose();
        });
    }

    /// 清除预编辑
    pub fn clear_compose(&mut self) {
        self.with_driver(|driver| {
            driver.clear_compose();
        });
    }
}
```

### 获取布局信息

```rust
impl TextInput {
    /// 获取布局（不可变借用）
    pub fn try_layout(&self) -> Option<&parley::Layout<TextColor>> {
        self.editor.try_layout()
    }

    /// 刷新布局（文本变更后调用）
    pub fn refresh_layout(&mut self) {
        with_text_contexts(|font_cx, layout_cx| {
            self.editor.refresh_layout(font_cx, layout_cx);
        });
    }

    /// 获取光标几何信息
    pub fn cursor_geometry(&self, size: f32) -> Option<BoundingBox> {
        self.editor.cursor_geometry(size)
    }

    /// 获取选区几何信息
    pub fn selection_geometry(&self) -> Vec<(BoundingBox, TextColor)> {
        self.editor.selection_geometry()
    }
}
```

## 文本测量

### Measurable Trait

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

1. **使用全局上下文**：避免在 Widget 中存储 FontContext
2. **缓存布局结果**：使用 `try_layout()` 避免重复计算
3. **及时刷新布局**：文本变更后调用 `refresh_layout()`
4. **处理空字符串**：`insert_or_replace_selection` 不接受空字符串
5. **检查 ICU4X 错误**：日语等语言可能需要额外数据文件

## 常见问题

### ICU4X 数据错误

```
ICU4X data error: No segmentation model for language: ja
```

这是警告信息，不影响基本功能。如需完整支持，可配置 ICU4X 数据文件。

### 空字符串 Panic

```rust
// 错误
editor.insert_or_replace_selection("");  // panic!

// 正确
if !text.is_empty() {
    editor.insert_or_replace_selection(text);
}
```

## 参考

- [架构设计文档](./architecture.md)
- [Widget 系统文档](./widget.md)
- [parley 文档](https://docs.rs/parley)
