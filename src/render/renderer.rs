//! Renderer trait — 渲染后端抽象

use crate::render::visual::LayeredElement;

pub trait Renderer {
    /// 光栅化 `elements`，返回渲染器内部持久 pixmap 的引用。
    ///
    /// 返回值借用 `&mut self`：调用方应在同一语句内消费（取像素/字节），
    /// 避免跨 &mut self 调用持有。
    fn render(&mut self, elements: &[LayeredElement]) -> &vello_cpu::Pixmap;
}
