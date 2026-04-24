# LieUI 项目编程指导

## 总体思路

**LieUI** 是一个极简 Rust GUI 库，采用独立的三棵树 WidgetTree、RenderTree、Layout Tree，以及 ViewContext 管理它们的架构。
**核心原则**：**简化设计优先**——最少概念、最少代码、最直观 API  
**技术栈**：
- **渲染后端**：vello_cpu（CPU 矢量渲染，`RenderContext` 立即模式 API）
- **文本布局**：parley（Linebender 官方文本引擎）
- **窗口/事件**：winit（跨平台窗口与事件循环）
**外部库API文档查询**：
- `cargo doc`生成文档到 `target/doc` 目录。

## 调试规范

1. 使用 `view.debug_render_tree = true` 输出渲染树 XML
2. 添加有意义的 `type_name()` 便于调试
3. 使用 `log` crate 进行日志记录

## 测试规范

每个模块应包含单元测试:

## 文档规范

1. 公共 API 必须有文档注释
2. 使用 `///` 描述函数/结构体
3. 使用 `//!` 描述模块
4. 示例代码使用 ```rust 标记

## 提交前检查

```bash
cargo check
cargo clippy
cargo fmt
cargo test
```
