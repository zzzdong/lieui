> ⚠️ **本文档已过时**（描述旧 `Widget`/`RenderNode` 架构），与当前 `v2-rewrite` 实现不符。
> 现行渲染管线入口为 `runtime::cv` → `LayeredElement`/`VisualElement` → `VelloRenderer`（vello_cpu 软件光栅化）→ `Application::blit_to_window`（softbuffer）。
> **请以 [`../guide.md`](../guide.md) 为最新权威文档**；本文档仅留作历史参考。

# 渲染系统文档

## 概述

LieUI 使用 **vello_cpu** 作为渲染后端，采用**立即模式**渲染方式。

## 核心概念

### RenderNode

渲染树节点，表示一个渲染指令：

```rust
pub enum RenderNode {
    /// 容器节点
    View {
        bounds: Rect,
        children: Vec<RenderNode>,
    },

    /// 矩形区域（支持背景、边框、圆角）
    Div {
        bounds: Rect,
        background: Option<Color>,
        border_color: Option<Color>,
        border_width: f32,
        border_radius: Option<f32>,
        children: Vec<RenderNode>,
    },

    /// 文本渲染
    Text {
        bounds: Rect,
        layout: TextLayout,
    },

    /// 图片渲染
    Image {
        bounds: Rect,
        width: u32,
        height: u32,
        data: Vec<u8>,
    },

    /// 自定义绘制
    Canvas {
        bounds: Rect,
        draw: Box<dyn Fn(&mut RenderContext, &Rect)>,
    },

    /// 带数据的自定义绘制
    CanvasWithData<T> {
        bounds: Rect,
        data: T,
        draw: Box<dyn Fn(&mut RenderContext, &Rect, &T)>,
    },

    /// 离屏渲染
    Pixmap {
        bounds: Rect,
        pixmap: Rc<RefCell<vello_cpu::Pixmap>>,
        opacity: Option<f32>,
    },
}
```

### VelloRenderer

渲染引擎，将 RenderTree 渲染到 Pixmap：

```rust
pub struct VelloRenderer;

impl VelloRenderer {
    pub fn new() -> Self { Self }

    pub fn render(&self, tree: &RenderNode, pixmap: &mut Pixmap) {
        let mut ctx = RenderContext::new(pixmap);
        self.render_node(&mut ctx, tree);
    }
}
```

### RenderContext

vello_cpu 提供的绘制上下文：

```rust
// vello_cpu::RenderContext
pub struct RenderContext {
    // 内部状态
}

impl RenderContext {
    /// 填充矩形
    pub fn fill_rect(&mut self, rect: &kurbo::Rect);

    /// 绘制文本布局
    pub fn draw_text(&mut self, layout: &Layout<Brush>, pos: kurbo::Point);

    /// 设置画笔
    pub fn set_paint(&mut self, paint: impl Into<Paint>);

    /// 保存/恢复状态
    pub fn save(&mut self);
    pub fn restore(&mut self);

    /// 透明度层
    pub fn push_opacity_layer(&mut self, opacity: f32);
    pub fn pop_layer(&mut self);
}
```

## 渲染流程

### 1. 生成 RenderTree

```rust
// ViewContext::render()
pub fn render(&self) -> Option<RenderNode> {
    self.widget_tree.root().map(|root_id| {
        self.render_widget(root_id)
    })
}

fn render_widget(&self, widget_id: WidgetId) -> RenderNode {
    let widget = self.get_widget(widget_id).unwrap();
    let layout_node = self.layout_ctx.get_layout(widget_id).unwrap();
    widget.render(layout_node, self)
}
```

### 2. 渲染到 Pixmap

```rust
// App::render_and_present()
if let Some(render_tree) = self.view.render() {
    if self.view.debug_render_tree {
        log::debug!("{}", render_tree.to_xml(0));
    }
    self.renderer.render(&render_tree, &mut self.pixmap);
}
```

### 3. 呈现到窗口

```rust
// 将 Pixmap 复制到 softbuffer Surface
let buffer = self.pixmap.data();
surface.present_with_buffer(buffer, width, height)?;
```

## RenderNode 类型详解

### View

容器节点，用于组织子节点：

```rust
RenderNode::view(bounds)
    .add_child(child1)
    .add_child(child2)
```

### Div

矩形区域，支持视觉样式：

```rust
RenderNode::div(bounds)
    .background(Color::WHITE)
    .border(Color::GRAY, 1.0)
    .border_radius(8.0)
```

### Text

文本渲染：

```rust
let layout = TextEngine::layout("Hello", &style, 1.0, Some(100.0));
RenderNode::text(bounds, layout)
```

### Canvas

自定义绘制：

```rust
RenderNode::canvas(bounds, |ctx, rect| {
    // 使用 vello_cpu API 绘制
    ctx.set_paint(Color::RED);
    ctx.fill_rect(&kurbo::Rect::new(
        rect.x as f64,
        rect.y as f64,
        (rect.x + rect.width) as f64,
        (rect.y + rect.height) as f64,
    ));
})
```

### CanvasWithData

带数据的自定义绘制：

```rust
RenderNode::canvas_with_data(
    bounds,
    my_data,
    |ctx, rect, data| {
        // 使用 data 进行绘制
    }
)
```

### Pixmap

离屏渲染：

```rust
// 创建 Pixmap 节点
let pixmap = Pixmap::new(width, height);
let node = RenderNode::from_pixmap(bounds, pixmap);

// 渲染时，Pixmap 会被绘制到目标
```

## 渲染实现细节

### Div 渲染

```rust
fn render_div(&self, ctx: &mut RenderContext, div: &DivNode) {
    // 1. 应用圆角裁剪（如果有）
    if let Some(radius) = div.border_radius {
        ctx.push_clip_rounded_rect(&div.bounds, radius);
    }

    // 2. 绘制背景
    if let Some(bg) = div.background {
        ctx.set_paint(bg);
        ctx.fill_rect(&div.bounds);
    }

    // 3. 绘制边框
    if let Some(border) = div.border_color {
        ctx.set_paint(border);
        ctx.stroke_rect(&div.bounds, div.border_width);
    }

    // 4. 绘制子节点
    for child in &div.children {
        self.render_node(ctx, child);
    }

    // 5. 恢复裁剪
    if div.border_radius.is_some() {
        ctx.pop_clip();
    }
}
```

### Text 渲染

```rust
fn render_text(&self, ctx: &mut RenderContext, text: &TextNode) {
    ctx.draw_text(&text.layout, kurbo::Point::new(
        text.bounds.x as f64,
        text.bounds.y as f64,
    ));
}
```

### Pixmap 渲染

```rust
fn render_pixmap(&self, ctx: &mut RenderContext, pixmap: &PixmapNode) {
    let pixmap_ref = pixmap.pixmap.borrow();

    // 创建 Image
    let image = Image {
        image: ImageSource::Pixmap(Arc::new(pixmap_ref.clone())),
        sampler: ImageSampler::default(),
    };

    // 应用透明度
    if let Some(opacity) = pixmap.opacity {
        ctx.push_opacity_layer(opacity);
    }

    // 绘制
    ctx.set_paint(image);
    ctx.fill_rect(&pixmap.bounds.to_kurbo());

    if pixmap.opacity.is_some() {
        ctx.pop_layer();
    }
}
```

## 调试工具

### XML 输出

启用渲染树调试：

```rust
ctx.debug_render_tree = true;
```

输出格式：

```xml
<View bounds="0,0 800x600">
  <Div bounds="10,10 200x50" background="#FFFFFF" border="#CCCCCC" border_width="1">
    <Text bounds="15,15 100x20">Hello</Text>
  </Div>
</View>
```

## 性能优化

### 1. 脏检查

只重新渲染变化的区域：

```rust
if widget.is_dirty() {
    // 重新生成渲染节点
}
```

### 2. 离屏渲染

使用 Pixmap 缓存复杂内容：

```rust
// 复杂 Widget 先渲染到 Pixmap
let pixmap = Pixmap::new(width, height);
// ... 渲染内容 ...

// 后续帧直接绘制 Pixmap
RenderNode::from_pixmap(bounds, pixmap)
```

### 3. 批量绘制

vello_cpu 会自动批处理绘制命令。

## 最佳实践

1. **最小化 RenderTree 大小**：只包含可见节点
2. **复用 Layout**：使用 `layout.computed_bounds()` 获取位置
3. **合理使用 Canvas**：简单图形优先使用 Div
4. **注意裁剪**：圆角和遮罩有性能开销

## 参考

- [架构设计文档](./architecture.md)
- [Widget 系统文档](./widget.md)
- [布局系统文档](./layout.md)
- [vello_cpu 文档](https://docs.rs/vello_cpu)
