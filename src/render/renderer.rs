//! Renderer trait — 渲染后端抽象

use crate::render::visual::LayeredElement;

pub trait Renderer {
    fn render(&mut self, elements: &[LayeredElement]) -> vello_cpu::Pixmap;
}
